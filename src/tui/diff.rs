// SPDX-License-Identifier: Apache-2.0

//! The diff screen: the hunks of one changed file.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::model::{ChangedFile, DiffGap, FileDiff};

const HEADER_HEIGHT: u16 = 1;
const GUTTER_WIDTH: usize = 4;
const HUNK_MARKER: &str = "@@";
const ADD_MARKER: char = '+';
const DELETE_MARKER: char = '-';
const NO_NEWLINE_MARKER: char = '\\';
const ADD_COLOR: Color = Color::Green;
const DELETE_COLOR: Color = Color::Red;
const HUNK_COLOR: Color = Color::Cyan;
/// A tab draws as this many spaces; a terminal cell grid has no tab stops inside a widget.
const TAB_SPACES: &str = "    ";

/// The most patch lines drawn for one file.
pub const MAX_DIFF_LINES: usize = 500;
/// Drawn after the first `MAX_DIFF_LINES` lines of a longer patch.
pub const CUT_LINE: &str = "diff cut: too long to show here, open it on GitHub";
/// Drawn when GitHub left the patch out because it is too large.
pub const TRUNCATED_LINE: &str = "diff unavailable: GitHub truncated it, open it on GitHub";
/// Drawn for an added or removed file with no patch and no changes.
pub const ADDED_OR_REMOVED_NO_TEXT_LINE: &str = "no text diff: empty or binary file";
/// Drawn for any other file with no patch and no changes.
pub const CHANGED_NO_TEXT_LINE: &str = "no text diff: binary file or mode change only";

/// How the diff is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffOptions {
    /// Colour added and deleted lines; off keeps only the `+` and `-` markers.
    pub color: bool,
}

/// Draws the diff of `file` with `options`. Every string is `Remote`, so it is already sanitized.
pub fn render_with(frame: &mut Frame, area: Rect, file: &ChangedFile, options: DiffOptions) {
    render_scrolled(frame, area, file, options, 0);
}

/// Draws the diff of `file` with `options`, starting `scroll` lines down the patch.
pub fn render_scrolled(
    frame: &mut Frame,
    area: Rect,
    file: &ChangedFile,
    options: DiffOptions,
    scroll: usize,
) {
    let [header, body] =
        Layout::vertical([Constraint::Length(HEADER_HEIGHT), Constraint::Min(0)]).areas(area);
    frame.render_widget(Paragraph::new(file.path.as_str()), header);
    match &file.diff {
        FileDiff::Patch(patch) => {
            let offset = u16::try_from(scroll).unwrap_or(u16::MAX);
            let lines = patch_lines(patch.as_str(), options);
            frame.render_widget(Paragraph::new(lines).scroll((offset, 0)), body);
        }
        FileDiff::Unavailable { gap, .. } => {
            if let Some(line) = gap_line(gap) {
                frame.render_widget(Paragraph::new(line), body);
            }
        }
    }
}

/// The fixed line drawn in place of a patch GitHub did not provide, if any.
fn gap_line(gap: &DiffGap) -> Option<&'static str> {
    match gap {
        DiffGap::Truncated => Some(TRUNCATED_LINE),
        DiffGap::AddedOrRemovedNoText => Some(ADDED_OR_REMOVED_NO_TEXT_LINE),
        DiffGap::ChangedNoText => Some(CHANGED_NO_TEXT_LINE),
        DiffGap::RenamedOnly => None,
    }
}

/// Draws the diff of `file` in colour.
pub fn render(frame: &mut Frame, area: Rect, file: &ChangedFile) {
    render_with(frame, area, file, DiffOptions { color: true });
}

/// The first `MAX_DIFF_LINES` patch lines with their gutters, then the cut line when more follow.
fn patch_lines(patch: &str, options: DiffOptions) -> Vec<Line<'static>> {
    let mut numbers = LineNumbers::default();
    let mut lines: Vec<Line> = patch
        .lines()
        .take(MAX_DIFF_LINES)
        .map(|line| numbers.line(line, options))
        .collect();
    if patch.lines().nth(MAX_DIFF_LINES).is_some() {
        lines.push(Line::from(CUT_LINE));
    }
    lines
}

/// The next old and new line number while walking a patch.
#[derive(Default)]
struct LineNumbers {
    old: u32,
    new: u32,
}

impl LineNumbers {
    fn line(&mut self, text: &str, options: DiffOptions) -> Line<'static> {
        let expanded = text.replace('\t', TAB_SPACES);
        let text = expanded.as_str();
        if text.starts_with(HUNK_MARKER) {
            (self.old, self.new) = hunk_start(text);
            return Line::styled(text.to_owned(), paint(Some(HUNK_COLOR), options));
        }
        let (old, new, color) = if text.starts_with(ADD_MARKER) {
            (None, Some(self.take_new()), Some(ADD_COLOR))
        } else if text.starts_with(DELETE_MARKER) {
            (Some(self.take_old()), None, Some(DELETE_COLOR))
        } else if text.starts_with(NO_NEWLINE_MARKER) {
            (None, None, None)
        } else {
            (Some(self.take_old()), Some(self.take_new()), None)
        };
        let style = paint(color, options);
        Line::from(vec![
            Span::raw(format!("{} {} ", gutter(old), gutter(new))),
            Span::styled(text.to_owned(), style),
        ])
    }

    fn take_old(&mut self) -> u32 {
        take(&mut self.old)
    }

    fn take_new(&mut self) -> u32 {
        take(&mut self.new)
    }
}

/// Returns the counter's current value and advances it.
fn take(counter: &mut u32) -> u32 {
    let number = *counter;
    *counter = counter.saturating_add(1);
    number
}

fn gutter(number: Option<u32>) -> String {
    number.map_or_else(
        || " ".repeat(GUTTER_WIDTH),
        |number| format!("{number:>GUTTER_WIDTH$}"),
    )
}

/// The style for `color`; plain when colour is off or the line has none.
fn paint(color: Option<Color>, options: DiffOptions) -> Style {
    match color {
        Some(color) if options.color => Style::new().fg(color),
        _ => Style::default(),
    }
}

/// The old and new start lines of a `@@ -old,len +new,len @@` header; 0 when malformed.
fn hunk_start(header: &str) -> (u32, u32) {
    let mut ranges = header.split_whitespace().skip(1);
    let mut start = |marker: char| {
        ranges
            .next()
            .and_then(|range| range.strip_prefix(marker))
            .and_then(|range| range.split(',').next())
            .and_then(|number| number.parse().ok())
            .unwrap_or(0)
    };
    let old = start(DELETE_MARKER);
    let new = start(ADD_MARKER);
    (old, new)
}
