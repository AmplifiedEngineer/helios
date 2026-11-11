// SPDX-License-Identifier: Apache-2.0

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, DetailTab, ListView, Screen, Step};

const FOOTER_SEPARATOR: &str = "  ";

/// Where the user is, as far as which keys do something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Mine,
    NeedsMe,
    Detail,
    Files,
    Diff,
}

impl Scope {
    /// The scope the app is in now.
    pub fn of(app: &App) -> Self {
        match (app.screen, app.view, app.tab) {
            (Screen::List, ListView::Mine, _) => Self::Mine,
            (Screen::List, ListView::NeedsMe, _) => Self::NeedsMe,
            (Screen::Detail, _, DetailTab::Files) => Self::Files,
            (Screen::Detail, _, _) => Self::Detail,
            (Screen::Diff, _, _) => Self::Diff,
        }
    }
}

const LISTS: &[Scope] = &[Scope::Mine, Scope::NeedsMe];
const ANYWHERE: &[Scope] = &[
    Scope::Mine,
    Scope::NeedsMe,
    Scope::Detail,
    Scope::Files,
    Scope::Diff,
];

/// One key binding: the key label and what it does, shown in the footer and the help.
pub struct Binding {
    pub key: &'static str,
    pub action: &'static str,
    /// Where the key does something; the footer and the help list it only there.
    pub scopes: &'static [Scope],
}

/// The single table the footer and the help are both drawn from.
pub const BINDINGS: &[Binding] = &[
    Binding {
        key: "j/k",
        action: "move",
        scopes: &[Scope::Mine, Scope::NeedsMe, Scope::Files],
    },
    Binding {
        key: "j/k",
        action: "scroll",
        scopes: &[Scope::Diff, Scope::Detail],
    },
    Binding {
        key: "g/G",
        action: "first/last",
        scopes: &[
            Scope::Mine,
            Scope::NeedsMe,
            Scope::Detail,
            Scope::Files,
            Scope::Diff,
        ],
    },
    Binding {
        key: "Enter",
        action: "open",
        scopes: &[Scope::Mine, Scope::NeedsMe, Scope::Files],
    },
    Binding {
        key: "Esc",
        action: "back",
        scopes: &[Scope::Detail, Scope::Files, Scope::Diff],
    },
    Binding {
        key: "m",
        action: "mine/needs me",
        scopes: LISTS,
    },
    Binding {
        key: "r",
        action: "refresh",
        scopes: LISTS,
    },
    Binding {
        key: "f",
        action: "filter",
        scopes: &[Scope::NeedsMe],
    },
    Binding {
        key: "d",
        action: "toggle drafts",
        scopes: LISTS,
    },
    Binding {
        key: "Tab",
        action: "next tab",
        scopes: &[Scope::Detail, Scope::Files],
    },
    Binding {
        key: "Shift-Tab",
        action: "previous tab",
        scopes: &[Scope::Detail, Scope::Files],
    },
    Binding {
        key: "o",
        action: "open in browser",
        scopes: ANYWHERE,
    },
    Binding {
        key: "?",
        action: "help",
        scopes: ANYWHERE,
    },
    Binding {
        key: "q",
        action: "quit",
        scopes: ANYWHERE,
    },
    Binding {
        key: "Ctrl-C",
        action: "quit",
        scopes: ANYWHERE,
    },
];

/// What the event loop does after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    Quit,
    /// Reload both lists now.
    Refresh,
    /// Open the current pull request in the web browser.
    OpenInBrowser,
}

fn step_for(code: KeyCode) -> Option<Step> {
    match code {
        KeyCode::Char('j') | KeyCode::Down => Some(Step::Next),
        KeyCode::Char('k') | KeyCode::Up => Some(Step::Previous),
        KeyCode::Char('g') | KeyCode::Home => Some(Step::First),
        KeyCode::Char('G') | KeyCode::End => Some(Step::Last),
        _ => None,
    }
}

fn is_back(code: KeyCode) -> bool {
    matches!(code, KeyCode::Esc | KeyCode::Backspace)
}

