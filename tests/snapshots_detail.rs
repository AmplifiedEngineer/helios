// SPDX-License-Identifier: Apache-2.0

use chrono::{TimeZone, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use helios::app::refresh::Clock;
use helios::app::{App, DetailState, DetailTab, LoadRequest, Screen};
use helios::github::errors::GithubError;
use helios::github::queries::{map_detail, map_files};
use helios::model::PrKey;
use helios::tui::detail;
use helios::tui::keys::{handle_key, BINDINGS};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use std::fs;
use std::time::SystemTime;

#[allow(dead_code)]
mod support;
use support::snapshot::assert_snapshot;

const HOST: &str = "github.com";
const ACCOUNT: &str = "octocat";
const REPO: &str = "cli/cli";
const PR_NUMBER: u64 = 1;
const FILES_FIXTURE: &str = "tests/fixtures/detail/files-synthetic.json";
const RECORDED_FIXTURE: &str = "tests/fixtures/detail/recorded-public-pr.json";
const SCREEN_WIDTH: u16 = 120;
const SCREEN_HEIGHT: u16 = 60;

const COMMENTS_HEADING: &str = "Issue comments";
const THREADS_HEADING: &str = "Review threads";
const ISSUE_AUTHOR: &str = "Ufktarimarge";
const ISSUE_BODY: &str = "Bana para laz";
const THREAD_PATH: &str = "repo-delete-private-forks.txtar";
const FIRST_REPLY_AUTHOR: &str = "copilot-pull-request-reviewer";
const SECOND_REPLY_AUTHOR: &str = "BagToad";
const SECOND_REPLY_BODY: &str = "gonna leave it";

/// A clock stopped 2 hours after the issue comment, 9 days 21 hours after the
/// second thread reply and 10 days 1 hour after the first.
struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        Utc.with_ymd_and_hms(2026, 11, 7, 20, 30, 46)
            .single()
            .expect("a valid instant")
            .into()
    }
}

fn open_detail(tab: DetailTab) -> App {
    let json = fs::read_to_string(RECORDED_FIXTURE).expect("read the detail fixture");
    let detail = map_detail(&json).expect("map the detail fixture");
    let key = PrKey {
        host: HOST.into(),
        account: ACCOUNT.into(),
        repo: REPO.into(),
        number: PR_NUMBER,
    };
    let mut app = App::new();
    app.select(key.clone());
    app.apply_detail(&key, Ok(detail));
    app.tab = tab;
    app
}

fn render_detail(app: &App) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(SCREEN_WIDTH, SCREEN_HEIGHT)).expect("test terminal");
    terminal
        .draw(|frame| detail::render(frame, frame.area(), app, &FixedClock))
        .expect("draw");
    terminal
        .backend()
        .buffer()
        .content()
        .chunks(usize::from(SCREEN_WIDTH))
        .map(|row| {
            let line: String = row.iter().map(|cell| cell.symbol()).collect();
            format!("{}\n", line.trim_end())
        })
        .collect()
}

fn position(screen: &str, needle: &str) -> usize {
    screen
        .find(needle)
        .unwrap_or_else(|| panic!("the screen lacks {needle:?}:\n{screen}"))
}

fn line_with<'a>(screen: &'a str, needle: &str) -> &'a str {
    screen
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no line holds {needle:?}:\n{screen}"))
}

#[test]
fn conversation_keeps_issue_comments_and_review_threads_separate() {
    // Arrange
    let app = open_detail(DetailTab::Conversation);

    // Act
    let screen = render_detail(&app);

    // Assert: the issue comment sits under its own heading, before the threads
    let comments = position(&screen, COMMENTS_HEADING);
    let threads = position(&screen, THREADS_HEADING);
    assert!(
        comments < threads,
        "the sections are out of order:\n{screen}"
    );
    let issue_body = position(&screen, ISSUE_BODY);
    assert!(
        comments < issue_body && issue_body < threads,
        "the issue comment is outside its section:\n{screen}"
    );

    // Assert: the thread path and both replies sit under the threads heading
    for needle in [
        THREAD_PATH,
        FIRST_REPLY_AUTHOR,
        SECOND_REPLY_AUTHOR,
        SECOND_REPLY_BODY,
    ] {
        assert!(
            position(&screen, needle) > threads,
            "{needle:?} is outside the threads section:\n{screen}"
        );
    }

    // Assert: ages come from the fake clock and the fixture timestamps
    assert!(line_with(&screen, ISSUE_AUTHOR).contains("2h"), "{screen}");
    assert!(
        line_with(&screen, FIRST_REPLY_AUTHOR).contains("10d"),
        "{screen}"
    );
    assert!(
        line_with(&screen, SECOND_REPLY_AUTHOR).contains("9d"),
        "{screen}"
    );
}

const ALL_TABS: [DetailTab; 5] = [
    DetailTab::Overview,
    DetailTab::Conversation,
    DetailTab::Commits,
    DetailTab::Files,
    DetailTab::Checks,
];

fn key() -> PrKey {
    PrKey {
        host: HOST.into(),
        account: ACCOUNT.into(),
        repo: REPO.into(),
        number: PR_NUMBER,
    }
}

fn assert_has_all(screen: &str, needles: &[&str]) {
    for needle in needles {
        assert!(
            screen.contains(needle),
            "the screen lacks {needle:?}:\n{screen}"
        );
    }
}

