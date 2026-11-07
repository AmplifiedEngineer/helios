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
pub enum Tag {
    Review,
    Assigned,
}

/// The review decision GitHub reports for a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
    /// A value this version of forgebird does not know, kept as received.
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
pub enum CheckState {
    Success,
    Failure,
    Error,
    Pending,
    Expected,
    /// A value this version of forgebird does not know, kept as received.
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
