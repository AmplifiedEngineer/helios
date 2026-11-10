// SPDX-License-Identifier: Apache-2.0

//! The pull request model shared by the fetch and render layers.

use chrono::{DateTime, Utc};

use crate::remote::Remote;
use crate::sanitize::sanitize;

/// Identifies a pull request across hosts and accounts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrKey {
    pub host: String,
    pub account: String,
    pub repo: String,
    pub number: u64,
}

/// Why a pull request is on the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Tag {
    Review,
    Assigned,
}

/// The review decision GitHub reports for a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
    /// A value this version of helios does not know, kept as received.
    Unknown(Remote),
}

impl ReviewDecision {
    pub fn from_raw(raw: &str) -> Self {
        match raw {
            "APPROVED" => Self::Approved,
            "CHANGES_REQUESTED" => Self::ChangesRequested,
            "REVIEW_REQUIRED" => Self::ReviewRequired,
            other => Self::Unknown(sanitize(other)),
        }
    }
}

/// The combined check state GitHub reports for a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckState {
    Success,
    Failure,
    Error,
    Pending,
    Expected,
    /// A value this version of helios does not know, kept as received.
    Unknown(Remote),
}

impl CheckState {
    pub fn from_raw(raw: &str) -> Self {
        match raw {
            "SUCCESS" => Self::Success,
            "FAILURE" => Self::Failure,
            "ERROR" => Self::Error,
            "PENDING" => Self::Pending,
            "EXPECTED" => Self::Expected,
            other => Self::Unknown(sanitize(other)),
        }
    }
}

/// One row of the pull request list. Text from GitHub is [`Remote`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrSummary {
    pub key: PrKey,
    pub title: Remote,
    pub author: Remote,
    pub is_draft: bool,
    pub updated_at: DateTime<Utc>,
    pub tags: Vec<Tag>,
    pub review_decision: Option<ReviewDecision>,
    pub check_state: Option<CheckState>,
}

/// Why a changed file has no diff to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiffGap {
    /// GitHub left the patch out because the diff is too large.
    Truncated,
    /// The file is binary, so it has no text diff.
    Binary,
    /// The file was renamed with no content change.
    RenamedOnly,
    /// No patch and no changes: an empty or binary file that was added or removed, or a mode-only
    /// change. The REST entry cannot tell these apart.
    Empty,
}

/// The diff of one changed file, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileDiff {
    Patch(Remote),
    Unavailable { gap: DiffGap, web_url: Remote },
}

/// One file changed by a pull request. Text from GitHub is [`Remote`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: Remote,
    /// Set when the file was renamed.
    pub previous_path: Option<Remote>,
    pub diff: FileDiff,
}

/// The lifecycle state GitHub reports for a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PrState {
    Open,
    Closed,
    Merged,
    /// A value this version of helios does not know, kept as received.
    Unknown(Remote),
}

/// One comment on the conversation or inside a review thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub author: Remote,
    pub body: Remote,
    pub created_at: DateTime<Utc>,
}

/// A review thread anchored to a line of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewThread {
    pub path: Remote,
    /// Missing when the line is gone from the current diff.
    pub line: Option<u64>,
    pub is_resolved: bool,
    pub comments: Vec<Comment>,
}

/// One commit of a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    pub oid: Remote,
    pub headline: Remote,
    pub author: Remote,
    pub date: DateTime<Utc>,
}

/// The state of one check run: its conclusion once finished, its status before.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckRunState {
    Success,
    Failure,
    Neutral,
    Cancelled,
    Skipped,
    TimedOut,
    ActionRequired,
    Queued,
    InProgress,
    /// A value this version of helios does not know, kept as received.
    Unknown(Remote),
}

/// One check run on the latest commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRun {
    pub name: Remote,
    pub state: CheckRunState,
    pub url: Option<Remote>,
}

/// Everything the detail view shows about one pull request. Text from GitHub is [`Remote`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrDetail {
    pub title: Remote,
    pub body: Remote,
    pub state: PrState,
    pub author: Remote,
    pub labels: Vec<Remote>,
    pub requested_reviewers: Vec<Remote>,
    pub review_decision: Option<ReviewDecision>,
    /// Conversation comments, kept apart from the review threads.
    pub comments: Vec<Comment>,
    pub threads: Vec<ReviewThread>,
    pub commits: Vec<CommitSummary>,
    pub check_runs: Vec<CheckRun>,
    /// GitHub holds more comments, threads, commits or checks than were fetched.
    pub partial: bool,
}
