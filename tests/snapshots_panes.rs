// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
mod support;
use support::clock::FixedClock;

use chrono::{TimeZone, Utc};
use helios::app::worker::Loaded;
use helios::app::{App, Pane};
use helios::model::{DetailCounts, PrDetail, PrKey, PrState, PrSummary};
use helios::sanitize::sanitize;
use helios::tui::{draw_with, mine};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use support::render::buffer_text;
use support::snapshot::assert_snapshot;

const WIDE_WIDTH: u16 = 120;
const NARROW_WIDTH: u16 = 80;
const HEIGHT: u16 = 16;
const LIST_PANE_WIDTH: usize = 48;
const BORDER_WIDTH: usize = 1;
const HOST: &str = "github.com";
const ACCOUNT: &str = "octo";
const REPO: &str = "org/repo";
const FOCUS_MARKER: &str = "> ";
const LIST_TITLE: &str = "[Mine]";
const DETAIL_TITLE: &str = "[Overview]";
const LIST_NOTE: &str = "2 PRs";
const DETAIL_NOTE: &str = "org/repo#1";
const FIRST_ROW_TITLE: &str = "First change";
const FIRST_ROW_NUMBER: &str = "#1";
const FIRST_ROW_LINE: usize = 2;
const PLAIN_CORNER: char = '┌';
const THICK_CORNER: char = '┏';
const RESIZE_TEXT: &str = "too small";
const TOO_SMALL_SIZES: [(u16, u16); 3] = [(39, 8), (40, 7), (39, 7)];
const MIN_SIZE: (u16, u16) = (40, 8);

fn row(number: u64, title: &str) -> PrSummary {
    PrSummary {
        key: PrKey {
            host: HOST.into(),
            account: ACCOUNT.into(),
            repo: REPO.into(),
            number,
        },
        title: sanitize(title),
        author: sanitize("octo"),
        is_draft: false,
        updated_at: Utc
            .with_ymd_and_hms(2026, 11, 5, 10, 0, 0)
            .single()
            .expect("instant"),
        tags: Vec::new(),
        review_decision: None,
        check_state: None,
    }
}

fn listed() -> App {
    let mut app = App::new();
    app.set_account_count(1);
    let generation = app.begin_refresh();
    app.apply_mine(Loaded {
        generation,
        partial: false,
        host: HOST.into(),
        account: ACCOUNT.into(),
        result: Ok(vec![row(1, FIRST_ROW_TITLE), row(2, "Second change")]),
    });
    app
}

/// A split app with the first row previewed and the list focused.
fn split_with_detail() -> App {
    let mut app = listed();
    app.set_split(true);
    app.open_highlighted();
    let key = app.selected.clone().expect("a selected PR");
    app.apply_detail(
        &key,
        Ok(PrDetail {
            title: sanitize(FIRST_ROW_TITLE),
            body: sanitize("The body text"),
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
        }),
    );
    app.pane = Pane::List;
    app
}

fn render(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| draw_with(frame, app, &FixedClock::after_the_list_fixtures()))
        .expect("draw");
    buffer_text(terminal.backend().buffer())
}

/// The list row as the Mine view draws it, in a pane as wide as the list pane's inside.
fn reference_row(app: &App) -> String {
    let inner = LIST_PANE_WIDTH - 2 * BORDER_WIDTH;
    let width = u16::try_from(inner).expect("a small width");
    let mut terminal = Terminal::new(TestBackend::new(width, HEIGHT)).expect("test terminal");
    terminal
        .draw(|frame| {
            mine::render(
                frame,
                frame.area(),
                app,
                &FixedClock::after_the_list_fixtures(),
            )
        })
        .expect("draw");
    let screen = buffer_text(terminal.backend().buffer());
    support::render::line_with(&screen, FIRST_ROW_NUMBER).to_owned()
}