/// Applies one key press to the app.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Outcome {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Outcome::Quit;
    }
    if app.show_help {
        app.show_help = false;
        return Outcome::Continue;
    }
    match key.code {
        KeyCode::Char('q') => return Outcome::Quit,
        KeyCode::Char('?') => {
            app.show_help = true;
            return Outcome::Continue;
        }
        KeyCode::Char('o') => return Outcome::OpenInBrowser,
        _ => {}
    }
    match Scope::of(app) {
        Scope::Mine | Scope::NeedsMe => list_key(app, key.code),
        Scope::Detail | Scope::Files => detail_key(app, key.code),
        Scope::Diff => diff_key(app, key.code),
    }
}

fn list_key(app: &mut App, code: KeyCode) -> Outcome {
    if let Some(step) = step_for(code) {
        app.move_cursor(step);
        return Outcome::Continue;
    }
    match code {
        KeyCode::Enter => app.open_highlighted(),
        KeyCode::Char('m') => app.toggle_view(),
        KeyCode::Char('r') => return Outcome::Refresh,
        KeyCode::Char('f') if app.view == ListView::NeedsMe => app.cycle_filter(),
        KeyCode::Char('d') => app.toggle_drafts(),
        _ => {}
    }
    Outcome::Continue
}

fn detail_key(app: &mut App, code: KeyCode) -> Outcome {
    let on_files = app.tab == DetailTab::Files;
    match code {
        KeyCode::Tab => app.next_tab(),
        KeyCode::BackTab => app.previous_tab(),
        KeyCode::Enter if on_files => app.open_file(),
        code if is_back(code) => app.back(),
        code if on_files => {
            if let Some(step) = step_for(code) {
                app.move_file(step);
            }
        }
        code => {
            if let Some(step) = step_for(code) {
                app.scroll_detail(step);
            }
        }
    }
    Outcome::Continue
}

fn diff_key(app: &mut App, code: KeyCode) -> Outcome {
    if is_back(code) {
        app.back();
    } else if let Some(step) = step_for(code) {
        app.scroll_diff(step);
    }
    Outcome::Continue
}

/// The bold key label followed by its action, shared by the footer and the help.
fn binding_spans(binding: &Binding) -> [Span<'static>; 2] {
    [
        Span::styled(binding.key, Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(format!(" {}", binding.action)),
    ]
}

fn bindings_in(scope: Option<Scope>) -> impl Iterator<Item = &'static Binding> {
    BINDINGS
        .iter()
        .filter(move |binding| scope.is_none_or(|scope| binding.scopes.contains(&scope)))
}

/// The footer text for `scope` packed into lines no wider than `width`.
pub fn footer_lines(scope: Option<Scope>, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let mut lines = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for binding in bindings_in(scope) {
        let item = binding_spans(binding);
        let item_width: usize = item.iter().map(Span::width).sum();
        let separator = if spans.is_empty() {
            0
        } else {
            FOOTER_SEPARATOR.len()
        };
        if !spans.is_empty() && used + separator + item_width > width {
            lines.push(Line::from(std::mem::take(&mut spans)));
            used = 0;
        } else if !spans.is_empty() {
            spans.push(Span::raw(FOOTER_SEPARATOR));
            used += separator;
        }
        used += item_width;
        spans.extend(item);
    }
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    lines
}

/// How many rows the footer for `scope` needs at `width`.
pub fn footer_height(scope: Scope, width: u16) -> u16 {
    u16::try_from(footer_lines(Some(scope), width).len()).unwrap_or(u16::MAX)
}

/// How many keys the help lists for `scope`.
pub fn help_height(scope: Scope) -> u16 {
    u16::try_from(bindings_in(Some(scope)).count()).unwrap_or(u16::MAX)
}

/// Draws the footer for `scope` from `BINDINGS`; with no scope, every binding.
pub fn render_footer_for(frame: &mut Frame, area: Rect, scope: Option<Scope>) {
    frame.render_widget(Paragraph::new(footer_lines(scope, area.width)), area);
}

/// Draws the help for `scope` from `BINDINGS`; with no scope, every binding.
pub fn render_help_for(frame: &mut Frame, area: Rect, scope: Option<Scope>) {
    let lines: Vec<Line> = bindings_in(scope)
        .map(|binding| Line::from(binding_spans(binding).to_vec()))
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

/// Draws the footer with every binding, wrapped to the width.
pub fn render_footer(frame: &mut Frame, area: Rect) {
    render_footer_for(frame, area, None);
}

/// Draws the help overlay with every binding.
pub fn render_help(frame: &mut Frame, area: Rect) {
    render_help_for(frame, area, None);
}
