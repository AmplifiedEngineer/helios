// SPDX-License-Identifier: Apache-2.0

//! A `[keys]` entry changes what a key does and what the footer and help show.

#[allow(dead_code)]
mod support;

use chrono::{TimeZone, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use helios::app::worker::Loaded;
use helios::app::{App, DetailTab};
use helios::config::parse;
use helios::model::{PrKey, PrSummary};
use helios::sanitize::sanitize;
use helios::tui::keys::{
    footer_lines_with, handle_key_with, render_help_for_with, KeyMap, Outcome, Scope,
};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use support::files::patch_file;

const HOST: &str = "github.com";
const ACCOUNT: &str = "octo";
const FIRST_PR: u64 = 1;
const FOOTER_WIDTH: u16 = 200;
const HELP_WIDTH: u16 = 60;
const HELP_HEIGHT: u16 = 24;

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

/// A list with two pull requests; with `files` it is open on the Files tab with one test file.
fn app(files: bool) -> App {
    let mut app = App::new();
    let generation = app.begin_refresh();
    app.apply_mine(Loaded {
        generation,
        partial: false,
        host: HOST.into(),
        account: ACCOUNT.into(),
        result: Ok(vec![summary(FIRST_PR), summary(2)]),
    });
    if files {
        app.open_highlighted();
        app.tab = DetailTab::Files;
        let paths = ["src/a.rs", "tests/a.rs"];
        app.apply_files(
            &key(FIRST_PR),
            paths.iter().map(|path| patch_file(path, "x")).collect(),
        );
    }
    app
}

fn keymap(text: &str) -> KeyMap {
    KeyMap::new(&parse(text).expect("the config parses").keys)
}

fn press(app: &mut App, keys: &KeyMap, code: KeyCode) -> Outcome {
    handle_key_with(app, KeyEvent::new(code, KeyModifiers::NONE), keys)
}

fn help_text(scope: Scope, keys: &KeyMap) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(HELP_WIDTH, HELP_HEIGHT)).expect("test terminal");
    terminal
        .draw(|frame| render_help_for_with(frame, frame.area(), Some(scope), keys))
        .expect("draw");
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn a_rebound_key_runs_its_action() {
    // Arrange
    let keys = keymap("[keys.files]\ntoggle_tests = [\"z\"]\n[keys.list]\nrefresh = [\"R\"]\n");
    let mut files = app(true);
    let mut list = app(false);

    // Act
    let old_files_key = press(&mut files, &keys, KeyCode::Char('x'));
    let tests_after_old_key = files.tests_open();
    press(&mut files, &keys, KeyCode::Char('z'));
    let tests_after_new_key = files.tests_open();
    let refresh_new = press(&mut list, &keys, KeyCode::Char('R'));
    let refresh_old = press(&mut list, &keys, KeyCode::Char('r'));
    press(&mut list, &keys, KeyCode::Enter);
    let refresh_in_detail = press(&mut list, &keys, KeyCode::Char('R'));

    // Assert
    assert_eq!(old_files_key, Outcome::Continue);
    assert!(!tests_after_old_key, "x no longer toggles the tests group");
    assert!(tests_after_new_key, "z toggles the tests group");
    assert_eq!(refresh_new, Outcome::Refresh);
    assert_eq!(refresh_old, Outcome::Continue, "r no longer refreshes");
    assert_eq!(
        refresh_in_detail,
        Outcome::Continue,
        "the list rebind stays in the list"
    );
}

#[test]
fn the_footer_and_help_show_the_rebound_keys() {
    // Arrange
    let keys = keymap("[keys.files]\ntoggle_tests = [\"z\"]\n");

    // Act
    let footer: String = footer_lines_with(Some(Scope::Files), FOOTER_WIDTH, &keys)
        .iter()
        .map(ToString::to_string)
        .collect();
    let help = help_text(Scope::Files, &keys);
    let list_footer: String = footer_lines_with(Some(Scope::Mine), FOOTER_WIDTH, &keys)
        .iter()
        .map(ToString::to_string)
        .collect();

    // Assert
    for (name, text) in [("footer", &footer), ("help", &help)] {
        assert!(text.contains("z tests group"), "{name} shows z: {text}");
        assert!(!text.contains("x tests group"), "{name} dropped x: {text}");
    }
    assert!(
        list_footer.contains("r refresh"),
        "other scopes keep their keys"
    );
}
