// SPDX-License-Identifier: Apache-2.0

use chrono::{TimeZone, Utc};
use helios::app::worker::Loaded;
use helios::app::{App, DetailTab, ListView, LoadRequest, Screen, Step};
use helios::github::errors::GithubError;
use helios::model::{ChangedFile, DiffGap, FileDiff, PrKey, PrSummary, Tag};
use helios::sanitize::sanitize;

const HOST: &str = "github.com";
const ACCOUNT: &str = "octo";

fn summary(number: u64, tags: Vec<Tag>) -> PrSummary {
    PrSummary {
        key: PrKey {
            host: HOST.into(),
            account: ACCOUNT.into(),
            repo: "org/repo".into(),
            number,
        },
        title: sanitize("title"),
        author: sanitize("author"),
        is_draft: false,
        updated_at: Utc.timestamp_opt(0, 0).unwrap(),
        tags,
        review_decision: None,
        check_state: None,
    }
}

fn loaded(generation: u64, rows: Vec<PrSummary>) -> Loaded {
    Loaded {
        generation,
        partial: false,
        host: HOST.into(),
        account: ACCOUNT.into(),
        result: Ok(rows),
    }
}

fn key(number: u64) -> PrKey {
    summary(number, Vec::new()).key
}

/// Mine holds 1, 2 and 3; Needs me holds 10 and 11.
fn loaded_app() -> App {
    let mut app = App::new();
    let generation = app.begin_refresh();
    app.apply_mine(loaded(
        generation,
        (1..=3).map(|n| summary(n, vec![])).collect(),
    ));
    app.apply(loaded(
        generation,
        vec![
            summary(10, vec![Tag::Review]),
            summary(11, vec![Tag::Assigned]),
        ],
    ));
    app
}

fn highlighted(app: &App) -> Option<u64> {
    app.highlighted().map(|row| row.key.number)
}

fn patch_file(path: &str, lines: usize) -> ChangedFile {
    let patch: String = (0..lines).map(|n| format!("+line {n}\n")).collect();
    ChangedFile {
        path: sanitize(path),
        previous_path: None,
        diff: FileDiff::Patch(sanitize(&patch)),
    }
}

fn app_on_files(files: Vec<ChangedFile>) -> App {
    let mut app = loaded_app();
    app.open_highlighted();
    app.apply_files(&key(1), files);
    app.tab = DetailTab::Files;
    app
}

#[test]
fn the_cursor_moves_over_the_visible_rows_and_stops_at_both_ends() {
    // Arrange
    let mut app = loaded_app();

    // Act
    app.move_cursor(Step::Previous);
    let at_top = highlighted(&app);
    app.move_cursor(Step::Next);
    app.move_cursor(Step::Next);
    app.move_cursor(Step::Next);
    let at_bottom = highlighted(&app);
    app.move_cursor(Step::First);
    let first = highlighted(&app);
    app.move_cursor(Step::Last);
    let last = highlighted(&app);

    // Assert
    assert_eq!(at_top, Some(1));
    assert_eq!(at_bottom, Some(3));
    assert_eq!(first, Some(1));
    assert_eq!(last, Some(3));
}

#[test]
fn an_empty_list_has_no_highlight_and_moving_is_harmless() {
    // Arrange
    let mut app = App::new();

    // Act
    app.move_cursor(Step::Last);
    app.move_cursor(Step::Next);
    app.open_highlighted();

    // Assert
    assert_eq!(app.highlighted().map(|row| row.key.number), None);
    assert_eq!(app.screen, Screen::List);
}

#[test]
fn switching_the_view_shows_the_other_list_from_its_first_row() {
    // Arrange
    let mut app = loaded_app();
    app.move_cursor(Step::Last);

    // Act
    app.toggle_view();

    // Assert
    assert_eq!(app.view, ListView::NeedsMe);
    assert_eq!(highlighted(&app), Some(10));
    app.toggle_view();
    assert_eq!(app.view, ListView::Mine);
}

#[test]
fn the_cursor_stays_on_its_pr_when_a_refresh_reorders_the_rows() {
    // Arrange
    let mut app = loaded_app();
    app.move_cursor(Step::Last);
    let generation = app.begin_refresh();

    // Act
    app.apply_mine(loaded(
        generation,
        [3, 1, 2].map(|n| summary(n, vec![])).to_vec(),
    ));

    // Assert
    assert_eq!(highlighted(&app), Some(3));
}

#[test]
fn drafts_are_left_out_of_the_cursor_range_until_shown() {
    // Arrange
    let mut app = App::new();
    let generation = app.begin_refresh();
    let mut draft = summary(2, vec![]);
    draft.is_draft = true;
    app.apply_mine(loaded(generation, vec![summary(1, vec![]), draft]));

    // Act
    app.move_cursor(Step::Last);
    let hidden = highlighted(&app);
    app.toggle_drafts();
    app.move_cursor(Step::Last);

    // Assert
    assert_eq!(hidden, Some(1));
    assert_eq!(highlighted(&app), Some(2));
}

