// SPDX-License-Identifier: Apache-2.0

//! Talking to GitHub: query text, response mapping, merging and paging.

use std::future::Future;

use crate::model::PrDetail;
use errors::GithubError;
use queries::{FilesListing, ListKind, Listing};

pub mod clients;
pub mod errors;
pub mod octo;
pub mod queries;

/// What the rest of helios needs from GitHub, so callers can swap in a fake.
pub trait GithubApi: Send + Sync {
    /// The rows of one list, and whether GitHub cut it short.
    fn list(&self, kind: ListKind) -> impl Future<Output = Result<Listing, GithubError>> + Send;

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
    ) -> impl Future<Output = Result<FilesListing, GithubError>> + Send;
}
