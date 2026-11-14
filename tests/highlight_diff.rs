// SPDX-License-Identifier: Apache-2.0

//! Diff lines are colored by token role, in the full-screen diff and in an open accordion row.

#[allow(dead_code)]
mod support;

use chrono::{TimeZone, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use helios::app::worker::Loaded;
use helios::app::{App, DetailTab};
use helios::model::{ChangedFile, PrKey, PrSummary};
use helios::sanitize::sanitize;
use helios::theme::Theme;
use helios::tui::cache::RenderCache;
use helios::tui::diff::{render_with, DiffOptions, MAX_INLINE_DIFF_LINES};
use helios::tui::draw::{draw_cached, SystemClock};
use helios::tui::keys::handle_key;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::Terminal;
use support::files::{file_with_patch as file, long_lines_patch};
use support::loop_harness::{pr_detail, FakeApi, Harness};
use support::render::{buffer_text, cell_colors};

const WIDTH: u16 = 120;
const HEIGHT: u16 = 30;
const HUNK: &str = "@@ -1,1 +1,2 @@";
/// The standard theme's keyword color, 0xcc79a7.
const STANDARD_KEYWORD: Color = Color::Rgb(0xcc, 0x79, 0xa7);
/// The standard theme's added color, 0x009e73.
const STANDARD_ADDED: Color = Color::Rgb(0x00, 0x9e, 0x73);
/// The standard theme's removed color, 0xd55e00.
const STANDARD_REMOVED: Color = Color::Rgb(0xd5, 0x5e, 0x00);
/// The longest code text, in bytes after the marker, that is highlighted.
const CODE_CAP: usize = 2000;
/// The bytes of code the first frame may highlight.
const FRAME_BYTES: usize = 65536;
const LONG_LINE_BYTES: usize = 2000;
const MINIFIED_PATCH_LINES: usize = 500;
const MINIFIED_LINE_BYTES: usize = 1000;
const MINIFIED_DRAW_LIMIT: usize = 40;

fn key() -> PrKey {
    PrKey {
        host: "github.com".into(),
        account: "octo".into(),
        repo: "org/repo".into(),
        number: 1,
    }
}

/// An app on the Files tab of one pull request holding `file`.
fn app_with(file: ChangedFile, theme: Theme) -> App {
    let mut app = App::new();
    let generation = app.begin_refresh();
    app.apply_mine(Loaded {
        generation,
        partial: false,
        host: "github.com".into(),
        account: "octo".into(),
        result: Ok(vec![PrSummary {
            key: key(),
            title: sanitize("title"),
            author: sanitize("author"),
            is_draft: false,
            updated_at: Utc.timestamp_opt(0, 0).unwrap(),
            tags: Vec::new(),
            review_decision: None,
            check_state: None,
        }]),
    });
    app.open_highlighted();
    app.tab = DetailTab::Files;
    app.apply_detail(&key(), Ok(pr_detail("title")));
    app.apply_files(&key(), vec![file]);
    app.set_theme(theme);
    app
}

fn press(app: &mut App, code: KeyCode) {
    handle_key(app, KeyEvent::new(code, KeyModifiers::NONE));
}

fn draw(app: &App, cache: &RenderCache) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("test terminal");
    terminal
        .draw(|frame| draw_cached(frame, app, &SystemClock, cache))
        .expect("draw");
    terminal.backend().buffer().clone()
}

/// The column and row of the first cell of the first row that holds `needle`.
fn find(buffer: &Buffer, needle: &str) -> (u16, u16) {
    let width = usize::from(buffer.area.width);
    for (row, cells) in buffer.content().chunks(width).enumerate() {
        let line: String = cells.iter().map(|cell| cell.symbol()).collect();
        if let Some(byte) = line.find(needle) {
            let column = line[..byte].chars().count();
            return (
                u16::try_from(column).expect("a column"),
                u16::try_from(row).expect("a row"),
            );
        }
    }
    panic!("no row holds {needle:?}:\n{}", buffer_text(buffer));
}

fn fg(buffer: &Buffer, at: (u16, u16)) -> Color {
    cell_colors(buffer, at.0, at.1).0
}

