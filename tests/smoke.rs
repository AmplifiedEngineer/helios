// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use chrono::{TimeZone, Utc};
use helios::cli::{self, Config};
use helios::github::errors::GithubError;
use helios::github::queries::ListKind;
use helios::github::GithubApi;
use helios::model::{ChangedFile, PrDetail, PrKey, PrSummary};
use helios::sanitize::sanitize;
use helios::smoke::{self, SmokeAccount};

const ACCOUNT: &str = "octo";
const REPO: &str = "org/repo";
const HOST_A: &str = "github.com";
const HOST_B: &str = "ghe.example.com";
const EXIT_FAILURE: i32 = 1;
const EXIT_SUCCESS: i32 = 0;

/// Answers each list with fixed PR numbers, or fails every call.
#[derive(Default)]
struct FakeApi {
    host: String,
    mine: Vec<u64>,
    drafts_in_mine: Vec<u64>,
    review: Vec<u64>,
    assigned: Vec<u64>,
    failure: bool,
}

impl FakeApi {
    fn rows(&self, numbers: &[u64], is_draft: bool) -> Vec<PrSummary> {
        numbers
            .iter()
            .map(|number| PrSummary {
                key: PrKey {
                    host: self.host.clone(),
                    account: ACCOUNT.into(),
                    repo: REPO.into(),
                    number: *number,
                },
                title: sanitize("title"),
                author: sanitize("author"),
                is_draft,
                updated_at: Utc.timestamp_opt(0, 0).unwrap(),
                tags: Vec::new(),
                review_decision: None,
                check_state: None,
            })
            .collect()
    }
}

impl GithubApi for FakeApi {
    async fn list(&self, kind: ListKind) -> Result<Vec<PrSummary>, GithubError> {
        if self.failure {
            return Err(GithubError::Network);
        }
        Ok(match kind {
            ListKind::Mine => {
                let mut rows = self.rows(&self.mine, false);
                rows.extend(self.rows(&self.drafts_in_mine, true));
                rows
            }
            ListKind::Review => self.rows(&self.review, false),
            ListKind::Assigned => self.rows(&self.assigned, false),
            _ => Vec::new(),
        })
    }

    async fn detail(&self, _: &str, _: &str, _: u64) -> Result<PrDetail, GithubError> {
        Err(GithubError::NotFoundOrHidden)
    }

    async fn files(&self, _: &str, _: &str, _: u64) -> Result<Vec<ChangedFile>, GithubError> {
        Err(GithubError::NotFoundOrHidden)
    }
}

fn account(host: &str, api: FakeApi) -> SmokeAccount<FakeApi> {
    SmokeAccount {
        host: host.into(),
        account: ACCOUNT.into(),
        api: FakeApi {
            host: host.into(),
            ..api
        },
    }
}

fn failing(host: &str) -> SmokeAccount<FakeApi> {
    account(
        host,
        FakeApi {
            failure: true,
            ..FakeApi::default()
        },
    )
}

fn config() -> Config {
    cli::parse(&[], &HashMap::new()).expect("defaults parse")
}

#[tokio::test]
async fn smoke_prints_the_mine_and_needs_me_counts() {
    // Arrange
    // Host A: 2 mine plus a hidden draft, needs me = {1, 2, 3} after the overlap merges.
    // Host B: 1 mine, needs me = {1}. Totals: Mine 3, Needs me 4.
    let accounts = [
        account(
            HOST_A,
            FakeApi {
                mine: vec![10, 11],
                drafts_in_mine: vec![12],
                review: vec![1, 2],
                assigned: vec![2, 3],
                ..FakeApi::default()
            },
        ),
        account(
            HOST_B,
            FakeApi {
                mine: vec![20],
                review: vec![1],
                ..FakeApi::default()
            },
        ),
    ];

    // Act
    let outcome = smoke::run(&accounts, &config()).await;

    // Assert
    let lines: Vec<&str> = outcome.text.lines().collect();
    assert!(lines.contains(&"Mine: 3"), "got {:?}", outcome.text);
    assert!(lines.contains(&"Needs me: 4"), "got {:?}", outcome.text);
    assert_eq!(outcome.exit_code, EXIT_SUCCESS);
}

#[tokio::test]
async fn smoke_reports_a_failing_host_and_counts_the_other() {
    // Arrange
    let accounts = [
        account(
            HOST_A,
            FakeApi {
                mine: vec![10, 11],
                review: vec![1],
                ..FakeApi::default()
            },
        ),
        failing(HOST_B),
    ];

    // Act
    let outcome = smoke::run(&accounts, &config()).await;

    // Assert
    let lines: Vec<&str> = outcome.text.lines().collect();
    assert!(lines.contains(&"Mine: 2"), "got {:?}", outcome.text);
    assert!(lines.contains(&"Needs me: 1"), "got {:?}", outcome.text);
    assert!(
        lines.contains(&format!("Host failed: {HOST_B}").as_str()),
        "got {:?}",
        outcome.text
    );
    assert_eq!(outcome.exit_code, EXIT_SUCCESS);
}

#[tokio::test]
async fn smoke_exits_with_failure_when_every_host_fails() {
    // Arrange
    let accounts = [failing(HOST_A), failing(HOST_B)];

    // Act
    let outcome = smoke::run(&accounts, &config()).await;

    // Assert
    assert!(outcome.text.contains(&format!("Host failed: {HOST_A}")));
    assert!(outcome.text.contains(&format!("Host failed: {HOST_B}")));
    assert_eq!(outcome.exit_code, EXIT_FAILURE);
}

#[tokio::test]
async fn smoke_strips_escapes_from_a_host_name() {
    // Arrange
    let accounts = [failing("evil\u{1b}[31m.example.com")];

    // Act
    let outcome = smoke::run(&accounts, &config()).await;

    // Assert
    assert!(!outcome.text.contains('\u{1b}'), "got {:?}", outcome.text);
    assert!(outcome.text.contains("Host failed:"));
}

#[tokio::test]
async fn smoke_with_no_accounts_exits_with_failure() {
    // Arrange
    let accounts: [SmokeAccount<FakeApi>; 0] = [];

    // Act
    let outcome = smoke::run(&accounts, &config()).await;

    // Assert
    assert!(
        outcome.text.lines().any(|line| line == "no accounts found"),
        "got {:?}",
        outcome.text
    );
    assert_eq!(outcome.exit_code, EXIT_FAILURE);
}
