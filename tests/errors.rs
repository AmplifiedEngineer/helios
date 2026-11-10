// SPDX-License-Identifier: Apache-2.0

//! The GraphQL envelope and the mapping of HTTP and GraphQL failures to `GithubError`.

// The shared server has helpers this file does not use.
#[allow(dead_code)]
mod support;

use std::error::Error;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use helios::accounts::{Account, AccountState};
use helios::github::errors::GithubError;
use helios::github::octo::{
    GraphqlResponse, Octo, TokenSource, DEFAULT_BASE_URI, DEFAULT_RATE_LIMIT_BACKOFF, ENDPOINTS,
};
use helios::secret::Secret;
use support::server::{ScriptedResponse, TestServer};

/// Planted token, built at runtime so no token-shaped literal sits in the source.
fn planted_token() -> String {
    format!("{}_{}", "tok", "planted")
}

const QUERY: &str = "{ viewer { login } }";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Allowance for the time the call itself takes when comparing against the clock.
/// Allowance over the injected timeout before a stalled call counts as hung.
const TIMEOUT_SLACK: Duration = Duration::from_secs(2);
const SHORT_TIMEOUT: Duration = Duration::from_millis(300);
const AUTHORIZATION_HEADER: &str = "Authorization";
const CLOCK_SLACK: Duration = Duration::from_secs(5);

/// GitHub's documented SSO header: `X-GitHub-SSO: required; url=...`.
const SSO_HEADER: &str = "X-GitHub-SSO";
const SSO_VALUE: &str = "required; url=https://github.test/orgs/acme/sso";
/// Primary limit: remaining is 0 and reset is a UTC epoch in seconds.
const REMAINING_HEADER: &str = "X-RateLimit-Remaining";
const RESET_HEADER: &str = "X-RateLimit-Reset";
/// Secondary limit: wait this many seconds.
const RETRY_AFTER_HEADER: &str = "Retry-After";
const REQUEST_ID_HEADER: &str = "X-GitHub-Request-Id";

const RESET_EPOCH_SECS: u64 = 2_000_000_000;
const RETRY_AFTER_SECS: u64 = 120;
/// Differs from the reset time, so a test shows which header won.
const REQUEST_ID: &str = "ABCD:1234:5678";

struct FixedToken(String);

impl TokenSource for FixedToken {
    fn fetch(&self) -> Result<Secret, GithubError> {
        Ok(Secret::new(self.0.clone()))
    }
}

fn account() -> Account {
    Account {
        host: "github.test".into(),
        login: "octo".into(),
        active: true,
        scopes: vec![],
        state: AccountState::Success,
    }
}

async fn call(response: ScriptedResponse) -> Result<GraphqlResponse, GithubError> {
    let server = TestServer::start(response);
    let token = Arc::new(FixedToken(planted_token()));
    let octo =
        Octo::new(&account(), &server.origin(), REQUEST_TIMEOUT, token).expect("client builds");
    octo.graphql(QUERY).await
}

fn graphql_error_body(kind: &str) -> String {
    graphql_error_with(kind, "nope")
}

fn graphql_error_with(kind: &str, message: &str) -> String {
    format!(r#"{{"data":null,"errors":[{{"type":"{kind}","message":"{message}"}}]}}"#)
}

#[tokio::test]
async fn graphql_response_with_data_and_errors_keeps_the_data() {
    // Arrange
    let body = r#"{"data":{"viewer":{"login":"octo"}},"errors":[{"type":"NOT_FOUND","message":"partial"}]}"#;

    // Act
    let response = call(ScriptedResponse::json(200, body))
        .await
        .expect("a partial answer is not an error");

    // Assert
    let data = response.data.expect("data is kept");
    assert_eq!(data["viewer"]["login"], "octo");
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].message, "partial");
    assert_eq!(response.errors[0].kind.as_deref(), Some("NOT_FOUND"));
}

