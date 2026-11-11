// SPDX-License-Identifier: Apache-2.0

//! Builds the pull request list queries and maps their responses, with no I/O.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;

use crate::model::{
    ChangedFile, CheckRun, CheckRunState, CheckState, Comment, CommitSummary, DiffGap, FileDiff,
    PrDetail, PrKey, PrState, PrSummary, ReviewDecision, ReviewThread, Tag,
};
use crate::remote::Remote;
use crate::sanitize::sanitize;

/// GitHub stops a search after this many results.
pub const SEARCH_CAP: usize = 1000;

/// Results asked for per page; the GraphQL maximum.
pub const PAGE_SIZE: usize = 100;

/// Pages that can hold `SEARCH_CAP` results; paging stops here even if rows stay short of the cap.
const MAX_PAGES: usize = SEARCH_CAP / PAGE_SIZE;

/// Variables: `$q` search string, `$first` page size, `$after` cursor or null.
pub const LIST_QUERY: &str = r#"query($q: String!, $first: Int!, $after: String) {
  search(query: $q, type: ISSUE, first: $first, after: $after) {
    issueCount
    pageInfo { hasNextPage endCursor }
    nodes {
      ... on PullRequest {
        number title isDraft updatedAt
        author { login }
        repository { nameWithOwner }
        reviewDecision
        commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
      }
    }
  }
  rateLimit { remaining resetAt cost }
}"#;

/// The searches behind the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ListKind {
    Mine,
    Review,
    Assigned,
}

const MINE_SEARCH: &str = "is:pr is:open author:@me archived:false sort:updated-desc";
const REVIEW_SEARCH: &str = "is:pr is:open review-requested:@me archived:false sort:updated-desc";
const ASSIGNED_SEARCH: &str =
    "is:pr is:open assignee:@me -author:@me archived:false sort:updated-desc";

/// Login shown for an author GitHub no longer has (a deleted account).
const GHOST_LOGIN: &str = "ghost";

impl ListKind {
    pub fn search(self) -> &'static str {
        match self {
            Self::Mine => MINE_SEARCH,
            Self::Review => REVIEW_SEARCH,
            Self::Assigned => ASSIGNED_SEARCH,
        }
    }
}

/// The GraphQL variables for one page of `kind`, resuming after `cursor`.
pub fn query_variables(kind: ListKind, cursor: Option<&str>) -> serde_json::Value {
    json!({ "q": kind.search(), "first": PAGE_SIZE, "after": cursor })
}

/// One mapped page of search results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPage {
    /// Rows carry no tag; the caller knows which search matched.
    pub rows: Vec<PrSummary>,
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
    pub issue_count: usize,
    /// The response held GraphQL errors next to its data, so rows may be missing.
    pub had_errors: bool,
}

/// All rows gathered over the pages, and whether the list is known to be incomplete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub rows: Vec<PrSummary>,
    /// The search cap cut the list short, or a page came back with GraphQL errors.
    pub partial: bool,
}

/// The changed files read for a pull request, and whether GitHub had more than the cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesListing {
    pub files: Vec<ChangedFile>,
    /// The page cap cut the list short, so the pull request has more files than shown.
    pub truncated: bool,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ListError {
    /// Carries no message text: serde_json echoes offending input values in it.
    #[error("could not parse the pull request search response ({category}) at line {line} column {column}")]
    Parse {
        category: &'static str,
        line: usize,
        column: usize,
    },
    #[error("GitHub answered the pull request search with errors and no data")]
    Api,
    #[error("a pull request in the search response has an unreadable update time")]
    Timestamp,
}

impl ListError {
    fn from_json(error: serde_json::Error) -> Self {
        let category = match error.classify() {
            serde_json::error::Category::Io => "io",
            serde_json::error::Category::Syntax => "syntax",
            serde_json::error::Category::Data => "data",
            serde_json::error::Category::Eof => "unexpected end of input",
        };
        Self::Parse {
            category,
            line: error.line(),
            column: error.column(),
        }
    }
}

#[derive(Deserialize)]
struct Response {
    data: Option<Data>,
    errors: Option<Vec<serde_json::Value>>,
}

