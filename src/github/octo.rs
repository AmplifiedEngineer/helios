// SPDX-License-Identifier: Apache-2.0

//! The GitHub client for one account.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use octocrab::service::middleware::retry::RetryConfig;
use octocrab::Octocrab;
use serde::Deserialize;

use crate::accounts::{self, Account};
use crate::github::errors::GithubError;
use crate::github::queries::{
    detail_variables, files_path, map_detail, map_files, map_page, query_variables, ListKind,
    Listing, Pager, DETAIL_QUERY, LIST_QUERY,
};
use crate::github::GithubApi;
use crate::model::{ChangedFile, PrDetail};
use crate::secret::Secret;

/// Wait applied to a rate limit error that carries no reset header.
pub const DEFAULT_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(60);

/// How long one request, or one `gh` call, may take.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

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
/// `get_pages` follows the links from the first page.
const FIRST_FILES_PAGE: usize = 1;
const GRAPHQL_PATH: &str = "/graphql";
/// Path suffix GHES puts on its REST base; its GraphQL endpoint sits one level up.
const GHES_REST_SUFFIX: &str = "/v3";
/// Most pages `get_pages` reads: 3000 changed files at 100 per page.
pub const MAX_PAGES: usize = 30;
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
const GRAPHQL_NOT_FOUND: &str = "NOT_FOUND";
/// Text, in lowercase, that marks a SAML single sign-on refusal.
const SAML_TEXT: &str = "saml";
/// Text, in lowercase, of GitHub's secondary rate limit refusal.
const SECONDARY_RATE_LIMIT_TEXT: &str = "secondary rate limit";

/// Base URI of github.com's API.
pub const DEFAULT_BASE_URI: &str = "https://api.github.com";
const GITHUB_COM: &str = "github.com";
/// Host suffix of GHE.com data residency tenants, whose API sits on an `api.` host.
const GHE_COM_SUFFIX: &str = ".ghe.com";
const GHE_COM_API_PREFIX: &str = "api.";

/// The base URI of the REST and GraphQL API for `host`.
pub fn api_base(host: &str) -> String {
    if host == GITHUB_COM {
        DEFAULT_BASE_URI.to_string()
    } else if host.ends_with(GHE_COM_SUFFIX) {
        format!("https://{GHE_COM_API_PREFIX}{host}")
    } else {
        format!("https://{host}/api{GHES_REST_SUFFIX}")
    }
}

/// Every path the client requests, relative to the base URI.
pub const ENDPOINTS: &[&str] = &[GRAPHQL_PATH, FILES_PATH_TEMPLATE];