/// The first list row: under the title line and the table header.
fn first_row_line(screen: &str) -> &str {
    screen.lines().nth(FIRST_ROW_LINE).unwrap_or_default()
}

fn first_line(screen: &str) -> &str {
    screen.lines().next().unwrap_or_default()
}

fn left_half(line: &str) -> String {
    line.chars().take(LIST_PANE_WIDTH).collect()
}

fn right_half(line: &str) -> String {
    line.chars().skip(LIST_PANE_WIDTH).collect()
}

#[test]
fn the_wide_screen_shows_both_panes_with_titles() {
    // Arrange
    let app = split_with_detail();

    // Act
    let screen = render(&app, WIDE_WIDTH, HEIGHT);

    // Assert
    let top = first_line(&screen);
    for text in [LIST_TITLE, LIST_NOTE] {
        assert!(
            left_half(top).contains(text),
            "list title lacks {text:?}:\n{screen}"
        );
    }
    for text in [DETAIL_TITLE, DETAIL_NOTE] {
        assert!(
            right_half(top).contains(text),
            "detail title lacks {text:?}:\n{screen}"
        );
    }
    assert!(top.contains(THICK_CORNER), "no focused border:\n{screen}");
    assert!(top.contains(PLAIN_CORNER), "no plain border:\n{screen}");
    let wide_row = first_row_line(&screen);
    let inside: String = left_half(wide_row)
        .chars()
        .skip(BORDER_WIDTH)
        .take(LIST_PANE_WIDTH - 2 * BORDER_WIDTH)
        .collect();
    assert_eq!(inside.trim_end(), reference_row(&app).trim_end());
    assert_snapshot("panes_wide", &screen);
}

#[test]
fn the_focused_pane_is_marked_in_text() {
    // Arrange
    let mut app = split_with_detail();

    for (focus, list_marked) in [(Pane::List, true), (Pane::Detail, false)] {
        // Act
        app.pane = focus;
        let screen = render(&app, WIDE_WIDTH, HEIGHT);

        // Assert
        let top = first_line(&screen);
        let list_marker = format!("{FOCUS_MARKER}{LIST_TITLE}");
        let detail_marker = format!("{FOCUS_MARKER}{DETAIL_TITLE}");
        assert_eq!(
            left_half(top).contains(&list_marker),
            list_marked,
            "{screen}"
        );
        assert_eq!(
            right_half(top).contains(&detail_marker),
            !list_marked,
            "{screen}"
        );
        assert_eq!(top.matches(FOCUS_MARKER).count(), 1, "{screen}");
    }
}

#[test]
fn the_narrow_screen_keeps_one_pane() {
    // Arrange
    let app = listed();

    // Act
    let screen = render(&app, NARROW_WIDTH, HEIGHT);

    // Assert
    let top = first_line(&screen);
    assert_eq!(
        screen.matches(PLAIN_CORNER).count(),
        1,
        "not exactly one bordered pane:\n{screen}"
    );
    assert!(
        !top.contains(FOCUS_MARKER),
        "a focus marker in narrow mode:\n{screen}"
    );
    assert_snapshot("panes_narrow", &screen);
}

#[test]
fn a_terminal_under_the_minimum_size_shows_a_resize_message() {
    // Arrange
    let app = listed();

    for (width, height) in TOO_SMALL_SIZES {
        // Act
        let screen = render(&app, width, height);

        // Assert
        let lines: Vec<&str> = screen.lines().filter(|line| !line.is_empty()).collect();
        assert_eq!(lines.len(), 1, "{width}x{height}:\n{screen}");
        assert!(
            lines[0].contains(RESIZE_TEXT),
            "{width}x{height}:\n{screen}"
        );
    }

    // Act
    let screen = render(&app, MIN_SIZE.0, MIN_SIZE.1);

    // Assert
    assert!(!screen.contains(RESIZE_TEXT), "{screen}");
    assert!(screen.contains(LIST_TITLE), "{screen}");
}
