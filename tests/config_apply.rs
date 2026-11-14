// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
mod support;

use chrono::{TimeZone, Utc};
use helios::app::startup::startup_app;
use helios::app::worker::Loaded;
use helios::app::{App, DetailTab, FileRow, ListView, PREVIEW_DELAY_MS};
use helios::config::{
    self, Config, DEFAULT_LIST_PERCENT, DEFAULT_NARROW_BELOW, DEFAULT_PREVIEW_DELAY_MS,
};
use helios::github::queries::map_page;
use helios::model::{PrKey, PrSummary};
use helios::sanitize::sanitize;
use helios::theme::Theme;
use helios::tui::draw::draw;
use helios::tui::layout::{LIST_PERCENT, NARROW_BELOW};
use helios::tui::list;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use std::fs;
use support::clock::FixedClock;
use support::files::patch_file;
use support::render::{buffer_text, line_with};

const HOST: &str = "github.com";
const ACCOUNT: &str = "octo";
const PR_NUMBER: u64 = 1;
const REVIEW_FIXTURE: &str = "tests/fixtures/list/synthetic-review.json";
const SCREEN_HEIGHT: u16 = 12;
const LIST_WIDTH: u16 = 100;
const DEFAULT_SPLIT_WIDTH: u16 = 110;
const WIDER_LIST_PERCENT: u16 = 60;
const FOCUSED_CORNER: char = '\u{250f}';
const PLAIN_CORNER: char = '\u{250c}';

fn settings(text: &str) -> Config {
    config::parse(text).expect("a valid settings file")
}

fn started(config: &Config) -> App {
    startup_app(config, None, None, |_| None)
}

fn top_row(app: &App, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, SCREEN_HEIGHT)).expect("terminal");
    terminal.draw(|frame| draw(frame, app)).expect("draw");
    let screen = buffer_text(terminal.backend().buffer());
    screen.lines().next().unwrap_or_default().to_string()
}

fn column_of(row: &str, corner: char) -> Option<usize> {
    row.chars().position(|c| c == corner)
}

#[test]
fn list_percent_and_narrow_limit_change_the_layout() {
    // Arrange
    let default = started(&Config::default());
    let wider = started(&settings("[layout]\nlist_percent = 60\n"));
    let raised = started(&settings("[layout]\nnarrow_below = 120\n"));
    let mut apps = [default, wider, raised];
    for app in &mut apps {
        app.sync_split(DEFAULT_SPLIT_WIDTH);
    }
    let [default, wider, raised] = apps;

    // Act
    let default_row = top_row(&default, DEFAULT_SPLIT_WIDTH);
    let wider_row = top_row(&wider, DEFAULT_SPLIT_WIDTH);
    let raised_row = top_row(&raised, DEFAULT_SPLIT_WIDTH);

    // Assert
    let default_border = column_of(&default_row, PLAIN_CORNER).expect("the default splits");
    let wider_border = column_of(&wider_row, PLAIN_CORNER).expect("the wider list splits");
    assert_eq!(
        default_border,
        usize::from(DEFAULT_SPLIT_WIDTH * DEFAULT_LIST_PERCENT / 100)
    );
    assert_eq!(
        wider_border,
        usize::from(DEFAULT_SPLIT_WIDTH * WIDER_LIST_PERCENT / 100)
    );
    assert!(column_of(&default_row, FOCUSED_CORNER).is_some());
    assert_eq!(column_of(&raised_row, FOCUSED_CORNER), None, "{raised_row}");
}

fn render_headers(config: &Config, view: ListView) -> String {
    let json = fs::read_to_string(REVIEW_FIXTURE).expect("read the list fixture");
    let rows = map_page(HOST, ACCOUNT, &json)
        .expect("map the fixture")
        .rows;
    let mut app = started(config);
    app.view = view;
    let mut terminal = Terminal::new(TestBackend::new(LIST_WIDTH, SCREEN_HEIGHT)).expect("tty");
    terminal
        .draw(|frame| {
            list::render_rows(
                frame,
                frame.area(),
                &app,
                rows.iter().collect(),
                &FixedClock::after_the_list_fixtures(),
                None,
            )
        })
        .expect("draw");
    let screen = buffer_text(terminal.backend().buffer());
    line_with(&screen, "TITLE").to_string()
}

fn header_names(header: &str) -> Vec<&str> {
    header.split_whitespace().collect()
}

