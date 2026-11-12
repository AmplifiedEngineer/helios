// SPDX-License-Identifier: Apache-2.0

//! A rate-limit backoff never holds the refresh longer than the scheduler maximum.

use helios::app::refresh::{Clock, Scheduler};
use helios::app::Screen;
use helios::cli::REFRESH_SECS_MAX;
use helios::github::errors::GithubError;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const START_SECS: u64 = 1_000_000;
/// Far enough ahead to be absurd, and still inside the range `SystemTime` holds.
const FAR_FUTURE_SECS: u64 = 1 << 60;
const ONE_SECOND: Duration = Duration::from_secs(1);
const CAP: Duration = Duration::from_secs(REFRESH_SECS_MAX);

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

fn rate_limited(retry_at: SystemTime) -> GithubError {
    GithubError::RateLimited { retry_at }
}

#[test]
fn a_far_future_reset_does_not_block_the_manual_refresh_past_the_cap() {
    // Arrange
    let clock = FakeClock::new();
    let mut scheduler = Scheduler::new(clock.clone());
    scheduler.on_error(&rate_limited(
        UNIX_EPOCH + Duration::from_secs(FAR_FUTURE_SECS),
    ));

    // Act
    clock.advance(CAP);

    // Assert
    assert!(scheduler.manual(), "the backoff outlived the cap");
}

#[test]
fn a_far_future_reset_does_not_stop_the_automatic_refresh_past_the_cap() {
    // Arrange
    let clock = FakeClock::new();
    let mut scheduler = Scheduler::new(clock.clone());
    scheduler.back_off(
        None,
        Some(UNIX_EPOCH + Duration::from_secs(FAR_FUTURE_SECS)),
    );

    // Act
    clock.advance(CAP);

    // Assert
    assert!(scheduler.tick(Screen::List), "the refresh never came due");
}

#[test]
fn a_reset_just_over_the_cap_is_cut_to_the_cap() {
    // Arrange
    let clock = FakeClock::new();
    let mut scheduler = Scheduler::new(clock.clone());
    scheduler.on_error(&rate_limited(clock.after(CAP + ONE_SECOND)));

    // Act
    clock.advance(CAP);

    // Assert
    assert!(scheduler.manual(), "one second over the cap was kept");
}

#[test]
fn a_reset_inside_the_cap_is_kept() {
    // Arrange
    let clock = FakeClock::new();
    let mut scheduler = Scheduler::new(clock.clone());
    scheduler.on_error(&rate_limited(clock.after(CAP - ONE_SECOND)));

    // Act
    clock.advance(CAP - ONE_SECOND - ONE_SECOND);

    // Assert
    assert!(!scheduler.manual(), "the backoff ended early");
}
