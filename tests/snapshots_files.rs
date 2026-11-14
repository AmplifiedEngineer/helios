// SPDX-License-Identifier: Apache-2.0

//! The Files tab drawn as an accordion: rows, open diffs, the tests group and the tab title.

#[allow(dead_code)]
mod support;

use helios::app::{App, DetailTab, Screen};
use helios::model::{ChangedFile, DetailCounts, FileDiff, PrDetail, PrKey, PrState};
use helios::sanitize::sanitize;
use helios::tui::detail::{self, NO_FILES_TEXT};
use helios::tui::diff::{count_changes, MAX_INLINE_DIFF_LINES};
use helios::tui::draw::draw_with;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use support::clock::FixedClock;
use support::files::patch_file;
use support::render::{buffer_text, line_with};
use support::snapshot::assert_snapshot;

const HOST: &str = "github.com";
const ACCOUNT: &str = "octo";
const REPO: &str = "org/repo";
const PR_NUMBER: u64 = 7;
const PANE_WIDTH: u16 = 70;
const PANE_HEIGHT: u16 = 14;
const TALL_HEIGHT: u16 = 260;
const SCREEN_WIDTH: u16 = 120;
const SCREEN_HEIGHT: u16 = 20;
const OVER_CAP_PATCH_LINES: usize = 250;
const CLOSED_MARKER: &str = "▸";
const OPEN_MARKER: &str = "▾";

fn key() -> PrKey {
    PrKey {
        host: HOST.into(),
        account: ACCOUNT.into(),
        repo: REPO.into(),
        number: PR_NUMBER,
    }
}

fn detail() -> PrDetail {
    PrDetail {
        title: sanitize("A change"),
        body: sanitize("body"),
        state: PrState::Open,
        author: sanitize("octo"),
        labels: Vec::new(),
        requested_reviewers: Vec::new(),
        review_decision: None,
        comments: Vec::new(),
        threads: Vec::new(),
        commits: Vec::new(),
        check_runs: Vec::new(),
        counts: DetailCounts::default(),
    }
}

fn app_with(files: Vec<ChangedFile>) -> App {
    let mut app = App::new();
    app.select(key());
    app.apply_detail(&key(), Ok(detail()));
    app.apply_files(&key(), files);
    app.tab = DetailTab::Files;
    app.screen = Screen::Detail;
    app
}

fn render_sized(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| {
            detail::render(
                frame,
                frame.area(),
                app,
                &FixedClock::at(2026, 11, 7, 20, 30, 46),
            )
        })
        .expect("draw");
    buffer_text(terminal.backend().buffer())
}

fn render(app: &App) -> String {
    render_sized(app, PANE_WIDTH, PANE_HEIGHT)
}

fn long_patch_file(path: &str, patch_lines: usize) -> ChangedFile {
    let added = patch_lines - 1;
    let body: Vec<String> = (0..added).map(|index| format!("+added-{index}")).collect();
    ChangedFile {
        path: sanitize(path),
        previous_path: None,
        diff: FileDiff::Patch(sanitize(&format!(
            "@@ -0,0 +1,{added} @@\n{}",
            body.join("\n")
        ))),
    }
}

fn cut_note() -> String {
    format!("Showing the first {MAX_INLINE_DIFF_LINES} lines. Press v for the full diff.")
}

/// The drawn lines between the row holding `row` and the cut note, or the screen end.
fn diff_line_count(screen: &str, row: &str) -> usize {
    screen
        .lines()
        .skip_while(|line| !line.contains(row))
        .skip(1)
        .take_while(|line| !line.contains(&cut_note()) && !line.trim().is_empty())
        .count()
}

#[test]
fn collapsed_rows_show_path_counts_and_a_closed_marker() {
    // Arrange
    let app = app_with(vec![
        patch_file("src/a.rs", "one"),
        patch_file("src/b.rs", "two"),
    ]);

    // Act
    let screen = render(&app);

    // Assert
    assert!(
        line_with(&screen, "src/a.rs").contains(&format!("{CLOSED_MARKER} src/a.rs  +1 -0")),
        "{screen}"
    );
    assert!(
        line_with(&screen, "src/b.rs").contains(&format!("{CLOSED_MARKER} src/b.rs  +1 -0")),
        "{screen}"
    );
    assert!(!screen.contains("+one"), "{screen}");
    assert_snapshot("files_collapsed", &screen);
}

