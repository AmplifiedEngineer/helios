// SPDX-License-Identifier: Apache-2.0

//! The GitHub client for one account.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use octocrab::service::middleware::retry::RetryConfig;
use octocrab::Octocrab;
use serde::Deserialize;

use crate::accounts::{self, Account};
use crate::github::errors::GithubError;
use crate::secret::Secret;

/// Wait applied to a rate limit error that carries no reset header.
pub const DEFAULT_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(60);

/// Supplies the token for each request, so a rotated token is picked up.
pub trait TokenSource: Send + Sync {
    fn fetch(&self) -> Result<Secret, GithubError>;
}

/// Reads the token from `gh auth token`.
pub struct GhTokens {
    pub program: PathBuf,
    pub host: String,
    pub login: String,
    pub timeout: Duration,
}

impl TokenSource for GhTokens {
    fn fetch(&self) -> Result<Secret, GithubError> {
        accounts::token(&self.program, &self.host, &self.login, self.timeout)
            .map_err(|_| GithubError::AuthRequired)
    }
}

/// One GraphQL error entry; `kind` is the `type` field GitHub sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphqlError {
    pub message: String,
    pub kind: Option<String>,
}

/// A GraphQL answer. `data` survives next to `errors` so a partial result is kept.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphqlResponse {
    pub data: Option<serde_json::Value>,
    pub errors: Vec<GraphqlError>,
}

/// Header GitHub sets on a 403 when the organisation needs single sign-on.
const SSO_HEADER: &str = "x-github-sso";
const REMAINING_HEADER: &str = "x-ratelimit-remaining";
const RESET_HEADER: &str = "x-ratelimit-reset";
const RETRY_AFTER_HEADER: &str = "retry-after";
const REQUEST_ID_HEADER: &str = "x-github-request-id";
const LINK_HEADER: &str = "link";
const NEXT_RELATION: &str = "rel=\"next\"";
/// Prefix of a scheme-relative target, which names another host.
const SCHEME_RELATIVE_PREFIX: &str = "//";
const GRAPHQL_PATH: &str = "/graphql";
/// Shape of the pull request files path built by `queries::files_path`.
const FILES_PATH_TEMPLATE: &str = "/repos/{owner}/{repo}/pulls/{number}/files";
/// Text the timeout layer's error carries.
const TIMEOUT_MESSAGE: &str = "timed out";
const STATUS_UNAUTHORIZED: u16 = 401;
const STATUS_FORBIDDEN: u16 = 403;
const STATUS_NOT_FOUND: u16 = 404;
const STATUS_TOO_MANY_REQUESTS: u16 = 429;
const GRAPHQL_FORBIDDEN: &str = "FORBIDDEN";
const GRAPHQL_RATE_LIMITED: &str = "RATE_LIMITED";

/// Base URI of github.com's API.
pub const DEFAULT_BASE_URI: &str = "https://api.github.com";

/// Every path the client requests, relative to the base URI.
pub const ENDPOINTS: &[&str] = &[GRAPHQL_PATH, FILES_PATH_TEMPLATE];

/// The GitHub client for one account.
pub struct Octo {
    base_uri: String,
    timeout: Duration,
    token_source: Arc<dyn TokenSource>,
}

#[derive(Deserialize)]
struct RawGraphqlError {
    message: String,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct RawGraphqlResponse {
    data: Option<serde_json::Value>,
    #[serde(default)]
    errors: Vec<RawGraphqlError>,
}

impl Octo {
    pub fn new(
        _account: &Account,
        base_uri: &str,
        timeout: Duration,
        token_source: Arc<dyn TokenSource>,
    ) -> Result<Self, GithubError> {
        Ok(Self {
            base_uri: base_uri.to_string(),
            timeout,
            token_source,
        })
    }

    pub fn github_com(
        account: &Account,
        timeout: Duration,
        token_source: Arc<dyn TokenSource>,
    ) -> Result<Self, GithubError> {
        Self::new(account, DEFAULT_BASE_URI, timeout, token_source)
    }

    pub fn base_uri(&self) -> &str {
        &self.base_uri
    }

