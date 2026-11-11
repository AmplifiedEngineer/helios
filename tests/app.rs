// SPDX-License-Identifier: Apache-2.0

use chrono::{TimeZone, Utc};
use helios::app::worker::{channel, Loaded};
use helios::app::{App, DetailState, LoadRequest};
use helios::github::errors::GithubError;
use helios::model::{PrKey, PrSummary};
use helios::sanitize::sanitize;

const HOST: &str = "github.com";
const ACCOUNT: &str = "octo";
const REPO: &str = "org/repo";

fn summary(number: u64) -> PrSummary {
    PrSummary {
        key: PrKey {
            host: HOST.into(),
            account: ACCOUNT.into(),
            repo: REPO.into(),
            number,
        },
        title: sanitize("title"),
        author: sanitize("author"),
        is_draft: false,
        updated_at: Utc.timestamp_opt(0, 0).unwrap(),
        tags: Vec::new(),
        review_decision: None,
        check_state: None,
    }
}

fn loaded(generation: u64, numbers: &[u64]) -> Loaded {
    Loaded {
        generation,
        partial: false,
        host: HOST.into(),
        account: ACCOUNT.into(),
        result: Ok(numbers.iter().map(|n| summary(*n)).collect()),
    }
}

fn numbers(app: &App) -> Vec<u64> {
    app.rows().iter().map(|row| row.key.number).collect()
}

#[test]
fn a_result_with_an_old_generation_is_dropped() {
    // Arrange
    let mut app = App::new();
    let old = app.begin_refresh();
    let current = app.begin_refresh();
    let (sender, receiver) = channel();
    sender.send(loaded(current, &[2])).unwrap();
    sender.send(loaded(old, &[1])).unwrap();

    // Act
    let applied: Vec<bool> = receiver.try_iter().map(|msg| app.apply(msg)).collect();

    // Assert
    assert_eq!(applied, vec![true, false]);
    assert_eq!(numbers(&app), vec![2]);
}

fn key(number: u64) -> PrKey {
    summary(number).key
}

fn app_with_rows(numbers: &[u64]) -> App {
    let mut app = App::new();
    let generation = app.begin_refresh();
    app.apply(loaded(generation, numbers));
    app
}

#[test]
fn the_cursor_stays_on_the_same_pr_when_a_refresh_reorders_rows() {
    // Arrange
    let mut app = app_with_rows(&[1, 2, 3]);
    app.select(key(3));
    let generation = app.begin_refresh();

    // Act
    app.apply(loaded(generation, &[3, 1, 2]));

    // Assert
    assert_eq!(app.cursor(), Some(&key(3)));
}

#[test]
fn the_cursor_moves_to_the_nearest_row_when_the_selected_pr_is_gone() {
    // Arrange
    let mut app = app_with_rows(&[1, 2, 3]);
    app.select(key(2));
    let generation = app.begin_refresh();

    // Act
    app.apply(loaded(generation, &[1, 3]));

    // Assert
    assert_eq!(app.cursor(), Some(&key(3)));
}

#[test]
fn a_vanished_pr_keeps_its_row_until_the_next_list_refresh() {
    // Arrange
    let mut app = app_with_rows(&[1, 2]);
    app.select(key(2));

    // Act
    app.apply_detail(&key(2), Err(GithubError::NotFoundOrHidden));

    // Assert
    assert_eq!(numbers(&app), vec![1, 2]);
    assert!(matches!(
        app.detail(),
        DetailState::Failed(GithubError::NotFoundOrHidden)
    ));

    // Act
    let generation = app.begin_refresh();
    app.apply(loaded(generation, &[1]));

    // Assert
    assert_eq!(numbers(&app), vec![1]);
}

const OTHER_HOST: &str = "ghe.example.com";
const OTHER_ACCOUNT: &str = "hubot";

fn summary_for(host: &str, account: &str, number: u64) -> PrSummary {
    let mut row = summary(number);
    row.key.host = host.into();
    row.key.account = account.into();
    row
}

fn loaded_for(generation: u64, host: &str, account: &str, numbers: &[u64]) -> Loaded {
    Loaded {
        generation,
        partial: false,
        host: host.into(),
        account: account.into(),
        result: Ok(numbers
            .iter()
            .map(|n| summary_for(host, account, *n))
            .collect()),
    }
}

#[test]
fn two_accounts_loaded_in_one_refresh_both_keep_their_rows() {
    // Arrange
    let mut app = App::new();
    let generation = app.begin_refresh();

    // Act
    app.apply(loaded_for(generation, HOST, ACCOUNT, &[1, 2]));
    app.apply(loaded_for(generation, OTHER_HOST, OTHER_ACCOUNT, &[3]));

    // Assert
    let mut found: Vec<(String, u64)> = app
        .rows()
        .iter()
        .map(|row| (row.key.host.clone(), row.key.number))
        .collect();
    found.sort();
    assert_eq!(
        found,
        vec![
            (OTHER_HOST.to_string(), 3),
            (HOST.to_string(), 1),
            (HOST.to_string(), 2)
        ]
    );
}

