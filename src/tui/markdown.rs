// SPDX-License-Identifier: Apache-2.0

//! Turns a parsed markdown `Document` into styled terminal lines.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::markdown::{Block, Document, Inline, Table};

const BULLET_MARKER: &str = "- ";
const ORDERED_SUFFIX: &str = ". ";
const CODE_INDENT: &str = "    ";
const CELL_SEPARATOR: &str = " | ";
const RULE_CHAR: char = '-';
const TABLE_RULE_CHAR: char = '-';
const UNCHECKED_BOX: &str = "[ ] ";
const CHECKED_BOX: &str = "[x] ";
/// Columns a nested list sits to the right of its parent item text.
const NESTED_LIST_INDENT: usize = 2;

/// Appended after the last block when the document was cut short.
pub const CUT_NOTE: &str = "[showing part of this text]";
/// Longest URL, in characters, shown whole next to a link.
pub const MAX_URL_DISPLAY: usize = 80;
const QUOTE_BAR: &str = "| ";
const ELLIPSIS: &str = "...";

/// One display column with the style it is drawn in.
type Cell = (char, Style);

/// Renders the document as lines no wider than `width` columns.
///
/// Widths count terminal columns, so wide glyphs wrap correctly.
pub fn render(document: &Document, width: u16) -> Vec<Line<'static>> {
    let mut lines = render_blocks(&document.blocks, usize::from(width).max(1));
    if document.cut {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(Line::styled(
            CUT_NOTE,
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    lines
}

fn render_blocks(blocks: &[Block], width: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            out.push(Line::default());
        }
        out.extend(render_block(block, width));
    }
    out
}

fn render_block(block: &Block, width: usize) -> Vec<Line<'static>> {
    match block {
        Block::Paragraph(content) => wrap_inlines(content, Style::default(), width),
        Block::Heading { content, .. } => wrap_inlines(
            content,
            Style::default().add_modifier(Modifier::BOLD),
            width,
        ),
        Block::List(items) => render_list(items, width, |_| BULLET_MARKER.to_string()),
        Block::OrderedList(items) => render_list(items, width, |index| {
            format!("{}{ORDERED_SUFFIX}", index + 1)
        }),
        Block::Quote(inner) => {
            let inner_width = width.saturating_sub(QUOTE_BAR.len()).max(1);
            render_blocks(inner, inner_width)
                .into_iter()
                .map(|line| prefixed(QUOTE_BAR, line))
                .collect()
        }
        Block::Code { text, .. } => text
            .lines()
            .map(|line| Line::from(format!("{CODE_INDENT}{line}")))
            .collect(),
        Block::Table(table) => render_table(table),
        Block::Rule => vec![Line::from(RULE_CHAR.to_string().repeat(width))],
    }
}

fn render_list(
    items: &[Vec<Block>],
    width: usize,
    marker: impl Fn(usize) -> String,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let marker = marker(index);
        let marker_width = marker.chars().count();
        let inner_width = width.saturating_sub(marker_width).max(1);
        let hanging = " ".repeat(marker_width);
        for (line_index, line) in render_item(item, inner_width).into_iter().enumerate() {
            let prefix = if line_index == 0 { &marker } else { &hanging };
            out.push(prefixed(prefix, line));
        }
    }
    out
}

/// Renders one list item; a list nested in it is pushed right by `NESTED_LIST_INDENT`.
fn render_item(item: &[Block], width: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    for (index, block) in item.iter().enumerate() {
        if index > 0 {
            out.push(Line::default());
        }
        let nested = matches!(block, Block::List(_) | Block::OrderedList(_));
        if nested {
            let inner = width.saturating_sub(NESTED_LIST_INDENT).max(1);
            let pad = " ".repeat(NESTED_LIST_INDENT);
            out.extend(
                render_block(block, inner)
                    .into_iter()
                    .map(|line| prefixed(&pad, line)),
            );
        } else {
            out.extend(render_block(block, width));
        }
    }
    out
}

fn prefixed(prefix: &str, line: Line<'static>) -> Line<'static> {
    let mut spans = vec![Span::raw(prefix.to_string())];
    spans.extend(line.spans);
    Line::from(spans)
}

fn render_table(table: &Table) -> Vec<Line<'static>> {
    let header: Vec<Vec<Cell>> = table
        .header
        .iter()
        .map(|cell| flatten(cell, Style::default().add_modifier(Modifier::BOLD)))
        .collect();
    let rows: Vec<Vec<Vec<Cell>>> = table
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| flatten(cell, Style::default()))
                .collect()
        })
        .collect();

    let columns = rows
        .iter()
        .map(Vec::len)
        .chain([header.len()])
        .max()
        .unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            rows.iter()
                .chain([&header])
                .filter_map(|row| row.get(column))
                .map(|cell| cells_width(cell))
                .max()
                .unwrap_or(0)
        })
        .collect();

    let rule = widths
        .iter()
        .map(|&w| TABLE_RULE_CHAR.to_string().repeat(w))
        .collect::<Vec<_>>()
        .join(CELL_SEPARATOR);

    let mut out = vec![table_row(&header, &widths), Line::from(rule)];
    out.extend(rows.iter().map(|row| table_row(row, &widths)));
    out
}

