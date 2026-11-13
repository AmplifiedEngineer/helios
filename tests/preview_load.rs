// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
mod support;

use std::sync::atomic::Ordering;
use std::time::{Duration, UNIX_EPOCH};

use helios::app::refresh::Clock;
use helios::app::worker::Message;
use helios::app::DetailState;
use helios::github::errors::GithubError;
use support::loop_harness::{pr_detail, pr_key, FakeApi, Harness};

const PAUSE: Duration = Duration::from_millis(300);
const BETWEEN_KEYS: Duration = Duration::from_millis(20);
const CURSOR_MOVES: usize = 10;
const FAR_AHEAD: Duration = Duration::from_secs(60);
const FIRST_PR: u64 = 1;
const SECOND_PR: u64 = 2;
const LATE_TITLE: &str = "Late detail of the second PR";

fn rate_limited() -> GithubError {
    GithubError::RateLimited {
        retry_at: UNIX_EPOCH,
    }
}

fn detail_calls(harness: &Harness) -> usize {
    harness.detail_calls.load(Ordering::SeqCst)
}

fn loaded_title(harness: &Harness) -> Option<String> {
    match harness.session.app().detail() {
        DetailState::Loaded(detail) => Some(detail.title.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn advance_keeps_sub_second_durations() {
    // Arrange
    let harness = Harness::start(FakeApi::new());
    let before = harness.clock.now();

    // Act
    harness.clock.advance(PAUSE);

    // Assert
    let moved = harness
        .clock
        .now()
        .duration_since(before)
        .expect("the clock moves forward");
    assert_eq!(moved, PAUSE);
}

#[test]
fn ten_cursor_moves_inside_the_pause_make_one_request() {
    // Arrange
    let mut harness = Harness::start_wide(FakeApi::new());

    // Act
    for index in 0..CURSOR_MOVES {
        let key = if index % 2 == 0 { 'j' } else { 'k' };
        harness.type_char(key);
        harness.clock.advance(BETWEEN_KEYS);
        harness.step();
    }
    let before_the_pause_ends = detail_calls(&harness);
    harness.advance_and_step(PAUSE);
    harness.advance_and_step(PAUSE);

    // Assert
    assert_eq!(before_the_pause_ends, 0);
    assert_eq!(detail_calls(&harness), 1);
}

#[test]
fn a_late_result_for_another_pr_is_dropped() {
    // Arrange
    let mut harness = Harness::start_wide(FakeApi::new());
    harness.type_char('j');
    harness.advance_and_step(PAUSE);
    assert_eq!(loaded_title(&harness).as_deref(), Some("Detail of 2"));
    harness.type_char('k');

    // Act
    harness.clock.advance(PAUSE);
    harness.step();
    harness.inject(Message::Detail(
        pr_key(SECOND_PR),
        Box::new(Ok(pr_detail(LATE_TITLE))),
    ));
    harness.settle();

    // Assert
    assert_eq!(
        harness.session.app().selected.as_ref(),
        Some(&pr_key(FIRST_PR))
    );
    assert_eq!(loaded_title(&harness).as_deref(), Some("Detail of 1"));
}

#[test]
fn a_rate_limited_preview_shows_a_message_and_does_not_retry_until_the_cursor_moves() {
    // Arrange
    let api = FakeApi {
        detail_failure: Some(rate_limited),
        ..FakeApi::new()
    };
    let mut harness = Harness::start_wide(api);
    harness.type_char('j');

    // Act
    harness.advance_and_step(PAUSE);
    let after_the_preview = detail_calls(&harness);
    harness.advance_and_step(FAR_AHEAD);
    harness.advance_and_step(FAR_AHEAD);
    let after_waiting = detail_calls(&harness);
    harness.type_char('k');
    harness.advance_and_step(PAUSE);

    // Assert
    assert_eq!(after_the_preview, 1);
    assert_eq!(after_waiting, 1);
    assert_eq!(detail_calls(&harness), 2);
    assert!(matches!(
        harness.session.app().detail(),
        DetailState::Failed(GithubError::RateLimited { .. })
    ));
}
