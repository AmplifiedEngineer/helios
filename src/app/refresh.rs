// SPDX-License-Identifier: Apache-2.0

//! When the list refreshes: a timer, a manual key, a pause in detail and a
//! backoff after a rate limit.

use super::Screen;
use crate::cli::{REFRESH_SECS_MAX, REFRESH_SECS_MIN};
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
    interval: Duration,
    next_due: SystemTime,
    /// When the latest rate-limit backoff ends; a manual refresh waits for it.
    backoff_until: Option<SystemTime>,
}

impl<C: Clock> Scheduler<C> {
    pub fn new(clock: C) -> Self {
        Self::with_interval(clock, REFRESH_INTERVAL)
    }

    /// A scheduler that refreshes every `interval`, kept within the bounds the
    /// `--refresh-secs` flag allows so a zero or huge value cannot spin or overflow.
    pub fn with_interval(clock: C, interval: Duration) -> Self {
        let interval = interval.clamp(
            Duration::from_secs(REFRESH_SECS_MIN),
            Duration::from_secs(REFRESH_SECS_MAX),
        );
        let next_due = clock.now() + interval;
        Self {
            clock,
            interval,
            next_due,
            backoff_until: None,
        }
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

    /// Returns whether the manual refresh may start now. It does not while a
    /// rate-limit backoff is running, so the key cannot hammer GitHub.
    pub fn manual(&mut self) -> bool {
        if self
            .backoff_until
            .is_some_and(|until| self.clock.now() < until)
        {
            return false;
        }
        self.schedule_next();
        true
    }

    /// Delays the next refresh after a failure; only a rate limit has an effect.
    pub fn on_error(&mut self, error: &GithubError) {
        if let GithubError::RateLimited { retry_at } = error {
            self.start_backoff(*retry_at);
        }
    }

    /// Delays the next refresh to the later of the two signals when both are
    /// present, otherwise to the one that is.
    pub fn back_off(&mut self, retry_after: Option<SystemTime>, reset: Option<SystemTime>) {
        // `None` sorts below `Some`, so `max` keeps the one present, or the later of both.
        if let Some(until) = retry_after.max(reset) {
            self.start_backoff(until);
        }
    }

    fn start_backoff(&mut self, until: SystemTime) {
        self.next_due = until;
        self.backoff_until = Some(until);
    }

    fn schedule_next(&mut self) {
        self.backoff_until = None;
        self.next_due = self.clock.now() + self.interval;
    }
}
