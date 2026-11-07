// SPDX-License-Identifier: Apache-2.0

use forgebird::model::{CheckState, PrKey, PrSummary, ReviewDecision, Tag};
use forgebird::remote::Remote;
use forgebird::sanitize::sanitize;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn key(host: &str, account: &str, repo: &str, number: u64) -> PrKey {
    PrKey {
        host: host.into(),
        account: account.into(),
        repo: repo.into(),
        number,
    }
}

fn hash_of(key: &PrKey) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

fn takes_remote(text: &Remote) -> &str {
    text.as_str()
}

#[test]
fn same_repo_and_number_on_two_hosts_are_distinct_keys() {
    // Arrange
    let github = key("github.com", "octo", "org/repo", 7);
    let enterprise = key("ghe.example.com", "octo", "org/repo", 7);

    // Act
    let equal = github == enterprise;

    // Assert
    assert!(!equal);
}

#[test]
fn same_repo_and_number_on_two_accounts_of_one_host_are_distinct_keys() {
    // Arrange
    let first = key("github.com", "octo", "org/repo", 7);
    let second = key("github.com", "other", "org/repo", 7);

    // Act
    let equal = first == second;

    // Assert
    assert!(!equal);
}

#[test]
fn equal_keys_hash_equal() {
    // Arrange
    let first = key("github.com", "octo", "org/repo", 7);
    let second = key("github.com", "octo", "org/repo", 7);

    // Act
    let equal = first == second;

    // Assert
    assert!(equal);
    assert_eq!(hash_of(&first), hash_of(&second));
    assert_ne!(Tag::Review, Tag::Assigned);
}

#[test]
fn known_review_decisions_map() {
    // Arrange
    let raws = ["APPROVED", "CHANGES_REQUESTED", "REVIEW_REQUIRED"];

    // Act
    let mapped = raws.map(ReviewDecision::from_raw);

    // Assert
    assert!(matches!(mapped[0], ReviewDecision::Approved));
    assert!(matches!(mapped[1], ReviewDecision::ChangesRequested));
    assert!(matches!(mapped[2], ReviewDecision::ReviewRequired));
}

#[test]
fn unknown_review_decision_keeps_the_raw_value() {
    // Arrange
    let raw = "SOMETHING_NEW";

    // Act
    let decision = ReviewDecision::from_raw(raw);

    // Assert
    match decision {
        ReviewDecision::Unknown(value) => assert_eq!(value.as_str(), raw),
        other => panic!("expected Unknown, got {other:?}"),
    }
}

#[test]
fn known_check_states_map() {
    // Arrange
    let raws = ["SUCCESS", "FAILURE", "ERROR", "PENDING", "EXPECTED"];

    // Act
    let mapped = raws.map(CheckState::from_raw);

    // Assert
    assert!(matches!(mapped[0], CheckState::Success));
    assert!(matches!(mapped[1], CheckState::Failure));
    assert!(matches!(mapped[2], CheckState::Error));
    assert!(matches!(mapped[3], CheckState::Pending));
    assert!(matches!(mapped[4], CheckState::Expected));
}

#[test]
fn unknown_check_state_keeps_the_raw_value() {
    // Arrange
    let raw = "SOMETHING_NEW";

    // Act
    let state = CheckState::from_raw(raw);

    // Assert
    match state {
        CheckState::Unknown(value) => assert_eq!(value.as_str(), raw),
        other => panic!("expected Unknown, got {other:?}"),
    }
}

#[test]
fn a_pr_summary_title_is_remote() {
    // Arrange
    let summary = PrSummary {
        key: key("github.com", "octo", "org/repo", 7),
        title: sanitize("Fix the thing"),
        author: sanitize("octocat"),
        tags: vec![Tag::Review],
        review_decision: None,
        check_state: None,
    };

    // Act
    let title = takes_remote(&summary.title);

    // Assert
    assert_eq!(title, "Fix the thing");
}

#[test]
fn unknown_value_is_sanitized() {
    // Arrange
    let raw = "BAD\u{1b}[31mVALUE";

    // Act
    let decision = ReviewDecision::from_raw(raw);
    let state = CheckState::from_raw(raw);

    // Assert
    match decision {
        ReviewDecision::Unknown(value) => assert!(!value.as_str().contains('\u{1b}')),
        other => panic!("expected Unknown, got {other:?}"),
    }
    match state {
        CheckState::Unknown(value) => assert!(!value.as_str().contains('\u{1b}')),
        other => panic!("expected Unknown, got {other:?}"),
    }
}
