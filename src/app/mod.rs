// SPDX-License-Identifier: Apache-2.0

//! The state the screens draw from and the workers feed.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use crate::github::errors::GithubError;
use crate::model::{ChangedFile, PrDetail, PrKey, PrSummary, Tag};
use worker::Loaded;

pub mod loader;
mod nav;
pub mod refresh;
pub mod worker;

pub use nav::Step;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    List,
    Detail,
    /// The diff of one file chosen on the Files tab.
    Diff,
}

/// Which list the list screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListView {
    #[default]
    Mine,
    NeedsMe,
}

impl ListView {
    /// The other list.
    pub fn other(self) -> Self {
        match self {
            Self::Mine => Self::NeedsMe,
            Self::NeedsMe => Self::Mine,
        }
    }
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
    /// The changed files of one pull request.
    Files(PrKey),
}

/// Which tagged rows the Needs me list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListFilter {
    #[default]
    All,
    Review,
    Assigned,
}

/// The lists each account loads: Mine and Needs me.
const LISTS_PER_ACCOUNT: usize = 2;
const SPINNER_FRAMES: [char; 4] = ['|', '/', '-', '\\'];

/// The host and login of one account.
pub type AccountKey = (String, String);

impl ListFilter {
    fn matches(self, row: &PrSummary) -> bool {
        match self {
            Self::All => true,
            Self::Review => row.tags.contains(&Tag::Review),
            Self::Assigned => row.tags.contains(&Tag::Assigned),
        }
    }
}

#[derive(Debug, Default)]
pub struct App {
    pub screen: Screen,
    pub selected: Option<PrKey>,
    pub filter: ListFilter,
    pub show_drafts: bool,
    pub tab: DetailTab,
    /// Whether the help overlay is open.
    pub show_help: bool,
    pub view: ListView,
    detail: DetailState,
    files: Option<Vec<ChangedFile>>,
    files_failure: Option<GithubError>,
    files_truncated: bool,
    files_requested: bool,
    file_cursor: usize,
    diff_scroll: usize,
    detail_scroll: usize,
    /// The furthest the open tab can scroll; the detail view records it each draw.
    detail_scroll_max: Cell<usize>,
    list_cursor: usize,
    mine_rows: Vec<PrSummary>,
    account_count: usize,
    answered: usize,
    notice: Option<String>,
    alert: Option<&'static str>,
    spinner_frame: usize,
    requests: Vec<LoadRequest>,
    generation: u64,
    cursor: usize,
    rows: Vec<PrSummary>,
    host_failures: BTreeMap<AccountKey, GithubError>,
    partial_accounts: BTreeSet<AccountKey>,
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
        self.answered = 0;
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

    /// The rows the list shows: drafts only when on, and with `by_tag` only the rows the
    /// filter selects.
    pub fn visible_rows(&self, by_tag: bool) -> Vec<&PrSummary> {
        self.rows
            .iter()
            .filter(|row| row.is_visible(self.show_drafts))
            .filter(|row| !by_tag || self.filter.matches(row))
            .collect()
    }

    /// The accounts whose latest load failed, by host and login.
    pub fn host_failures(&self) -> &BTreeMap<AccountKey, GithubError> {
        &self.host_failures
    }

