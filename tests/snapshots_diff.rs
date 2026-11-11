// SPDX-License-Identifier: Apache-2.0

use helios::github::queries::map_files;
use helios::model::ChangedFile;
use helios::tui::diff::{
    render_scrolled, render_with, DiffOptions, ADDED_OR_REMOVED_NO_TEXT_LINE, CHANGED_NO_TEXT_LINE,
    CUT_LINE, MAX_DIFF_LINES, TRUNCATED_LINE,
};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::Terminal;
use serde_json::json;

#[allow(dead_code)]
mod support;
use support::render::{buffer_text as screen_text, line_with};
use support::snapshot::assert_snapshot;

const SCREEN_WIDTH: u16 = 80;
const SCREEN_HEIGHT: u16 = 24;
const TALL_HEIGHT: u16 = 620;
const BLOB_URL: &str = "https://github.com/octo/repo/blob/abc/file.rs";
const FILE_NAME: &str = "src/lib.rs";

const HUNK_HEADER: &str = "@@ -10,4 +10,4 @@ fn demo() {";
const CONTEXT_BEFORE: &str = "ctxbefore";
const DELETED: &str = "deletedline";
const ADDED: &str = "addedline";
const CONTEXT_AFTER: &str = "ctxafter";
const HUNK_PATCH: &str =
    "@@ -10,4 +10,4 @@ fn demo() {\n ctxbefore\n-deletedline\n+addedline\n ctxafter";

const ADD_COLOR: Color = Color::Green;
const DELETE_COLOR: Color = Color::Red;

fn file_with(patch: Option<&str>, status: &str, changes: u64) -> ChangedFile {
    let json = json!([{
        "filename": FILE_NAME,
        "previous_filename": null,
        "status": status,
        "changes": changes,
        "blob_url": BLOB_URL,
        "patch": patch
    }])
    .to_string();
    map_files(&json).expect("map the files fixture").remove(0)
}

fn patch_file(patch: &str) -> ChangedFile {
    file_with(Some(patch), "modified", 1)
}

fn draw(file: &ChangedFile, color: bool, height: u16) -> Buffer {
    let mut terminal =
        Terminal::new(TestBackend::new(SCREEN_WIDTH, height)).expect("test terminal");
    terminal
        .draw(|frame| render_with(frame, frame.area(), file, DiffOptions { color }))
        .expect("draw");
    terminal.backend().buffer().clone()
}

/// The cells of the first row holding `needle`, from the needle's first column.
fn cells_of<'a>(buffer: &'a Buffer, needle: &str) -> (usize, &'a [ratatui::buffer::Cell]) {
    let width = usize::from(buffer.area.width);
    buffer
        .content()
        .chunks(width)
        .find_map(|row| {
            let line: String = row.iter().map(|cell| cell.symbol()).collect();
            line.find(needle).map(|column| (column, row))
        })
        .unwrap_or_else(|| panic!("no row holds {needle:?}:\n{}", screen_text(buffer)))
}

#[test]
fn hunks_render_with_old_and_new_line_numbers_in_two_gutters() {
    // Arrange: a hunk with context, a delete and an add, starting at line 10 on both sides
    let file = patch_file(HUNK_PATCH);

    // Act
    let screen = screen_text(&draw(&file, true, SCREEN_HEIGHT));

    // Assert: the header shows, each line carries its own numbers, and the whole screen is pinned
    assert!(screen.contains(HUNK_HEADER), "no hunk header:\n{screen}");
    let context: Vec<&str> = line_with(&screen, CONTEXT_BEFORE)
        .split_whitespace()
        .collect();
    assert_eq!(&context[..2], ["10", "10"], "context gutters");
    let deleted: Vec<&str> = line_with(&screen, DELETED).split_whitespace().collect();
    assert_eq!(deleted[0], "11", "the deleted line keeps its old number");
    assert!(deleted.iter().skip(1).all(|t| t.parse::<u32>().is_err()));
    let added: Vec<&str> = line_with(&screen, ADDED).split_whitespace().collect();
    assert_eq!(added[0], "11", "the added line keeps its new number");
    let after: Vec<&str> = line_with(&screen, CONTEXT_AFTER)
        .split_whitespace()
        .collect();
    assert_eq!(
        &after[..2],
        ["12", "12"],
        "context gutters after the change"
    );
    assert_snapshot("diff_hunks", &screen);
}

