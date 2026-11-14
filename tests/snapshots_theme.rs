// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
mod support;
use support::clock::FixedClock;

use chrono::{TimeZone, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use helios::app::worker::Loaded;
use helios::app::{App, DetailTab, Step};
use helios::model::{
    CheckRun, CheckRunState, CheckState, DetailCounts, PrDetail, PrKey, PrState, PrSummary,
    ReviewDecision,
};
use helios::sanitize::sanitize;
use helios::theme::{ColorDepth, Theme};
use helios::tui::cache::RenderCache;
use helios::tui::draw::draw_cached;
use helios::tui::keys::handle_key;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::Terminal;
use support::files::patch_file;
use support::render::{buffer_text, cell_colors};

const WIDTH: u16 = 100;
const HEIGHT: u16 = 16;
const HOST: &str = "github.com";
const ACCOUNT: &str = "octo";
/// The terminal row of the first list row: the top border, then the header.
const FIRST_ROW_Y: u16 = 2;
const SELECTED_ROW_Y: u16 = FIRST_ROW_Y;
const FAILING_ROW_Y: u16 = FIRST_ROW_Y + 1;

fn row(
    number: u64,
    title: &str,
    check_state: Option<CheckState>,
    review_decision: Option<ReviewDecision>,
) -> PrSummary {
    PrSummary {
        key: PrKey {
            host: HOST.into(),
            account: ACCOUNT.into(),
            repo: "org/repo".into(),
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
        review_decision,
        check_state,
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
        result: Ok(vec![
            row(
                1,
                "First change",
                Some(CheckState::Success),
                Some(ReviewDecision::Approved),
            ),
            row(
                2,
                "Second change",
                Some(CheckState::Failure),
                Some(ReviewDecision::ChangesRequested),
            ),
            row(
                3,
                "Third change",
                Some(CheckState::Pending),
                Some(ReviewDecision::ReviewRequired),
            ),
        ]),
    });
    app
}

/// The second pull request opened on `tab`, with files and a failing check.
fn opened(state: PrState, tab: DetailTab) -> App {
    let mut app = listed();
    app.move_cursor(Step::Next);
    app.open_highlighted();
    let key = app.selected.clone().expect("a selected PR");
    app.apply_detail(
        &key,
        Ok(PrDetail {
            title: sanitize("Second change"),
            body: sanitize("body"),
            state,
            author: sanitize("octo"),
            labels: Vec::new(),
            requested_reviewers: Vec::new(),
            review_decision: Some(ReviewDecision::ChangesRequested),
            comments: Vec::new(),
            threads: Vec::new(),
            commits: Vec::new(),
            check_runs: vec![CheckRun {
                name: sanitize("build"),
                state: CheckRunState::Failure,
                url: None,
            }],
            counts: DetailCounts::default(),
        }),
    );
    app.apply_files(
        &key,
        vec![patch_file("src/a.rs", "one"), patch_file("src/b.rs", "two")],
    );
    app.tab = tab;
    app
}

fn draw(app: &App, cache: &RenderCache) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("test terminal");
    terminal
        .draw(|frame| draw_cached(frame, app, &FixedClock::after_the_list_fixtures(), cache))
        .expect("draw");
    terminal.backend().buffer().clone()
}

fn press(app: &mut App, code: KeyCode) {
    handle_key(app, KeyEvent::new(code, KeyModifiers::NONE));
}

/// The column where `needle` starts on terminal row `y`.
fn column_of(buffer: &Buffer, y: u16, needle: &str) -> u16 {
    let text = buffer_text(buffer);
    let line = text.lines().nth(usize::from(y)).expect("a drawn row");
    let byte = line
        .find(needle)
        .unwrap_or_else(|| panic!("row {y} holds no {needle:?}:\n{text}"));
    u16::try_from(line[..byte].chars().count()).expect("a column")
}

#[test]
fn t_cycles_the_theme_in_order() {
    // Arrange
    let mut app = listed();
    let mut seen = vec![app.theme()];

    // Act
    for _ in 0..Theme::ALL.len() {
        press(&mut app, KeyCode::Char('t'));
        seen.push(app.theme());
    }

    // Assert
    assert_eq!(
        seen,
        [
            Theme::Standard,
            Theme::Spectrum,
            Theme::Dark,
            Theme::Light,
            Theme::Mono,
            Theme::Standard,
        ]
    );
}

