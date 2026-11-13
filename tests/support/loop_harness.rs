// SPDX-License-Identifier: Apache-2.0

//! A headless event loop: a test terminal, scripted keys, a fake GitHub and a movable clock.

// A shared helper: each test binary uses only part of it.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, TimeZone, Utc};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use helios::app::loader::{Loader, Source};
use helios::app::refresh::Clock;
use helios::app::App;
use helios::github::errors::GithubError;
use helios::github::queries::{FilesListing, ListKind, Listing};
use helios::github::GithubApi;
use helios::model::{DetailCounts, PrDetail, PrKey, PrState, PrSummary, ReviewDecision};
use helios::sanitize::sanitize;
use helios::tui::run::{EventSource, Flow, Session};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Modifier;
use ratatui::Terminal;

use super::files::patch_file;
use super::render::buffer_text;

pub const HOST: &str = "github.com";
pub const LOGIN: &str = "octo";
pub const REPO: &str = "org/repo";
/// One column under the split width, so the loop tests keep the single-pane flow.
pub const SCREEN_WIDTH: u16 = 99;
pub const SCREEN_HEIGHT: u16 = 24;
const SETTLE_LIMIT: Duration = Duration::from_secs(10);
const HOLD_POLL: Duration = Duration::from_millis(1);
const SETTLE_POLL: Duration = Duration::from_millis(1);

pub fn when() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 5, 10, 0, 0)
        .single()
        .expect("a valid instant")
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

/// Answers every call from fixed data, counts the list calls, or fails them all.
pub struct FakeApi {
    pub login: String,
    pub list_calls: Arc<AtomicUsize>,
    /// Makes every list call fail.
    pub failure: Option<fn() -> GithubError>,
    /// Makes every detail and files call fail.
    pub detail_failure: Option<fn() -> GithubError>,
    /// Makes every list call wait until the flag is set.
    pub hold: Option<Arc<AtomicBool>>,
}

impl FakeApi {
    pub fn new() -> Self {
        Self::for_login(LOGIN)
    }

    pub fn for_login(login: &str) -> Self {
        Self {
            login: login.into(),
            list_calls: Arc::new(AtomicUsize::new(0)),
            failure: None,
            detail_failure: None,
            hold: None,
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
        Self::fail(self.detail_failure)?;
        Ok(PrDetail {
            title: sanitize(&format!("Detail of {number}")),
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
        })
    }

    async fn files(
        &self,
        _owner: &str,
        _repo: &str,
        _number: u64,
    ) -> Result<FilesListing, GithubError> {
        Self::fail(self.detail_failure)?;
        Ok(FilesListing {
            files: vec![
                patch_file("src/a.rs", "added in a"),
                patch_file("src/b.rs", "added in b"),
            ],
            truncated: false,
        })
    }
}

/// A clock the test moves by hand.
#[derive(Clone)]
pub struct TestClock(Arc<AtomicU64>);

impl TestClock {
    pub fn advance(&self, by: Duration) {
        self.0.fetch_add(by.as_secs(), Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(self.0.load(Ordering::SeqCst))
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
    pub session: Session<TestBackend, Script, TestClock, FakeApi>,
    pub clock: TestClock,
    pub list_calls: Arc<AtomicUsize>,
    script: Script,
    _runtime: tokio::runtime::Runtime,
}

impl Harness {
    /// A loop over one account served by `api`, already settled on its first load.
    pub fn start(api: FakeApi) -> Self {
        let mut harness = Self::start_unsettled(vec![api]);
        harness.settle();
        harness
    }

    /// A loop over `apis`, one account each, with the first load still running.
    pub fn start_unsettled(apis: Vec<FakeApi>) -> Self {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let list_calls = apis.first().map_or_else(
            || Arc::new(AtomicUsize::new(0)),
            |api| Arc::clone(&api.list_calls),
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
        let clock = TestClock(Arc::new(AtomicU64::new(when().timestamp().unsigned_abs())));
        let script = Script::default();
        let terminal =
            Terminal::new(TestBackend::new(SCREEN_WIDTH, SCREEN_HEIGHT)).expect("test terminal");
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
            list_calls,
            script,
            _runtime: runtime,
        }
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

    pub fn type_char(&mut self, c: char) -> Flow {
        self.press(KeyCode::Char(c))
    }

    pub fn buffer(&self) -> &Buffer {
        self.session.terminal().backend().buffer()
    }

    pub fn screen(&self) -> String {
        buffer_text(self.buffer())
    }

    /// The text of the row drawn in reverse video, which is the cursor.
    pub fn cursor_line(&self) -> String {
        let buffer = self.buffer();
        let width = usize::from(buffer.area.width);
        buffer
            .content()
            .chunks(width)
            .find(|row| row[0].modifier.contains(Modifier::REVERSED))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .unwrap_or_default()
    }
}
