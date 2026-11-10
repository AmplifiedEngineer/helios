// SPDX-License-Identifier: Apache-2.0

//! Failures of a GitHub call. No variant carries a token, a response body or
//! serde text, so any of them can be logged or shown as is.

use std::time::SystemTime;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GithubError {
    #[error("GitHub rejected the credentials")]
    AuthRequired,
    #[error("the token is not allowed to do this")]
    PermissionDenied,
    #[error("the organisation requires single sign-on for this token")]
    SsoBlocked,
    #[error("the resource does not exist or is hidden from this token")]
    NotFoundOrHidden,
    #[error("GitHub rate limit reached")]
    RateLimited { retry_at: SystemTime },
    #[error("could not reach GitHub")]
    Network,
    #[error("GitHub did not answer in time")]
    Timeout,
    #[error("GitHub answered with status {status} (request id {request_id:?})")]
    Api {
        status: u16,
        request_id: Option<String>,
    },
    #[error("GitHub sent a response helios could not read")]
    Decode,
}