    /// Runs a GraphQL query. A 401 re-fetches the token and retries once.
    pub async fn graphql(&self, query: &str) -> Result<GraphqlResponse, GithubError> {
        match self.graphql_once(query).await {
            Err(GithubError::AuthRequired) => self.graphql_once(query).await,
            other => other,
        }
    }

    async fn graphql_once(&self, query: &str) -> Result<GraphqlResponse, GithubError> {
        let client = self.client()?;
        let payload = serde_json::json!({ "query": query });
        let response = client
            ._post(GRAPHQL_PATH, Some(&payload))
            .await
            .map_err(transport_error)?;
        let status = response.status();
        let signals = {
            let header = header_text(|name| response.headers().get(name));
            Signals::read(&header)
        };
        if !status.is_success() {
            return Err(map_status(status.as_u16(), signals));
        }
        let body = client
            .body_to_string(response)
            .await
            .map_err(transport_error)?;
        let raw: RawGraphqlResponse =
            serde_json::from_str(&body).map_err(|_| GithubError::Decode)?;
        envelope(raw, &signals)
    }

    /// Reads every page of a REST list, following relative `Link` paths, and
    /// returns the items of all pages in order.
    ///
    /// A next link that is not a path on the same base is rejected with
    /// `GithubError::Decode`, so the token never goes to another host and a
    /// truncated list is never passed off as complete.
    pub async fn get_pages(&self, path: &str) -> Result<Vec<serde_json::Value>, GithubError> {
        let mut items = Vec::new();
        let mut next = Some(path.to_string());
        while let Some(current) = next {
            let (page, link) = self.get_page(&current).await?;
            items.extend(page);
            next = match link {
                Some(value) => next_path(&value)?,
                None => None,
            };
        }
        Ok(items)
    }

    /// Fetches one page. A 401 re-fetches the token and retries once.
    async fn get_page(&self, path: &str) -> Result<Page, GithubError> {
        match self.get_page_once(path).await {
            Err(GithubError::AuthRequired) => self.get_page_once(path).await,
            other => other,
        }
    }

    async fn get_page_once(&self, path: &str) -> Result<Page, GithubError> {
        let client = self.client()?;
        let response = client._get(path).await.map_err(transport_error)?;
        let status = response.status();
        let (link, signals) = {
            let header = header_text(|name| response.headers().get(name));
            (header(LINK_HEADER), Signals::read(&header))
        };
        if !status.is_success() {
            return Err(map_status(status.as_u16(), signals));
        }
        let body = client
            .body_to_string(response)
            .await
            .map_err(transport_error)?;
        let items = serde_json::from_str(&body).map_err(|_| GithubError::Decode)?;
        Ok((items, link))
    }

