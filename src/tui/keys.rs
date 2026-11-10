// SPDX-License-Identifier: Apache-2.0

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, Screen};

const FOOTER_SEPARATOR: &str = "  ";

/// One key binding: the key label and what it does, shown in the footer and the help.
pub struct Binding {
    pub key: &'static str,
    pub action: &'static str,
}

/// The single table the footer and the help are both drawn from.
pub const BINDINGS: &[Binding] = &[
    Binding {
        key: "r",
        action: "refresh",
    },
    Binding {
        key: "f",
        action: "filter",
    },
    Binding {
        key: "d",
        action: "toggle drafts",
    },
    Binding {
        key: "Tab",
        action: "next tab",
    },
    Binding {
        key: "Shift-Tab",
        action: "previous tab",
    },
    Binding {
        key: "q",
        action: "quit",
    },
    Binding {
        key: "Ctrl-C",
        action: "quit",
    },
];

/// Applies one key press to the app.
pub fn handle_key(app: &mut App, key: KeyEvent) {
    if app.screen != Screen::Detail {
        return;
    }
    match key.code {
        KeyCode::Tab => app.tab = app.tab.next(),
        KeyCode::BackTab => app.tab = app.tab.previous(),
        _ => {}
    }
}

/// The bold key label followed by its action, shared by the footer and the help.
fn binding_spans(binding: &Binding) -> [Span<'static>; 2] {
    [
        Span::styled(binding.key, Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(format!(" {}", binding.action)),
    ]
}

/// Draws the one-line footer from `BINDINGS`.
pub fn render_footer(frame: &mut Frame, area: Rect) {
    let mut spans = Vec::new();
    for binding in BINDINGS {
        if !spans.is_empty() {
            spans.push(Span::raw(FOOTER_SEPARATOR));
        }
        spans.extend(binding_spans(binding));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Draws the help overlay from `BINDINGS`.
pub fn render_help(frame: &mut Frame, area: Rect) {
    let lines: Vec<Line> = BINDINGS
        .iter()
        .map(|binding| Line::from(binding_spans(binding).to_vec()))
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}
