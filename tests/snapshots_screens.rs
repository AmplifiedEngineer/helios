// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
mod support;

use chrono::{TimeZone, Utc};
use helios::app::refresh::Clock;
use helios::app::worker::Loaded;
use helios::app::{App, DetailTab, Step};
use helios::github::errors::GithubError;
use helios::model::{PrDetail, PrKey, PrState, PrSummary};
use helios::sanitize::sanitize;
use helios::tui::draw_with;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use std::time::SystemTime;
use support::files::patch_file;
use support::render::buffer_text;
use support::snapshot::assert_snapshot;

const WIDTH: u16 = 100;
const HEIGHT: u16 = 16;
const HOST: &str = "github.com";
const ACCOUNT: &str = "octo";

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        Utc.with_ymd_and_hms(2026, 11, 5, 12, 0, 0)
            .single()
            .expect("a valid instant")
            .into()
    }
}

fn row(number: u64, title: &str) -> PrSummary {
    PrSummary {
        key: PrKey {
            host: HOST.into(),
            account: ACCOUNT.into(),
            repo: "org/repo".into(),
            number,
        },
        title: sanitize(title),
        author: sanitize("octo"),
        is_draft: false,
        updated_at: Utc
            .with_ymd_and_hms(2026, 11, 5, 10, 0, 0)
            .single()
            .expect("instant"),
        tags: Vec::new(),
        review_decision: None,
        check_state: None,
    }
}

fn render(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("test terminal");
    terminal
        .draw(|frame| draw_with(frame, app, &FixedClock))
        .expect("draw");
    buffer_text(terminal.backend().buffer())
}

fn listed() -> App {
    let mut app = App::new();
    app.set_account_count(1);
    let generation = app.begin_refresh();
    app.apply_mine(Loaded {
        generation,
        partial: false,
        host: HOST.into(),
        account: ACCOUNT.into(),
        result: Ok(vec![row(1, "First change"), row(2, "Second change")]),
    });
    app
}

fn on_files() -> App {
    let mut app = listed();
    app.move_cursor(Step::Next);
    app.open_highlighted();
    let key = app.selected.clone().expect("a selected PR");
    app.apply_detail(
        &key,
        Ok(PrDetail {
            title: sanitize("Second change"),
            body: sanitize("body"),
            state: PrState::Open,
            author: sanitize("octo"),
            labels: Vec::new(),
            requested_reviewers: Vec::new(),
            review_decision: None,
            comments: Vec::new(),
            threads: Vec::new(),
            commits: Vec::new(),
            check_runs: Vec::new(),
            partial: false,
        }),
    );
    app.apply_files(
        &key,
        vec![patch_file("src/a.rs", "one"), patch_file("src/b.rs", "two")],
    );
    app.tab = DetailTab::Files;
    app
}

#[test]
fn the_list_screen_shows_the_views_the_rows_and_the_list_keys() {
    // Arrange
    let app = listed();

    // Act
    let screen = render(&app);

    // Assert
    assert_snapshot("screen_list", &screen);
}

#[test]
fn the_list_says_loading_until_the_accounts_answer() {
    // Arrange
    let mut app = App::new();
    app.set_account_count(1);
    app.begin_refresh();

    // Act
    let screen = render(&app);

    // Assert
    assert!(screen.contains("Loading..."), "{screen}");
    assert_snapshot("screen_loading", &screen);
}

#[test]
fn the_list_says_so_when_there_is_no_account() {
    // Arrange
    let app = App::new();

    // Act
    let screen = render(&app);

    // Assert
    assert_snapshot("screen_no_accounts", &screen);
}

#[test]
fn a_notice_replaces_the_no_account_text() {
    // Arrange
    let mut app = App::new();
    app.set_notice("Could not read the gh accounts: gh was not found".into());

    // Act
    let screen = render(&app);

    // Assert
    assert!(screen.contains("gh was not found"), "{screen}");
}

#[test]
fn a_failed_account_shows_its_message_under_the_rows() {
    // Arrange
    let mut app = listed();
    let generation = app.begin_refresh();
    app.apply_mine(Loaded {
        generation,
        partial: false,
        host: "ghe.example.com".into(),
        account: ACCOUNT.into(),
        result: Err(GithubError::Timeout),
    });

    // Act
    let screen = render(&app);

    // Assert
    assert!(
        screen.contains("ghe.example.com: GitHub did not answer in time"),
        "{screen}"
    );
}

#[test]
fn the_help_overlay_lists_the_keys_of_the_screen_it_covers() {
    // Arrange
    let mut app = listed();
    app.show_help = true;

    // Act
    let screen = render(&app);

    // Assert
    assert_snapshot("screen_help_list", &screen);
}

#[test]
fn the_files_tab_shows_the_files_with_the_files_keys() {
    // Arrange
    let app = on_files();

    // Act
    let screen = render(&app);

    // Assert
    assert_snapshot("screen_files", &screen);
}

#[test]
fn the_diff_screen_shows_the_selected_file_scrolled() {
    // Arrange
    let mut app = on_files();
    app.move_file(Step::Next);
    app.open_file();
    app.scroll_diff(Step::Next);

    // Act
    let screen = render(&app);

    // Assert
    assert_snapshot("screen_diff", &screen);
}

fn header_of(screen: &str) -> &str {
    screen.lines().next().expect("a header line")
}

#[test]
fn the_loading_header_shows_a_spinner_that_moves_on_each_tick() {
    // Arrange
    let mut app = App::new();
    app.set_account_count(1);
    app.begin_refresh();
    let first = render(&app);

    // Act
    app.tick();
    let second = render(&app);

    // Assert
    assert_ne!(header_of(&first), header_of(&second), "{first}\n{second}");
    assert!(header_of(&first).contains("[Mine]"), "{first}");
}

#[test]
fn a_loaded_list_shows_no_spinner_and_a_tick_changes_nothing() {
    // Arrange
    let mut app = App::new();
    app.set_account_count(1);
    let generation = app.begin_refresh();
    for apply in [App::apply_mine, App::apply] {
        apply(
            &mut app,
            Loaded {
                generation,
                partial: false,
                host: HOST.into(),
                account: ACCOUNT.into(),
                result: Ok(vec![row(1, "First change")]),
            },
        );
    }
    let before = render(&app);

    // Act
    app.tick();
    let after = render(&app);

    // Assert
    assert!(!app.is_loading());
    assert_eq!(before, after);
    assert_eq!(header_of(&after).trim_end(), "[Mine]  Needs me");
}