    /// Whether any account's latest list was cut short by GitHub.
    pub fn has_partial_list(&self) -> bool {
        !self.partial_accounts.is_empty()
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

    /// Hands the pending loads to the event loop, so none runs twice.
    pub fn take_requests(&mut self) -> Vec<LoadRequest> {
        std::mem::take(&mut self.requests)
    }

    /// Keeps the changed files of the open pull request for the Files tab.
    pub fn apply_files(&mut self, key: &PrKey, files: Vec<ChangedFile>) {
        if self.selected.as_ref() == Some(key) {
            self.files = Some(files);
        }
    }

    /// Notes that the pull request has more files than the Files tab loaded.
    pub fn mark_files_truncated(&mut self, key: &PrKey) {
        if self.selected.as_ref() == Some(key) {
            self.files_truncated = true;
        }
    }

    /// Whether the loaded files are only the first part of the pull request's files.
    pub fn files_truncated(&self) -> bool {
        self.files_truncated
    }

    /// The changed files of the open pull request, once loaded.
    pub fn files(&self) -> Option<&[ChangedFile]> {
        self.files.as_deref()
    }

    /// Records a failed files load for the open pull request, so the Files tab can say so.
    pub fn apply_files_failure(&mut self, key: &PrKey, error: GithubError) {
        if self.selected.as_ref() == Some(key) {
            self.files_failure = Some(error);
            self.files_requested = false;
        }
    }

    /// Why the files of the open pull request did not load, if they did not.
    pub fn files_failure(&self) -> Option<&GithubError> {
        self.files_failure.as_ref()
    }

    pub fn select(&mut self, key: PrKey) {
        if let Some(index) = self.index_of(&key) {
            self.cursor = index;
        }
        self.selected = Some(key);
        self.detail = DetailState::Loading;
        self.reset_files();
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
        match self.cursor().cloned() {
            Some(key) => self.select(key),
            None => self.clear_selection(),
        }
    }

    fn clear_selection(&mut self) {
        self.selected = None;
        self.detail = DetailState::Loading;
        self.reset_files();
    }

    fn reset_files(&mut self) {
        self.files = None;
        self.files_failure = None;
        self.files_truncated = false;
        self.files_requested = false;
        self.file_cursor = 0;
        self.diff_scroll = 0;
        self.detail_scroll = 0;
    }

    /// Returns whether the result was applied.
    pub fn apply(&mut self, loaded: Loaded) -> bool {
        if loaded.generation < self.generation {
            return false;
        }
        self.answered += 1;
        self.keep_highlight(|app| match loaded.result {
            Ok(rows) => {
                app.clear_failure(&loaded.host, &loaded.account);
                app.track_partial(&loaded.host, &loaded.account, loaded.partial);
                app.rows
                    .retain(|row| row.key.host != loaded.host || row.key.account != loaded.account);
                app.rows.extend(rows);
                if app.screen == Screen::List {
                    app.restore_cursor();
                }
            }
            Err(error) => app.record_failure(loaded.host, loaded.account, error),
        });
        true
    }

    /// Like `apply`, for one account's rows of the Mine list. Returns whether it was applied.
    pub fn apply_mine(&mut self, loaded: Loaded) -> bool {
        if loaded.generation < self.generation {
            return false;
        }
        self.answered += 1;
        self.keep_highlight(|app| match loaded.result {
            Ok(rows) => {
                app.clear_failure(&loaded.host, &loaded.account);
                app.track_partial(&loaded.host, &loaded.account, loaded.partial);
                app.mine_rows
                    .retain(|row| row.key.host != loaded.host || row.key.account != loaded.account);
                app.mine_rows.extend(rows);
            }
            Err(error) => app.record_failure(loaded.host, loaded.account, error),
        });
        true
    }

    fn track_partial(&mut self, host: &str, account: &str, partial: bool) {
        let account = (host.to_owned(), account.to_owned());
        if partial {
            self.partial_accounts.insert(account);
        } else {
            self.partial_accounts.remove(&account);
        }
    }

    fn clear_failure(&mut self, host: &str, account: &str) {
        self.host_failures
            .remove(&(host.to_owned(), account.to_owned()));
    }

    fn record_failure(&mut self, host: String, account: String, error: GithubError) {
        self.host_failures.insert((host, account), error);
    }

    /// Tells the app how many accounts it loads for, so it can tell loading from empty.
    pub fn set_account_count(&mut self, count: usize) {
        self.account_count = count;
    }

    pub fn account_count(&self) -> usize {
        self.account_count
    }

    /// Whether the current refresh still waits for an answer: each account answers once for
    /// each of the two lists.
    pub fn is_loading(&self) -> bool {
        self.answered < self.account_count * LISTS_PER_ACCOUNT
    }

    /// Moves the spinner on one frame while a load is running.
    pub fn tick(&mut self) {
        if self.is_loading() {
            self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
        }
    }

    /// The spinner frame to draw, or `None` when nothing is loading.
    pub fn spinner(&self) -> Option<char> {
        self.is_loading()
            .then(|| SPINNER_FRAMES.get(self.spinner_frame).copied())
            .flatten()
    }

    /// A fixed note for the screen, such as why no account could be read.
    pub fn set_notice(&mut self, notice: String) {
        self.notice = Some(notice);
    }

    /// A short fixed line for the header, cleared by the next key press.
    pub fn set_alert(&mut self, alert: &'static str) {
        self.alert = Some(alert);
    }

    pub fn alert(&self) -> Option<&'static str> {
        self.alert
    }

    pub fn clear_alert(&mut self) {
        self.alert = None;
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }
}
