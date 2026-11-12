// SPDX-License-Identifier: Apache-2.0

//! `Octo` as the `GithubApi`, against the std test server and recorded or synthetic fixtures.

// The shared server has helpers this file does not use.
#[allow(dead_code)]
mod support;

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::Duration;

use helios::accounts::{Account, AccountState};
use helios::github::errors::GithubError;
use helios::github::octo::{Octo, TokenSource, MAX_FILES, MAX_PAGES};
use helios::github::queries::{
    files_path, map_detail, map_files, map_page, ListKind, FILES_PER_PAGE,
};
use helios::github::GithubApi;
use helios::model::PrSummary;
use helios::secret::Secret;
use support::github::{planted_token, FixedToken};
use support::server::{ScriptedResponse, TestServer};

const HOST: &str = "github.test";
const LOGIN: &str = "octo";
const OWNER: &str = "octo";
const REPO: &str = "widgets";
const NUMBER: u64 = 7;
const FIRST_PAGE: usize = 1;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const NEXT_CURSOR: &str = "cursor-page-two";
const OK: u16 = 200;
const NOT_FOUND: u16 = 404;

/// Hands out one token, counts the asks and notes which thread served the last one.
struct WatchedToken {
    fetches: AtomicUsize,
    thread: Mutex<Option<ThreadId>>,
}

impl WatchedToken {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            fetches: AtomicUsize::new(0),
            thread: Mutex::new(None),
        })
    }
}

impl TokenSource for WatchedToken {
    fn fetch(&self) -> Result<Secret, GithubError> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        *self.thread.lock().expect("thread lock") = Some(std::thread::current().id());
        Ok(Secret::new(planted_token()))
    }
}

fn fixture(relative: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(relative),
    )
    .expect("read fixture")
}

fn octo(server: &TestServer) -> Octo {
    octo_with(server, Arc::new(FixedToken(planted_token())))
}

fn octo_with(server: &TestServer, token: Arc<dyn TokenSource>) -> Octo {
    let account = Account {
        host: HOST.into(),
        login: LOGIN.into(),
        active: true,
        scopes: vec![],
        state: AccountState::Success,
    };
    Octo::new(&account, &server.origin(), REQUEST_TIMEOUT, token).expect("build client")
}

fn rows(json: &str) -> Vec<PrSummary> {
    map_page(HOST, LOGIN, json).expect("map page").rows
}

/// The fixture page marked as having a next page after `NEXT_CURSOR`.
fn with_next_page(json: &str) -> String {
    let mut value: serde_json::Value = serde_json::from_str(json).expect("fixture json");
    value["data"]["search"]["pageInfo"] =
        serde_json::json!({ "hasNextPage": true, "endCursor": NEXT_CURSOR });
    value.to_string()
}

#[tokio::test]
async fn octo_list_returns_mapped_pull_requests() {
    // Arrange
    let first = with_next_page(&fixture("list/synthetic-review.json"));
    let second = fixture("list/synthetic-assigned.json");
    let mut expected = rows(&first);
    expected.extend(rows(&second));
    let server = TestServer::start_with(move |request| {
        let body = if request.body.contains(NEXT_CURSOR) {
            &second
        } else {
            &first
        };
        ScriptedResponse::json(OK, body)
    });
    let api = octo(&server);

    // Act
    let listed = api.list(ListKind::Mine).await.expect("list");

    // Assert
    assert_eq!(listed.rows, expected);
    assert!(!listed.partial, "a whole list is not partial");
    assert_eq!(server.requests().len(), 2, "follows the cursor to page two");
    assert!(server.requests()[0].path.ends_with("/graphql"));
    assert!(server.requests()[0].body.contains(ListKind::Mine.search()));
}

