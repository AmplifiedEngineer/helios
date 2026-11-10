// SPDX-License-Identifier: Apache-2.0

use helios::app::refresh::{Clock, Scheduler, REFRESH_INTERVAL};
use helios::app::Screen;
use helios::github::errors::GithubError;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const START_SECS: u64 = 1_000_000;
const TWO_MINUTES: Duration = Duration::from_secs(120);
const ONE_SECOND: Duration = Duration::from_secs(1);
const SHORT_WAIT: Duration = Duration::from_secs(300);
const LONG_WAIT: Duration = Duration::from_secs(900);

#[derive(Clone)]
struct FakeClock(Rc<Cell<SystemTime>>);

impl FakeClock {
    fn new() -> Self {
        Self(Rc::new(Cell::new(
            UNIX_EPOCH + Duration::from_secs(START_SECS),
        )))
    }

    fn advance(&self, by: Duration) {
        self.0.set(self.0.get() + by);
    }

    fn after(&self, by: Duration) -> SystemTime {
        self.0.get() + by
    }
}

impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        self.0.get()
    }
}

fn scheduler() -> (Scheduler<FakeClock>, FakeClock) {
    let clock = FakeClock::new();
    (Scheduler::new(clock.clone()), clock)
}

/// Asserts the scheduler stays quiet until `wait` has passed, then fires on the list.
fn assert_waits_until(scheduler: &mut Scheduler<FakeClock>, clock: &FakeClock, wait: Duration) {
    clock.advance(wait - ONE_SECOND);
    assert!(
        !scheduler.tick(Screen::List),
        "refreshed before the backoff ended"
    );
    clock.advance(ONE_SECOND);
    assert!(
        scheduler.tick(Screen::List),
        "did not refresh when the backoff ended"
    );
}

#[test]
fn tick_after_120_seconds_triggers_a_refresh_with_the_fake_clock() {
    // Arrange
    let (mut scheduler, clock) = scheduler();
    clock.advance(TWO_MINUTES - ONE_SECOND);
    let early = scheduler.tick(Screen::List);

    // Act
    clock.advance(ONE_SECOND);
    let due = scheduler.tick(Screen::List);

    // Assert
    assert!(!early, "refreshed before the interval passed");
    assert!(due, "did not refresh after 120 seconds");
    assert_eq!(REFRESH_INTERVAL, TWO_MINUTES);
}

#[test]
fn manual_refresh_fires_at_once() {
    // Arrange
    let (mut scheduler, _clock) = scheduler();

    // Act
    let fired = scheduler.manual();

    // Assert
    assert!(fired, "the refresh key did not fire at once");
}

#[test]
fn no_auto_refresh_while_detail_is_open() {
    // Arrange
    let (mut scheduler, clock) = scheduler();
    clock.advance(REFRESH_INTERVAL * 2);

    // Act
    let in_detail = scheduler.tick(Screen::Detail);
    let back_on_list = scheduler.tick(Screen::List);

    // Assert
    assert!(!in_detail, "refreshed while detail was open");
    assert!(
        back_on_list,
        "the missed refresh did not run back on the list"
    );
}

#[test]
fn rate_limited_error_backs_off_to_the_retry_time() {
    // Arrange
    let (mut scheduler, clock) = scheduler();
    let error = GithubError::RateLimited {
        retry_at: clock.after(LONG_WAIT),
    };

    // Act
    scheduler.on_error(&error);

    // Assert
    assert_waits_until(&mut scheduler, &clock, LONG_WAIT);
}

#[test]
fn other_errors_do_not_delay_the_next_refresh() {
    // Arrange
    let (mut scheduler, clock) = scheduler();
    clock.advance(TWO_MINUTES);

    // Act
    scheduler.on_error(&GithubError::Network);

    // Assert
    assert!(
        scheduler.tick(Screen::List),
        "a network error delayed the refresh"
    );
}

#[test]
fn only_retry_after_present_backs_off_to_it() {
    // Arrange
    let (mut scheduler, clock) = scheduler();

    // Act
    scheduler.back_off(Some(clock.after(SHORT_WAIT)), None);

    // Assert
    assert_waits_until(&mut scheduler, &clock, SHORT_WAIT);
}

#[test]
fn only_reset_present_backs_off_to_it() {
    // Arrange
    let (mut scheduler, clock) = scheduler();

    // Act
    scheduler.back_off(None, Some(clock.after(SHORT_WAIT)));

    // Assert
    assert_waits_until(&mut scheduler, &clock, SHORT_WAIT);
}

#[test]
fn reset_wins_when_both_are_present_and_reset_is_later() {
    // Arrange
    let (mut scheduler, clock) = scheduler();

    // Act
    scheduler.back_off(Some(clock.after(SHORT_WAIT)), Some(clock.after(LONG_WAIT)));

    // Assert
    assert_waits_until(&mut scheduler, &clock, LONG_WAIT);
}

#[test]
fn retry_after_wins_when_both_are_present_and_reset_is_earlier() {
    // Arrange
    let (mut scheduler, clock) = scheduler();

    // Act
    scheduler.back_off(Some(clock.after(LONG_WAIT)), Some(clock.after(SHORT_WAIT)));

    // Assert
    assert_waits_until(&mut scheduler, &clock, LONG_WAIT);
}
