// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyModifiers};
use helios::app::refresh::REFRESH_INTERVAL;
use helios::app::worker::Message;
use helios::app::Screen;
use helios::github::errors::GithubError;
use helios::tui::run::Flow;
use support::loop_harness::{FakeApi, Harness};

const PAST_INTERVAL: Duration = Duration::from_secs(REFRESH_INTERVAL.as_secs() + 1);
const FIRST_LOAD_CALLS: usize = 3;

fn assert_shows(screen: &str, needles: &[&str]) {
    for needle in needles {
        assert!(
            screen.contains(needle),
            "the screen lacks {needle:?}:\n{screen}"
        );
    }
}

fn assert_hides(screen: &str, needles: &[&str]) {
    for needle in needles {
        assert!(
            !screen.contains(needle),
            "the screen shows {needle:?}:\n{screen}"
        );
    }
}

fn open_files_tab(harness: &mut Harness) {
    harness.press(KeyCode::Enter);
    for _ in 0..3 {
        harness.press(KeyCode::Tab);
    }
}

#[test]
fn the_mine_list_shows_after_the_first_load() {
    // Arrange and Act
    let harness = Harness::start(FakeApi::new());

    // Assert
    let screen = harness.screen();
    assert_shows(&screen, &["[Mine]", "Needs me", "Mine one", "Mine two"]);
    assert_hides(&screen, &["Review ten", "Loading"]);
}

#[test]
fn m_switches_to_the_needs_me_list_and_back() {
    // Arrange
    let mut harness = Harness::start(FakeApi::new());

    // Act
    harness.type_char('m');
    let needs_me = harness.screen();
    harness.type_char('m');

    // Assert
    assert_shows(&needs_me, &["[Needs me]", "Review ten", "Assigned eleven"]);
    assert_hides(&needs_me, &["Mine one"]);
    assert_shows(&harness.screen(), &["[Mine]", "Mine one"]);
}

#[test]
fn the_cursor_follows_j_k_and_the_arrow_keys() {
    // Arrange
    let mut harness = Harness::start(FakeApi::new());
    let at_start = harness.cursor_line();

    // Act
    harness.type_char('j');
    let after_j = harness.cursor_line();
    harness.press(KeyCode::Up);
    let after_up = harness.cursor_line();
    harness.type_char('G');
    let at_last = harness.cursor_line();
    harness.type_char('g');

    // Assert
    assert!(at_start.contains("Mine one"), "{at_start}");
    assert!(after_j.contains("Mine two"), "{after_j}");
    assert!(after_up.contains("Mine one"), "{after_up}");
    assert!(at_last.contains("Mine two"), "{at_last}");
    assert!(harness.cursor_line().contains("Mine one"));
}

#[test]
fn enter_opens_the_pr_under_the_cursor_and_loads_its_detail() {
    // Arrange
    let mut harness = Harness::start(FakeApi::new());
    harness.type_char('j');

    // Act
    harness.press(KeyCode::Enter);

    // Assert
    let screen = harness.screen();
    assert_eq!(harness.session.app().screen, Screen::Detail);
    assert_shows(
        &screen,
        &["[Overview]", "Detail of 2", "The body text", "Approved"],
    );
}

#[test]
fn tab_and_shift_tab_move_between_the_detail_tabs() {
    // Arrange
    let mut harness = Harness::start(FakeApi::new());
    harness.press(KeyCode::Enter);

    // Act
    harness.press(KeyCode::Tab);
    let forward = harness.screen();
    harness.press_with(KeyCode::BackTab, KeyModifiers::SHIFT);
    harness.press_with(KeyCode::BackTab, KeyModifiers::SHIFT);

    // Assert
    assert_shows(&forward, &["[Conversation]", "Issue comments"]);
    assert_shows(&harness.screen(), &["[Checks]"]);
}

#[test]
fn the_files_tab_lists_the_files_and_enter_opens_a_diff_that_esc_closes() {
    // Arrange
    let mut harness = Harness::start(FakeApi::new());
    open_files_tab(&mut harness);
    let files = harness.screen();
    harness.type_char('j');
    let second = harness.cursor_line();

    // Act
    harness.press(KeyCode::Enter);
    let diff = harness.screen();
    harness.press(KeyCode::Esc);

    // Assert
    assert_shows(&files, &["[Files]", "src/a.rs", "src/b.rs"]);
    assert!(second.contains("src/b.rs"), "{second}");
    assert_shows(&diff, &["src/b.rs", "+added in b"]);
    assert_hides(&diff, &["[Files]"]);
    assert_eq!(harness.session.app().screen, Screen::Detail);
    assert_shows(&harness.screen(), &["[Files]", "src/a.rs"]);
}

