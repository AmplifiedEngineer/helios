// SPDX-License-Identifier: Apache-2.0

//! Draws the screen the app is on: header, body, footer and the help overlay.

use std::time::SystemTime;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::refresh::Clock;
use crate::app::{App, DetailTab, ListView, Screen};
use crate::tui::keys::{self, Scope};
use crate::tui::{detail, diff, list};

const HEADER_HEIGHT: u16 = 1;
const HEADER_SEPARATOR: &str = "  ";
const LOADING_TEXT: &str = "Loading...";
const EMPTY_TEXT: &str = "No pull requests.";
const NO_ACCOUNTS_TEXT: &str =
    "No GitHub accounts found. Run `gh auth login`, then start helios again.";
const DIFF_HEADER: &str = "Diff";
const HELP_TITLE: &str = "Keys";
const HELP_WIDTH: u16 = 40;
const BORDER_ROWS: u16 = 2;
const TABS: [(DetailTab, &str); 5] = [
    (DetailTab::Overview, "Overview"),
    (DetailTab::Conversation, "Conversation"),
    (DetailTab::Commits, "Commits"),
    (DetailTab::Files, "Files"),
    (DetailTab::Checks, "Checks"),
];
const VIEWS: [(ListView, &str); 2] = [(ListView::Mine, "Mine"), (ListView::NeedsMe, "Needs me")];

/// The wall clock.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Draws the app with ages measured against the wall clock.
pub fn draw(frame: &mut Frame, app: &App) {
    draw_with(frame, app, &SystemClock);
}

/// Draws the app with ages measured against `clock`.
pub fn draw_with(frame: &mut Frame, app: &App, clock: &impl Clock) {
    let scope = Scope::of(app);
    let footer_height = keys::footer_height(scope, frame.area().width);
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(HEADER_HEIGHT),
        Constraint::Min(0),
        Constraint::Length(footer_height),
    ])
    .areas(frame.area());
    frame.render_widget(Paragraph::new(header_line(app)), header);
    match app.screen {
        Screen::List => draw_list(frame, body, app, clock),
        Screen::Detail => detail::render(frame, body, app, clock),
        Screen::Diff => draw_diff(frame, body, app),
    }
    keys::render_footer_for(frame, footer, Some(scope));
    if app.show_help {
        draw_help(frame, body, scope);
    }
}

/// The names in `names` on one line, the current one in brackets and bold.
fn choice_line<T: PartialEq + Copy>(names: &[(T, &'static str)], current: T) -> Line<'static> {
    let mut spans = Vec::new();
    for (value, name) in names {
        if !spans.is_empty() {
            spans.push(Span::raw(HEADER_SEPARATOR));
        }
        if *value == current {
            spans.push(Span::styled(
                format!("[{name}]"),
                Style::new().add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::raw(*name));
        }
    }
    Line::from(spans)
}

fn header_line(app: &App) -> Line<'static> {
    match app.screen {
        Screen::List => {
            let mut line = choice_line(&VIEWS, app.view);
            if let Some(frame) = app.spinner() {
                line.push_span(Span::raw(format!("{HEADER_SEPARATOR}{frame}")));
            }
            line
        }
        Screen::Detail => choice_line(&TABS, app.tab),
        Screen::Diff => Line::from(DIFF_HEADER),
    }
}

fn draw_list(frame: &mut Frame, area: Rect, app: &App, clock: &impl Clock) {
    if app.account_count() == 0 {
        let text = app.notice().unwrap_or(NO_ACCOUNTS_TEXT);
        frame.render_widget(Paragraph::new(text), area);
        return;
    }
    let rows = app.list_rows();
    let empty = rows.is_empty();
    list::render_rows(frame, area, app, rows, clock, Some(app.list_cursor()));
    if empty {
        let text = if app.is_loading() {
            LOADING_TEXT
        } else {
            EMPTY_TEXT
        };
        let below_header = Rect {
            y: area.y.saturating_add(1),
            height: area.height.saturating_sub(1),
            ..area
        };
        frame.render_widget(Paragraph::new(text), below_header);
    }
}

fn draw_diff(frame: &mut Frame, area: Rect, app: &App) {
    if let Some(file) = app.selected_file() {
        let options = diff::DiffOptions { color: true };
        diff::render_scrolled(frame, area, file, options, app.diff_scroll());
    }
}

fn draw_help(frame: &mut Frame, area: Rect, scope: Scope) {
    let width = HELP_WIDTH.min(area.width);
    let height = keys::help_height(scope)
        .saturating_add(BORDER_ROWS)
        .min(area.height);
    let popup = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);
    let block = Block::bordered().title(HELP_TITLE);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    keys::render_help_for(frame, inner, Some(scope));
}
