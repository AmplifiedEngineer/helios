// SPDX-License-Identifier: Apache-2.0

//! A headless event loop: a test terminal, scripted keys, a fake GitHub and a movable clock.

// A shared helper: each test binary uses only part of it.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, TimeZone, Utc};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use helios::app::loader::{Loader, Source};
use helios::app::refresh::Clock;
use helios::app::worker::Message;
use helios::app::App;
use helios::github::errors::GithubError;
use helios::github::queries::{FilesListing, ListKind, Listing};
use helios::github::GithubApi;
use helios::model::{
    ChangedFile, DetailCounts, PrDetail, PrKey, PrState, PrSummary, ReviewDecision,
};
use helios::sanitize::sanitize;
use helios::tui::run::{EventSource, Flow, Session};
use ratatui::backend::{Backend, ClearType, TestBackend, WindowSize};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Position, Size};
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

use super::files::patch_file;
use super::render::buffer_text;

pub const HOST: &str = "github.com";
pub const LOGIN: &str = "octo";
pub const REPO: &str = "org/repo";
/// One column under the split width, so the loop tests keep the single-pane flow.
/// A later change to the pane drawing revisits this width.
pub const SCREEN_WIDTH: u16 = 99;
/// Wide enough for the split panes.
pub const WIDE_SCREEN_WIDTH: u16 = 120;
pub const SCREEN_HEIGHT: u16 = 24;
/// The first column inside the list pane's left border.
const PANE_TEXT_COLUMN: usize = 1;
const SETTLE_LIMIT: Duration = Duration::from_secs(10);
const HOLD_POLL: Duration = Duration::from_millis(1);
const SETTLE_POLL: Duration = Duration::from_millis(1);

pub fn when() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 5, 10, 0, 0)
        .single()
        .expect("a valid instant")
}

/// The key of the pull request `number` that the fake lists serve for the default login.
pub fn pr_key(number: u64) -> PrKey {
    key_for(LOGIN, number)
}

fn key_for(login: &str, number: u64) -> PrKey {
    PrKey {
        host: HOST.into(),
        account: login.into(),
        repo: REPO.into(),
        number,
    }
}

fn summary(login: &str, number: u64, title: &str) -> PrSummary {
    PrSummary {
        key: key_for(login, number),
        title: sanitize(title),
        author: sanitize("octo"),
        is_draft: false,
        updated_at: when(),
        tags: Vec::new(),
        review_decision: None,
        check_state: None,
    }
}

/// A test terminal that counts how many times a frame is flushed to it.
pub struct CountingBackend {
    inner: TestBackend,
    draws: Arc<AtomicUsize>,
}

impl CountingBackend {
    fn new(width: u16, height: u16, draws: Arc<AtomicUsize>) -> Self {
        Self {
            inner: TestBackend::new(width, height),
            draws,
        }
    }

    pub fn buffer(&self) -> &Buffer {
        self.inner.buffer()
    }
}

impl Backend for CountingBackend {
    type Error = <TestBackend as Backend>::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.draws.fetch_add(1, Ordering::SeqCst);
        self.inner.draw(content)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }
}

/// Answers every call from fixed data, counts the list calls, or fails them all.
pub struct FakeApi {
    /// The changed files every files call returns.
    pub files: Vec<ChangedFile>,
    pub login: String,
    pub list_calls: Arc<AtomicUsize>,
    pub detail_calls: Arc<AtomicUsize>,
    /// Makes every list call fail.
    pub failure: Option<fn() -> GithubError>,
    /// Makes every detail and files call fail.
    pub detail_failure: Option<fn() -> GithubError>,
    /// Makes every list call wait until the flag is set.
    pub hold: Option<Arc<AtomicBool>>,
    /// Makes every detail call wait until the flag is set.
    pub detail_hold: Option<Arc<AtomicBool>>,
}

impl FakeApi {
    pub fn new() -> Self {
        Self::for_login(LOGIN)
    }

    pub fn for_login(login: &str) -> Self {
        Self {
            files: vec![
                patch_file("src/a.rs", "added in a"),
                patch_file("src/b.rs", "added in b"),
            ],
            login: login.into(),
            list_calls: Arc::new(AtomicUsize::new(0)),
            detail_calls: Arc::new(AtomicUsize::new(0)),
            failure: None,
            detail_failure: None,
            hold: None,
            detail_hold: None,
        }
    }