#[tokio::test]
async fn status_401_is_auth_required() {
    // Arrange
    let response = ScriptedResponse::json(401, r#"{"message":"Bad credentials"}"#);

    // Act
    let result = call(response).await;

    // Assert
    assert!(
        matches!(result, Err(GithubError::AuthRequired)),
        "{result:?}"
    );
}

#[tokio::test]
async fn status_403_without_other_signals_is_permission_denied() {
    // Arrange
    let response = ScriptedResponse::json(403, r#"{"message":"Resource not accessible"}"#);

    // Act
    let result = call(response).await;

    // Assert
    assert!(
        matches!(result, Err(GithubError::PermissionDenied)),
        "{result:?}"
    );
}

#[tokio::test]
async fn status_403_with_the_sso_header_is_sso_blocked() {
    // Arrange
    let response = ScriptedResponse::json(403, r#"{"message":"Resource protected by SSO"}"#)
        .with_header(SSO_HEADER, SSO_VALUE);

    // Act
    let result = call(response).await;

    // Assert
    assert!(matches!(result, Err(GithubError::SsoBlocked)), "{result:?}");
}

#[tokio::test]
async fn graphql_forbidden_error_is_sso_blocked() {
    // Arrange
    let body = graphql_error_with(
        "FORBIDDEN",
        "Resource protected by organization SAML enforcement",
    );
    let response = ScriptedResponse::json(200, &body);

    // Act
    let result = call(response).await;

    // Assert
    assert!(matches!(result, Err(GithubError::SsoBlocked)), "{result:?}");
}

#[tokio::test]
async fn status_404_is_not_found_or_hidden() {
    // Arrange
    let response = ScriptedResponse::json(404, r#"{"message":"Not Found"}"#);

    // Act
    let result = call(response).await;

    // Assert
    assert!(
        matches!(result, Err(GithubError::NotFoundOrHidden)),
        "{result:?}"
    );
}

#[tokio::test]
async fn primary_limit_403_retries_at_the_reset_time() {
    // Arrange
    let response = ScriptedResponse::json(403, r#"{"message":"API rate limit exceeded"}"#)
        .with_header(REMAINING_HEADER, "0")
        .with_header(RESET_HEADER, &RESET_EPOCH_SECS.to_string());

    // Act
    let result = call(response).await;

    // Assert
    let Err(GithubError::RateLimited { retry_at }) = result else {
        panic!("expected RateLimited, got {result:?}");
    };
    assert_eq!(retry_at, UNIX_EPOCH + Duration::from_secs(RESET_EPOCH_SECS));
}

#[tokio::test]
async fn status_429_with_retry_after_retries_that_many_seconds_from_now() {
    // Arrange
    let response = ScriptedResponse::json(429, r#"{"message":"secondary rate limit"}"#)
        .with_header(RETRY_AFTER_HEADER, &RETRY_AFTER_SECS.to_string());
    let before = SystemTime::now();

    // Act
    let result = call(response).await;

    // Assert
    let Err(GithubError::RateLimited { retry_at }) = result else {
        panic!("expected RateLimited, got {result:?}");
    };
    let earliest = before + Duration::from_secs(RETRY_AFTER_SECS);
    assert!(retry_at >= earliest, "{retry_at:?} < {earliest:?}");
    assert!(retry_at <= earliest + CLOCK_SLACK, "{retry_at:?} too late");
}

#[tokio::test]
async fn retry_after_wins_over_the_reset_time() {
    // Arrange
    let response = ScriptedResponse::json(429, r#"{"message":"rate limited"}"#)
        .with_header(REMAINING_HEADER, "0")
        .with_header(RESET_HEADER, &RESET_EPOCH_SECS.to_string())
        .with_header(RETRY_AFTER_HEADER, &RETRY_AFTER_SECS.to_string());
    let before = SystemTime::now();

    // Act
    let result = call(response).await;

    // Assert
    let Err(GithubError::RateLimited { retry_at }) = result else {
        panic!("expected RateLimited, got {result:?}");
    };
    assert!(retry_at >= before + Duration::from_secs(RETRY_AFTER_SECS));
    assert!(retry_at < UNIX_EPOCH + Duration::from_secs(RESET_EPOCH_SECS));
}

#[tokio::test]
async fn graphql_rate_limited_without_a_header_uses_the_default_backoff() {
    // Arrange
    let response = ScriptedResponse::json(200, &graphql_error_body("RATE_LIMITED"));
    let before = SystemTime::now();

    // Act
    let result = call(response).await;

    // Assert
    let Err(GithubError::RateLimited { retry_at }) = result else {
        panic!("expected RateLimited, got {result:?}");
    };
    let earliest = before + DEFAULT_RATE_LIMIT_BACKOFF;
    assert!(retry_at >= earliest, "{retry_at:?} < {earliest:?}");
    assert!(retry_at <= earliest + CLOCK_SLACK, "{retry_at:?} too late");
}

#[tokio::test]
async fn status_500_is_api_with_status_and_request_id() {
    // Arrange
    let response = ScriptedResponse::json(500, r#"{"message":"boom"}"#)
        .with_header(REQUEST_ID_HEADER, REQUEST_ID);

    // Act
    let result = call(response).await;

    // Assert
    let Err(GithubError::Api { status, request_id }) = result else {
        panic!("expected Api, got {result:?}");
    };
    assert_eq!(status, 500);
    assert_eq!(request_id.as_deref(), Some(REQUEST_ID));
}

#[tokio::test]
async fn undecodable_body_is_decode() {
    // Arrange
    let response = ScriptedResponse::json(200, "this is not json");

    // Act
    let result = call(response).await;

    // Assert
    assert!(matches!(result, Err(GithubError::Decode)), "{result:?}");
}

/// Hands out the same token and counts how often it was asked for one.
struct CountingToken {
    fetches: AtomicUsize,
}

impl TokenSource for CountingToken {
    fn fetch(&self) -> Result<Secret, GithubError> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        Ok(Secret::new(planted_token()))
    }
}

/// Accepts connections and never answers, until dropped.
struct StalledListener {
    origin: String,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl StalledListener {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
        let addr = listener.local_addr().expect("listener address");
        let stop = Arc::new(AtomicBool::new(false));
        let held = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            let mut open = Vec::new();
            for stream in listener.incoming() {
                if held.load(Ordering::SeqCst) {
                    break;
                }
                open.extend(stream.ok());
            }
        });
        Self {
            origin: format!("http://{addr}"),
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for StalledListener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocked accept so the thread sees the stop flag.
        let _ = std::net::TcpStream::connect(self.origin.trim_start_matches("http://"));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Debug, Display and every link of the source chain, one string each.
fn renderings(error: &GithubError) -> Vec<String> {
    let mut out = vec![format!("{error:?}"), error.to_string()];
    let mut source = error.source();
    while let Some(cause) = source {
        out.push(format!("{cause:?}"));
        out.push(cause.to_string());
        source = cause.source();
    }
    out
}

async fn graphql_error(origin: &str, timeout: Duration) -> GithubError {
    let token = Arc::new(FixedToken(planted_token()));
    let octo = Octo::new(&account(), origin, timeout, token).expect("client builds");
    octo.graphql(QUERY).await.expect_err("the call fails")
}

#[tokio::test]
async fn no_error_variant_leaks_the_token() {
    // Arrange
    let echoing = |status: u16, extra: Option<(&'static str, &'static str)>| {
        let server = TestServer::start_with(move |request| {
            let echoed = request.header("authorization").unwrap_or_default();
            let mut response = ScriptedResponse::json(status, &format!("not json: {echoed}"));
            if let Some((name, value)) = extra {
                response = response.with_header(name, value);
            }
            response
        });
        server
    };
    let cases = [
        echoing(401, None),
        echoing(403, None),
        echoing(403, Some((SSO_HEADER, SSO_VALUE))),
        echoing(404, None),
        echoing(429, Some((RETRY_AFTER_HEADER, "1"))),
        echoing(500, None),
        echoing(200, None),
    ];
    let stalled = StalledListener::start();
    let closed = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
        format!(
            "http://{}",
            listener.local_addr().expect("listener address")
        )
    };
    let mut errors = Vec::new();

    // Act
    for server in &cases {
        errors.push(graphql_error(&server.origin(), REQUEST_TIMEOUT).await);
    }
    errors.push(graphql_error(&closed, REQUEST_TIMEOUT).await);
    errors.push(graphql_error(&stalled.origin, SHORT_TIMEOUT).await);

    // Assert
    assert!(matches!(errors[0], GithubError::AuthRequired), "{errors:?}");
    assert!(
        matches!(errors[1], GithubError::PermissionDenied),
        "{errors:?}"
    );
    assert!(matches!(errors[2], GithubError::SsoBlocked), "{errors:?}");
    assert!(
        matches!(errors[3], GithubError::NotFoundOrHidden),
        "{errors:?}"
    );
    assert!(
        matches!(errors[4], GithubError::RateLimited { .. }),
        "{errors:?}"
    );
    assert!(matches!(errors[5], GithubError::Api { .. }), "{errors:?}");
    assert!(matches!(errors[6], GithubError::Decode), "{errors:?}");
    assert!(matches!(errors[7], GithubError::Network), "{errors:?}");
    assert!(matches!(errors[8], GithubError::Timeout), "{errors:?}");
    let token = planted_token();
    for error in &errors {
        for text in renderings(error) {
            assert!(!text.contains(&token), "token leaked into {text}");
        }
    }
}

#[tokio::test]
async fn a_ghes_base_uri_receives_the_authorization_header() {
    // Arrange
    let server = TestServer::start(ScriptedResponse::json(200, r#"{"data":{}}"#));
    let token = Arc::new(FixedToken(planted_token()));
    let octo =
        Octo::new(&account(), &server.origin(), REQUEST_TIMEOUT, token).expect("client builds");

    // Act
    let result = octo.graphql(QUERY).await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let header = requests[0].header(AUTHORIZATION_HEADER);
    assert!(
        header.is_some_and(|value| value.contains(&planted_token())),
        "{header:?}"
    );
}

#[test]
fn the_default_base_uri_is_github_com() {
    // Arrange
    let token = Arc::new(FixedToken(planted_token()));

    // Act
    let octo = Octo::github_com(&account(), REQUEST_TIMEOUT, token).expect("client builds");

    // Assert
    assert_eq!(DEFAULT_BASE_URI, "https://api.github.com");
    assert_eq!(octo.base_uri(), DEFAULT_BASE_URI);
}

#[test]
fn every_endpoint_path_is_relative() {
    // Arrange
    let endpoints = ENDPOINTS;

    // Act
    let absolute: Vec<_> = endpoints
        .iter()
        .filter(|path| !path.starts_with('/') || path.contains("://"))
        .collect();

    // Assert
    assert!(!endpoints.is_empty(), "no endpoints are listed");
    assert!(absolute.is_empty(), "not relative: {absolute:?}");
}

#[tokio::test]
async fn a_401_refetches_the_token_once_and_retries_once() {
    // Arrange
    let server = TestServer::start(ScriptedResponse::json(
        401,
        r#"{"message":"Bad credentials"}"#,
    ));
    let tokens = Arc::new(CountingToken {
        fetches: AtomicUsize::new(0),
    });
    let octo = Octo::new(
        &account(),
        &server.origin(),
        REQUEST_TIMEOUT,
        tokens.clone(),
    )
    .expect("client builds");

    // Act
    let result = octo.graphql(QUERY).await;

    // Assert
    assert!(
        matches!(result, Err(GithubError::AuthRequired)),
        "{result:?}"
    );
    assert_eq!(tokens.fetches.load(Ordering::SeqCst), 2);
    assert_eq!(server.requests().len(), 2);
}

#[tokio::test]
async fn a_stalled_socket_times_out_within_the_slack() {
    // Arrange
    let stalled = StalledListener::start();
    let token = Arc::new(FixedToken(planted_token()));
    let octo = Octo::new(&account(), &stalled.origin, SHORT_TIMEOUT, token).expect("client builds");
    let started = Instant::now();

    // Act
    let result = octo.graphql(QUERY).await;
    let elapsed = started.elapsed();

    // Assert
    assert!(matches!(result, Err(GithubError::Timeout)), "{result:?}");
    assert!(elapsed < SHORT_TIMEOUT + TIMEOUT_SLACK, "took {elapsed:?}");
}

#[tokio::test]
async fn graphql_forbidden_without_saml_is_permission_denied() {
    // Arrange
    let body = graphql_error_with(
        "FORBIDDEN",
        "Resource not accessible by personal access token",
    );
    let response = ScriptedResponse::json(200, &body);

    // Act
    let result = call(response).await;

    // Assert
    assert!(
        matches!(result, Err(GithubError::PermissionDenied)),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_403_naming_a_secondary_rate_limit_is_rate_limited() {
    // Arrange
    let response = ScriptedResponse::json(
        403,
        r#"{"message":"You have exceeded a secondary rate limit. Please wait."}"#,
    );
    let before = SystemTime::now();

    // Act
    let result = call(response).await;

    // Assert
    let Err(GithubError::RateLimited { retry_at }) = result else {
        panic!("expected RateLimited, got {result:?}");
    };
    assert!(retry_at >= before + DEFAULT_RATE_LIMIT_BACKOFF);
}

#[tokio::test]
async fn an_enormous_retry_after_or_reset_does_not_panic() {
    for header in ["retry-after", "x-ratelimit-reset"] {
        // Arrange
        let response = ScriptedResponse::json(429, r#"{"message":"slow down"}"#)
            .with_header(header, &u64::MAX.to_string());

        // Act
        let result = call(response).await;

        // Assert
        assert!(
            matches!(result, Err(GithubError::RateLimited { .. })),
            "{header}: {result:?}"
        );
    }
}