#[test]
fn a_theme_change_rebuilds_cached_lines() {
    // Arrange
    let mut app = opened(PrState::Open, DetailTab::Overview);
    let cache = RenderCache::new();
    draw(&app, &cache);
    let after_first_draw = cache.builds();

    // Act
    press(&mut app, KeyCode::Char('t'));
    draw(&app, &cache);
    let after_theme_change = cache.builds();
    draw(&app, &cache);

    // Assert
    assert_eq!(after_first_draw, 1);
    assert_eq!(after_theme_change, 2);
    assert_eq!(
        cache.builds(),
        2,
        "a repeat draw with the same theme reuses"
    );
}

#[test]
fn status_text_is_never_color_only() {
    // Arrange
    let mut list = listed();
    list.set_theme(Theme::Mono);
    let mut checks = opened(PrState::Merged, DetailTab::Checks);
    checks.set_theme(Theme::Mono);
    let mut overview = opened(PrState::Closed, DetailTab::Overview);
    overview.set_theme(Theme::Mono);
    let cache = RenderCache::new();

    // Act
    let list_screen = buffer_text(&draw(&list, &cache));
    let checks_screen = buffer_text(&draw(&checks, &cache));
    let overview_screen = buffer_text(&draw(&overview, &cache));

    // Assert
    for word in ["pass", "fail", "pending", "approved", "changes", "required"] {
        assert!(list_screen.contains(word), "no {word:?}:\n{list_screen}");
    }
    assert!(checks_screen.contains("build Failure"), "{checks_screen}");
    assert!(overview_screen.contains("octo Closed"), "{overview_screen}");
    assert!(overview_screen.contains("Changes requested"));
}

#[test]
fn mono_draws_no_color() {
    // Arrange
    let screens = [
        listed(),
        opened(PrState::Open, DetailTab::Files),
        opened(PrState::Open, DetailTab::Checks),
    ];
    let cache = RenderCache::new();

    for mut app in screens {
        app.set_theme(Theme::Mono);

        // Act
        let buffer = draw(&app, &cache);

        // Assert
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                assert_eq!(
                    cell_colors(&buffer, x, y),
                    (Color::Reset, Color::Reset),
                    "cell {x},{y}:\n{}",
                    buffer_text(&buffer)
                );
            }
        }
    }
}

#[test]
fn a_16_color_terminal_gets_named_colors() {
    // Arrange
    let mut app = listed();
    app.set_theme(Theme::Dark);
    app.set_depth(ColorDepth::Ansi16);

    // Act
    let buffer = draw(&app, &RenderCache::new());

    // Assert
    // The dark theme's selection is white on blue, and its error color is red.
    let selected_title = column_of(&buffer, SELECTED_ROW_Y, "First");
    assert_eq!(
        cell_colors(&buffer, selected_title, SELECTED_ROW_Y),
        (Color::White, Color::Blue),
        "the F of First change on the selected row"
    );
    let failing_check = column_of(&buffer, FAILING_ROW_Y, "fail");
    assert_eq!(
        cell_colors(&buffer, failing_check, FAILING_ROW_Y),
        (Color::Red, Color::Reset),
        "the f of fail on the second row"
    );
}

#[test]
fn an_rgb_theme_is_reduced_on_a_16_color_terminal() {
    // Arrange
    // Standard's selection is ffffff on 1f4e79 and its error is d55e00. The
    // nearest ANSI colors are White, DarkGray (distance 11653, under Blue's
    // 20734) and Red (8900, under LightRed's 10600). In the xterm cube they
    // are index 231, 24 and 166.
    let cases = [
        (
            ColorDepth::Ansi16,
            (Color::White, Color::DarkGray),
            (Color::Red, Color::Reset),
        ),
        (
            ColorDepth::Ansi256,
            (Color::Indexed(231), Color::Indexed(24)),
            (Color::Indexed(166), Color::Reset),
        ),
    ];

    for (depth, selected, failing) in cases {
        let mut app = listed();
        app.set_theme(Theme::Standard);
        app.set_depth(depth);

        // Act
        let buffer = draw(&app, &RenderCache::new());

        // Assert
        let selected_title = column_of(&buffer, SELECTED_ROW_Y, "First");
        assert_eq!(
            cell_colors(&buffer, selected_title, SELECTED_ROW_Y),
            selected,
            "{depth:?}: the F of First change on the selected row"
        );
        let failing_check = column_of(&buffer, FAILING_ROW_Y, "fail");
        assert_eq!(
            cell_colors(&buffer, failing_check, FAILING_ROW_Y),
            failing,
            "{depth:?}: the f of fail on the second row"
        );
    }
}
