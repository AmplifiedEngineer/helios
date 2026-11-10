// SPDX-License-Identifier: Apache-2.0

//! When the list refreshes: a timer, a manual key, a pause in detail and a
//! backoff after a rate limit.

use super::Screen;
use crate::github::errors::GithubError;
use std::time::{Duration, SystemTime};

/// How long the list waits between automatic refreshes.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(120);

/// The source of the current time, so tests can move it without sleeping.
pub trait Clock {
    fn now(&self) -> SystemTime;
}

#[derive(Debug)]
pub struct Scheduler<C: Clock> {
    clock: C,
    next_due: SystemTime,
}

impl<C: Clock> Scheduler<C> {
    pub fn new(clock: C) -> Self {
        let next_due = clock.now() + REFRESH_INTERVAL;
        Self { clock, next_due }
    }

    /// Returns whether an automatic refresh is due now. A refresh that falls
    /// due while detail is open runs on the first tick back on the list.
    pub fn tick(&mut self, screen: Screen) -> bool {
        if screen == Screen::Detail || self.clock.now() < self.next_due {
            return false;
        }
        self.schedule_next();
        true
    }

    /// Returns whether the manual refresh may start now.
    pub fn manual(&mut self) -> bool {
        self.schedule_next();
        true
    }

    /// Delays the next refresh after a failure; only a rate limit has an effect.
    pub fn on_error(&mut self, error: &GithubError) {
        if let GithubError::RateLimited { retry_at } = error {
            self.next_due = *retry_at;
        }
    }

    /// Delays the next refresh to the later of the two signals when both are
    /// present, otherwise to the one that is.
    pub fn back_off(&mut self, retry_after: Option<SystemTime>, reset: Option<SystemTime>) {
        // `None` sorts below `Some`, so `max` keeps the one present, or the later of both.
        if let Some(until) = retry_after.max(reset) {
            self.next_due = until;
        }
    }

    fn schedule_next(&mut self) {
        self.next_due = self.clock.now() + REFRESH_INTERVAL;
    }
}