fn full_screen(file: ChangedFile, theme: Theme) -> App {
    let mut app = app_with(file, theme);
    press(&mut app, KeyCode::Char('v'));
    app
}

fn accordion(file: ChangedFile, theme: Theme) -> App {
    let mut app = app_with(file, theme);
    press(&mut app, KeyCode::Enter);
    app
}

#[test]
fn added_lines_keep_their_marker_and_get_role_colors() {
    // Arrange
    let patch = format!("{HUNK}\n context\n+fn main() {{}}\n-let old = 1;");
    let colored = full_screen(file("src/a.rs", &patch), Theme::Standard);
    let mono = full_screen(file("src/a.rs", &patch), Theme::Mono);

    // Act
    let buffer = draw(&colored, &RenderCache::new());
    let mono_buffer = draw(&mono, &RenderCache::new());

    // Assert
    let (plus_x, plus_y) = find(&buffer, "+fn main");
    assert_eq!(buffer[(plus_x, plus_y)].symbol(), "+");
    assert_eq!(fg(&buffer, (plus_x, plus_y)), STANDARD_ADDED, "the plus");
    assert_eq!(
        fg(&buffer, (plus_x + 1, plus_y)),
        STANDARD_KEYWORD,
        "the f of fn"
    );
    let (minus_x, minus_y) = find(&buffer, "-let old");
    assert_eq!(buffer[(minus_x, minus_y)].symbol(), "-");
    assert_eq!(
        fg(&buffer, (minus_x, minus_y)),
        STANDARD_REMOVED,
        "the minus"
    );
    assert_eq!(
        fg(&buffer, (minus_x + 1, minus_y)),
        STANDARD_KEYWORD,
        "the l of let"
    );
    assert_eq!(buffer_text(&buffer), buffer_text(&mono_buffer));
    for cell in [(plus_x, plus_y), (plus_x + 1, plus_y), (minus_x, minus_y)] {
        assert_eq!(fg(&mono_buffer, cell), Color::Reset, "mono cell {cell:?}");
    }
}

#[test]
fn the_language_comes_from_the_file_extension() {
    // Arrange
    let cases = [
        ("src/a.rs", "fn f() {}", STANDARD_KEYWORD),
        ("tool.py", "def f(): pass", STANDARD_KEYWORD),
        ("tool.py", "fn f() {}", STANDARD_ADDED),
        ("notes.txt", "fn f() {}", STANDARD_ADDED),
    ];

    for (path, code, expected) in cases {
        let app = full_screen(
            file(path, &format!("{HUNK}\n context\n+{code}")),
            Theme::Standard,
        );

        // Act
        let buffer = draw(&app, &RenderCache::new());

        // Assert
        let (plus_x, plus_y) = find(&buffer, &format!("+{code}"));
        assert_eq!(
            fg(&buffer, (plus_x + 1, plus_y)),
            expected,
            "first letter of {code:?} in {path}"
        );
    }
}

#[test]
fn an_open_accordion_diff_is_highlighted() {
    // Arrange
    let patch = format!("{HUNK}\n context\n+fn main() {{}}");
    let app = accordion(file("src/a.rs", &patch), Theme::Standard);

    // Act
    let buffer = draw(&app, &RenderCache::new());

    // Assert
    let (plus_x, plus_y) = find(&buffer, "+fn main");
    assert_eq!(fg(&buffer, (plus_x, plus_y)), STANDARD_ADDED, "the plus");
    assert_eq!(
        fg(&buffer, (plus_x + 1, plus_y)),
        STANDARD_KEYWORD,
        "the f of fn"
    );
}