#[test]
fn overview_tab_shows_title_author_state_review_and_body() {
    // Arrange
    let app = open_detail(DetailTab::Overview);

    // Act
    let screen = render_detail(&app);

    // Assert
    assert_has_all(
        &screen,
        &[
            "Require confirmation before `gh repo delete` deletes private forks",
            "BagToad",
            "Merged",
            "Approved",
            "Thank you for contributing to GitHub CLI!",
        ],
    );
    assert_snapshot("detail_overview", &screen);
}

#[test]
fn commits_tab_lists_the_commits_in_order() {
    // Arrange
    let app = open_detail(DetailTab::Commits);

    // Act
    let screen = render_detail(&app);

    // Assert
    let first = position(
        &screen,
        "Require confirmation before deleting a private repo's forks",
    );
    let second = position(
        &screen,
        "Accept cascading fork deletion in acceptance cleanup",
    );
    let third = position(
        &screen,
        "Add acceptance test for deleting a private repo with forks",
    );
    assert!(first < second && second < third, "{screen}");
    assert_has_all(&screen, &["7bfdc1b", "4948470", "b17c155", "BagToad"]);
    assert_snapshot("detail_commits", &screen);
}

#[test]
fn files_tab_marks_truncated_binary_and_renamed_files() {
    // Arrange
    let mut app = open_detail(DetailTab::Files);
    let json = fs::read_to_string(FILES_FIXTURE).expect("read the files fixture");
    app.apply_files(&key(), map_files(&json).expect("map the files fixture"));

    // Act
    let screen = render_detail(&app);

    // Assert
    assert!(
        line_with(&screen, "data/huge.json").contains("truncated"),
        "{screen}"
    );
    assert!(
        line_with(&screen, "assets/logo.png").contains("binary"),
        "{screen}"
    );
    let renamed = line_with(&screen, "src/renamed.rs");
    assert!(
        renamed.contains("renamed") && renamed.contains("src/original.rs"),
        "{screen}"
    );
    assert!(
        line_with(&screen, "src/moved.rs").contains("renamed"),
        "{screen}"
    );
    assert!(
        !line_with(&screen, "src/lib.rs").contains("truncated"),
        "{screen}"
    );
    assert_snapshot("detail_files", &screen);
}

#[test]
fn checks_tab_shows_each_check_run_with_its_state() {
    // Arrange
    let app = open_detail(DetailTab::Checks);

    // Act
    let screen = render_detail(&app);

    // Assert
    assert!(
        line_with(&screen, "label-external")
            .to_lowercase()
            .contains("skipped"),
        "{screen}"
    );
    assert!(
        line_with(&screen, "lint")
            .to_lowercase()
            .contains("success"),
        "{screen}"
    );
    assert_snapshot("detail_checks", &screen);
}

#[test]
fn loading_state_renders_a_loading_notice() {
    // Arrange
    let mut app = App::new();
    app.select(key());

    // Act
    let screen = render_detail(&app);

    // Assert
    assert_has_all(&screen, &["Loading"]);
    assert_snapshot("detail_loading", &screen);
}

#[test]
fn not_found_or_hidden_renders_a_fixed_message_inline() {
    // Arrange
    let mut app = App::new();
    app.select(key());
    app.apply_detail(&key(), Err(GithubError::NotFoundOrHidden));

    // Act
    let screen = render_detail(&app);

    // Assert
    assert_has_all(&screen, &["not found or hidden"]);
    assert!(
        !screen.contains("NotFoundOrHidden"),
        "raw error text:\n{screen}"
    );
    assert_snapshot("detail_hidden", &screen);
}

fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    handle_key(app, KeyEvent::new(code, modifiers));
}

#[test]
fn tab_cycles_forward_through_the_five_tabs_and_wraps() {
    // Arrange
    let mut app = open_detail(DetailTab::Overview);
    app.screen = Screen::Detail;
    let mut seen = vec![app.tab];

    // Act
    for _ in 0..ALL_TABS.len() {
        press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        seen.push(app.tab);
    }

    // Assert
    let mut expected = ALL_TABS.to_vec();
    expected.push(DetailTab::Overview);
    assert_eq!(seen, expected);
}

#[test]
fn shift_tab_cycles_backward_through_the_five_tabs_and_wraps() {
    // Arrange
    let mut app = open_detail(DetailTab::Overview);
    app.screen = Screen::Detail;
    let mut seen = vec![app.tab];

    // Act
    for _ in 0..ALL_TABS.len() {
        press(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
        seen.push(app.tab);
    }

    // Assert
    let mut expected: Vec<DetailTab> = ALL_TABS.iter().rev().copied().collect();
    expected.rotate_right(1);
    expected.push(DetailTab::Overview);
    assert_eq!(seen, expected);
}

#[test]
fn the_key_table_lists_the_tab_keys() {
    // Arrange
    let wanted = [("Tab", "next tab"), ("Shift-Tab", "previous tab")];

    // Act
    let listed: Vec<(&str, &str)> = BINDINGS.iter().map(|b| (b.key, b.action)).collect();

    // Assert
    for pair in wanted {
        assert!(
            listed.contains(&pair),
            "BINDINGS lacks {pair:?}: {listed:?}"
        );
    }
}

#[test]
fn opening_a_pull_request_shows_loading_and_requests_one_detail_load() {
    // Arrange
    let mut app = open_detail(DetailTab::Overview);
    let other = PrKey {
        number: PR_NUMBER + 1,
        ..key()
    };

    // Act
    app.open(other.clone());

    // Assert
    assert!(matches!(app.detail(), DetailState::Loading));
    assert_eq!(app.selected, Some(other.clone()));
    assert_eq!(app.requests(), [LoadRequest::Detail(other)]);
}