#[test]
fn added_and_deleted_lines_use_distinct_colours() {
    // Arrange
    let file = patch_file(HUNK_PATCH);

    // Act
    let buffer = draw(&file, true, SCREEN_HEIGHT);

    // Assert: the add is green, the delete is red, the context is neither
    let (added_at, added_row) = cells_of(&buffer, ADDED);
    let (deleted_at, deleted_row) = cells_of(&buffer, DELETED);
    let (context_at, context_row) = cells_of(&buffer, CONTEXT_BEFORE);
    assert_eq!(added_row[added_at].fg, ADD_COLOR);
    assert_eq!(deleted_row[deleted_at].fg, DELETE_COLOR);
    assert_ne!(context_row[context_at].fg, ADD_COLOR);
    assert_ne!(context_row[context_at].fg, DELETE_COLOR);
}

#[test]
fn without_colour_markers_remain_and_no_cell_is_coloured() {
    // Arrange
    let file = patch_file(HUNK_PATCH);

    // Act
    let buffer = draw(&file, false, SCREEN_HEIGHT);

    // Assert: the markers stay, no cell has a foreground or background colour
    let screen = screen_text(&buffer);
    assert!(line_with(&screen, ADDED).contains(&format!("+{ADDED}")));
    assert!(line_with(&screen, DELETED).contains(&format!("-{DELETED}")));
    for cell in buffer.content() {
        assert_eq!(cell.fg, Color::Reset, "a coloured cell {:?}", cell.symbol());
        assert_eq!(cell.bg, Color::Reset, "a coloured cell {:?}", cell.symbol());
    }
    assert_snapshot("diff_no_colour", &screen);
}

#[test]
fn a_patch_over_the_bound_shows_its_first_lines_then_one_cut_line() {
    // Arrange: a hunk of one context line more than the bound allows
    let lines = MAX_DIFF_LINES + 5;
    let mut patch = format!("@@ -1,{lines} +1,{lines} @@\n");
    for n in 1..=lines {
        patch.push_str(&format!(" row{n:04}\n"));
    }
    let file = patch_file(&patch);

    // Act
    let screen = screen_text(&draw(&file, true, TALL_HEIGHT));

    // Assert: the first lines show, the cut line appears once, the rest never shows
    assert!(screen.contains("row0001"), "the first line is missing");
    assert_eq!(screen.matches(CUT_LINE).count(), 1, "cut line:\n{screen}");
    let after_bound = format!("row{:04}", MAX_DIFF_LINES + 1);
    assert!(
        !screen.contains(&after_bound),
        "a line past the bound shows"
    );
    assert!(!screen.contains(&format!("row{lines:04}")));
    let (_, kept): (Vec<_>, Vec<_>) = screen.lines().partition(|l| !l.contains("row"));
    assert!(kept.len() <= MAX_DIFF_LINES, "more rows than the bound");
    assert_snapshot("diff_cut", &screen);
}

#[test]
fn a_truncated_file_renders_one_fixed_line() {
    // Arrange: GitHub left the patch out of a changed file
    let file = file_with(None, "modified", 900);

    // Act
    let screen = screen_text(&draw(&file, true, SCREEN_HEIGHT));

    // Assert
    assert_eq!(screen.matches(TRUNCATED_LINE).count(), 1, "{screen}");
    assert!(!screen.contains(CHANGED_NO_TEXT_LINE));
    assert_snapshot("diff_truncated", &screen);
}

