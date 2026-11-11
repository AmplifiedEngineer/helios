// SPDX-License-Identifier: Apache-2.0

//! Talking to GitHub: query text, response mapping, merging and paging.

use std::future::Future;

use crate::model::{ChangedFile, PrDetail, PrSummary};
use errors::GithubError;
use queries::ListKind;

pub mod errors;
pub mod octo;
pub mod queries;

/// What the rest of helios needs from GitHub, so callers can swap in a fake.
pub trait GithubApi: Send + Sync {
    fn list(
        &self,
        kind: ListKind,
    ) -> impl Future<Output = Result<Vec<PrSummary>, GithubError>> + Send;

    fn detail(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> impl Future<Output = Result<PrDetail, GithubError>> + Send;

    fn files(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> impl Future<Output = Result<Vec<ChangedFile>, GithubError>> + Send;
}
