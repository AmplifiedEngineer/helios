// SPDX-License-Identifier: Apache-2.0

//! Draws the screen the app is on: bordered panes, footer and the help overlay.

use std::time::SystemTime;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use ratatui::Frame;

use crate::app::refresh::Clock;
use crate::app::{App, DetailTab, FileRow, ListView, Screen};
use crate::sanitize::sanitize;
use crate::tui::cache::{HighlightBudget, RenderCache};
use crate::tui::diff::Highlighting;
use crate::tui::keys::{self, Scope};
use crate::tui::layout::{self, Panes, MIN_TERMINAL_HEIGHT, MIN_TERMINAL_WIDTH};
use crate::tui::style::Styles;
use crate::tui::{detail, diff, list};

const HEADER_SEPARATOR: &str = "  ";
const FOCUS_MARKER: &str = "> ";
const FOCUSED_BORDER: BorderType = BorderType::Thick;
const UNFOCUSED_BORDER: BorderType = BorderType::Plain;
const ROW_COUNT_UNIT: &str = "PRs";
const NO_SELECTION_TEXT: &str = "Select a pull request to preview it.";
const TOO_SMALL_PREFIX: &str = "Terminal too small: needs at least";
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

/// Draws the app, reusing rendered lines from `cache`.
pub fn draw_cached(frame: &mut Frame, app: &App, clock: &impl Clock, cache: &RenderCache) {
    // Only a diff drawn in this frame can ask for another one.
    cache.clear_pending();
    let budget = HighlightBudget::per_frame();
    let area = frame.area();
    if area.width < MIN_TERMINAL_WIDTH || area.height < MIN_TERMINAL_HEIGHT {
        let text = format!("{TOO_SMALL_PREFIX} {MIN_TERMINAL_WIDTH}x{MIN_TERMINAL_HEIGHT}");
        frame.render_widget(Paragraph::new(text), area);
        return;
    }
    let scope = Scope::of(app);
    let footer_height = keys::footer_height(scope, area.width);
    let [body, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(footer_height)]).areas(area);
    draw_panes(frame, body, app, clock, cache, &budget);
    keys::render_footer_for(frame, footer, Some(scope));
    if app.show_help {
        draw_help(frame, body, scope);
    }
}

/// Draws the app with ages measured against `clock`.
pub fn draw_with(frame: &mut Frame, app: &App, clock: &impl Clock) {
    draw_cached(frame, app, clock, &RenderCache::new());
}

/// The names in `names` on one line, the current one in brackets and bold.
fn choice_line<T: PartialEq + Copy, S: AsRef<str>>(
    names: &[(T, S)],
    current: T,
    current_style: Style,
) -> Line<'static> {
    let mut spans = Vec::new();
    for (value, name) in names {
        if !spans.is_empty() {
            spans.push(Span::raw(HEADER_SEPARATOR));
        }
        if *value == current {
            spans.push(Span::styled(format!("[{}]", name.as_ref()), current_style));
        } else {
            spans.push(Span::raw(name.as_ref().to_owned()));
        }
    }
    Line::from(spans)
}

/// The tab names, with the Files tab carrying the count of test files once they are known.
fn tab_labels(app: &App) -> Vec<(DetailTab, String)> {
    let tests = app.file_rows().iter().find_map(|row| match row {
        FileRow::Tests(count) => Some(*count),
        FileRow::File(_) => None,
    });
    TABS.iter()
        .map(|(tab, name)| match (tab, tests) {
            (DetailTab::Files, Some(count)) => (*tab, format!("{name} ({count} tests)")),
            _ => (*tab, (*name).to_owned()),
        })
        .collect()
}

/// Whether a pane carries the focus marker, and if so whether it holds the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Unmarked,
    Focused,
    Unfocused,
}

