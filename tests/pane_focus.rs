// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
mod support;

use chrono::{TimeZone, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use helios::app::worker::Loaded;
use helios::app::{App, DetailTab, LoadRequest, Pane, Screen};
use helios::model::{PrKey, PrSummary};
use helios::sanitize::sanitize;
use helios::tui::keys::{handle_key, Outcome};

fn key(number: u64) -> PrKey {
    PrKey {
        host: "github.com".into(),
        account: "octo".into(),
        repo: "org/repo".into(),
        number,
    }
}

fn row(number: u64) -> PrSummary {
    PrSummary {
        key: key(number),
        title: sanitize("title"),
        author: sanitize("author"),
        is_draft: false,
        updated_at: Utc.timestamp_opt(0, 0).unwrap(),
        tags: vec![],
        review_decision: None,
        check_state: None,
    }
}

fn app(split: bool) -> App {
    let mut app = App::new();
    let generation = app.begin_refresh();
    app.apply_mine(Loaded {
        generation,
        partial: false,
        host: "github.com".into(),
        account: "octo".into(),
        result: Ok(vec![row(1), row(2)]),
    });
    app.set_split(split);
    app
}

fn press(app: &mut App, code: KeyCode) -> Outcome {
    handle_key(app, KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn enter_moves_focus_to_the_detail_pane() {
    // Arrange
    let mut enter = app(true);
    let mut l = app(true);
    press(&mut enter, KeyCode::Down);
    press(&mut l, KeyCode::Down);

    // Act
    press(&mut enter, KeyCode::Enter);
    press(&mut l, KeyCode::Char('l'));

    // Assert
    for app in [&mut enter, &mut l] {
        assert_eq!(app.pane, Pane::Detail);
        assert_eq!(app.screen, Screen::List);
        assert_eq!(app.selected, Some(key(2)));
        assert_eq!(app.take_requests(), vec![LoadRequest::Detail(key(2))]);
    }
}

#[test]
fn escape_returns_focus_to_the_list() {
    // Arrange
    let mut esc = app(true);
    let mut h = app(true);
    let mut idle = app(true);
    press(&mut esc, KeyCode::Enter);
    press(&mut h, KeyCode::Enter);
    let focused = (esc.pane, h.pane);

    // Act
    press(&mut esc, KeyCode::Esc);
    press(&mut h, KeyCode::Char('h'));
    press(&mut idle, KeyCode::Esc);

    // Assert
    assert_eq!(focused, (Pane::Detail, Pane::Detail));
    assert_eq!(esc.pane, Pane::List);
    assert_eq!(h.pane, Pane::List);
    assert_eq!((idle.pane, idle.screen), (Pane::List, Screen::List));
    assert_eq!(esc.screen, Screen::List);
    assert_eq!(press(&mut idle, KeyCode::Char('q')), Outcome::Quit);
}

#[test]
fn tab_switches_tabs_only_when_the_detail_has_focus() {
    // Arrange
    let mut app = app(true);

    // Act
    press(&mut app, KeyCode::Tab);
    let with_list_focus = app.tab;
    press(&mut app, KeyCode::Enter);
    let focus = (app.screen, app.pane);
    press(&mut app, KeyCode::Tab);
    let after_tab = app.tab;
    press(&mut app, KeyCode::BackTab);

    // Assert
    assert_eq!(focus, (Screen::List, Pane::Detail));
    assert_eq!(with_list_focus, DetailTab::Overview);
    assert_eq!(after_tab, DetailTab::Conversation);
    assert_eq!(app.tab, DetailTab::Overview);
}

#[test]
fn a_narrow_terminal_keeps_the_single_pane_flow() {
    // Arrange
    let mut app = app(false);

    // Act
    press(&mut app, KeyCode::Enter);
    let opened = (app.screen, app.pane);
    press(&mut app, KeyCode::Esc);
    let closed = (app.screen, app.pane);
    press(&mut app, KeyCode::Char('l'));
    let opened_by_l = (app.screen, app.pane);
    press(&mut app, KeyCode::Char('h'));

    // Assert
    assert_eq!(opened, (Screen::Detail, Pane::List));
    assert_eq!(closed, (Screen::List, Pane::List));
    assert_eq!(opened_by_l, (Screen::Detail, Pane::List));
    assert_eq!((app.screen, app.pane), (Screen::List, Pane::List));
}