#[test]
fn backspace_and_esc_walk_back_to_the_list() {
    // Arrange
    let mut harness = Harness::start(FakeApi::new());
    harness.press(KeyCode::Enter);

    // Act
    harness.press(KeyCode::Backspace);

    // Assert
    assert_eq!(harness.session.app().screen, Screen::List);
    assert_shows(&harness.screen(), &["[Mine]", "Mine one"]);
}

#[test]
fn r_reloads_both_lists_on_the_list_screen_only() {
    // Arrange
    let mut harness = Harness::start(FakeApi::new());
    assert_eq!(harness.list_calls.load(Ordering::SeqCst), FIRST_LOAD_CALLS);

    // Act
    harness.type_char('r');
    let after_refresh = harness.list_calls.load(Ordering::SeqCst);
    harness.press(KeyCode::Enter);
    harness.type_char('r');

    // Assert
    assert_eq!(after_refresh, FIRST_LOAD_CALLS * 2);
    assert_eq!(harness.list_calls.load(Ordering::SeqCst), after_refresh);
}

#[test]
fn the_list_refreshes_by_itself_once_the_interval_has_passed() {
    // Arrange
    let mut harness = Harness::start(FakeApi::new());

    // Act
    harness.step();
    let early = harness.list_calls.load(Ordering::SeqCst);
    harness.clock.advance(PAST_INTERVAL);
    harness.step();
    harness.settle();

    // Assert
    assert_eq!(early, FIRST_LOAD_CALLS);
    assert_eq!(
        harness.list_calls.load(Ordering::SeqCst),
        FIRST_LOAD_CALLS * 2
    );
}

#[test]
fn an_open_detail_pauses_the_automatic_refresh() {
    // Arrange
    let mut harness = Harness::start(FakeApi::new());
    harness.press(KeyCode::Enter);

    // Act
    harness.clock.advance(PAST_INTERVAL);
    harness.step();
    harness.settle();

    // Assert
    assert_eq!(harness.list_calls.load(Ordering::SeqCst), FIRST_LOAD_CALLS);
}

#[test]
fn a_rate_limit_delays_the_next_automatic_refresh() {
    // Arrange
    let mut api = FakeApi::new();
    api.failure = Some(|| GithubError::RateLimited {
        retry_at: std::time::UNIX_EPOCH + Duration::from_secs(u32::MAX.into()),
    });
    let mut harness = Harness::start(api);

    // Act
    harness.clock.advance(PAST_INTERVAL);
    harness.step();
    harness.settle();

    // Assert
    assert_eq!(harness.list_calls.load(Ordering::SeqCst), FIRST_LOAD_CALLS);
    assert_shows(
        &harness.screen(),
        &["github.com: GitHub rate limit reached"],
    );
}

#[test]
fn a_failing_host_is_named_on_the_list() {
    // Arrange
    let mut api = FakeApi::new();
    api.failure = Some(|| GithubError::Network);

    // Act
    let harness = Harness::start(api);

    // Assert
    let screen = harness.screen();
    assert_shows(
        &screen,
        &["github.com: could not reach GitHub", "No pull requests."],
    );
}

#[test]
fn a_failed_detail_load_says_so_in_the_detail() {
    // Arrange
    let mut api = FakeApi::new();
    api.detail_failure = Some(|| GithubError::NotFoundOrHidden);
    let mut harness = Harness::start(api);

    // Act
    harness.press(KeyCode::Enter);

    // Assert
    assert_shows(&harness.screen(), &["not found or hidden"]);
}

