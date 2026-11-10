// SPDX-License-Identifier: Apache-2.0

use chrono::{TimeZone, Utc};
use helios::app::worker::{channel, Loaded};
use helios::app::{App, DetailState};
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