/// The GitHub client for one account.
pub struct Octo {
    host: String,
    login: String,
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
        account: &Account,
        base_uri: &str,
        timeout: Duration,
        token_source: Arc<dyn TokenSource>,
    ) -> Result<Self, GithubError> {
        Ok(Self {
            host: account.host.clone(),
            login: account.login.clone(),
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
        self.graphql_with(query, &serde_json::Value::Null).await
    }

    /// Runs a GraphQL query with its variables. A 401 re-fetches the token and retries once.
    pub async fn graphql_with(
        &self,
        query: &str,
        variables: &serde_json::Value,
    ) -> Result<GraphqlResponse, GithubError> {
        match self.graphql_once(query, variables).await {
            Err(GithubError::AuthRequired) => self.graphql_once(query, variables).await,
            other => other,
        }
    }

    async fn graphql_once(
        &self,
        query: &str,
        variables: &serde_json::Value,
    ) -> Result<GraphqlResponse, GithubError> {
        let client = self.graphql_client()?;
        let payload = serde_json::json!({ "query": query, "variables": variables });
        let response = client
            ._post(GRAPHQL_PATH, Some(&payload))
            .await
            .map_err(transport_error)?;
        let checked = check_status(response.status().as_u16(), |name| {
            response.headers().get(name)
        });
        let (_, signals) = match checked {
            Err(GithubError::PermissionDenied) => {
                let retry_at =
                    Signals::read(header_text(|name| response.headers().get(name))).retry_at();
                let body = client.body_to_string(response).await.unwrap_or_default();
                return Err(forbidden_body(&body, retry_at));
            }
            other => other?,
        };
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
    /// A next link may be a path or an absolute URL on the same base. Any other
    /// target, a repeated page, or more than `MAX_PAGES` pages is rejected with
    /// `GithubError::Decode`, so the token never goes to another host and a
    /// truncated list is never passed off as complete.
    pub async fn get_pages(&self, path: &str) -> Result<Vec<serde_json::Value>, GithubError> {
        let mut items = Vec::new();
        let mut next = Some(path.to_string());
        let mut seen = HashSet::new();
        let mut client = self.client()?;
        while let Some(current) = next {
            if seen.len() >= MAX_PAGES || !seen.insert(current.clone()) {
                return Err(GithubError::Decode);
            }
            let (page, link) = match self.get_page_once(&client, &current).await {
                Err(GithubError::AuthRequired) => {
                    client = self.client()?;
                    self.get_page_once(&client, &current).await?
                }
                other => other?,
            };
            items.extend(page);
            next = match link {
                Some(value) => next_path(&value, &self.base_uri)?,
                None => None,
            };
        }
        Ok(items)
    }

    /// Fetches one page with `client`, which carries the token for the whole walk.
    async fn get_page_once(&self, client: &Octocrab, path: &str) -> Result<Page, GithubError> {
        let response = client._get(path).await.map_err(transport_error)?;
        let checked = check_status(response.status().as_u16(), |name| {
            response.headers().get(name)
        });
        let (link, _) = match checked {
            Err(GithubError::PermissionDenied) => {
                let retry_at =
                    Signals::read(header_text(|name| response.headers().get(name))).retry_at();
                let body = client.body_to_string(response).await.unwrap_or_default();
                return Err(forbidden_body(&body, retry_at));
            }
            other => other?,
        };
        let body = client
            .body_to_string(response)
            .await
            .map_err(transport_error)?;
        let items = serde_json::from_str(&body).map_err(|_| GithubError::Decode)?;
        Ok((items, link))
    }

    fn client(&self) -> Result<Octocrab, GithubError> {
        self.client_for(&self.base_uri)
    }

    /// The client for GraphQL, whose endpoint is `<host>/api/graphql` on GHES.
    fn graphql_client(&self) -> Result<Octocrab, GithubError> {
        let base = self
            .base_uri
            .strip_suffix(GHES_REST_SUFFIX)
            .unwrap_or(&self.base_uri);
        self.client_for(base)
    }

    fn client_for(&self, base: &str) -> Result<Octocrab, GithubError> {
        let token = self.token_source.fetch()?;
        Octocrab::builder()
            .base_uri(base)
            .map_err(|_| GithubError::Network)?
            .personal_token(token.expose().to_string())
            .add_retry_config(RetryConfig::None)
            .set_connect_timeout(Some(self.timeout))
            .set_read_timeout(Some(self.timeout))
            .build()
            .map_err(|_| GithubError::Network)
    }
}

impl Octo {
    /// The body text of a GraphQL answer that carries data.
    async fn graphql_body(
        &self,
        query: &str,
        variables: &serde_json::Value,
    ) -> Result<(String, Vec<GraphqlError>), GithubError> {
        let response = self.graphql_with(query, variables).await?;
        let data = response.data.ok_or(GithubError::Decode)?;
        let messages: Vec<_> = response
            .errors
            .iter()
            .map(|error| serde_json::json!({ "message": error.message }))
            .collect();
        let body = serde_json::json!({ "data": data, "errors": messages }).to_string();
        Ok((body, response.errors))
    }
}

impl GithubApi for Octo {
    async fn list(&self, kind: ListKind) -> Result<Listing, GithubError> {
        let mut pager = Pager::default();
        loop {
            let variables = query_variables(kind, pager.cursor());
            let (body, _) = self.graphql_body(LIST_QUERY, &variables).await?;
            let page = map_page(&self.host, &self.login, &body).map_err(|_| GithubError::Decode)?;
            if !pager.push(page) {
                return Ok(pager.finish());
            }
        }
    }

    async fn detail(&self, owner: &str, repo: &str, number: u64) -> Result<PrDetail, GithubError> {
        let variables = detail_variables(owner, repo, number);
        let (body, errors) = self.graphql_body(DETAIL_QUERY, &variables).await?;
        map_detail(&body).map_err(|_| classify_errors(&errors).unwrap_or(GithubError::Decode))
    }

    async fn files(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<Vec<ChangedFile>, GithubError> {
        let items = self
            .get_pages(&files_path(owner, repo, number, FIRST_FILES_PAGE))
            .await?;
        let json = serde_json::Value::Array(items).to_string();
        map_files(&json).map_err(|_| GithubError::Decode)
    }
}

/// The `Link` header and rate limit signals of a successful response; any other
/// status becomes its error. `get` looks a header up by lowercase name.
fn check_status<V: AsRef<[u8]>>(
    status: u16,
    get: impl Fn(&str) -> Option<V>,
) -> Result<(Option<String>, Signals), GithubError> {
    let header = header_text(get);
    let signals = Signals::read(&header);
    if (200..300).contains(&status) {
        Ok((header(LINK_HEADER), signals))
    } else {
        Err(map_status(status, signals))
    }
}

/// The items of one page and the raw `Link` header, if any.
type Page = (Vec<serde_json::Value>, Option<String>);

/// The `rel="next"` target of a `Link` header as a path on `base_uri`. An
/// absolute URL is accepted only when it starts with `base_uri` and continues
/// with a path or query. Returns `Ok(None)` on the last page.
fn next_path(link: &str, base_uri: &str) -> Result<Option<String>, GithubError> {
    let target = link
        .split(',')
        .map(str::trim)
        .find(|entry| entry.contains(NEXT_RELATION))
        .and_then(|entry| entry.strip_prefix('<')?.split_once('>'))
        .map(|(target, _)| target);
    let Some(target) = target else {
        return Ok(None);
    };
    let base = base_uri.trim_end_matches('/');
    if let Some(rest) = target.strip_prefix(base) {
        if rest.starts_with('/') || rest.starts_with('?') {
            return Ok(Some(rest.to_string()));
        }
        return Err(GithubError::Decode);
    }
    if target.starts_with('/') && !target.starts_with(SCHEME_RELATIVE_PREFIX) {
        return Ok(Some(target.to_string()));
    }
    Err(GithubError::Decode)
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
        let default = SystemTime::now() + DEFAULT_RATE_LIMIT_BACKOFF;
        let wanted = match (self.retry_after, self.reset) {
            (Some(wait), _) => SystemTime::now().checked_add(Duration::from_secs(wait)),
            (None, Some(reset)) => UNIX_EPOCH.checked_add(Duration::from_secs(reset)),
            (None, None) => None,
        };
        wanted.unwrap_or(default)
    }

    fn rate_limited(&self) -> bool {
        self.retry_after.is_some() || self.rate_limit_exhausted
    }
}

fn envelope(raw: RawGraphqlResponse, signals: &Signals) -> Result<GraphqlResponse, GithubError> {
    if raw.data.is_none() {
        let has = |kind: &str| raw.errors.iter().any(|e| e.kind.as_deref() == Some(kind));
        if let Some(error) = raw
            .errors
            .iter()
            .find(|e| e.kind.as_deref() == Some(GRAPHQL_FORBIDDEN))
        {
            return Err(if signals.sso {
                GithubError::SsoBlocked
            } else {
                forbidden(&error.message)
            });
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

/// SSO when the message names SAML enforcement, otherwise a plain permission failure.
fn forbidden(message: &str) -> GithubError {
    if message.to_lowercase().contains(SAML_TEXT) {
        GithubError::SsoBlocked
    } else {
        GithubError::PermissionDenied
    }
}

/// The error a 403 without headers stands for, read from its body.
fn forbidden_body(body: &str, retry_at: SystemTime) -> GithubError {
    if body.to_lowercase().contains(SECONDARY_RATE_LIMIT_TEXT) {
        GithubError::RateLimited { retry_at }
    } else {
        forbidden(body)
    }
}

/// The error GraphQL error types stand for when the data has a null where a
/// value was needed.
fn classify_errors(errors: &[GraphqlError]) -> Option<GithubError> {
    errors.iter().find_map(|error| match error.kind.as_deref() {
        Some(GRAPHQL_NOT_FOUND) => Some(GithubError::NotFoundOrHidden),
        Some(GRAPHQL_FORBIDDEN) => Some(forbidden(&error.message)),
        _ => None,
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