#[test]
fn an_open_file_shows_its_diff_under_the_row() {
    // Arrange
    let mut app = app_with(vec![
        patch_file("src/a.rs", "one"),
        patch_file("src/b.rs", "two"),
    ]);
    app.toggle_selected_row();

    // Act
    let screen = render(&app);

    // Assert
    let row = screen.find("src/a.rs").expect("the open file row");
    let diff = screen.find("+one").expect("the open file diff");
    let closed = screen.find("src/b.rs").expect("the closed file row");
    assert!(row < diff && diff < closed, "{screen}");
    assert!(
        line_with(&screen, "src/a.rs").contains(OPEN_MARKER),
        "{screen}"
    );
    assert!(!screen.contains("+two"), "{screen}");
    assert_snapshot("files_open", &screen);
}

#[test]
fn a_diff_over_the_cap_shows_a_full_screen_row() {
    // Arrange
    let mut over = app_with(vec![long_patch_file("src/over.rs", OVER_CAP_PATCH_LINES)]);
    over.toggle_selected_row();
    let mut exact = app_with(vec![long_patch_file("src/exact.rs", MAX_INLINE_DIFF_LINES)]);
    exact.toggle_selected_row();

    // Act
    let over_screen = render_sized(&over, PANE_WIDTH, TALL_HEIGHT);
    let exact_screen = render_sized(&exact, PANE_WIDTH, TALL_HEIGHT);

    // Assert
    assert_eq!(
        diff_line_count(&over_screen, "src/over.rs"),
        MAX_INLINE_DIFF_LINES,
        "{over_screen}"
    );
    assert!(over_screen.contains(&cut_note()), "{over_screen}");
    assert_eq!(
        diff_line_count(&exact_screen, "src/exact.rs"),
        MAX_INLINE_DIFF_LINES,
        "{exact_screen}"
    );
    assert!(!exact_screen.contains(&cut_note()), "{exact_screen}");
}

#[test]
fn the_tests_group_row_shows_its_count_and_hides_its_files() {
    // Arrange
    let mut app = app_with(vec![
        patch_file("src/a.rs", "one"),
        patch_file("tests/one.rs", "two"),
        patch_file("tests/two.rs", "three"),
    ]);

    // Act
    let closed = render(&app);
    app.toggle_tests();
    let open = render(&app);

    // Assert
    assert!(
        line_with(&closed, "Tests").contains(&format!("{CLOSED_MARKER} Tests (2 files)")),
        "{closed}"
    );
    assert!(!closed.contains("tests/one.rs"), "{closed}");
    assert!(!closed.contains("tests/two.rs"), "{closed}");
    assert!(
        line_with(&open, "Tests").contains(&format!("{OPEN_MARKER} Tests (2 files)")),
        "{open}"
    );
    assert!(
        line_with(&open, "tests/one.rs").starts_with(&format!("  {CLOSED_MARKER} tests/one.rs")),
        "{open}"
    );
    assert!(
        line_with(&open, "tests/two.rs").starts_with(&format!("  {CLOSED_MARKER} tests/two.rs")),
        "{open}"
    );
}

#[test]
fn the_files_tab_title_shows_the_test_count() {
    // Arrange
    let with_tests = app_with(vec![
        patch_file("src/a.rs", "one"),
        patch_file("tests/one.rs", "two"),
        patch_file("tests/two.rs", "three"),
    ]);
    let without_tests = app_with(vec![patch_file("src/a.rs", "one")]);
    let draw = |app: &App| {
        let mut terminal =
            Terminal::new(TestBackend::new(SCREEN_WIDTH, SCREEN_HEIGHT)).expect("test terminal");
        terminal
            .draw(|frame| draw_with(frame, app, &FixedClock::at(2026, 11, 7, 20, 30, 46)))
            .expect("draw");
        buffer_text(terminal.backend().buffer())
    };

    // Act
    let counted = draw(&with_tests);
    let plain = draw(&without_tests);

    // Assert
    assert!(counted.contains("[Files (2 tests)]"), "{counted}");
    assert!(plain.contains("[Files]"), "{plain}");
    assert!(!plain.contains("tests)"), "{plain}");
}

#[test]
fn an_empty_files_list_shows_the_no_files_text() {
    // Arrange
    let app = app_with(Vec::new());

    // Act
    let screen = render(&app);

    // Assert
    assert_eq!(NO_FILES_TEXT, "No files changed.");
    assert!(screen.contains(NO_FILES_TEXT), "{screen}");
}

#[test]
fn counting_reads_added_and_removed_lines_inside_hunks() {
    // Arrange
    let patch = "--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n context\n-old\n--dashes\n+new\n+++plus\n+more\n\\ No newline at end of file";

    // Act
    let counts = count_changes(patch);

    // Assert
    assert_eq!(counts, (3, 2));
    assert_eq!(count_changes(""), (0, 0));
}
