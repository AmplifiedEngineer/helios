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
/// Two gutters and the space after each.
const GUTTER_PREFIX_WIDTH: usize = GUTTER_WIDTH * 2 + 2;
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

/// The most patch lines drawn under an open file in the Files tab.
pub const MAX_INLINE_DIFF_LINES: usize = 200;

/// Drawn after the capped lines of a patch that has more.
fn inline_cut_line() -> String {
    format!("Showing the first {MAX_INLINE_DIFF_LINES} lines. Press v for the full diff.")
}

/// The added and removed line counts of a patch. File headers before the first hunk do not count.
pub fn count_changes(patch: &str) -> (usize, usize) {
    let hunks = patch
        .lines()
        .skip_while(|line| !line.starts_with(HUNK_MARKER));
    let (mut added, mut removed) = (0, 0);
    for line in hunks {
        if line.starts_with(ADD_MARKER) {
            added += 1;
        } else if line.starts_with(DELETE_MARKER) {
            removed += 1;
        }
    }
    (added, removed)
}

/// The rows of `file` drawn under its row in the Files tab, each starting with `indent`: the first
/// `MAX_INLINE_DIFF_LINES` patch lines wrapped to `width`, then a note when more follow.
pub fn inline_lines(file: &ChangedFile, width: usize, indent: &str) -> Vec<Line<'static>> {
    let options = DiffOptions { color: true };
    let inner = width.saturating_sub(indent.chars().count());
    let rows = match &file.diff {
        FileDiff::Patch(patch) => {
            let mut numbers = LineNumbers::default();
            let mut rows: Vec<Line<'static>> = patch
                .as_str()
                .lines()
                .take(MAX_INLINE_DIFF_LINES)
                .flat_map(|line| numbers.line(line, options, inner))
                .collect();
            if patch.as_str().lines().nth(MAX_INLINE_DIFF_LINES).is_some() {
                rows.push(Line::from(inline_cut_line()));
            }
            rows
        }
        FileDiff::Unavailable { gap, .. } => gap_line(gap).map(Line::from).into_iter().collect(),
    };
    rows.into_iter()
        .map(|mut row| {
            row.spans.insert(0, Span::raw(indent.to_owned()));
            row
        })
        .collect()
}

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

/// Draws the diff of `file` with `options`, starting `scroll` patch lines down; a long line wraps
/// onto more rows, but it still counts as one line.
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
            let rows = patch_rows(patch.as_str(), options, usize::from(body.width), scroll);
            frame.render_widget(Paragraph::new(rows), body);
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

/// The rows of the first `MAX_DIFF_LINES` patch lines after skipping `scroll` of them, each wrapped
/// to `width` with its gutters, then the cut line when more follow.
fn patch_rows(
    patch: &str,
    options: DiffOptions,
    width: usize,
    scroll: usize,
) -> Vec<Line<'static>> {
    let mut numbers = LineNumbers::default();
    let mut lines: Vec<Vec<Line>> = patch
        .lines()
        .take(MAX_DIFF_LINES)
        .map(|line| numbers.line(line, options, width))
        .collect();
    if patch.lines().nth(MAX_DIFF_LINES).is_some() {
        lines.push(vec![Line::from(CUT_LINE)]);
    }
    lines.into_iter().skip(scroll).flatten().collect()
}

/// The next old and new line number while walking a patch.
#[derive(Default)]
struct LineNumbers {
    old: u32,
    new: u32,
}

impl LineNumbers {
    /// The rows of one patch line: the first carries the numbers, the rest a blank gutter.
    fn line(&mut self, text: &str, options: DiffOptions, width: usize) -> Vec<Line<'static>> {
        let expanded = text.replace('\t', TAB_SPACES);
        let text = expanded.as_str();
        if text.starts_with(HUNK_MARKER) {
            (self.old, self.new) = hunk_start(text);
            let style = paint(Some(HUNK_COLOR), options);
            return wrap(text, width)
                .into_iter()
                .map(|row| Line::styled(row, style))
                .collect();
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
        let numbers = format!("{} {} ", gutter(old), gutter(new));
        let blank = " ".repeat(GUTTER_PREFIX_WIDTH);
        wrap(text, width.saturating_sub(GUTTER_PREFIX_WIDTH))
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                let prefix = if index == 0 {
                    numbers.clone()
                } else {
                    blank.clone()
                };
                Line::from(vec![Span::raw(prefix), Span::styled(row, style)])
            })
            .collect()
    }

    fn take_old(&mut self) -> u32 {
        take(&mut self.old)
    }

    fn take_new(&mut self) -> u32 {
        take(&mut self.new)
    }
}

/// Splits `text` into rows no wider than `width` cells, never inside a glyph. Each row holds at
/// least one glyph, so a width too small for a wide glyph still makes progress.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut rows = vec![String::new()];
    let mut used = 0;
    for glyph in text.chars() {
        let glyph_width = Span::raw(glyph.to_string()).width();
        if used > 0 && used + glyph_width > width {
            rows.push(String::new());
            used = 0;
        }
        if let Some(row) = rows.last_mut() {
            row.push(glyph);
        }
        used += glyph_width;
    }
    rows
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
