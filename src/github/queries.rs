// SPDX-License-Identifier: Apache-2.0

//! Builds the pull request list queries and maps their responses, with no I/O.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;

use crate::model::{CheckState, PrKey, PrSummary, ReviewDecision, Tag};
use crate::sanitize::sanitize;

/// GitHub stops a search after this many results.
pub const SEARCH_CAP: usize = 1000;

/// Results asked for per page; the GraphQL maximum.
pub const PAGE_SIZE: usize = 100;

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
pub enum ListKind {
    Mine,
    Review,
    Assigned,
}

const MINE_SEARCH: &str = "is:pr is:open author:@me";
const REVIEW_SEARCH: &str = "is:pr is:open review-requested:@me";
const ASSIGNED_SEARCH: &str = "is:pr is:open assignee:@me -author:@me";

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

#[derive(Debug, thiserror::Error)]
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

impl PrNode {
    fn into_summary(self, host: &str, account: &str) -> Result<PrSummary, ListError> {
        let updated_at = DateTime::parse_from_rfc3339(&self.updated_at)
            .map_err(|_| ListError::Timestamp)?
            .with_timezone(&Utc);
        let author = self.author.map_or(GHOST_LOGIN.to_owned(), |a| a.login);
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

/// Pages through `fetch` (given the cursor to resume after) until the last page or the search cap.
pub fn collect(
    mut fetch: impl FnMut(Option<&str>) -> Result<SearchPage, ListError>,
) -> Result<Listing, ListError> {
    let mut rows = Vec::new();
    let mut partial = false;
    let mut cursor: Option<String> = None;
    loop {
        let page = fetch(cursor.as_deref())?;
        partial |= page.had_errors || page.issue_count >= SEARCH_CAP;
        rows.extend(page.rows);
        match page.end_cursor {
            Some(next) if page.has_next_page && rows.len() < SEARCH_CAP => cursor = Some(next),
            _ => break,
        }
    }
    Ok(Listing { rows, partial })
}

pub fn filter_drafts(rows: Vec<PrSummary>, show_drafts: bool) -> Vec<PrSummary> {
    rows.into_iter()
        .filter(|row| show_drafts || !row.is_draft)
        .collect()
}
