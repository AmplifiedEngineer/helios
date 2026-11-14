// SPDX-License-Identifier: Apache-2.0

//! The diff screen: the hunks of one changed file.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::highlight::LineRoles;
use crate::model::{ChangedFile, DiffGap, FileDiff};
use crate::theme::{ColorDepth, Role, Theme};
use crate::tui::cache::{HighlightBudget, RenderCache};
use crate::tui::style::Styles;

const HEADER_HEIGHT: u16 = 1;
const GUTTER_WIDTH: usize = 4;
/// Two gutters and the space after each.
const GUTTER_PREFIX_WIDTH: usize = GUTTER_WIDTH * 2 + 2;
const HUNK_MARKER: &str = "@@";
const ADD_MARKER: char = '+';
const DELETE_MARKER: char = '-';
const NO_NEWLINE_MARKER: char = '\\';
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

/// Where a diff takes its token roles from: the cache that keeps them, this frame's budget, and
/// the key of the pull request they are kept under.
#[derive(Clone, Copy)]
pub struct Highlighting<'a> {
    cache: &'a RenderCache,
    budget: &'a HighlightBudget,
    pr_key: &'a str,
}

impl<'a> Highlighting<'a> {
    /// None in the mono theme, which has no colors to show, so no work is spent on roles.
    pub fn for_theme(
        theme: Theme,
        pr_key: &'a str,
        cache: &'a RenderCache,
        budget: &'a HighlightBudget,
    ) -> Option<Self> {
        (theme != Theme::Mono).then_some(Self {
            cache,
            budget,
            pr_key,
        })
    }

    /// The roles of the first `limit` patch lines of `file`, as far as the budget allows.
    fn roles(&self, file: &ChangedFile, patch: &str, limit: usize) -> Vec<LineRoles> {
        let path = file.path.as_str();
        self.cache
            .diff_roles_to(self.pr_key, path, patch, limit, self.budget)
    }
}

/// The styles of a draw that has no theme: the dark theme's named colors, or none.
fn options_styles(options: DiffOptions) -> Styles {
    let theme = if options.color {
        Theme::Dark
    } else {
        Theme::Mono
    };
    Styles::new(theme, ColorDepth::Ansi16)
}

/// The rows of `file` drawn under its row in the Files tab, each starting with `indent`: the first
/// `MAX_INLINE_DIFF_LINES` patch lines wrapped to `width`, then a note when more follow.
pub fn inline_lines(file: &ChangedFile, width: usize, indent: &str) -> Vec<Line<'static>> {
    let styles = options_styles(DiffOptions { color: true });
    inline_lines_highlighted(file, width, indent, &styles, None)
}

/// Like `inline_lines`, with the colors of `styles` and the token roles from `highlight`.
pub fn inline_lines_highlighted(
    file: &ChangedFile,
    width: usize,
    indent: &str,
    styles: &Styles,
    highlight: Option<Highlighting>,
) -> Vec<Line<'static>> {
    let inner = width.saturating_sub(indent.chars().count());
    let rows = match &file.diff {
        FileDiff::Patch(patch) => {
            let roles = highlight.map_or_else(Vec::new, |h| {
                h.roles(file, patch.as_str(), MAX_INLINE_DIFF_LINES)
            });
            let mut numbers = LineNumbers::default();
            let mut rows: Vec<Line<'static>> = patch
                .as_str()
                .lines()
                .take(MAX_INLINE_DIFF_LINES)
                .enumerate()
                .flat_map(|(index, line)| numbers.line(line, styles, roles.get(index), inner))
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
    render_highlighted(frame, area, file, scroll, &options_styles(options), None);
}

/// Like `render_scrolled`, with the colors of `styles` and the token roles from `highlight`.
pub fn render_highlighted(
    frame: &mut Frame,
    area: Rect,
    file: &ChangedFile,
    scroll: usize,
    styles: &Styles,
    highlight: Option<Highlighting>,
) {
    let [header, body] =
        Layout::vertical([Constraint::Length(HEADER_HEIGHT), Constraint::Min(0)]).areas(area);
    frame.render_widget(Paragraph::new(file.path.as_str()), header);
    match &file.diff {
        FileDiff::Patch(patch) => {
            let roles =
                highlight.map_or_else(Vec::new, |h| h.roles(file, patch.as_str(), MAX_DIFF_LINES));
            let width = usize::from(body.width);
            let rows = patch_rows(patch.as_str(), styles, &roles, width, scroll);
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
    styles: &Styles,
    roles: &[LineRoles],
    width: usize,
    scroll: usize,
) -> Vec<Line<'static>> {
    let mut numbers = LineNumbers::default();
    let mut lines: Vec<Vec<Line>> = patch
        .lines()
        .take(MAX_DIFF_LINES)
        .enumerate()
        .map(|(index, line)| numbers.line(line, styles, roles.get(index), width))
        .collect();
    if patch.lines().nth(MAX_DIFF_LINES).is_some() {
        lines.push(vec![Line::from(CUT_LINE)]);
    }
    lines.into_iter().skip(scroll).flatten().collect()
}

/// Walks the lines of a patch and tells which of them hold code to highlight.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CodeLines {
    in_hunk: bool,
}

impl CodeLines {
    /// The text after the marker of `line`. Hunk headers, `\ No newline` notes and file headers
    /// before the first hunk hold no code.
    pub(crate) fn code_of<'a>(&mut self, line: &'a str) -> Option<&'a str> {
        if line.starts_with(HUNK_MARKER) {
            self.in_hunk = true;
            return None;
        }
        if !self.in_hunk || line.starts_with(NO_NEWLINE_MARKER) {
            return None;
        }
        let mut chars = line.chars();
        chars.next()?;
        Some(chars.as_str())
    }
}

