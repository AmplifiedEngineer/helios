// SPDX-License-Identifier: Apache-2.0

//! The state the screens draw from and the workers feed.

use std::collections::BTreeMap;

use crate::github::errors::GithubError;
use crate::model::{PrDetail, PrKey, PrSummary};
use worker::Loaded;

pub mod refresh;
pub mod worker;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    List,
    Detail,
}

/// What the detail screen shows for the selected pull request.
#[derive(Debug, Default)]
pub enum DetailState {
    #[default]
    Loading,
    Loaded(Box<PrDetail>),
    Failed(GithubError),
}

/// Which tagged rows the Needs me list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListFilter {
    #[default]
    All,
    Review,
    Assigned,
}

#[derive(Debug, Default)]
pub struct App {
    pub screen: Screen,
    pub selected: Option<PrKey>,
    pub filter: ListFilter,
    pub show_drafts: bool,
    detail: DetailState,
    generation: u64,
    cursor: usize,
    rows: Vec<PrSummary>,
    host_failures: BTreeMap<String, GithubError>,
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a refresh and returns its generation.
    pub fn begin_refresh(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// Moves the filter to the next of all, review and assigned, and back to all.
    pub fn cycle_filter(&mut self) {
        self.filter = match self.filter {
            ListFilter::All => ListFilter::Review,
            ListFilter::Review => ListFilter::Assigned,
            ListFilter::Assigned => ListFilter::All,
        };
    }

    /// Shows or hides draft pull requests.
    pub fn toggle_drafts(&mut self) {
        self.show_drafts = !self.show_drafts;
    }

    pub fn rows(&self) -> &[PrSummary] {
        &self.rows
    }

    /// The hosts whose latest load failed, by host name.
    pub fn host_failures(&self) -> &BTreeMap<String, GithubError> {
        &self.host_failures
    }

    pub fn select(&mut self, key: PrKey) {
        if let Some(index) = self.index_of(&key) {
            self.cursor = index;
        }
        self.selected = Some(key);
        self.detail = DetailState::Loading;
    }

    /// The key of the row the cursor is on.
    pub fn cursor(&self) -> Option<&PrKey> {
        self.rows.get(self.cursor).map(|row| &row.key)
    }

    pub fn detail(&self) -> &DetailState {
        &self.detail
    }

    /// Shows a detail load, unless the selection has moved to another PR.
    pub fn apply_detail(&mut self, key: &PrKey, result: Result<PrDetail, GithubError>) {
        if self.selected.as_ref() != Some(key) {
            return;
        }
        self.detail = match result {
            Ok(detail) => DetailState::Loaded(Box::new(detail)),
            Err(error) => DetailState::Failed(error),
        };
    }

    fn index_of(&self, key: &PrKey) -> Option<usize> {
        self.rows.iter().position(|row| &row.key == key)
    }

    /// Puts the cursor back on the selected PR, or on the nearest row when it is gone.
    fn restore_cursor(&mut self) {
        let found = self.selected.as_ref().and_then(|key| self.index_of(key));
        if let Some(index) = found {
            self.cursor = index;
            return;
        }
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
        self.selected = self.cursor().cloned();
    }

    /// Returns whether the result was applied.
    pub fn apply(&mut self, loaded: Loaded) -> bool {
        if loaded.generation < self.generation {
            return false;
        }
        match loaded.result {
            Ok(rows) => {
                self.host_failures.remove(&loaded.host);
                self.rows = rows;
                self.restore_cursor();
            }
            Err(error) => {
                self.host_failures.insert(loaded.host, error);
            }
        }
        true
    }
}