#[test]
fn column_lists_choose_and_order_columns() {
    // Arrange
    let chosen = settings("[columns]\nmine = [\"title\", \"number\"]\n");
    let mut emptied = Config::default();
    emptied.columns.mine = Vec::new();

    // Act
    let mine = render_headers(&chosen, ListView::Mine);
    let needs_me = render_headers(&chosen, ListView::NeedsMe);
    let default = render_headers(&Config::default(), ListView::Mine);
    let empty = render_headers(&emptied, ListView::Mine);

    // Assert
    assert_eq!(header_names(&mine), ["TITLE", "#"], "{mine}");
    assert!(header_names(&needs_me).contains(&"REPO"), "{needs_me}");
    assert!(header_names(&default).contains(&"REPO"), "{default}");
    assert_eq!(header_names(&empty), header_names(&default));
}

#[test]
fn the_theme_name_picks_the_start_theme() {
    // Arrange
    let dark = settings("[theme]\nname = \"dark\"\n");
    let no_color = |name: &str| (name == "NO_COLOR").then(|| "1".to_string());

    // Act
    let from_file = startup_app(&dark, None, None, |_| None);
    let from_flag = startup_app(&dark, Some("light"), None, |_| None);
    let forced_mono = startup_app(&dark, None, None, no_color);

    // Assert
    assert_eq!(from_file.theme(), Theme::Dark);
    assert_eq!(from_flag.theme(), Theme::Light);
    assert_eq!(forced_mono.theme(), Theme::Mono);
}

#[test]
fn no_color_keeps_the_theme_on_mono_when_t_is_pressed() {
    // Arrange
    let no_color = |name: &str| (name == "NO_COLOR").then(|| "1".to_string());
    let mut app = startup_app(&settings(""), None, None, no_color);

    // Act
    app.cycle_theme();

    // Assert
    assert_eq!(app.theme(), Theme::Mono);
}

#[test]
fn t_still_cycles_the_theme_without_no_color() {
    // Arrange
    let mut app = startup_app(&settings(""), None, None, |_| None);
    let start = app.theme();

    // Act
    app.cycle_theme();

    // Assert
    assert_eq!(app.theme(), start.next());
}

fn key(number: u64) -> PrKey {
    PrKey {
        host: HOST.into(),
        account: ACCOUNT.into(),
        repo: "org/repo".into(),
        number,
    }
}

fn app_with_files(config: &Config, paths: &[&str]) -> App {
    let mut app = started(config);
    let generation = app.begin_refresh();
    let row = PrSummary {
        key: key(PR_NUMBER),
        title: sanitize("title"),
        author: sanitize("author"),
        is_draft: false,
        updated_at: Utc.timestamp_opt(0, 0).unwrap(),
        tags: Vec::new(),
        review_decision: None,
        check_state: None,
    };
    app.apply_mine(Loaded {
        generation,
        partial: false,
        host: HOST.into(),
        account: ACCOUNT.into(),
        result: Ok(vec![row]),
    });
    app.open_highlighted();
    app.tab = DetailTab::Files;
    let files = paths.iter().map(|path| patch_file(path, "x")).collect();
    app.apply_files(&key(PR_NUMBER), files);
    app
}

#[test]
fn test_patterns_and_the_collapsed_default_come_from_the_file() {
    // Arrange
    let custom = settings("[files]\ntest_dirs = [\"e2e\"]\ncollapsed = false\n");
    let paths = ["src/a.rs", "e2e/a.ts"];

    // Act
    let app = app_with_files(&custom, &paths);
    let default = app_with_files(&Config::default(), &paths);

    // Assert
    assert!(app
        .file_rows()
        .iter()
        .any(|row| matches!(row, FileRow::Tests(1))));
    assert!(app.is_file_open(0));
    assert!(app.tests_open());
    assert!(!default
        .file_rows()
        .iter()
        .any(|row| matches!(row, FileRow::Tests(_))));
    assert!(!default.is_file_open(0));
}

#[test]
fn the_tui_constants_equal_the_config_defaults() {
    // Arrange
    let config = Config::default();

    // Act
    let layout = config.layout;

    // Assert
    assert_eq!(LIST_PERCENT, layout.list_percent);
    assert_eq!(NARROW_BELOW, layout.narrow_below);
    assert_eq!(PREVIEW_DELAY_MS, layout.preview_delay_ms);
    assert_eq!(DEFAULT_NARROW_BELOW, NARROW_BELOW);
    assert_eq!(DEFAULT_PREVIEW_DELAY_MS, PREVIEW_DELAY_MS);
}
