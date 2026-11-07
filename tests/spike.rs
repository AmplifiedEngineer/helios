// SPDX-License-Identifier: Apache-2.0

//! Characterisation of octocrab 0.54.9 behaviour that the client design relies on.

mod support;

use std::io::{Read, Write};
use std::net::TcpStream;

use octocrab::service::middleware::retry::RetryConfig;
use octocrab::Octocrab;
use serde_json::json;
use support::server::{ScriptedResponse, TestServer};

const TOKEN: &str = "ghp_planted_spike_token";

fn client_with(server: &TestServer, retry: RetryConfig) -> Octocrab {
    // GHES-style base URI: the API lives under /api/v3/.
    Octocrab::builder()
        .base_uri(format!("{}/api/v3/", server.origin()))
        .expect("valid base uri")
        .personal_token(TOKEN.to_string())
        .add_retry_config(retry)
        .build()
        .expect("client builds")
}

fn client_for(server: &TestServer) -> Octocrab {
    client_with(server, RetryConfig::None)
}

fn viewer_query() -> serde_json::Value {
    json!({ "query": "{ viewer { login } }" })
}

fn ok_json() -> ScriptedResponse {
    ScriptedResponse::json(200, r#"{"ok":true}"#)
}

#[tokio::test]
async fn relative_path_carries_authorization_to_the_ghes_base_uri() {
    // Arrange
    let server = TestServer::start(ok_json());
    let client = client_for(&server);

    // Act
    let response = client._get("/user").await.expect("request succeeds");

    // Assert
    assert_eq!(response.status(), 200);
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/api/v3/user");
    assert_eq!(
        requests[0].header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
}

#[tokio::test]
async fn absolute_url_to_another_host_carries_no_authorization() {
    // Arrange
    let ghes = TestServer::start(ok_json());
    let other = TestServer::start(ok_json());
    let client = client_for(&ghes);

    // Act
    let response = client
        ._get(format!("{}/elsewhere", other.origin()))
        .await
        .expect("request succeeds");

    // Assert
    assert_eq!(response.status(), 200);
    let requests = other.requests();
    assert_eq!(requests.len(), 1);
    // The base URI path prefix is also applied to absolute URLs.
    assert_eq!(requests[0].path, "/api/v3/elsewhere");
    assert_eq!(requests[0].header("authorization"), None);
    assert!(ghes.requests().is_empty());
}

// Positive control: proves the server records an Authorization header, so the
// "no authorization" assertions above cannot pass vacuously.
#[test]
fn planted_authorization_sent_straight_to_a_listener_is_recorded() {
    // Arrange
    let server = TestServer::start(ok_json());

    // Act
    let mut stream = TcpStream::connect(server.addr()).expect("connect");
    write!(
        stream,
        "GET /control HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {TOKEN}\r\nConnection: close\r\n\r\n",
        server.addr()
    )
    .expect("write request");
    let mut reply = String::new();
    stream.read_to_string(&mut reply).expect("read reply");

    // Assert
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
}

#[tokio::test]
async fn raw_post_graphql_succeeds() {
    // Arrange
    let scripted = r#"{"data":{"viewer":{"login":"octo"}},"errors":[{"message":"partial"}]}"#;
    let server = TestServer::start(ScriptedResponse::json(200, scripted));
    let client = client_for(&server);
    let query = viewer_query();

    // Act
    let value: serde_json::Value = client
        .post("/graphql", Some(&query))
        .await
        .expect("raw post succeeds");

    // Assert
    assert_eq!(value["data"]["viewer"]["login"], "octo");
    assert_eq!(value["errors"][0]["message"], "partial");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert!(requests[0].body.contains("viewer"));
}

// Pinned for octocrab 0.54.9. The exposing method is `_post`: `post`
// deserialises the body and drops the headers, so they are read from the raw
// response `_post` returns.
#[tokio::test]
async fn ratelimit_remaining_is_exposed_on_raw_post() {
    // Arrange
    let server = TestServer::start(ok_json().with_header("x-ratelimit-remaining", "4321"));
    let client = client_for(&server);
    let query = viewer_query();

    // Act
    let response = client
        ._post("/graphql", Some(&query))
        .await
        .expect("raw post succeeds");

    // Assert
    assert_eq!(server.requests().len(), 1);
    assert_eq!(
        response
            .headers()
            .get("x-ratelimit-remaining")
            .map(|v| v.to_str().unwrap()),
        Some("4321")
    );
    let body = client.body_to_string(response).await.expect("body");
    assert_eq!(body, r#"{"ok":true}"#);
}

// Pinned for octocrab 0.54.9. Exposed by `_post`, like the rate limit header
// above.
#[tokio::test]
async fn retry_after_is_exposed_on_raw_post() {
    // Arrange
    let server = TestServer::start(
        ScriptedResponse::json(429, r#"{"message":"secondary rate limit"}"#)
            .with_header("retry-after", "17"),
    );
    let client = client_for(&server);
    let query = viewer_query();

    // Act
    let response = client
        ._post("/graphql", Some(&query))
        .await
        .expect("raw post returns the 429 response");

    // Assert
    assert_eq!(server.requests().len(), 1);
    assert_eq!(response.status(), 429);
    assert_eq!(
        response
            .headers()
            .get("retry-after")
            .map(|v| v.to_str().unwrap()),
        Some("17")
    );
}

// The default `RetryConfig::Simple(3)` resends a 429 three
// times, so a caller that owns its retry policy must opt out.
#[tokio::test]
async fn default_client_resends_a_429_three_times() {
    // Arrange
    let server = TestServer::start(
        ScriptedResponse::json(429, r#"{"message":"secondary rate limit"}"#)
            .with_header("retry-after", "17"),
    );
    let client = client_with(&server, RetryConfig::Simple(3));
    let query = viewer_query();

    // Act
    let response = client
        ._post("/graphql", Some(&query))
        .await
        .expect("raw post returns the 429 response");

    // Assert
    assert_eq!(response.status(), 429);
    assert_eq!(server.requests().len(), 4);
}

fn redirect_to(location: &str) -> ScriptedResponse {
    ScriptedResponse::json(302, "{}").with_header("Location", location)
}

// Runs only with the `follow-redirect` feature, which octocrab enables by default.
#[cfg(feature = "follow-redirect")]
#[tokio::test]
async fn cross_origin_redirect_is_followed_without_authorization() {
    // Arrange
    let other = TestServer::start(ok_json());
    let ghes = TestServer::start(redirect_to(&format!("{}/landing", other.origin())));
    let client = client_for(&ghes);

    // Act
    let _ = client._get("/user").await;

    // Assert
    assert_eq!(ghes.requests().len(), 1);
    let requests = other.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].header("authorization"), None);
}

// Without redirect following the 3xx must surface to the
// caller and nothing may reach the second origin.
#[cfg(not(feature = "follow-redirect"))]
#[tokio::test]
async fn cross_origin_redirect_is_not_followed() {
    // Arrange
    let other = TestServer::start(ok_json());
    let ghes = TestServer::start(redirect_to(&format!("{}/landing", other.origin())));
    let client = client_for(&ghes);

    // Act
    let result = client._get("/user").await;

    // Assert
    assert_eq!(ghes.requests().len(), 1);
    match result {
        Ok(response) => assert!(response.status().is_redirection()),
        Err(error) => assert!(format!("{error:?}").contains("302")),
    }
    assert!(other.requests().is_empty());
}