    fn fail(failure: Option<fn() -> GithubError>) -> Result<(), GithubError> {
        failure.map_or(Ok(()), |make| Err(make()))
    }
}

impl GithubApi for FakeApi {
    async fn list(&self, kind: ListKind) -> Result<Listing, GithubError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(hold) = &self.hold {
            while !hold.load(Ordering::SeqCst) {
                tokio::time::sleep(HOLD_POLL).await;
            }
        }
        Self::fail(self.failure)?;
        let login = self.login.as_str();
        let rows = match kind {
            ListKind::Mine => vec![summary(login, 1, "Mine one"), summary(login, 2, "Mine two")],
            ListKind::Review => vec![summary(login, 10, "Review ten")],
            ListKind::Assigned => vec![summary(login, 11, "Assigned eleven")],
            _ => Vec::new(),
        };
        Ok(Listing {
            rows,
            partial: false,
        })
    }

    async fn detail(
        &self,
        _owner: &str,
        _repo: &str,
        number: u64,
    ) -> Result<PrDetail, GithubError> {
        self.detail_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(hold) = &self.detail_hold {
            while !hold.load(Ordering::SeqCst) {
                tokio::time::sleep(HOLD_POLL).await;
            }
        }
        Self::fail(self.detail_failure)?;
        Ok(pr_detail(&format!("Detail of {number}")))
    }

    async fn files(
        &self,
        _owner: &str,
        _repo: &str,
        _number: u64,
    ) -> Result<FilesListing, GithubError> {
        Self::fail(self.detail_failure)?;
        Ok(FilesListing {
            files: self.files.clone(),
            truncated: false,
        })
    }
}

/// A detail with `title`, the same shape the fake serves.
pub fn pr_detail(title: &str) -> PrDetail {
    PrDetail {
        title: sanitize(title),
        body: sanitize("The body text"),
        state: PrState::Open,
        author: sanitize("octo"),
        labels: Vec::new(),
        requested_reviewers: Vec::new(),
        review_decision: Some(ReviewDecision::Approved),
        comments: Vec::new(),
        threads: Vec::new(),
        commits: Vec::new(),
        check_runs: Vec::new(),
        counts: DetailCounts::default(),
    }
}

/// A clock the test moves by hand, in whole milliseconds.
#[derive(Clone)]
pub struct TestClock(Arc<AtomicU64>);

impl TestClock {
    pub fn advance(&self, by: Duration) {
        let millis = u64::try_from(by.as_millis()).expect("a duration that fits in u64 millis");
        self.0.fetch_add(millis, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(self.0.load(Ordering::SeqCst))
    }
}

#[derive(Clone, Default)]
pub struct Script(Arc<Mutex<VecDeque<Event>>>);

impl Script {
    fn push(&self, event: Event) {
        self.0.lock().expect("script lock").push_back(event);
    }
}

impl EventSource for Script {
    fn next(&mut self, _timeout: Duration) -> io::Result<Option<Event>> {
        Ok(self.0.lock().expect("script lock").pop_front())
    }
}

pub struct Harness {
    pub session: Session<CountingBackend, Script, TestClock, FakeApi>,
    pub clock: TestClock,
    draws: Arc<AtomicUsize>,
    pub list_calls: Arc<AtomicUsize>,
    pub detail_calls: Arc<AtomicUsize>,
    script: Script,
    results: Sender<Message>,
    _runtime: tokio::runtime::Runtime,
}

impl Harness {
    /// A loop over one account served by `api`, already settled on its first load.
    pub fn start(api: FakeApi) -> Self {
        let mut harness = Self::start_unsettled(vec![api]);
        harness.settle();
        harness
    }

    /// Like `start`, on a terminal wide enough for the split panes.
    pub fn start_wide(api: FakeApi) -> Self {
        let mut harness = Self::start_sized(vec![api], WIDE_SCREEN_WIDTH);
        harness.settle();
        harness.resize(WIDE_SCREEN_WIDTH);
        harness
    }

    /// Like `start`, on a terminal of `width` by `height`, for a diff that needs room.
    pub fn start_with_size(api: FakeApi, width: u16, height: u16) -> Self {
        let mut harness = Self::start_sized_to(vec![api], width, height);
        harness.settle();
        harness
    }