#[derive(Deserialize)]
struct Data {
    search: Search,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Search {
    issue_count: usize,
    page_info: PageInfo,
    nodes: Vec<Option<Node>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

/// A search hit that is not a pull request arrives as an empty object.
#[derive(Deserialize)]
#[serde(untagged)]
enum Node {
    Pr(Box<PrNode>),
    Other(Other),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Other {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrNode {
    number: u64,
    title: String,
    is_draft: bool,
    updated_at: String,
    author: Option<Author>,
    repository: Repository,
    review_decision: Option<String>,
    commits: Commits,
}

#[derive(Deserialize)]
struct Author {
    login: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repository {
    name_with_owner: String,
}

#[derive(Deserialize)]
struct Commits {
    nodes: Vec<CommitNode>,
}

#[derive(Deserialize)]
struct CommitNode {
    commit: Commit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Commit {
    status_check_rollup: Option<Rollup>,
}

#[derive(Deserialize)]
struct Rollup {
    state: String,
}

fn login_or_ghost(author: Option<Author>) -> String {
    author.map_or(GHOST_LOGIN.to_owned(), |a| a.login)
}

impl PrNode {
    fn into_summary(self, host: &str, account: &str) -> Result<PrSummary, ListError> {
        let updated_at = DateTime::parse_from_rfc3339(&self.updated_at)
            .map_err(|_| ListError::Timestamp)?
            .with_timezone(&Utc);
        let author = login_or_ghost(self.author);
        let check_state = self
            .commits
            .nodes
            .into_iter()
            .next()
            .and_then(|node| node.commit.status_check_rollup)
            .map(|rollup| CheckState::from_raw(&rollup.state));
        Ok(PrSummary {
            key: PrKey {
                host: host.to_owned(),
                account: account.to_owned(),
                repo: sanitize(&self.repository.name_with_owner).to_string(),
                number: self.number,
            },
            title: sanitize(&self.title),
            author: sanitize(&author),
            is_draft: self.is_draft,
            updated_at,
            tags: Vec::new(),
            review_decision: self
                .review_decision
                .as_deref()
                .map(ReviewDecision::from_raw),
            check_state,
        })
    }
}

/// Maps one search response into rows, skipping hits that are not pull requests.
pub fn map_page(host: &str, account: &str, json: &str) -> Result<SearchPage, ListError> {
    let response: Response = serde_json::from_str(json).map_err(ListError::from_json)?;
    let had_errors = response.errors.is_some_and(|errors| !errors.is_empty());
    let search = response.data.ok_or(ListError::Api)?.search;
    let rows = search
        .nodes
        .into_iter()
        .flatten()
        .filter_map(|node| match node {
            Node::Pr(pr) => Some(pr.into_summary(host, account)),
            Node::Other(_) => None,
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SearchPage {
        rows,
        has_next_page: search.page_info.has_next_page,
        end_cursor: search.page_info.end_cursor,
        issue_count: search.issue_count,
        had_errors,
    })
}

/// Merges the two "needs me" searches by key, `review` winning, and tags each row.
///
/// Review rows come first, each group newest update first, ties by key.
pub fn merge_needs_me(review: Vec<PrSummary>, assigned: Vec<PrSummary>) -> Vec<PrSummary> {
    let mut seen: HashSet<PrKey> = HashSet::new();
    let mut tagged = |rows: Vec<PrSummary>, tag: Tag| -> Vec<PrSummary> {
        let mut group: Vec<PrSummary> = rows
            .into_iter()
            .filter(|row| seen.insert(row.key.clone()))
            .map(|row| PrSummary {
                tags: vec![tag],
                ..row
            })
            .collect();
        group.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.key.cmp(&b.key))
        });
        group
    };
    let mut merged = tagged(review, Tag::Review);
    merged.extend(tagged(assigned, Tag::Assigned));
    merged
}

/// Pages through `fetch` (given the cursor to resume after) until the last page or a bound.
///
/// Stops early, marking the list partial, after `MAX_PAGES` pages or when GitHub repeats a cursor.
pub fn collect(
    mut fetch: impl FnMut(Option<&str>) -> Result<SearchPage, ListError>,
) -> Result<Listing, ListError> {
    let mut pager = Pager::default();
    loop {
        let page = fetch(pager.cursor())?;
        if !pager.push(page) {
            return Ok(pager.finish());
        }
    }
}

/// The paging rules of `collect`, for callers that fetch pages asynchronously.
#[derive(Debug, Default)]
pub struct Pager {
    rows: Vec<PrSummary>,
    partial: bool,
    cursor: Option<String>,
    fetched: usize,
}

impl Pager {
    /// The cursor to resume after, `None` for the first page.
    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }

    /// Takes in a page; returns whether another page should be fetched.
    pub fn push(&mut self, page: SearchPage) -> bool {
        self.fetched += 1;
        self.partial |= page.had_errors || page.issue_count >= SEARCH_CAP;
        self.rows.extend(page.rows);
        let next = match page.end_cursor {
            Some(next) if page.has_next_page && self.rows.len() < SEARCH_CAP => next,
            _ => return false,
        };
        if self.fetched == MAX_PAGES || self.cursor.as_deref() == Some(next.as_str()) {
            self.partial = true;
            return false;
        }
        self.cursor = Some(next);
        true
    }

    pub fn finish(self) -> Listing {
        Listing {
            rows: self.rows,
            partial: self.partial,
        }
    }
}

pub fn filter_drafts(rows: Vec<PrSummary>, show_drafts: bool) -> Vec<PrSummary> {
    rows.into_iter()
        .filter(|row| row.is_visible(show_drafts))
        .collect()
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FilesError {
    #[error("could not parse the pull request files response")]
    Parse,
}

/// Files asked for per page; the REST maximum.
pub const FILES_PER_PAGE: usize = 100;

const STATUS_RENAMED: &str = "renamed";
const STATUS_ADDED: &str = "added";
const STATUS_REMOVED: &str = "removed";

/// The relative REST path of one page of a pull request's files.
pub fn files_path(owner: &str, repo: &str, number: u64, page: usize) -> String {
    format!("/repos/{owner}/{repo}/pulls/{number}/files?per_page={FILES_PER_PAGE}&page={page}")
}

#[derive(Deserialize)]
struct FileNode {
    filename: String,
    previous_filename: Option<String>,
    status: String,
    changes: u64,
    blob_url: String,
    patch: Option<String>,
}

impl FileNode {
    /// A missing patch with no changes is told apart by status only, since the REST entry carries
    /// no binary flag.
    fn gap(&self) -> DiffGap {
        if self.changes > 0 {
            return DiffGap::Truncated;
        }
        match self.status.as_str() {
            STATUS_RENAMED => DiffGap::RenamedOnly,
            STATUS_ADDED | STATUS_REMOVED => DiffGap::AddedOrRemovedNoText,
            _ => DiffGap::ChangedNoText,
        }
    }

    fn into_changed_file(self) -> ChangedFile {
        let diff = match &self.patch {
            Some(patch) => FileDiff::Patch(sanitize(patch)),
            None => FileDiff::Unavailable {
                gap: self.gap(),
                web_url: sanitize(&self.blob_url),
            },
        };
        ChangedFile {
            path: sanitize(&self.filename),
            previous_path: self.previous_filename.as_deref().map(sanitize),
            diff,
        }
    }
}

/// Maps one page of the REST pulls files response into changed files.
pub fn map_files(json: &str) -> Result<Vec<ChangedFile>, FilesError> {
    let nodes: Vec<FileNode> = serde_json::from_str(json).map_err(|_| FilesError::Parse)?;
    Ok(nodes.into_iter().map(FileNode::into_changed_file).collect())
}

/// Variables: `$owner`, `$repo` and `$number` of the pull request.
pub const DETAIL_QUERY: &str = r#"query($owner: String!, $repo: String!, $number: Int!) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      title body state
      author { login }
      labels(first: 50) { nodes { name } }
      reviewRequests(first: 50) { nodes { requestedReviewer { __typename ... on User { login } ... on Team { name } } } }
      reviewDecision
      comments(first: 100) { totalCount nodes { author { login } body createdAt } }
      reviewThreads(first: 100) {
        totalCount
        nodes {
          isResolved path line
          comments(first: 50) { nodes { author { login } body createdAt } }
        }
      }
      commits(first: 100) {
        totalCount
        nodes { commit { oid messageHeadline committedDate author { name user { login } } } }
      }
      checkRuns: commits(last: 1) {
        nodes { commit { statusCheckRollup { contexts(first: 100) { totalCount nodes { __typename ... on CheckRun { name status conclusion detailsUrl } } } } } }
      }
    }
  }
}"#;

/// The GraphQL variables for the detail query of one pull request.
pub fn detail_variables(owner: &str, repo: &str, number: u64) -> serde_json::Value {
    json!({ "owner": owner, "repo": repo, "number": number })
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DetailError {
    #[error("could not parse the pull request detail response")]
    Parse,
    #[error("a pull request in the detail response has an unreadable time")]
    Timestamp,
}

#[derive(Deserialize)]
struct DetailResponse {
    data: DetailData,
}

#[derive(Deserialize)]
struct DetailData {
    repository: DetailRepository,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetailRepository {
    pull_request: DetailNode,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetailNode {
    title: String,
    body: String,
    state: String,
    author: Option<Author>,
    labels: Nodes<Label>,
    review_requests: Nodes<ReviewRequest>,
    review_decision: Option<String>,
    comments: Nodes<CommentNode>,
    review_threads: Nodes<ThreadNode>,
    commits: Nodes<DetailCommitNode>,
    check_runs: Nodes<CheckRunsCommitNode>,
}

#[derive(Deserialize)]
struct Nodes<T> {
    nodes: Vec<T>,
    #[serde(default, rename = "totalCount")]
    total_count: usize,
}

impl<T> Nodes<T> {
    /// Whether GitHub reported more items than the page holds.
    fn is_cut(&self) -> bool {
        self.total_count > self.nodes.len()
    }
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewRequest {
    requested_reviewer: Option<Reviewer>,
}

/// A user has a login and a team a name; any other reviewer kind has neither.
#[derive(Deserialize)]
struct Reviewer {
    login: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentNode {
    author: Option<Author>,
    body: String,
    created_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadNode {
    is_resolved: bool,
    path: String,
    line: Option<u64>,
    comments: Nodes<CommentNode>,
}

#[derive(Deserialize)]
struct DetailCommitNode {
    commit: DetailCommit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetailCommit {
    oid: String,
    message_headline: String,
    committed_date: String,
    author: Option<GitActor>,
}

#[derive(Deserialize)]
struct GitActor {
    name: Option<String>,
    user: Option<Author>,
}

#[derive(Deserialize)]
struct CheckRunsCommitNode {
    commit: CheckRunsCommit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckRunsCommit {
    status_check_rollup: Option<ChecksRollup>,
}

#[derive(Deserialize)]
struct ChecksRollup {
    contexts: Nodes<CheckNode>,
}

/// A status context node carries only its type name, so every other field is optional.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckNode {
    name: Option<String>,
    status: Option<String>,
    conclusion: Option<String>,
    details_url: Option<String>,
}

const STATUS_COMPLETED: &str = "COMPLETED";

fn parse_time(raw: &str) -> Result<DateTime<Utc>, DetailError> {
    DateTime::parse_from_rfc3339(raw)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|_| DetailError::Timestamp)
}

fn pr_state(raw: &str) -> PrState {
    match raw {
        "OPEN" => PrState::Open,
        "CLOSED" => PrState::Closed,
        "MERGED" => PrState::Merged,
        other => PrState::Unknown(sanitize(other)),
    }
}

fn check_run_state(raw: &str) -> CheckRunState {
    match raw {
        "SUCCESS" => CheckRunState::Success,
        "FAILURE" => CheckRunState::Failure,
        "NEUTRAL" => CheckRunState::Neutral,
        "CANCELLED" => CheckRunState::Cancelled,
        "SKIPPED" => CheckRunState::Skipped,
        "TIMED_OUT" => CheckRunState::TimedOut,
        "ACTION_REQUIRED" => CheckRunState::ActionRequired,
        "QUEUED" => CheckRunState::Queued,
        "IN_PROGRESS" => CheckRunState::InProgress,
        other => CheckRunState::Unknown(sanitize(other)),
    }
}

impl CommentNode {
    fn into_comment(self) -> Result<Comment, DetailError> {
        Ok(Comment {
            author: sanitize(&login_or_ghost(self.author)),
            body: sanitize(&self.body),
            created_at: parse_time(&self.created_at)?,
        })
    }
}

fn into_comments(nodes: Nodes<CommentNode>) -> Result<Vec<Comment>, DetailError> {
    nodes
        .nodes
        .into_iter()
        .map(CommentNode::into_comment)
        .collect()
}

impl ThreadNode {
    fn into_thread(self) -> Result<ReviewThread, DetailError> {
        Ok(ReviewThread {
            path: sanitize(&self.path),
            line: self.line,
            is_resolved: self.is_resolved,
            comments: into_comments(self.comments)?,
        })
    }
}

impl DetailCommit {
    fn into_summary(self) -> Result<CommitSummary, DetailError> {
        let author = self
            .author
            .and_then(|actor| actor.user.map(|user| user.login).or(actor.name))
            .unwrap_or_else(|| GHOST_LOGIN.to_owned());
        Ok(CommitSummary {
            oid: sanitize(&self.oid),
            headline: sanitize(&self.message_headline),
            author: sanitize(&author),
            date: parse_time(&self.committed_date)?,
        })
    }
}

impl CheckNode {
    /// `None` for a node that is not a check run.
    fn into_check_run(self) -> Option<CheckRun> {
        let name = self.name?;
        let raw = match (self.status.as_deref(), self.conclusion) {
            (Some(STATUS_COMPLETED), Some(conclusion)) => conclusion,
            (Some(status), _) => status.to_owned(),
            (None, Some(conclusion)) => conclusion,
            (None, None) => return None,
        };
        Some(CheckRun {
            name: sanitize(&name),
            state: check_run_state(&raw),
            url: self.details_url.as_deref().map(sanitize),
        })
    }
}

fn into_check_runs(nodes: Nodes<CheckRunsCommitNode>) -> Vec<CheckRun> {
    nodes
        .nodes
        .into_iter()
        .filter_map(|node| node.commit.status_check_rollup)
        .flat_map(|rollup| rollup.contexts.nodes)
        .filter_map(CheckNode::into_check_run)
        .collect()
}

fn reviewer_name(reviewer: Reviewer) -> Option<Remote> {
    reviewer.login.or(reviewer.name).map(|name| sanitize(&name))
}

/// Maps the detail query response into a [`PrDetail`].
pub fn map_detail(json: &str) -> Result<PrDetail, DetailError> {
    let response: DetailResponse = serde_json::from_str(json).map_err(|_| DetailError::Parse)?;
    let pr = response.data.repository.pull_request;
    let partial = pr.comments.is_cut()
        || pr.review_threads.is_cut()
        || pr.commits.is_cut()
        || pr.check_runs.nodes.iter().any(|node| {
            node.commit
                .status_check_rollup
                .as_ref()
                .is_some_and(|rollup| rollup.contexts.is_cut())
        });
    Ok(PrDetail {
        partial,
        title: sanitize(&pr.title),
        body: sanitize(&pr.body),
        state: pr_state(&pr.state),
        author: sanitize(&login_or_ghost(pr.author)),
        labels: pr
            .labels
            .nodes
            .iter()
            .map(|label| sanitize(&label.name))
            .collect(),
        requested_reviewers: pr
            .review_requests
            .nodes
            .into_iter()
            .filter_map(|request| request.requested_reviewer)
            .filter_map(reviewer_name)
            .collect(),
        review_decision: pr.review_decision.as_deref().map(ReviewDecision::from_raw),
        comments: into_comments(pr.comments)?,
        threads: pr
            .review_threads
            .nodes
            .into_iter()
            .map(ThreadNode::into_thread)
            .collect::<Result<_, _>>()?,
        commits: pr
            .commits
            .nodes
            .into_iter()
            .map(|node| node.commit.into_summary())
            .collect::<Result<_, _>>()?,
        check_runs: into_check_runs(pr.check_runs),
    })
}
