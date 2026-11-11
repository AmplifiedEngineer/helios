// SPDX-License-Identifier: Apache-2.0

//! Transport behaviour of the GitHub client against the std test server.

// The shared server has helpers this file does not use.
#[allow(dead_code)]
mod support;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use helios::accounts::{Account, AccountState};
use helios::github::errors::GithubError;
use helios::github::octo::{Octo, TokenSource};
use helios::secret::Secret;
use support::server::{RecordedRequest, ScriptedResponse, TestServer};

const QUERY: &str = "{ viewer { login } }";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const GRAPHQL_BODY: &str = r#"{"data":{"viewer":{"login":"octo"}}}"#;
const RESET_EPOCH_SECS: u64 = 4_000_000_000;
const RETRY_AFTER_SECS: u64 = 120;
const CLOCK_SLACK: Duration = Duration::from_secs(5);
const REDIRECT_STATUS: u16 = 302;

/// Planted token, built at runtime so no token-shaped literal sits in the source.
fn planted_token() -> String {
    format!("{}_{}", "tok", "planted")
}

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

/// Sends one raw request carrying a planted `Authorization` header.
fn send_with_authorization(server: &TestServer) {
    let mut stream = TcpStream::connect(server.addr()).expect("connect to listener");
    let request = format!(
        "POST /graphql HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\n\
         Content-Length: 0\r\nConnection: close\r\n\r\n",
        server.addr(),
        planted_token()
    );
    stream.write_all(request.as_bytes()).expect("send request");
    let mut reply = String::new();
    let _ = stream.read_to_string(&mut reply);
}

#[test]
fn recording_listener_captures_a_planted_authorization_header() {
    // Arrange
    let listener = TestServer::start(ScriptedResponse::json(200, GRAPHQL_BODY));

    // Act
    send_with_authorization(&listener);

    // Assert
    let recorded = listener.requests();
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].header("Authorization"),
        Some(format!("Bearer {}", planted_token()).as_str()),
        "the recorder must see Authorization, or the redirect assertions prove nothing"
    );
}

#[tokio::test]
async fn cross_origin_redirect_does_not_carry_authorization() {
    // Arrange
    let other_origin = TestServer::start(ScriptedResponse::json(200, GRAPHQL_BODY));
    let target = format!("{}/graphql", other_origin.origin());
    let redirecting = TestServer::start(
        ScriptedResponse::json(REDIRECT_STATUS, "{}").with_header("Location", &target),
    );
    let token = Arc::new(FixedToken(planted_token()));
    let octo = Octo::new(&account(), &redirecting.origin(), REQUEST_TIMEOUT, token)
        .expect("client builds");

    // Act
    let result = octo.graphql(QUERY).await;

    // Assert
    let first = redirecting.requests();
    assert_eq!(first.len(), 1, "the client reached the first listener once");
    assert!(
        first[0].header("Authorization").is_some(),
        "the first hop is authenticated"
    );
    let second = other_origin.requests();
    #[cfg(not(feature = "follow-redirect"))]
    {
        assert!(
            matches!(
                result,
                Err(GithubError::Api {
                    status: REDIRECT_STATUS,
                    ..
                })
            ),
            "{result:?}"
        );
        assert!(second.is_empty(), "no request reached the other origin");
    }
    #[cfg(feature = "follow-redirect")]
    {
        let _ = result;
        assert_eq!(
            second.len(),
            1,
            "exactly one request reached the other origin"
        );
        assert!(
            second[0].header("Authorization").is_none(),
            "Authorization must not cross origins"
        );
    }
}

fn octo_for(server: &TestServer) -> Octo {
    let token = Arc::new(FixedToken(planted_token()));
    Octo::new(&account(), &server.origin(), REQUEST_TIMEOUT, token).expect("client builds")
}

fn link_to(path: &str) -> String {
    format!("<{path}>; rel=\"next\"")
}

fn paths(requests: &[RecordedRequest]) -> Vec<&str> {
    requests.iter().map(|r| r.path.as_str()).collect()
}

/// Three pages chained by relative `Link` paths, the last without a next link.
fn paged_server() -> TestServer {
    TestServer::start_with(|request| match request.path.as_str() {
        "/items?page=1" => {
            ScriptedResponse::json(200, "[1,2]").with_header("Link", &link_to("/items?page=2"))
        }
        "/items?page=2" => {
            ScriptedResponse::json(200, "[3]").with_header("Link", &link_to("/items?page=3"))
        }
        "/items?page=3" => {
            ScriptedResponse::json(200, "[4]").with_header("Link", "</items?page=2>; rel=\"prev\"")
        }
        _ => ScriptedResponse::json(404, "{}"),
    })
}