/// A bordered pane with `title` on the left and `note` on the right.
fn pane_block(
    mut title: Line<'static>,
    note: String,
    focus: Focus,
    styles: &Styles,
) -> Block<'static> {
    let (border, border_style) = if focus == Focus::Focused {
        (FOCUSED_BORDER, styles.border_focused)
    } else {
        (UNFOCUSED_BORDER, styles.border)
    };
    if focus == Focus::Focused {
        title.spans.insert(0, Span::raw(FOCUS_MARKER));
    }
    Block::bordered()
        .border_type(border)
        .border_style(border_style)
        .title_style(styles.title)
        .title(title)
        .title(Line::from(note).right_aligned())
}

/// The alert when one is set and the screen is `shown_on`, else the pane's own note.
fn note_or_alert(app: &App, shown_on: Screen, note: impl FnOnce() -> String) -> String {
    match app.alert() {
        Some(alert) if app.screen == shown_on => alert.to_owned(),
        _ => note(),
    }
}

/// Draws the screen's panes inside `body`: two side by side on a split list screen, else one.
fn draw_panes(
    frame: &mut Frame,
    body: Rect,
    app: &App,
    clock: &impl Clock,
    cache: &RenderCache,
    budget: &HighlightBudget,
) {
    if app.is_split() && app.screen == Screen::List {
        if let Panes::Split { list, detail } = layout::panes(body, app.pane, &app.layout()) {
            let (list_focus, detail_focus) = match app.pane {
                layout::Pane::List => (Focus::Focused, Focus::Unfocused),
                layout::Pane::Detail => (Focus::Unfocused, Focus::Focused),
            };
            draw_list_pane(frame, list, app, clock, list_focus);
            draw_detail_pane(frame, detail, app, (clock, cache, budget), detail_focus);
            return;
        }
    }
    match app.screen {
        Screen::List => draw_list_pane(frame, body, app, clock, Focus::Unmarked),
        Screen::Detail => {
            draw_detail_pane(frame, body, app, (clock, cache, budget), Focus::Unmarked)
        }
        Screen::Diff => draw_diff_pane(frame, body, app, cache, budget),
    }
}

fn draw_list_pane(frame: &mut Frame, area: Rect, app: &App, clock: &impl Clock, focus: Focus) {
    let styles = Styles::of(app);
    let mut title = choice_line(&VIEWS, app.view, styles.tab_current);
    if let Some(spinner) = app.spinner() {
        title.push_span(Span::raw(format!("{HEADER_SEPARATOR}{spinner}")));
    }
    let note = note_or_alert(app, Screen::List, || {
        format!("{} {ROW_COUNT_UNIT}", app.list_rows().len())
    });
    let block = pane_block(title, note, focus, &styles);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    draw_list(frame, inner, app, clock);
}

fn draw_detail_pane(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    (clock, cache, budget): (&impl Clock, &RenderCache, &HighlightBudget),
    focus: Focus,
) {
    let note = note_or_alert(app, Screen::Detail, || {
        app.selected.as_ref().map_or_else(String::new, |key| {
            format!("{}#{}", sanitize(&key.repo), key.number)
        })
    });
    let styles = Styles::of(app);
    let tabs = choice_line(&tab_labels(app), app.tab, styles.tab_current);
    let block = pane_block(tabs, note, focus, &styles);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if app.selected.is_some() {
        detail::render_budgeted(frame, inner, app, clock, cache, budget);
    } else {
        frame.render_widget(Paragraph::new(NO_SELECTION_TEXT), inner);
    }
}

fn draw_diff_pane(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    cache: &RenderCache,
    budget: &HighlightBudget,
) {
    let note = note_or_alert(app, Screen::Diff, String::new);
    let block = pane_block(
        Line::from(DIFF_HEADER),
        note,
        Focus::Unmarked,
        &Styles::of(app),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    draw_diff(frame, inner, app, cache, budget);
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

fn draw_diff(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    cache: &RenderCache,
    budget: &HighlightBudget,
) {
    if let Some(file) = app.selected_file() {
        let pr_key = app.selected.as_ref().map(detail::pr_key_text);
        let highlight = Highlighting::for_theme(
            app.theme(),
            pr_key.as_deref().unwrap_or_default(),
            cache,
            budget,
        );
        let styles = Styles::of(app);
        diff::render_highlighted(frame, area, file, app.diff_scroll(), &styles, highlight);
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