    /// A loop over `apis`, one account each, with the first load still running.
    pub fn start_unsettled(apis: Vec<FakeApi>) -> Self {
        Self::start_sized(apis, SCREEN_WIDTH)
    }

    fn start_sized(apis: Vec<FakeApi>, width: u16) -> Self {
        Self::start_sized_to(apis, width, SCREEN_HEIGHT)
    }

    fn start_sized_to(apis: Vec<FakeApi>, width: u16, height: u16) -> Self {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let list_calls = apis.first().map_or_else(
            || Arc::new(AtomicUsize::new(0)),
            |api| Arc::clone(&api.list_calls),
        );
        let detail_calls = apis.first().map_or_else(
            || Arc::new(AtomicUsize::new(0)),
            |api| Arc::clone(&api.detail_calls),
        );
        let sources = apis
            .into_iter()
            .map(|api| Source {
                host: HOST.into(),
                login: api.login.clone(),
                api: Arc::new(api),
            })
            .collect();
        let (loader, results) = Loader::new(runtime.handle().clone(), sources);
        let sender = loader.sender();
        let clock = TestClock(Arc::new(AtomicU64::new(
            when().timestamp_millis().unsigned_abs(),
        )));
        let script = Script::default();
        let draws = Arc::new(AtomicUsize::new(0));
        let terminal = Terminal::new(CountingBackend::new(width, height, Arc::clone(&draws)))
            .expect("test terminal");
        let session = Session::new(
            terminal,
            App::new(),
            script.clone(),
            clock.clone(),
            loader,
            results,
        );
        Self {
            session,
            clock,
            draws,
            list_calls,
            detail_calls,
            script,
            results: sender,
            _runtime: runtime,
        }
    }

    /// Sends a resize event and lets the loop handle it, so it sets the split flag.
    pub fn resize(&mut self, width: u16) {
        self.script.push(Event::Resize(width, SCREEN_HEIGHT));
        self.step();
    }

    /// Puts a message on the channel the loads answer on, as a late result would arrive.
    pub fn inject(&self, message: Message) {
        self.results
            .send(message)
            .expect("the session still listens");
    }

    /// Moves the clock by `by`, steps once, and waits for any load that step started.
    pub fn advance_and_step(&mut self, by: Duration) {
        self.clock.advance(by);
        self.step();
        self.settle();
    }

    /// Steps until no load is running, then once more to take in the last results.
    pub fn settle(&mut self) {
        let deadline = Instant::now() + SETTLE_LIMIT;
        while self.session.pending_loads() > 0 {
            assert!(Instant::now() < deadline, "a load never finished");
            thread::sleep(SETTLE_POLL);
        }
        self.step();
    }

    pub fn step(&mut self) -> Flow {
        self.session.step(Duration::ZERO).expect("a step")
    }

    /// Presses a key, lets the loop handle it and every load it starts finish.
    pub fn press(&mut self, code: KeyCode) -> Flow {
        self.press_with(code, KeyModifiers::NONE)
    }

    /// Presses a key and takes exactly one step, so the next step is the next tick.
    pub fn press_once(&mut self, code: KeyCode) -> Flow {
        self.script.push(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }));
        self.step()
    }

    /// How many frames have been flushed to the terminal so far.
    pub fn draws(&self) -> usize {
        self.draws.load(Ordering::SeqCst)
    }

    pub fn press_with(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Flow {
        self.script.push(Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }));
        let flow = self.step();
        self.settle();
        flow
    }

    /// Types a key and steps once without waiting for the loads it starts.
    pub fn type_char_unsettled(&mut self, c: char) -> Flow {
        self.script.push(Event::Key(KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }));
        self.step()
    }

    pub fn type_char(&mut self, c: char) -> Flow {
        self.press(KeyCode::Char(c))
    }

    pub fn buffer(&self) -> &Buffer {
        self.session.terminal().backend().buffer()
    }

    pub fn screen(&self) -> String {
        buffer_text(self.buffer())
    }

    /// The text of the row drawn in reverse video or on a background color, which is the cursor.
    pub fn cursor_line(&self) -> String {
        let buffer = self.buffer();
        let width = usize::from(buffer.area.width);
        buffer
            .content()
            .chunks(width)
            .find(|row| {
                let cell = &row[PANE_TEXT_COLUMN];
                cell.modifier.contains(Modifier::REVERSED) || cell.bg != Color::Reset
            })
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .unwrap_or_default()
    }
}
