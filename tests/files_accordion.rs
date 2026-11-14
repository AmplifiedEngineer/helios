// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
mod support;

use chrono::{TimeZone, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use helios::app::worker::Loaded;
use helios::app::{App, DetailTab, FileRow, Screen};
use helios::model::{PrKey, PrSummary};
use helios::sanitize::sanitize;
use helios::tui::keys::handle_key;
use support::files::patch_file;

const HOST: &str = "github.com";
const ACCOUNT: &str = "octo";
const FIRST_PR: u64 = 1;
const SECOND_PR: u64 = 2;

fn key(number: u64) -> PrKey {
    PrKey {
        host: HOST.into(),
        account: ACCOUNT.into(),
        repo: "org/repo".into(),
        number,
    }
}

fn summary(number: u64) -> PrSummary {
    PrSummary {
        key: key(number),
        title: sanitize("title"),
        author: sanitize("author"),
        is_draft: false,
        updated_at: Utc.timestamp_opt(0, 0).unwrap(),
        tags: Vec::new(),
        review_decision: None,
        check_state: None,
    }
}

/// Files 0 and 2 are source files; file 1 is the only test file.
fn app_with(paths: &[&str]) -> App {
    let mut app = App::new();
    let generation = app.begin_refresh();
    app.apply_mine(Loaded {
        generation,
        partial: false,
        host: HOST.into(),
        account: ACCOUNT.into(),
        result: Ok(vec![summary(FIRST_PR), summary(SECOND_PR)]),
    });
    app.open_highlighted();
    app.tab = DetailTab::Files;
    let files = paths.iter().map(|path| patch_file(path, "x")).collect();
    app.apply_files(&key(FIRST_PR), files);
    app
}

fn app_with_tests() -> App {
    app_with(&["src/a.rs", "tests/a.rs", "src/b.rs"])
}

fn press(app: &mut App, code: KeyCode) {
    handle_key(app, KeyEvent::new(code, KeyModifiers::NONE));
}

fn open_flags(app: &App, count: usize) -> Vec<bool> {
    (0..count).map(|index| app.is_file_open(index)).collect()
}

#[test]
fn every_file_starts_collapsed() {
    // Arrange
    let app = app_with_tests();

    // Act
    let open = open_flags(&app, 3);

    // Assert
    assert_eq!(open, [false, false, false]);
    assert!(!app.tests_open());
    assert_eq!(
        app.file_rows(),
        [FileRow::File(0), FileRow::File(2), FileRow::Tests(1)]
    );
}

#[test]
fn enter_toggles_one_file() {
    // Arrange
    let mut app = app_with_tests();

    // Act
    press(&mut app, KeyCode::Enter);
    let after_enter = open_flags(&app, 3);
    press(&mut app, KeyCode::Char(' '));
    let after_space = open_flags(&app, 3);
    press(&mut app, KeyCode::Char(' '));
    let after_second_space = open_flags(&app, 3);

    // Assert
    assert_eq!(after_enter, [true, false, false]);
    assert_eq!(after_space, [false, false, false]);
    assert_eq!(after_second_space, [true, false, false]);
    assert_eq!(app.screen, Screen::Detail);
}

#[test]
fn e_opens_all_and_shift_e_closes_all() {
    // Arrange
    let mut app = app_with_tests();

    // Act
    press(&mut app, KeyCode::Char('e'));
    let opened = open_flags(&app, 3);
    let group_opened = app.tests_open();
    press(&mut app, KeyCode::Char('E'));
    let closed = open_flags(&app, 3);

    // Assert
    assert_eq!(opened, [true, true, true]);
    assert!(group_opened);
    assert_eq!(closed, [false, false, false]);
    assert!(!app.tests_open());
}

#[test]
fn x_toggles_the_tests_group() {
    // Arrange
    let mut app = app_with_tests();
    let closed_rows = app.file_rows();

    // Act
    press(&mut app, KeyCode::Char('x'));
    let open_rows = app.file_rows();
    let group_open = app.tests_open();
    press(&mut app, KeyCode::Char('x'));

    // Assert
    assert_eq!(
        closed_rows,
        [FileRow::File(0), FileRow::File(2), FileRow::Tests(1)]
    );
    assert!(group_open);
    assert_eq!(
        open_rows,
        [
            FileRow::File(0),
            FileRow::File(2),
            FileRow::Tests(1),
            FileRow::File(1)
        ]
    );
    assert!(!app.tests_open());
    assert_eq!(app.file_rows(), closed_rows);
}

#[test]
fn the_tests_group_row_appears_only_when_a_test_file_exists() {
    // Arrange
    let mut app = app_with(&["src/a.rs", "src/b.rs"]);

    // Act
    press(&mut app, KeyCode::Char('x'));

    // Assert
    assert_eq!(app.file_rows(), [FileRow::File(0), FileRow::File(1)]);
}

#[test]
fn the_tests_row_counts_every_test_file() {
    // Arrange
    let app = app_with(&["tests/a.rs", "src/a.rs", "tests/b.rs"]);

    // Act
    let rows = app.file_rows();

    // Assert
    assert_eq!(rows, [FileRow::File(1), FileRow::Tests(2)]);
}

#[test]
fn closing_the_group_with_the_cursor_inside_moves_it_to_the_group_row() {
    // Arrange
    let mut app = app_with_tests();
    press(&mut app, KeyCode::Char('x'));
    press(&mut app, KeyCode::Char('G'));
    let inside = app.file_cursor();

    // Act
    press(&mut app, KeyCode::Char('x'));

    // Assert
    assert_eq!(inside, 3);
    assert_eq!(app.file_cursor(), 2);
    assert_eq!(
        app.file_rows().get(app.file_cursor()),
        Some(&FileRow::Tests(1))
    );
}

const TWO_TEST_FILES: [&str; 4] = ["src/a.rs", "src/b.rs", "tests/c.rs", "tests/d.rs"];

#[test]
fn closing_the_tests_group_moves_the_cursor_to_the_group_row() {
    // Arrange
    let mut app = app_with(&TWO_TEST_FILES);
    press(&mut app, KeyCode::Char('x'));
    press(&mut app, KeyCode::Char('G'));
    let on_second_test_file = app.file_cursor();

    // Act
    press(&mut app, KeyCode::Char('x'));

    // Assert
    assert_eq!(on_second_test_file, 4);
    assert_eq!(
        app.file_rows().get(app.file_cursor()),
        Some(&FileRow::Tests(2))
    );
    assert!(app.selected_file().is_none());
}

#[test]
fn shift_e_closes_the_group_and_keeps_the_cursor_on_a_valid_row() {
    // Arrange
    let mut app = app_with(&TWO_TEST_FILES);
    press(&mut app, KeyCode::Char('e'));
    press(&mut app, KeyCode::Char('G'));

    // Act
    press(&mut app, KeyCode::Char('E'));

    // Assert
    assert!(!app.tests_open());
    assert!(app.file_cursor() < app.file_rows().len());
    assert_eq!(
        app.file_rows().get(app.file_cursor()),
        Some(&FileRow::Tests(2))
    );
}

#[test]
fn enter_on_the_tests_row_toggles_the_group() {
    // Arrange
    let mut app = app_with_tests();
    press(&mut app, KeyCode::Char('G'));

    // Act
    press(&mut app, KeyCode::Enter);

    // Assert
    assert!(app.tests_open());
    assert!(app.selected_file().is_none());
    assert_eq!(app.file_rows().len(), 4);
}

#[test]
fn v_opens_the_full_screen_diff_and_escape_keeps_the_open_rows() {
    // Arrange
    let mut app = app_with_tests();
    press(&mut app, KeyCode::Char('e'));

    // Act
    press(&mut app, KeyCode::Char('v'));
    let on_diff = app.screen;
    press(&mut app, KeyCode::Esc);

    // Assert
    assert_eq!(on_diff, Screen::Diff);
    assert_eq!(app.screen, Screen::Detail);
    assert_eq!(open_flags(&app, 3), [true, true, true]);
    assert!(app.tests_open());
}

#[test]
fn v_on_the_tests_row_opens_nothing() {
    // Arrange
    let mut app = app_with_tests();
    press(&mut app, KeyCode::Char('G'));

    // Act
    press(&mut app, KeyCode::Char('v'));

    // Assert
    assert_eq!(app.screen, Screen::Detail);
}

#[test]
fn the_open_state_resets_when_another_pr_opens() {
    // Arrange
    let mut app = app_with_tests();
    press(&mut app, KeyCode::Char('e'));

    // Act
    app.open(key(SECOND_PR));
    app.apply_files(
        &key(SECOND_PR),
        vec![patch_file("src/a.rs", "x"), patch_file("tests/a.rs", "x")],
    );

    // Assert
    assert_eq!(open_flags(&app, 2), [false, false]);
    assert!(!app.tests_open());
}