    fn client(&self) -> Result<Octocrab, GithubError> {
        let token = self.token_source.fetch()?;
        Octocrab::builder()
            .base_uri(self.base_uri.as_str())
            .map_err(|_| GithubError::Network)?
            .personal_token(token.expose().to_string())
            .add_retry_config(RetryConfig::None)
            .set_connect_timeout(Some(self.timeout))
            .set_read_timeout(Some(self.timeout))
            .build()
            .map_err(|_| GithubError::Network)
    }
}

/// The items of one page and the raw `Link` header, if any.
type Page = (Vec<serde_json::Value>, Option<String>);

/// The `rel="next"` target of a `Link` header, when it is a path on the same
/// base. Returns `Ok(None)` on the last page.
fn next_path(link: &str) -> Result<Option<String>, GithubError> {
    let target = link
        .split(',')
        .map(str::trim)
        .find(|entry| entry.contains(NEXT_RELATION))
        .and_then(|entry| entry.strip_prefix('<')?.split_once('>'))
        .map(|(target, _)| target);
    match target {
        None => Ok(None),
        Some(path) if path.starts_with('/') && !path.starts_with(SCHEME_RELATIVE_PREFIX) => {
            Ok(Some(path.to_string()))
        }
        Some(_) => Err(GithubError::Decode),
    }
}

/// Maps a failure below HTTP, telling a timeout apart from other network errors.
fn transport_error(error: octocrab::Error) -> GithubError {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
    while let Some(cause) = source {
        if is_timeout(cause) {
            return GithubError::Timeout;
        }
        source = cause.source();
    }
    GithubError::Network
}

/// Whether one error in the chain is a timeout. The tower timeout error and the
/// connector's error are not nameable through octocrab's public API, so the
/// message is the only signal for them; an I/O timeout is matched by kind.
fn is_timeout(cause: &(dyn std::error::Error + 'static)) -> bool {
    let io_timeout = cause
        .downcast_ref::<std::io::Error>()
        .is_some_and(|io| io.kind() == std::io::ErrorKind::TimedOut);
    io_timeout || cause.to_string().contains(TIMEOUT_MESSAGE)
}

/// Turns a raw header lookup into one that yields owned text, so callers need
/// not name the HTTP header types.
fn header_text<V: AsRef<[u8]>>(get: impl Fn(&str) -> Option<V>) -> impl Fn(&str) -> Option<String> {
    move |name| {
        let value = get(name)?;
        std::str::from_utf8(value.as_ref()).ok().map(str::to_string)
    }
}

/// The response headers the error mapping reads, parsed once.
struct Signals {
    sso: bool,
    rate_limit_exhausted: bool,
    retry_after: Option<u64>,
    reset: Option<u64>,
    request_id: Option<String>,
}

impl Signals {
    /// `header` returns a header value by lowercase name.
    fn read(header: impl Fn(&str) -> Option<String>) -> Self {
        let seconds = |name: &str| header(name).and_then(|v| v.parse::<u64>().ok());
        Self {
            sso: header(SSO_HEADER).is_some(),
            rate_limit_exhausted: seconds(REMAINING_HEADER) == Some(0),
            retry_after: seconds(RETRY_AFTER_HEADER),
            reset: seconds(RESET_HEADER),
            request_id: header(REQUEST_ID_HEADER),
        }
    }

    /// `Retry-After` wins over the reset time, then the default backoff applies.
    fn retry_at(&self) -> SystemTime {
        match (self.retry_after, self.reset) {
            (Some(wait), _) => SystemTime::now() + Duration::from_secs(wait),
            (None, Some(reset)) => UNIX_EPOCH + Duration::from_secs(reset),
            (None, None) => SystemTime::now() + DEFAULT_RATE_LIMIT_BACKOFF,
        }
    }

    fn rate_limited(&self) -> bool {
        self.retry_after.is_some() || self.rate_limit_exhausted
    }
}

fn envelope(raw: RawGraphqlResponse, signals: &Signals) -> Result<GraphqlResponse, GithubError> {
    if raw.data.is_none() {
        let has = |kind: &str| raw.errors.iter().any(|e| e.kind.as_deref() == Some(kind));
        if has(GRAPHQL_FORBIDDEN) {
            return Err(GithubError::SsoBlocked);
        }
        if has(GRAPHQL_RATE_LIMITED) {
            return Err(GithubError::RateLimited {
                retry_at: signals.retry_at(),
            });
        }
    }
    Ok(GraphqlResponse {
        data: raw.data,
        errors: raw
            .errors
            .into_iter()
            .map(|e| GraphqlError {
                message: e.message,
                kind: e.kind,
            })
            .collect(),
    })
}

fn map_status(status: u16, signals: Signals) -> GithubError {
    match status {
        STATUS_UNAUTHORIZED => GithubError::AuthRequired,
        STATUS_FORBIDDEN if signals.sso => GithubError::SsoBlocked,
        STATUS_FORBIDDEN if signals.rate_limited() => GithubError::RateLimited {
            retry_at: signals.retry_at(),
        },
        STATUS_FORBIDDEN => GithubError::PermissionDenied,
        STATUS_NOT_FOUND => GithubError::NotFoundOrHidden,
        STATUS_TOO_MANY_REQUESTS => GithubError::RateLimited {
            retry_at: signals.retry_at(),
        },
        _ => GithubError::Api {
            status,
            request_id: signals.request_id,
        },
    }
}
