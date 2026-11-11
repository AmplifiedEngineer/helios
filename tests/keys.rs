// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
mod support;

use chrono::{TimeZone, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use helios::app::worker::Loaded;
use helios::app::{App, DetailTab, ListFilter, ListView, Screen};
use helios::model::{PrKey, PrSummary, Tag};
use helios::sanitize::sanitize;
use helios::tui::keys::{footer_lines, handle_key, Outcome, Scope};

fn row(number: u64, tags: Vec<Tag>) -> PrSummary {
    PrSummary {
        key: PrKey {
            host: "github.com".into(),
            account: "octo".into(),
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

fn app() -> App {
    let mut app = App::new();
    let generation = app.begin_refresh();
    let load = |rows| Loaded {
        generation,
        partial: false,
        host: "github.com".into(),
        account: "octo".into(),
        result: Ok(rows),
    };
    app.apply_mine(load(vec![row(1, vec![]), row(2, vec![])]));
    app.apply(load(vec![row(10, vec![Tag::Review])]));
    app
}

fn press(app: &mut App, code: KeyCode) -> Outcome {
    handle_key(app, KeyEvent::new(code, KeyModifiers::NONE))
}

fn footer_text(scope: Scope) -> String {
    footer_lines(Some(scope), 200)
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn quit_keys_quit_and_help_swallows_the_next_key() {
    // Arrange
    let mut app = app();

    // Act
    let q = press(&mut app, KeyCode::Char('q'));
    let ctrl_c = handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );
    press(&mut app, KeyCode::Char('?'));
    let opened = app.show_help;
    let swallowed = press(&mut app, KeyCode::Char('q'));

    // Assert
    assert_eq!((q, ctrl_c), (Outcome::Quit, Outcome::Quit));
    assert!(opened);
    assert_eq!(swallowed, Outcome::Continue);
    assert!(!app.show_help);
}

#[test]
fn r_asks_for_a_refresh_only_on_the_list() {
    // Arrange
    let mut app = app();

    // Act
    let on_list = press(&mut app, KeyCode::Char('r'));
    press(&mut app, KeyCode::Enter);
    let in_detail = press(&mut app, KeyCode::Char('r'));

    // Assert
    assert_eq!(on_list, Outcome::Refresh);
    assert_eq!(in_detail, Outcome::Continue);
}

#[test]
fn list_keys_move_open_switch_filter_and_toggle_drafts() {
    // Arrange
    let mut app = app();

    // Act
    press(&mut app, KeyCode::Char('f'));
    let filter_on_mine = app.filter;
    press(&mut app, KeyCode::Char('m'));
    press(&mut app, KeyCode::Char('f'));
    press(&mut app, KeyCode::Char('d'));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Char('k'));
    press(&mut app, KeyCode::Enter);

    // Assert
    assert_eq!(filter_on_mine, ListFilter::All, "f is a Needs me key");
    assert_eq!(app.view, ListView::NeedsMe);
    assert_eq!(app.filter, ListFilter::Review);
    assert!(app.show_drafts);
    assert_eq!(app.screen, Screen::Detail);
    assert_eq!(app.selected.as_ref().map(|key| key.number), Some(10));
}

#[test]
fn detail_keys_switch_tabs_and_only_the_files_tab_takes_enter() {
    // Arrange
    let mut app = app();
    press(&mut app, KeyCode::Enter);

    // Act
    press(&mut app, KeyCode::Enter);
    let still_detail = app.screen;
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::BackTab);
    press(&mut app, KeyCode::BackTab);
    press(&mut app, KeyCode::Esc);

    // Assert
    assert_eq!(still_detail, Screen::Detail);
    assert_eq!(app.tab, DetailTab::Checks);
    assert_eq!(app.screen, Screen::List);
}

#[test]
fn the_footer_lists_only_the_keys_that_work_on_the_screen() {
    // Act
    let mine = footer_text(Scope::Mine);
    let needs_me = footer_text(Scope::NeedsMe);
    let detail = footer_text(Scope::Detail);
    let files = footer_text(Scope::Files);
    let diff = footer_text(Scope::Diff);

    // Assert
    assert!(mine.contains("refresh") && mine.contains("toggle drafts"));
    assert!(!mine.contains("filter") && needs_me.contains("filter"));
    assert!(detail.contains("next tab") && !detail.contains("refresh"));
    assert!(!detail.contains("move") && files.contains("move"));
    assert!(diff.contains("scroll") && !diff.contains("next tab"));
    assert!([&mine, &needs_me, &detail, &files, &diff]
        .iter()
        .all(|text| text.contains("quit") && text.contains("help")));
}