#[test]
fn a_line_at_the_cap_is_highlighted_and_one_byte_over_is_plain() {
    // The cap counts the code after the marker, as the highlighter does, so the context
    // marker is not part of the 2000 bytes.
    // Arrange
    let at_cap = format!("{:<CODE_CAP$}", "fn");
    let over_cap = format!("{:<width$}", "fn", width = CODE_CAP + 1);
    let cases = [(at_cap, STANDARD_KEYWORD), (over_cap, Color::Reset)];

    for (code, expected) in cases {
        let app = full_screen(
            file("src/a.rs", &format!("{HUNK}\n {code}")),
            Theme::Standard,
        );

        // Act
        let buffer = draw(&app, &RenderCache::new());

        // Assert
        let (fn_x, fn_y) = find(&buffer, "fn");
        assert_eq!(
            fg(&buffer, (fn_x, fn_y)),
            expected,
            "the f of fn in {} bytes",
            code.len()
        );
    }
}

#[test]
fn a_diff_over_the_frame_budget_falls_back_to_plain() {
    // The long lines use more than one frame's budget, so the last line waits for a later frame.
    // A plain added line shows the added color.
    // Arrange
    let count = FRAME_BYTES / LONG_LINE_BYTES + 8;
    let patch = long_lines_patch(count, LONG_LINE_BYTES, "fn last() {}");
    let mut app = full_screen(file("src/a.rs", &patch), Theme::Standard);
    press(&mut app, KeyCode::Char('G'));
    let cache = RenderCache::new();

    // Act
    let first = draw(&app, &cache);
    let second = draw(&app, &cache);

    // Assert
    let (x, y) = find(&first, "+fn last");
    assert_eq!(fg(&first, (x, y)), STANDARD_ADDED, "the plus");
    assert_eq!(
        fg(&first, (x + 1, y)),
        STANDARD_ADDED,
        "the f past the budget"
    );
    assert_eq!(
        fg(&second, (x + 1, y)),
        STANDARD_KEYWORD,
        "the f once the next frame has run"
    );
}

/// A hunk header and then added lines of `MINIFIED_LINE_BYTES` bytes, `MINIFIED_PATCH_LINES` in all.
fn minified_patch() -> String {
    let mut patch = String::from(HUNK);
    for _ in 1..MINIFIED_PATCH_LINES {
        patch.push_str(&format!("\n+{:<MINIFIED_LINE_BYTES$}", "var a=1;"));
    }
    patch
}

#[test]
fn a_minified_500_line_diff_is_bounded() {
    // Arrange
    let app = full_screen(file("src/min.js", &minified_patch()), Theme::Standard);
    let cache = RenderCache::new();
    let code_bytes = (MINIFIED_PATCH_LINES - 1) * MINIFIED_LINE_BYTES;

    // Act
    draw(&app, &cache);
    let after_one = cache.highlighted_bytes();
    draw(&app, &cache);
    let after_two = cache.highlighted_bytes();
    let mut draws = 2;
    while cache.take_pending() && draws < MINIFIED_DRAW_LIMIT {
        draw(&app, &cache);
        draws += 1;
    }

    // Assert
    assert!(after_one > 0 && after_one <= FRAME_BYTES, "{after_one}");
    assert!(after_two > after_one, "the next draw adds work");
    assert!(after_two <= after_one + FRAME_BYTES, "{after_two}");
    assert_eq!(
        cache.highlighted_bytes(),
        code_bytes,
        "every line is highlighted once"
    );
}