#[tokio::test]
async fn pagination_follows_relative_link_paths_to_the_last_page() {
    // Arrange
    let server = paged_server();
    let octo = octo_for(&server);

    // Act
    let items = octo.get_pages("/items?page=1").await;

    // Assert
    let items = items.expect("all pages are read");
    let numbers: Vec<u64> = items.iter().filter_map(|v| v.as_u64()).collect();
    assert_eq!(numbers, vec![1, 2, 3, 4]);
    assert_eq!(
        paths(&server.requests()),
        vec!["/items?page=1", "/items?page=2", "/items?page=3"],
        "each next page is requested once, by its relative path, and the last page ends the walk"
    );
}

#[tokio::test]
async fn pagination_keeps_the_host_and_authorization_on_every_page() {
    // Arrange
    let server = paged_server();
    let octo = octo_for(&server);

    // Act
    let _ = octo.get_pages("/items?page=1").await;

    // Assert
    let requests = server.requests();
    assert_eq!(requests.len(), 3, "every page was requested");
    let expected_auth = format!("Bearer {}", planted_token());
    for request in &requests {
        assert_eq!(
            request.header("Host"),
            Some(server.addr().to_string().as_str())
        );
        assert_eq!(
            request.header("Authorization"),
            Some(expected_auth.as_str()),
            "page {} lost Authorization",
            request.path
        );
    }
}

#[tokio::test]
async fn a_single_page_without_a_next_link_makes_one_request() {
    // Arrange
    let server = TestServer::start(ScriptedResponse::json(200, "[7]"));
    let octo = octo_for(&server);

    // Act
    let items = octo.get_pages("/items?page=1").await;

    // Assert
    assert_eq!(items.expect("the page is read"), vec![serde_json::json!(7)]);
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn a_next_link_to_another_host_is_rejected_without_a_request_to_it() {
    for target_of in [
        |other: &TestServer| format!("{}/items?page=2", other.origin()),
        |other: &TestServer| format!("//{}/items?page=2", other.addr()),
    ] {
        // Arrange
        let other_host = TestServer::start(ScriptedResponse::json(200, "[2]"));
        let target = target_of(&other_host);
        let first = TestServer::start(
            ScriptedResponse::json(200, "[1]").with_header("Link", &link_to(&target)),
        );
        let octo = octo_for(&first);

        // Act
        let result = octo.get_pages("/items?page=1").await;

        // Assert
        assert!(
            matches!(result, Err(GithubError::Decode)),
            "{target}: {result:?}"
        );
        assert!(
            other_host.requests().is_empty(),
            "no request reached the other host for {target}"
        );
    }
}

async fn graphql_against(response: ScriptedResponse) -> Result<(), GithubError> {
    let server = TestServer::start(response);
    octo_for(&server).graphql(QUERY).await.map(|_| ())
}

#[tokio::test]
async fn a_403_with_remaining_quota_is_a_permission_error_even_with_a_reset_header() {
    // Arrange
    let response = ScriptedResponse::json(403, r#"{"message":"Resource not accessible"}"#)
        .with_header("x-ratelimit-remaining", "4999")
        .with_header("x-ratelimit-reset", &RESET_EPOCH_SECS.to_string());

    // Act
    let result = graphql_against(response).await;

    // Assert
    assert!(
        matches!(result, Err(GithubError::PermissionDenied)),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_403_with_retry_after_is_a_rate_limit_at_that_offset() {
    // Arrange
    let response = ScriptedResponse::json(403, r#"{"message":"secondary rate limit"}"#)
        .with_header("retry-after", &RETRY_AFTER_SECS.to_string());
    let before = SystemTime::now();

    // Act
    let result = graphql_against(response).await;

    // Assert
    let Err(GithubError::RateLimited { retry_at }) = result else {
        panic!("expected RateLimited, got {result:?}");
    };
    let earliest = before + Duration::from_secs(RETRY_AFTER_SECS);
    assert!(retry_at >= earliest && retry_at <= earliest + CLOCK_SLACK);
    assert!(retry_at < UNIX_EPOCH + Duration::from_secs(RESET_EPOCH_SECS));
}

#[tokio::test]
async fn a_403_with_the_sso_header_wins_over_rate_limit_headers() {
    // Arrange
    let response = ScriptedResponse::json(403, r#"{"message":"SAML enforcement"}"#)
        .with_header("x-github-sso", "required; url=https://github.test/sso")
        .with_header("x-ratelimit-remaining", "0")
        .with_header("retry-after", &RETRY_AFTER_SECS.to_string());

    // Act
    let result = graphql_against(response).await;

    // Assert
    assert!(matches!(result, Err(GithubError::SsoBlocked)), "{result:?}");
}