#[test]
fn a_later_load_replaces_only_its_own_accounts_rows() {
    // Arrange
    let mut app = App::new();
    let first = app.begin_refresh();
    app.apply(loaded_for(first, HOST, ACCOUNT, &[1, 2]));
    app.apply(loaded_for(first, OTHER_HOST, OTHER_ACCOUNT, &[3]));
    let second = app.begin_refresh();

    // Act
    app.apply(loaded_for(second, HOST, ACCOUNT, &[2]));

    // Assert
    let mut numbers: Vec<u64> = app.rows().iter().map(|row| row.key.number).collect();
    numbers.sort_unstable();
    assert_eq!(numbers, vec![2, 3]);
}

#[test]
fn a_good_load_clears_only_its_own_accounts_failure() {
    // Arrange
    let mut app = App::new();
    let generation = app.begin_refresh();
    for account in [ACCOUNT, OTHER_ACCOUNT] {
        app.apply(Loaded {
            generation,
            partial: false,
            host: HOST.into(),
            account: account.into(),
            result: Err(GithubError::Network),
        });
    }

    // Act
    app.apply(loaded_for(generation, HOST, ACCOUNT, &[1]));

    // Assert
    let failed: Vec<&str> = app
        .host_failures()
        .keys()
        .map(|(_, account)| account.as_str())
        .collect();
    assert_eq!(failed, vec![OTHER_ACCOUNT]);
}

#[test]
fn a_host_that_failed_and_then_loaded_shows_no_failure() {
    // Arrange
    let mut app = App::new();
    let generation = app.begin_refresh();
    app.apply(Loaded {
        generation,
        partial: false,
        host: HOST.into(),
        account: ACCOUNT.into(),
        result: Err(GithubError::Network),
    });

    // Act
    let next = app.begin_refresh();
    app.apply(loaded_for(next, HOST, ACCOUNT, &[1]));

    // Assert
    assert!(app.host_failures().is_empty());
}

#[test]
fn a_selection_that_moves_on_refresh_drops_the_old_detail_and_files() {
    // Arrange
    let mut app = app_with_rows(&[1, 2, 3]);
    app.select(key(2));
    app.apply_detail(&key(2), Err(GithubError::Network));
    app.apply_files(&key(2), Vec::new());
    let generation = app.begin_refresh();

    // Act
    app.apply(loaded(generation, &[1, 3]));

    // Assert
    assert!(matches!(app.detail(), DetailState::Loading));
    assert!(app.files().is_none());
}

#[test]
fn a_detail_result_for_a_pr_that_is_no_longer_selected_is_dropped() {
    // Arrange
    let mut app = app_with_rows(&[1, 2]);
    app.select(key(1));
    app.select(key(2));

    // Act
    app.apply_detail(&key(1), Err(GithubError::Network));
    app.apply_files(&key(1), Vec::new());

    // Assert
    assert!(matches!(app.detail(), DetailState::Loading));
    assert!(app.files().is_none());
}

#[test]
fn taking_the_requests_hands_them_over_once() {
    // Arrange
    let mut app = app_with_rows(&[1, 2]);
    app.open(key(1));
    app.open(key(2));

    // Act
    let taken = app.take_requests();
    let again = app.take_requests();

    // Assert
    assert_eq!(
        taken,
        vec![LoadRequest::Detail(key(1)), LoadRequest::Detail(key(2))]
    );
    assert!(again.is_empty(), "a request ran twice: {again:?}");
    assert!(app.requests().is_empty());
}

#[test]
fn visible_rows_apply_the_draft_and_tag_rules() {
    // Arrange
    let mut app = App::new();
    let generation = app.begin_refresh();
    let mut draft = summary(1);
    draft.is_draft = true;
    let mut reviewed = summary(2);
    reviewed.tags = vec![helios::model::Tag::Review];
    let assigned = {
        let mut row = summary(3);
        row.tags = vec![helios::model::Tag::Assigned];
        row
    };
    app.apply(Loaded {
        generation,
        partial: false,
        host: HOST.into(),
        account: ACCOUNT.into(),
        result: Ok(vec![draft, reviewed, assigned]),
    });
    let numbers = |app: &App, by_tag: bool| -> Vec<u64> {
        app.visible_rows(by_tag)
            .iter()
            .map(|row| row.key.number)
            .collect()
    };

    // Act
    let hidden = numbers(&app, true);
    app.toggle_drafts();
    let shown = numbers(&app, true);
    app.cycle_filter();
    let review_only = numbers(&app, true);
    let untagged_filter = numbers(&app, false);

    // Assert
    assert_eq!(hidden, vec![2, 3]);
    assert_eq!(shown, vec![1, 2, 3]);
    assert_eq!(review_only, vec![2]);
    assert_eq!(
        untagged_filter,
        vec![1, 2, 3],
        "the tag filter is off for Mine"
    );
}

#[test]
fn a_cut_list_marks_its_account_partial_until_a_whole_one_replaces_it() {
    // Arrange
    let mut app = App::new();
    let generation = app.begin_refresh();
    let mut cut = loaded(generation, &[1]);
    cut.partial = true;

    // Act
    app.apply(cut);
    let after_cut = app.has_partial_list();
    let generation = app.begin_refresh();
    app.apply(loaded(generation, &[1, 2]));

    // Assert
    assert!(after_cut, "a cut list left no marker");
    assert!(!app.has_partial_list(), "a whole list kept the marker");
}