#[test]
fn highlighting_leaves_the_text_of_every_row_unchanged() {
    // Arrange
    let long = "x".repeat(300);
    let patch = format!("{HUNK}\n context\n+\tfn main() {{ let s = \"{long}\"; }}\n-removed");
    let changed = file("src/a.rs", &patch);
    let app = full_screen(changed.clone(), Theme::Standard);
    let mut plain = Terminal::new(TestBackend::new(WIDTH - 2, HEIGHT)).expect("test terminal");
    plain
        .draw(|frame| render_with(frame, frame.area(), &changed, DiffOptions { color: true }))
        .expect("draw");
    let expected = buffer_text(plain.backend().buffer());

    // Act
    let screen = buffer_text(&draw(&app, &RenderCache::new()));

    // Assert: the pane's border is one column on each side, and the path row comes first.
    let inside: Vec<String> = screen
        .lines()
        .skip(1)
        .map(|row| {
            let columns: Vec<char> = row.chars().collect();
            let end = columns.len().saturating_sub(1).max(1);
            columns[1..end]
                .iter()
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect();
    let wanted: Vec<&str> = expected.lines().collect();
    let rows = wanted.iter().rposition(|row| !row.is_empty()).unwrap_or(0) + 1;
    assert_eq!(inside[..rows], wanted[..rows]);
}

/// One row per 2000-byte line: the line, its gutters and the pane border fit.
const LOOP_WIDTH: u16 = 2100;
/// Room for the hunk header and every long line above the last one.
const LOOP_HEIGHT: u16 = 44;
const FILES_TAB_PRESSES: usize = 3;

/// A loop on the Files tab of the first pull request, showing `file`.
fn loop_on_files_tab(file: ChangedFile) -> Harness {
    let api = FakeApi {
        files: vec![file],
        ..FakeApi::new()
    };
    let mut harness = Harness::start_with_size(api, LOOP_WIDTH, LOOP_HEIGHT);
    harness.press(KeyCode::Enter);
    for _ in 0..FILES_TAB_PRESSES {
        harness.press(KeyCode::Tab);
    }
    harness
}

#[test]
fn a_pending_highlight_triggers_a_redraw_on_the_next_tick() {
    // The key that opens the diff draws the over-budget frame; the step after it is the tick.
    // Arrange
    let count = FRAME_BYTES / LONG_LINE_BYTES + 1;
    let patch = long_lines_patch(count, LONG_LINE_BYTES, "fn last() {}");
    let mut harness = loop_on_files_tab(file("src/a.rs", &patch));

    // Act
    harness.press_once(KeyCode::Char('v'));
    let over_budget = harness.buffer().clone();
    let draws_after_the_key = harness.draws();
    harness.step();
    let next_tick = harness.buffer().clone();

    // Assert
    let (x, y) = find(&over_budget, "+fn last");
    assert_eq!(
        fg(&over_budget, (x + 1, y)),
        STANDARD_ADDED,
        "plain at first"
    );
    assert_eq!(
        fg(&next_tick, (x + 1, y)),
        STANDARD_KEYWORD,
        "the f of fn after one tick"
    );
    assert_eq!(harness.draws(), draws_after_the_key + 1);
}

#[test]
fn no_pending_highlight_makes_no_extra_redraw() {
    // The count is the number of frames flushed to the test terminal.
    // Arrange
    let patch = format!("{HUNK}\n context\n+fn main() {{}}");
    let mut harness = loop_on_files_tab(file("src/a.rs", &patch));
    harness.press_once(KeyCode::Char('v'));
    let after_the_key = harness.draws();

    // Act
    harness.step();
    harness.step();

    // Assert
    assert_eq!(
        harness.draws(),
        after_the_key,
        "a settled diff does not redraw"
    );
}

#[test]
fn an_accordion_diff_highlights_only_the_lines_it_draws() {
    // Arrange: the accordion draws the first 200 patch lines, the hunk header and 199 code lines.
    let mut app = accordion(file("src/min.js", &minified_patch()), Theme::Standard);
    let cache = RenderCache::new();
    let drawn_code_bytes = (MAX_INLINE_DIFF_LINES - 1) * MINIFIED_LINE_BYTES;
    let all_code_bytes = (MINIFIED_PATCH_LINES - 1) * MINIFIED_LINE_BYTES;
    let draw_until_settled = |app: &App| {
        let mut draws = 0;
        loop {
            draw(app, &cache);
            draws += 1;
            if !cache.take_pending() || draws >= MINIFIED_DRAW_LIMIT {
                break;
            }
        }
    };

    // Act
    draw_until_settled(&app);
    let after_accordion = cache.highlighted_bytes();
    let pending_after_accordion = cache.take_pending();
    press(&mut app, KeyCode::Char('v'));
    draw_until_settled(&app);

    // Assert
    assert_eq!(after_accordion, drawn_code_bytes);
    assert!(!pending_after_accordion);
    assert_eq!(
        cache.highlighted_bytes(),
        all_code_bytes,
        "the full screen resumes past line 200 and never redoes the first 200"
    );
}
