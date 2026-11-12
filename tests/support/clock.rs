// SPDX-License-Identifier: Apache-2.0

//! A clock that always reads the same instant.

use chrono::{TimeZone, Utc};
use helios::app::refresh::Clock;
use std::time::SystemTime;

/// A clock stopped at one instant.
pub struct FixedClock(SystemTime);

impl FixedClock {
    /// A clock stopped at the given UTC date and time.
    pub fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> Self {
        let instant = Utc
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .expect("a valid instant");
        Self(instant.into())
    }

    /// A clock stopped two hours after the list fixtures' `updatedAt`.
    pub fn after_the_list_fixtures() -> Self {
        Self::at(2026, 11, 5, 12, 0, 0)
    }
}

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}