#[test]
fn a_modified_file_without_text_says_binary_or_mode_change() {
    // Arrange: a modified file with no patch and no counted changes
    let file = file_with(None, "modified", 0);

    // Act
    let screen = screen_text(&draw(&file, true, SCREEN_HEIGHT));

    // Assert
    assert_eq!(screen.matches(CHANGED_NO_TEXT_LINE).count(), 1, "{screen}");
    assert!(!screen.contains(TRUNCATED_LINE));
    assert_snapshot("diff_changed_no_text", &screen);
}

#[test]
fn an_added_or_removed_file_without_text_says_empty_or_binary() {
    for status in ["added", "removed"] {
        // Arrange
        let file = file_with(None, status, 0);

        // Act
        let screen = screen_text(&draw(&file, true, SCREEN_HEIGHT));

        // Assert
        assert_eq!(
            screen.matches(ADDED_OR_REMOVED_NO_TEXT_LINE).count(),
            1,
            "{status}: {screen}"
        );
        assert!(!screen.contains(CHANGED_NO_TEXT_LINE), "{status}");
    }
}

#[test]
fn a_hunk_header_at_the_largest_line_number_does_not_overflow() {
    // Arrange
    let patch = "@@ -4294967295 +4294967295 @@\n context\n context";
    let file = patch_file(patch);

    // Act
    let screen = screen_text(&draw(&file, false, SCREEN_HEIGHT));

    // Assert: both context rows keep the largest number; a wrapped counter would show 0
    let rows: Vec<&str> = screen.lines().filter(|l| l.contains("context")).collect();
    assert_eq!(rows.len(), 2, "{screen}");
    assert!(
        rows.iter().all(|row| row.contains("4294967295")),
        "{screen}"
    );
    assert!(!screen.contains(" 0 "), "{screen}");
}

#[test]
fn a_patch_of_exactly_the_line_cap_has_no_cut_line() {
    // Arrange: one header and enough context rows to reach the cap exactly
    let rows = "\n context".repeat(MAX_DIFF_LINES - 1);
    let patch = format!("@@ -1,{0} +1,{0} @@{rows}", MAX_DIFF_LINES - 1);
    let file = patch_file(&patch);

    // Act
    let screen = screen_text(&draw(&file, false, TALL_HEIGHT));

    // Assert
    assert!(!screen.contains(CUT_LINE), "a full patch was cut");
    assert_eq!(screen.matches("context").count(), MAX_DIFF_LINES - 1);
}

#[test]
fn a_second_hunk_restarts_the_gutters_after_a_no_newline_line() {
    // Arrange
    let patch = "@@ -1,2 +1,2 @@\n keep\n-old\n+new\n\\ No newline at end of file\n@@ -40,1 +50,1 @@\n tail";
    let file = patch_file(patch);

    // Act
    let screen = screen_text(&draw(&file, false, SCREEN_HEIGHT));

    // Assert
    assert!(line_with(&screen, "keep").contains(" 1    1 "), "{screen}");
    let marker = line_with(&screen, "No newline");
    assert!(
        marker.trim_start().starts_with("\\ No newline"),
        "{marker:?}"
    );
    let tail = line_with(&screen, "tail");
    assert!(tail.contains(" 40   50 "), "{tail:?}");
}

#[test]
fn tab_indented_code_keeps_its_indent_on_screen() {
    // Arrange: a patch whose added line is indented with one tab, as Go and Makefile code is
    let file = patch_file("@@ -1,1 +1,2 @@\n ctxbefore\n+\tindented");

    // Act
    let text = screen_text(&draw(&file, false, SCREEN_HEIGHT));

    // Assert: the tab turns into spaces, so the text does not touch the marker
    assert!(
        text.contains("+    indented"),
        "tab indent lost on screen:\n{text}"
    );
}

/// Columns taken by the two line number gutters and their spaces.
const GUTTER_COLUMNS: usize = 10;
const TEXT_COLUMNS: usize = SCREEN_WIDTH as usize - GUTTER_COLUMNS;
const WRAP_ROWS: usize = 3;