#[test]
fn q_and_ctrl_c_quit_from_any_screen() {
    // Arrange
    let mut with_help = Harness::start(FakeApi::new());
    with_help.press(KeyCode::Char('?'));
    let mut in_detail = Harness::start(FakeApi::new());
    in_detail.press(KeyCode::Enter);

    // Act
    let from_help = with_help.press_with(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let from_detail = in_detail.press(KeyCode::Char('q'));

    // Assert
    assert_eq!(from_help, Flow::Quit);
    assert_eq!(from_detail, Flow::Quit);
}

#[test]
fn the_question_mark_opens_the_help_and_any_key_closes_it() {
    // Arrange
    let mut harness = Harness::start(FakeApi::new());

    // Act
    harness.press(KeyCode::Char('?'));
    let help = harness.screen();
    harness.press(KeyCode::Char('j'));

    // Assert
    assert_shows(&help, &["Keys", "refresh"]);
    assert_hides(&harness.screen(), &["Keys"]);
    assert!(
        harness.cursor_line().contains("Mine one"),
        "the closing key moved the cursor"
    );
}

#[test]
fn with_no_account_the_list_says_how_to_log_in() {
    // Arrange and Act
    let mut harness = Harness::start_unsettled(Vec::new());
    harness.settle();

    // Assert
    assert_shows(
        &harness.screen(),
        &["No GitHub accounts found", "gh auth login"],
    );
}

#[test]
fn two_accounts_both_show_their_rows() {
    // Arrange and Act
    let mut harness = Harness::start_unsettled(vec![FakeApi::new(), FakeApi::for_login("hubot")]);
    harness.settle();

    // Assert
    assert_eq!(harness.screen().matches("Mine one").count(), 2);
}

fn record_opened_urls(harness: &mut Harness) -> Arc<Mutex<Vec<String>>> {
    let opened = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&opened);
    harness.session.set_opener(Box::new(move |url, _results| {
        sink.lock().expect("sink lock").push(url.to_owned())
    }));
    opened
}

#[test]
fn o_opens_the_pr_under_the_cursor_and_the_open_pr_in_the_browser() {
    // Arrange
    let mut harness = Harness::start(FakeApi::new());
    let opened = record_opened_urls(&mut harness);

    // Act
    harness.type_char('j');
    harness.type_char('o');
    harness.press(KeyCode::Enter);
    harness.press(KeyCode::Char('o'));

    // Assert
    let urls = opened.lock().expect("sink lock").clone();
    assert_eq!(urls, vec!["https://github.com/org/repo/pull/2"; 2]);
}

#[test]
fn o_does_nothing_on_an_empty_list() {
    // Arrange
    let mut harness = Harness::start_unsettled(Vec::new());
    let opened = record_opened_urls(&mut harness);

    // Act
    harness.type_char('o');

    // Assert
    assert!(opened.lock().expect("sink lock").is_empty());
}

#[test]
fn the_configured_refresh_interval_sets_when_the_list_refreshes() {
    // Arrange
    let mut harness = Harness::start(FakeApi::new());
    let configured = Duration::from_secs(600);
    harness.session.set_refresh_interval(configured);

    // Act
    harness.clock.advance(PAST_INTERVAL);
    harness.step();
    harness.settle();
    let before_due = harness.list_calls.load(Ordering::SeqCst);
    harness.clock.advance(configured);
    harness.step();
    harness.settle();

    // Assert
    assert_eq!(before_due, FIRST_LOAD_CALLS);
    assert!(harness.list_calls.load(Ordering::SeqCst) > FIRST_LOAD_CALLS);
}

#[test]
fn the_spinner_moves_between_two_loop_ticks_while_loading_and_stops_when_loaded() {
    // Arrange: the first load waits until the test lets it go
    let release = Arc::new(AtomicBool::new(false));
    let mut api = FakeApi::new();
    api.hold = Some(Arc::clone(&release));
    let mut harness = Harness::start_unsettled(vec![api]);

    // Act
    harness.step();
    let first = harness.session.app().spinner();
    harness.step();
    let second = harness.session.app().spinner();
    release.store(true, Ordering::SeqCst);
    harness.settle();

    // Assert
    assert!(first.is_some(), "no spinner while loading");
    assert_ne!(first, second, "the spinner did not move");
    assert_eq!(harness.session.app().spinner(), None);
    assert_hides(&harness.screen(), &["Loading"]);
}

const OPEN_FAILED_TEXT: &str = "Could not open the browser";

fn fail_to_open(harness: &mut Harness) {
    harness.session.set_opener(Box::new(|_url, results| {
        results
            .send(Message::OpenFailed)
            .expect("the loop still listens");
    }));
}

#[test]
fn a_failed_open_shows_a_notice_that_the_next_key_clears() {
    // Arrange
    let mut harness = Harness::start(FakeApi::new());
    fail_to_open(&mut harness);

    // Act
    harness.type_char('o');
    let after_open = harness.screen();
    harness.type_char('j');
    let after_key = harness.screen();

    // Assert
    assert_shows(&after_open, &[OPEN_FAILED_TEXT, "Mine one"]);
    assert_hides(&after_key, &[OPEN_FAILED_TEXT]);
}

#[test]
fn a_failed_open_shows_its_notice_on_the_detail_screen_too() {
    // Arrange
    let mut harness = Harness::start(FakeApi::new());
    fail_to_open(&mut harness);
    harness.press(KeyCode::Enter);

    // Act
    harness.type_char('o');

    // Assert
    assert_shows(&harness.screen(), &[OPEN_FAILED_TEXT]);
}