/// The next old and new line number while walking a patch.
#[derive(Default)]
struct LineNumbers {
    old: u32,
    new: u32,
    walk: CodeLines,
}

impl LineNumbers {
    /// The rows of one patch line: the first carries the numbers, the rest a blank gutter.
    ///
    /// The marker takes the line's added or removed style. A code token takes its role style,
    /// and a plain token keeps the line's style, so a line that is not highlighted yet looks the same.
    fn line(
        &mut self,
        raw: &str,
        styles: &Styles,
        roles: Option<&LineRoles>,
        width: usize,
    ) -> Vec<Line<'static>> {
        let code = self.walk.code_of(raw);
        if raw.starts_with(HUNK_MARKER) {
            (self.old, self.new) = hunk_start(&raw.replace('\t', TAB_SPACES));
            let segments = [(expand(raw), styles.border_focused)];
            return wrap(&segments, width).into_iter().map(Line::from).collect();
        }
        let (old, new, style) = if raw.starts_with(ADD_MARKER) {
            (None, Some(self.take_new()), styles.added)
        } else if raw.starts_with(DELETE_MARKER) {
            (Some(self.take_old()), None, styles.removed)
        } else if raw.starts_with(NO_NEWLINE_MARKER) {
            (None, None, Style::default())
        } else {
            (
                Some(self.take_old()),
                Some(self.take_new()),
                Style::default(),
            )
        };
        let segments = match code {
            Some(code) => {
                let marker = &raw[..raw.len() - code.len()];
                let mut segments = vec![(expand(marker), style)];
                segments.extend(code_segments(code, roles, style, styles));
                segments
            }
            None => vec![(expand(raw), style)],
        };
        let numbers = format!("{} {} ", gutter(old), gutter(new));
        let blank = " ".repeat(GUTTER_PREFIX_WIDTH);
        wrap(&segments, width.saturating_sub(GUTTER_PREFIX_WIDTH))
            .into_iter()
            .enumerate()
            .map(|(index, mut spans)| {
                let prefix = if index == 0 {
                    numbers.clone()
                } else {
                    blank.clone()
                };
                spans.insert(0, Span::raw(prefix));
                Line::from(spans)
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

fn expand(text: &str) -> String {
    text.replace('\t', TAB_SPACES)
}

/// The pieces of `code`, one per role run, each with the style it is drawn in. Without roles that
/// cover the whole text, the code is one piece in `line_style`.
fn code_segments(
    code: &str,
    roles: Option<&LineRoles>,
    line_style: Style,
    styles: &Styles,
) -> Vec<(String, Style)> {
    let covered = roles.filter(|runs| {
        runs.last()
            .is_some_and(|(_, range)| range.end == code.len())
    });
    let Some(runs) = covered else {
        return vec![(expand(code), line_style)];
    };
    runs.iter()
        .filter_map(|(role, range)| {
            let text = code.get(range.clone())?;
            let style = if *role == Role::Plain {
                line_style
            } else {
                line_style.patch(styles.role(*role))
            };
            Some((expand(text), style))
        })
        .collect()
}

/// Splits styled text into rows of spans no wider than `width` cells, never inside a glyph. Each
/// row holds at least one glyph, so a width too small for a wide glyph still makes progress.
fn wrap(segments: &[(String, Style)], width: usize) -> Vec<Vec<Span<'static>>> {
    let mut rows: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut used = 0;
    for (text, style) in segments {
        for glyph in text.chars() {
            let glyph_width = Span::raw(glyph.to_string()).width();
            if used > 0 && used + glyph_width > width {
                rows.push(Vec::new());
                used = 0;
            }
            if let Some(row) = rows.last_mut() {
                match row.last_mut() {
                    Some(span) if span.style == *style => span.content.to_mut().push(glyph),
                    _ => row.push(Span::styled(glyph.to_string(), *style)),
                }
            }
            used += glyph_width;
        }
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