fn draw_scrolled(file: &ChangedFile, scroll: usize) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(SCREEN_WIDTH, SCREEN_HEIGHT)).expect("test terminal");
    terminal
        .draw(|frame| {
            render_scrolled(
                frame,
                frame.area(),
                file,
                DiffOptions { color: false },
                scroll,
            );
        })
        .expect("draw");
    screen_text(terminal.backend().buffer())
}

/// The rows after the file header, up to the first blank row.
fn body_rows(screen: &str) -> Vec<&str> {
    screen
        .lines()
        .skip(1)
        .take_while(|row| !row.is_empty())
        .collect()
}

#[test]
fn a_line_three_times_the_width_wraps_to_three_rows_with_the_number_on_the_first() {
    // Arrange: the marker plus filler fills exactly three text areas
    let long = format!("+{}", "a".repeat(TEXT_COLUMNS * WRAP_ROWS - 1));
    let file = patch_file(&format!("@@ -1,1 +7,1 @@\n{long}"));

    // Act
    let screen = screen_text(&draw(&file, false, SCREEN_HEIGHT));

    // Assert: the hunk row, then three rows; only the first carries the number
    let rows = body_rows(&screen);
    assert_eq!(rows.len(), 1 + WRAP_ROWS, "{screen}");
    assert!(rows[1].trim_start().starts_with("7 "), "{screen}");
    for row in &rows[2..] {
        assert!(row.starts_with(&" ".repeat(GUTTER_COLUMNS)), "{row:?}");
        assert_eq!(row.trim_start().len(), TEXT_COLUMNS, "{row:?}");
    }
    assert_eq!(screen.matches('a').count(), TEXT_COLUMNS * WRAP_ROWS - 1);
    assert_snapshot("diff_wrapped", &screen);
}

#[test]
fn scrolling_moves_one_patch_line_even_when_the_top_line_wraps() {
    // Arrange: a wrapped added line, then one short added line
    let long = format!("+{}", "w".repeat(TEXT_COLUMNS * WRAP_ROWS));
    let file = patch_file(&format!("@@ -1,1 +1,2 @@\n{long}\n+tail"));

    // Act: skip the hunk line, then the wrapped line
    let one = draw_scrolled(&file, 1);
    let two = draw_scrolled(&file, 2);

    // Assert
    assert!(body_rows(&one)[0].contains('w'), "{one}");
    assert!(!two.contains('w'), "{two}");
    assert!(body_rows(&two)[0].contains("tail"), "{two}");
}

#[test]
fn a_wrapped_tab_line_still_draws_the_tab_as_four_spaces() {
    // Arrange: a tab, then filler that forces a wrap
    let file = patch_file(&format!("@@ -1,1 +1,1 @@\n+\t{}", "t".repeat(TEXT_COLUMNS)));

    // Act
    let screen = screen_text(&draw(&file, false, SCREEN_HEIGHT));

    // Assert: the first row ends right after the spaces and filler, so the tab took four cells
    let rows = body_rows(&screen);
    assert_eq!(rows.len(), 3, "{screen}");
    let first = rows[1].trim_start();
    assert!(first.contains("+    t"), "{screen}");
    assert_eq!(rows[1].chars().count(), SCREEN_WIDTH as usize, "{screen}");
}

#[test]
fn wide_glyphs_are_never_split_across_rows() {
    // Arrange: two-cell glyphs; the marker leaves one odd cell on each full row
    let glyphs = 100;
    let file = patch_file(&format!("@@ -1,1 +1,1 @@\n+{}", "日".repeat(glyphs)));

    // Act
    let screen = screen_text(&draw(&file, false, SCREEN_HEIGHT));

    // Assert: every glyph is whole, and no row holds more than fit
    assert_eq!(screen.matches('日').count(), glyphs, "{screen}");
    let per_row = (TEXT_COLUMNS - 1) / 2;
    let rows = body_rows(&screen);
    assert_eq!(rows[1].matches('日').count(), per_row, "{screen}");
    assert!(rows
        .iter()
        .all(|row| row.matches('日').count() <= per_row + 1));
}
