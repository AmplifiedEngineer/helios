// SPDX-License-Identifier: Apache-2.0

//! The event loop: keys in, background results in, one frame out.

use std::io;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};
use ratatui::backend::Backend;
use ratatui::Terminal;

use crate::app::loader::Loader;
use crate::app::refresh::{Clock, Scheduler};
use crate::app::worker::Message;
use crate::app::{App, ListView, Screen};
use crate::browser;
use crate::github::errors::GithubError;
use crate::github::GithubApi;
use crate::tui::cache::RenderCache;
use crate::tui::draw::draw_cached_with;
use crate::tui::keys::{handle_key_with, KeyMap, Outcome};

/// How long one step waits for a key before it looks at the background results again.
pub const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Where the loop reads terminal events from, so a test can script them.
pub trait EventSource {
    /// The next event, or `None` when none arrives within `timeout`.
    fn next(&mut self, timeout: Duration) -> io::Result<Option<Event>>;
}

/// The events of the real terminal.
pub struct TerminalEvents;

impl EventSource for TerminalEvents {
    fn next(&mut self, timeout: Duration) -> io::Result<Option<Event>> {
        if event::poll(timeout)? {
            event::read().map(Some)
        } else {
            Ok(None)
        }
    }
}

/// Whether the loop goes on after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Running,
    Quit,
}

/// What the loop calls with the address of a pull request to show it in a browser, and the
/// channel to report a failure on.
pub type Opener = Box<dyn FnMut(&str, &Sender<Message>)>;

/// The fixed text shown when the browser did not open.
const OPEN_FAILED_ALERT: &str = "Could not open the browser";

/// Opens the address with the system opener on its own thread, so the loop never waits for
/// it, and sends a failure back as a message.
fn system_opener() -> Opener {
    Box::new(|url, results| {
        let url = url.to_owned();
        let results = results.clone();
        thread::spawn(move || {
            if browser::open(&url).is_err() {
                // The loop has quit when this fails, and nobody is left to tell.
                let _ = results.send(Message::OpenFailed);
            }
        });
    })
}

/// Everything one run of the loop owns.
pub struct Session<B: Backend, E, C: Clock, A> {
    terminal: Terminal<B>,
    app: App,
    keys: KeyMap,
    events: E,
    clock: C,
    cache: RenderCache,
    scheduler: Scheduler<C>,
    loader: Loader<A>,
    results: Receiver<Message>,
    sender: Sender<Message>,
    opener: Opener,
    dirty: bool,
}