#[tokio::test]
async fn octo_list_builds_one_client_for_every_page() {
    // Arrange
    let first = with_next_page(&fixture("list/synthetic-review.json"));
    let second = fixture("list/synthetic-assigned.json");
    let server = TestServer::start_with(move |request| {
        let body = if request.body.contains(NEXT_CURSOR) {
            &second
        } else {
            &first
        };
        ScriptedResponse::json(OK, body)
    });
    let token = WatchedToken::new();
    let api = octo_with(&server, token.clone());

    // Act
    api.list(ListKind::Mine).await.expect("list");

    // Assert
    assert_eq!(server.requests().len(), 2, "the list has two pages");
    assert_eq!(token.fetches.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn octo_fetches_the_token_off_the_async_thread() {
    // Arrange
    let body = fixture("list/synthetic-review.json");
    let server = TestServer::start(ScriptedResponse::json(OK, &body));
    let token = WatchedToken::new();
    let api = octo_with(&server, token.clone());

    // Act
    api.list(ListKind::Mine).await.expect("list");

    // Assert
    let served_on = token.thread.lock().expect("thread lock").expect("fetched");
    assert_ne!(served_on, std::thread::current().id());
}

#[tokio::test]
async fn octo_list_marks_a_page_with_graphql_errors_partial() {
    // Arrange
    let mut value: serde_json::Value =
        serde_json::from_str(&fixture("list/synthetic-review.json")).expect("fixture json");
    value["errors"] = serde_json::json!([{ "message": "a node failed" }]);
    let server = TestServer::start(ScriptedResponse::json(OK, &value.to_string()));
    let api = octo(&server);

    // Act
    let listed = api.list(ListKind::Review).await.expect("list");

    // Assert
    assert!(listed.partial, "the cut list carried no partial flag");
    assert!(!listed.rows.is_empty(), "the rows that arrived are kept");
}

#[tokio::test]
async fn octo_detail_returns_the_mapped_detail() {
    // Arrange
    let body = fixture("detail/recorded-public-pr.json");
    let expected = map_detail(&body).expect("map detail");
    let server = TestServer::start(ScriptedResponse::json(OK, &body));
    let api = octo(&server);

    // Act
    let detail = api.detail(OWNER, REPO, NUMBER).await.expect("detail");

    // Assert
    assert_eq!(detail, expected);
    let sent = &server.requests()[0];
    assert!(sent.path.ends_with("/graphql"));
    assert!(sent.body.contains(REPO) && sent.body.contains(&NUMBER.to_string()));
}

#[tokio::test]
async fn octo_files_returns_the_mapped_files() {
    // Arrange
    let body = fixture("detail/files-synthetic.json");
    let expected = map_files(&body).expect("map files");
    let wanted = files_path(OWNER, REPO, NUMBER, FIRST_PAGE);
    let server = TestServer::start_with(move |request| {
        if request.path == wanted {
            ScriptedResponse::json(OK, &body)
        } else {
            ScriptedResponse::json(NOT_FOUND, "{}")
        }
    });
    let api = octo(&server);

    // Act
    let listing = api.files(OWNER, REPO, NUMBER).await.expect("files");

    // Assert
    assert_eq!(listing.files, expected);
    assert!(!listing.truncated);
}

/// One page of `FILES_PER_PAGE` distinct files, linked to the next page unless `last`.
fn files_page_response(page: usize, last: bool) -> ScriptedResponse {
    let nodes: Vec<serde_json::Value> = (0..FILES_PER_PAGE)
        .map(|index| {
            serde_json::json!({
                "filename": format!("dir/{page}/file-{index}.rs"),
                "previous_filename": null,
                "status": "modified",
                "changes": 1,
                "blob_url": "https://github.com/o/r/blob/a/f",
                "patch": "@@ -1 +1 @@\n-a\n+b"
            })
        })
        .collect();
    let response = ScriptedResponse::json(OK, &serde_json::Value::Array(nodes).to_string());
    if last {
        response
    } else {
        let next = files_path(OWNER, REPO, NUMBER, page + 1);
        response.with_header("Link", &format!("<{next}>; rel=\"next\""))
    }
}

/// A server that serves `pages` pages of files, linking each to the next except the last.
fn files_server(pages: usize) -> TestServer {
    TestServer::start_with(move |request| {
        let page = (FIRST_PAGE..=pages)
            .find(|page| request.path == files_path(OWNER, REPO, NUMBER, *page));
        match page {
            Some(page) => files_page_response(page, page == pages),
            None => ScriptedResponse::json(NOT_FOUND, "{}"),
        }
    })
}

#[tokio::test]
async fn octo_files_of_more_than_the_cap_returns_the_cap_and_marks_it_cut() {
    // Arrange
    let server = files_server(MAX_PAGES + 1);
    let api = octo(&server);

    // Act
    let listing = api.files(OWNER, REPO, NUMBER).await.expect("files");

    // Assert
    assert_eq!(listing.files.len(), MAX_FILES);
    assert!(listing.truncated);
    assert_eq!(
        server.requests().len(),
        MAX_PAGES,
        "no request past the cap"
    );
}

#[tokio::test]
async fn octo_files_of_exactly_the_cap_is_not_marked_cut() {
    // Arrange
    let server = files_server(MAX_PAGES);
    let api = octo(&server);

    // Act
    let listing = api.files(OWNER, REPO, NUMBER).await.expect("files");

    // Assert
    assert_eq!(listing.files.len(), MAX_FILES);
    assert!(!listing.truncated);
    assert_eq!(server.requests().len(), MAX_PAGES);
}

#[tokio::test]
async fn octo_detail_of_a_hidden_or_missing_pull_request_is_not_found() {
    for data in [
        r#"{"repository":null}"#,
        r#"{"repository":{"pullRequest":null}}"#,
    ] {
        // Arrange
        let body = format!(
            r#"{{"data":{data},"errors":[{{"type":"NOT_FOUND","message":"Could not resolve"}}]}}"#
        );
        let server = TestServer::start(ScriptedResponse::json(OK, &body));
        let api = octo(&server);

        // Act
        let result = api.detail(OWNER, REPO, NUMBER).await;

        // Assert
        assert!(
            matches!(result, Err(GithubError::NotFoundOrHidden)),
            "{data}: {result:?}"
        );
    }
}

#[test]
fn api_base_picks_the_endpoint_for_each_kind_of_host() {
    // Arrange
    let cases = [
        ("github.com", "https://api.github.com"),
        ("acme.ghe.com", "https://api.acme.ghe.com"),
        ("ghe.example.com", "https://ghe.example.com/api/v3"),
    ];

    for (host, expected) in cases {
        // Act
        let base = helios::github::octo::api_base(host);

        // Assert
        assert_eq!(base, expected, "{host}");
    }
}
