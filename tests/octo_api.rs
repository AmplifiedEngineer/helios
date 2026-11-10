// SPDX-License-Identifier: Apache-2.0

//! `Octo` as the `GithubApi`, against the std test server and recorded or synthetic fixtures.

// The shared server has helpers this file does not use.
#[allow(dead_code)]
mod support;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use helios::accounts::{Account, AccountState};
use helios::github::errors::GithubError;
use helios::github::octo::{Octo, TokenSource};
use helios::github::queries::{files_path, map_detail, map_files, map_page, ListKind};
use helios::github::GithubApi;
use helios::model::PrSummary;
use helios::secret::Secret;
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

struct FixedToken;

impl TokenSource for FixedToken {
    fn fetch(&self) -> Result<Secret, GithubError> {
        Ok(Secret::new("planted".to_string()))
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
    let account = Account {
        host: HOST.into(),
        login: LOGIN.into(),
        active: true,
        scopes: vec![],
        state: AccountState::Success,
    };
    Octo::new(
        &account,
        &server.origin(),
        REQUEST_TIMEOUT,
        Arc::new(FixedToken),
    )
    .expect("build client")
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
    assert_eq!(listed, expected);
    assert_eq!(server.requests().len(), 2, "follows the cursor to page two");
    assert!(server.requests()[0].path.ends_with("/graphql"));
    assert!(server.requests()[0].body.contains(ListKind::Mine.search()));
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
    let files = api.files(OWNER, REPO, NUMBER).await.expect("files");

    // Assert
    assert_eq!(files, expected);
}