impl<B, E, C, A> Session<B, E, C, A>
where
    B: Backend,
    E: EventSource,
    C: Clock + Clone,
    A: GithubApi + 'static,
{
    /// Builds the session and starts the first load of every account.
    pub fn new(
        terminal: Terminal<B>,
        mut app: App,
        events: E,
        clock: C,
        loader: Loader<A>,
        results: Receiver<Message>,
    ) -> Self {
        app.set_account_count(loader.source_count());
        let scheduler = Scheduler::new(clock.clone());
        let sender = loader.sender();
        let mut session = Self {
            terminal,
            app,
            keys: KeyMap::default(),
            events,
            clock,
            cache: RenderCache::new(),
            scheduler,
            loader,
            results,
            sender,
            opener: system_opener(),
            dirty: true,
        };
        session.start_refresh();
        session
    }

    /// Sets how long the list waits between automatic refreshes.
    pub fn set_refresh_interval(&mut self, interval: Duration) {
        self.scheduler = Scheduler::with_interval(self.clock.clone(), interval);
    }

    /// Sets the keys the loop reads and the footer and help show.
    pub fn set_keys(&mut self, keys: KeyMap) {
        self.keys = keys;
        self.dirty = true;
    }

    /// Replaces the browser opener, for a test.
    pub fn set_opener(&mut self, opener: Opener) {
        self.opener = opener;
    }

    pub fn app(&self) -> &App {
        &self.app
    }

    pub fn terminal(&self) -> &Terminal<B> {
        &self.terminal
    }

    /// How many background loads have not yet sent their result.
    pub fn pending_loads(&self) -> usize {
        self.loader.pending()
    }

    /// Runs steps until the user quits.
    pub fn run(&mut self) -> io::Result<()> {
        while self.step(POLL_INTERVAL)? == Flow::Running {}
        Ok(())
    }

    /// One turn of the loop: results, the refresh timer, at most one event, then a frame
    /// when anything changed. It waits at most `timeout` for the event.
    pub fn step(&mut self, timeout: Duration) -> io::Result<Flow> {
        // The last frame left diff lines unhighlighted, so draw again to fill in more.
        if self.cache.take_pending() {
            self.dirty = true;
        }
        self.drain_results();
        if self.app.is_loading() {
            self.app.tick();
            self.dirty = true;
        }
        if self.scheduler.tick(scheduler_screen(self.app.screen)) {
            self.start_refresh();
        }
        let flow = match self.events.next(timeout)? {
            Some(event) => self.handle_event(event),
            None => Flow::Running,
        };
        self.app.fire_preview(self.clock.now());
        self.run_requests();
        self.drain_results();
        if self.dirty {
            self.draw()?;
        }
        Ok(flow)
    }

    /// Sets the split flag from `width`, or from the terminal's current width when unknown.
    fn sync_split(&mut self, width: Option<u16>) {
        let width = width.or_else(|| self.terminal.size().ok().map(|size| size.width));
        if let Some(width) = width {
            self.app.sync_split(width);
        }
    }

    fn handle_event(&mut self, event: Event) -> Flow {
        self.dirty = true;
        let resized = match event {
            Event::Resize(width, _) => Some(width),
            _ => None,
        };
        self.sync_split(resized);
        let Event::Key(key) = event else {
            return Flow::Running;
        };
        if key.kind != KeyEventKind::Press {
            return Flow::Running;
        }
        self.app.clear_alert();
        let outcome = handle_key_with(&mut self.app, key, &self.keys);
        self.app.schedule_preview(self.clock.now());
        match outcome {
            Outcome::Continue => Flow::Running,
            Outcome::Quit => Flow::Quit,
            Outcome::OpenInBrowser => {
                let url = self.app.current_pr().and_then(browser::pr_url);
                if let Some(url) = url {
                    (self.opener)(&url, &self.sender);
                }
                Flow::Running
            }
            Outcome::Refresh => {
                if self.scheduler.manual() {
                    self.start_refresh();
                }
                Flow::Running
            }
        }
    }

    fn start_refresh(&mut self) {
        let generation = self.app.begin_refresh();
        self.loader.refresh(generation);
        self.dirty = true;
    }

    fn run_requests(&mut self) {
        for request in self.app.take_requests() {
            self.loader.load(request);
        }
    }

    fn drain_results(&mut self) {
        while let Ok(message) = self.results.try_recv() {
            self.dirty = true;
            match message {
                Message::List(view, loaded) => {
                    self.note_failure(loaded.result.as_ref().err());
                    match view {
                        ListView::Mine => self.app.apply_mine(loaded),
                        ListView::NeedsMe => self.app.apply(loaded),
                    };
                }
                Message::Detail(key, result) => {
                    self.note_failure(result.as_ref().as_ref().err());
                    self.app.note_detail_answered(&key);
                    self.app.apply_detail(&key, *result);
                }
                Message::OpenFailed => self.app.set_alert(OPEN_FAILED_ALERT),
                Message::Files(key, result) => {
                    self.note_failure(result.as_ref().err());
                    match result {
                        Ok(listing) => {
                            self.app.apply_files(&key, listing.files);
                            if listing.truncated {
                                self.app.mark_files_truncated(&key);
                            }
                        }
                        Err(error) => self.app.apply_files_failure(&key, error),
                    }
                }
            }
        }
    }

    fn note_failure(&mut self, error: Option<&GithubError>) {
        if let Some(error) = error {
            self.scheduler.on_error(error);
        }
    }

    fn draw(&mut self) -> io::Result<()> {
        let (app, clock, cache, keys) = (&self.app, &self.clock, &self.cache, &self.keys);
        self.terminal
            .draw(|frame| draw_cached_with(frame, app, clock, cache, keys))
            .map_err(|error| io::Error::other(error.to_string()))?;
        self.dirty = false;
        Ok(())
    }
}

/// The refresh timer pauses on every screen but the list.
fn scheduler_screen(screen: Screen) -> Screen {
    match screen {
        Screen::List => Screen::List,
        Screen::Detail | Screen::Diff => Screen::Detail,
    }
}