fn table_row(row: &[Vec<Cell>], widths: &[usize]) -> Line<'static> {
    let mut cells: Vec<Cell> = Vec::new();
    for (column, &width) in widths.iter().enumerate() {
        if column > 0 {
            cells.extend(CELL_SEPARATOR.chars().map(|c| (c, Style::default())));
        }
        let content = row.get(column).map(Vec::as_slice).unwrap_or(&[]);
        cells.extend_from_slice(content);
        cells.extend((cells_width(content)..width).map(|_| (' ', Style::default())));
    }
    to_line(&cells)
}

fn wrap_inlines(content: &[Inline], base: Style, width: usize) -> Vec<Line<'static>> {
    wrap(&flatten(content, base), width)
        .iter()
        .map(|cells| to_line(cells))
        .collect()
}

/// Flattens inline nodes into styled columns; newlines become spaces.
fn flatten(content: &[Inline], style: Style) -> Vec<Cell> {
    let mut out = Vec::new();
    for inline in content {
        match inline {
            Inline::Text(text) => push_text(&mut out, text, style),
            Inline::Emphasis(inner) => {
                out.extend(flatten(inner, style.add_modifier(Modifier::ITALIC)));
            }
            Inline::Strong(inner) => {
                out.extend(flatten(inner, style.add_modifier(Modifier::BOLD)));
            }
            Inline::Code(text) => push_text(&mut out, text, style.add_modifier(Modifier::REVERSED)),
            Inline::Link { url, content } => {
                let text = flatten(content, style);
                match url {
                    Some(url) if text.iter().map(|&(c, _)| c).eq(url.chars()) => {
                        push_text(&mut out, &shorten_url(url), style);
                    }
                    Some(url) => {
                        out.extend(text);
                        push_text(&mut out, &format!(" ({})", shorten_url(url)), style);
                    }
                    None => out.extend(text),
                }
            }
            Inline::Image { alt, .. } => push_text(&mut out, &format!("[image: {alt}]"), style),
            Inline::TaskMarker(done) => {
                let box_text = if *done { CHECKED_BOX } else { UNCHECKED_BOX };
                push_text(&mut out, box_text, style);
            }
        }
    }
    out
}

/// Cuts a URL longer than `MAX_URL_DISPLAY` and ends it with an ellipsis.
fn shorten_url(url: &str) -> String {
    if url.chars().count() <= MAX_URL_DISPLAY {
        return url.to_string();
    }
    let kept = MAX_URL_DISPLAY - ELLIPSIS.len();
    let head: String = url.chars().take(kept).collect();
    format!("{head}{ELLIPSIS}")
}

fn push_text(out: &mut Vec<Cell>, text: &str, style: Style) {
    out.extend(
        text.chars()
            .map(|c| (if c == '\n' { ' ' } else { c }, style)),
    );
}

/// Terminal columns one character takes.
fn char_width(c: char) -> usize {
    let mut buf = [0u8; 4];
    Span::raw(&*c.encode_utf8(&mut buf)).width()
}

fn cells_width(cells: &[Cell]) -> usize {
    cells.iter().map(|&(c, _)| char_width(c)).sum()
}

/// Length of the longest prefix that fits `width` columns (at least one cell), or `None` if all fit.
fn overflow_cut(cells: &[Cell], width: usize) -> Option<usize> {
    let mut used = 0;
    for (index, &(c, _)) in cells.iter().enumerate() {
        used += char_width(c);
        if used > width {
            return Some(index.max(1));
        }
    }
    None
}

/// Greedy word wrap; a word wider than `width` is split across lines.
fn wrap(cells: &[Cell], width: usize) -> Vec<Vec<Cell>> {
    let mut lines: Vec<Vec<Cell>> = Vec::new();
    let mut current: Vec<Cell> = Vec::new();
    let mut index = 0;
    while index < cells.len() {
        let space = if cells[index].0 == ' ' {
            let cell = cells[index];
            while index < cells.len() && cells[index].0 == ' ' {
                index += 1;
            }
            Some(cell)
        } else {
            None
        };
        let start = index;
        while index < cells.len() && cells[index].0 != ' ' {
            index += 1;
        }
        let mut word = &cells[start..index];
        if word.is_empty() {
            continue;
        }
        let gap = usize::from(space.is_some() && !current.is_empty());
        if !current.is_empty() && cells_width(&current) + gap + cells_width(word) > width {
            lines.push(std::mem::take(&mut current));
        } else if let (Some(space), true) = (space, gap == 1) {
            current.push(space);
        }
        while let Some(cut) = overflow_cut(word, width) {
            let (head, tail) = word.split_at(cut);
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            lines.push(head.to_vec());
            word = tail;
        }
        current.extend_from_slice(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Groups neighbouring columns of the same style into spans.
fn to_line(cells: &[Cell]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut run_style = Style::default();
    for &(c, style) in cells {
        if !run.is_empty() && style != run_style {
            spans.push(Span::styled(std::mem::take(&mut run), run_style));
        }
        run_style = style;
        run.push(c);
    }
    if !run.is_empty() {
        spans.push(Span::styled(run, run_style));
    }
    Line::from(spans)
}
