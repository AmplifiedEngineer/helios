// SPDX-License-Identifier: Apache-2.0

pub mod cache;
pub mod detail;
pub mod diff;
pub mod draw;
pub mod keys;
pub mod list;
pub mod markdown;
pub mod mine;
pub mod needs_me;
pub mod run;

pub use draw::{draw, draw_with, SystemClock};

use std::io::{self, Stdout, Write};
use std::panic;

use crossterm::cursor::Show;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};

type Tui = Terminal<CrosstermBackend<Stdout>>;

const FALLBACK_WIDTH: u16 = 80;
const FALLBACK_HEIGHT: u16 = 24;

/// Switches to the alternate screen in raw mode.
///
/// With `tolerate_raw_mode_failure` a raw-mode error is ignored, so the shell can start
/// where stdin is not a terminal, and an unknown terminal size falls back to a fixed one.
pub fn init_terminal(tolerate_raw_mode_failure: bool) -> io::Result<Tui> {
    match enable_raw_mode() {
        Err(error) if !tolerate_raw_mode_failure => return Err(error),
        Err(_) | Ok(()) => {}
    }
    execute!(io::stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    match Terminal::new(backend) {
        Err(_) if tolerate_raw_mode_failure => Terminal::with_options(
            CrosstermBackend::new(io::stdout()),
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, FALLBACK_WIDTH, FALLBACK_HEIGHT)),
            },
        ),
        result => result,
    }
}

/// Leaves the alternate screen, shows the cursor and turns raw mode off.
///
/// Every step runs even when an earlier one fails, so a panic never leaves the terminal broken.
pub fn restore_terminal() {
    let mut stdout = io::stdout();
    let _ = execute!(stdout, LeaveAlternateScreen, Show);
    let _ = stdout.flush();
    let _ = disable_raw_mode();
}

/// Restores the terminal before the previous panic hook prints its message.
pub fn install_panic_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));
}
