// SPDX-License-Identifier: Apache-2.0

//! The state the screens draw from and the workers feed.

use std::collections::BTreeMap;

use crate::github::errors::GithubError;
use crate::model::{ChangedFile, PrDetail, PrKey, PrSummary};
use worker::Loaded;

pub mod refresh;
pub mod worker;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    List,
    Detail,
}

/// The tabs of the detail screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailTab {
    #[default]
    Overview,
    Conversation,
    Commits,
    Files,
    Checks,
}

/// What the detail screen shows for the selected pull request.
#[derive(Debug, Default)]
pub enum DetailState {
    #[default]
    Loading,
    Loaded(Box<PrDetail>),
    Failed(GithubError),
}

/// A background load the app asks for; the event loop runs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadRequest {
    /// The detail of one pull request.
    Detail(PrKey),
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
    pub tab: DetailTab,
    detail: DetailState,
    files: Option<Vec<ChangedFile>>,
    requests: Vec<LoadRequest>,
    generation: u64,
    cursor: usize,
    rows: Vec<PrSummary>,
    host_failures: BTreeMap<String, GithubError>,
}

impl DetailTab {
    const ORDER: [Self; 5] = [
        Self::Overview,
        Self::Conversation,
        Self::Commits,
        Self::Files,
        Self::Checks,
    ];

    fn position(self) -> usize {
        Self::ORDER.iter().position(|tab| *tab == self).unwrap_or(0)
    }

    /// The tab after this one, wrapping from the last to the first.
    pub fn next(self) -> Self {
        Self::ORDER[(self.position() + 1) % Self::ORDER.len()]
    }

    /// The tab before this one, wrapping from the first to the last.
    pub fn previous(self) -> Self {
        Self::ORDER[(self.position() + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }
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

    /// Opens a pull request from the list: shows the loading state and asks for its detail.
    pub fn open(&mut self, key: PrKey) {
        self.select(key.clone());
        self.requests.push(LoadRequest::Detail(key));
    }

    /// The loads asked for and not yet taken by the event loop.
    pub fn requests(&self) -> &[LoadRequest] {
        &self.requests
    }

    /// Keeps the changed files of the open pull request for the Files tab.
    pub fn apply_files(&mut self, key: &PrKey, files: Vec<ChangedFile>) {
        if self.selected.as_ref() == Some(key) {
            self.files = Some(files);
        }
    }

    /// The changed files of the open pull request, once loaded.
    pub fn files(&self) -> Option<&[ChangedFile]> {
        self.files.as_deref()
    }

    pub fn select(&mut self, key: PrKey) {
        if let Some(index) = self.index_of(&key) {
            self.cursor = index;
        }
        self.selected = Some(key);
        self.detail = DetailState::Loading;
        self.files = None;
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