#[test]
fn opening_the_highlighted_pr_shows_its_overview_and_asks_for_its_detail() {
    // Arrange
    let mut app = loaded_app();
    app.move_cursor(Step::Next);

    // Act
    app.open_highlighted();

    // Assert
    assert_eq!(app.screen, Screen::Detail);
    assert_eq!(app.tab, DetailTab::Overview);
    assert_eq!(app.take_requests(), vec![LoadRequest::Detail(key(2))]);
}

#[test]
fn back_goes_from_diff_to_detail_to_list_and_stays_on_the_list() {
    // Arrange
    let mut app = app_on_files(vec![patch_file("a.rs", 3)]);
    app.open_file();
    assert_eq!(app.screen, Screen::Diff);

    // Act
    app.back();
    let after_diff = app.screen;
    app.back();
    let after_detail = app.screen;
    app.back();

    // Assert
    assert_eq!(after_diff, Screen::Detail);
    assert_eq!(after_detail, Screen::List);
    assert_eq!(app.screen, Screen::List);
}

#[test]
fn the_files_are_asked_for_once_when_the_files_tab_first_shows() {
    // Arrange
    let mut app = loaded_app();
    app.open_highlighted();
    app.take_requests();

    // Act
    app.next_tab();
    app.next_tab();
    app.next_tab();
    let first = app.take_requests();
    app.next_tab();
    app.previous_tab();
    let second = app.take_requests();

    // Assert
    assert_eq!(app.tab, DetailTab::Files);
    assert_eq!(first, vec![LoadRequest::Files(key(1))]);
    assert!(second.is_empty(), "the files were asked for twice");
}

#[test]
fn a_failed_files_load_can_be_asked_for_again_by_leaving_and_returning() {
    // Arrange
    let mut app = loaded_app();
    app.open_highlighted();
    for _ in 0..3 {
        app.next_tab();
    }
    app.take_requests();

    // Act
    app.apply_files_failure(&key(1), GithubError::Network);
    app.next_tab();
    app.previous_tab();

    // Assert
    assert!(matches!(app.files_failure(), Some(GithubError::Network)));
    assert_eq!(app.take_requests(), vec![LoadRequest::Files(key(1))]);
}

#[test]
fn the_file_cursor_moves_inside_the_files_and_opens_the_selected_diff() {
    // Arrange
    let mut app = app_on_files(vec![
        patch_file("a.rs", 1),
        patch_file("b.rs", 1),
        patch_file("c.rs", 1),
    ]);

    // Act
    app.move_file(Step::Next);
    app.move_file(Step::Next);
    app.move_file(Step::Next);
    let at_end = app.selected_file().map(|file| file.path.to_string());
    app.move_file(Step::First);
    app.move_file(Step::Next);
    app.open_file();

    // Assert
    assert_eq!(at_end.as_deref(), Some("c.rs"));
    assert_eq!(app.screen, Screen::Diff);
    assert_eq!(
        app.selected_file()
            .map(|file| file.path.to_string())
            .as_deref(),
        Some("b.rs")
    );
}

#[test]
fn a_diff_opens_at_the_top_and_scrolls_within_its_lines() {
    // Arrange
    let mut app = app_on_files(vec![patch_file("a.rs", 4)]);
    app.open_file();

    // Act
    app.scroll_diff(Step::Previous);
    let top = app.diff_scroll();
    app.scroll_diff(Step::Next);
    let one = app.diff_scroll();
    app.scroll_diff(Step::Last);
    let bottom = app.diff_scroll();
    app.scroll_diff(Step::Next);

    // Assert
    assert_eq!((top, one, bottom), (0, 1, 3));
    assert_eq!(app.diff_scroll(), 3);
}

#[test]
fn a_file_without_a_patch_does_not_scroll_and_an_empty_files_list_opens_nothing() {
    // Arrange
    let binary = ChangedFile {
        path: sanitize("img.png"),
        previous_path: None,
        diff: FileDiff::Unavailable {
            gap: DiffGap::Binary,
            web_url: sanitize("https://github.test/blob"),
        },
    };
    let mut empty = app_on_files(Vec::new());
    let mut app = app_on_files(vec![binary]);

    // Act
    empty.open_file();
    app.open_file();
    app.scroll_diff(Step::Last);

    // Assert
    assert_eq!(empty.screen, Screen::Detail);
    assert_eq!(app.screen, Screen::Diff);
    assert_eq!(app.diff_scroll(), 0);
}

#[test]
fn the_app_is_loading_until_every_account_has_answered_both_lists() {
    // Arrange
    let mut app = App::new();
    app.set_account_count(1);
    let generation = app.begin_refresh();
    let before = app.is_loading();

    // Act
    app.apply_mine(loaded(generation, vec![]));
    let half = app.is_loading();
    app.apply(loaded(generation, vec![]));

    // Assert
    assert!(before && half);
    assert!(!app.is_loading());
}
