// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
mod support;

use chrono::{TimeZone, Utc};
use helios::app::refresh::Clock;
use helios::app::worker::Loaded;
use helios::app::{App, ListFilter};
use helios::github::errors::GithubError;
use helios::github::queries::{map_page, merge_needs_me};
use helios::model::{PrSummary, ReviewDecision};
use helios::sanitize::sanitize;
use helios::tui::{mine, needs_me};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use std::fs;
use std::time::SystemTime;
use support::render::buffer_text;
use support::snapshot::assert_snapshot;

const HOST: &str = "github.com";
const ACCOUNT: &str = "octocat";
const REVIEW_FIXTURE: &str = "tests/fixtures/list/synthetic-review.json";
const ASSIGNED_FIXTURE: &str = "tests/fixtures/list/synthetic-assigned.json";
const HIDDEN_FIXTURE: &str = "tests/fixtures/list/synthetic-hidden-chars.json";
const DOWN_HOST: &str = "ghe.example.com";
const SCREEN_WIDTH: u16 = 120;
const SCREEN_HEIGHT: u16 = 24;

const REVIEW_TITLES: [&str; 2] = ["Shared change", "Review only"];
const ASSIGNED_TITLES: [&str; 3] = ["Assigned only", "No checks yet", "Ghost PR"];
const DRAFT_TITLE: &str = "Draft work";

/// A clock stopped two hours after the fixtures' `updatedAt`.
struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        Utc.with_ymd_and_hms(2026, 11, 5, 12, 0, 0)
            .single()
            .expect("a valid instant")
            .into()
    }
}

fn fixture_rows(path: &str) -> Vec<PrSummary> {
    let json = fs::read_to_string(path).expect("read the list fixture");
    map_page(HOST, ACCOUNT, &json)
        .expect("map the list fixture")
        .rows
}

fn needs_me_app() -> App {
    let rows = merge_needs_me(fixture_rows(REVIEW_FIXTURE), fixture_rows(ASSIGNED_FIXTURE));
    let mut app = App::new();
    let generation = app.begin_refresh();
    let applied = app.apply(Loaded {
        generation,
        partial: false,
        host: HOST.into(),
        account: ACCOUNT.into(),
        result: Ok(rows),
    });
    assert!(applied, "the fixture rows were not applied");
    app
}

fn render_needs_me(app: &App) -> String {
    render_needs_me_at(app, SCREEN_WIDTH)
}

fn render_needs_me_at(app: &App, width: u16) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(width, SCREEN_HEIGHT)).expect("test terminal");
    terminal
        .draw(|frame| needs_me::render(frame, frame.area(), app, &FixedClock))
        .expect("draw");
    buffer_text(terminal.backend().buffer())
}

fn assert_shows(screen: &str, titles: &[&str]) {
    for title in titles {
        assert!(
            screen.contains(title),
            "the screen lacks {title:?}:\n{screen}"
        );
    }
}

fn assert_hides(screen: &str, titles: &[&str]) {
    for title in titles {
        assert!(
            !screen.contains(title),
            "the screen shows {title:?}:\n{screen}"
        );
    }
}

#[test]
fn f_cycles_all_review_assigned_and_back() {
    // Arrange: the Needs me list with drafts hidden, on the default filter
    let mut app = needs_me_app();
    let all = [REVIEW_TITLES.as_slice(), ASSIGNED_TITLES.as_slice()].concat();
    let steps: [(ListFilter, &str, &[&str], &[&str]); 4] = [
        (
            ListFilter::Review,
            "needs_me_filter_review",
            &REVIEW_TITLES,
            &ASSIGNED_TITLES,
        ),
        (
            ListFilter::Assigned,
            "needs_me_filter_assigned",
            &ASSIGNED_TITLES,
            &REVIEW_TITLES,
        ),
        (ListFilter::All, "needs_me_filter_all", &all, &[DRAFT_TITLE]),
        (
            ListFilter::Review,
            "needs_me_filter_review",
            &REVIEW_TITLES,
            &ASSIGNED_TITLES,
        ),
    ];
    assert_eq!(
        app.filter,
        ListFilter::All,
        "the list does not start on all"
    );
    assert_shows(&render_needs_me(&app), &all);

    for (expected, snapshot, shown, hidden) in steps {
        // Act
        app.cycle_filter();
        let screen = render_needs_me(&app);

        // Assert
        assert_eq!(app.filter, expected, "f moved to the wrong filter");
        assert_shows(&screen, shown);
        assert_hides(&screen, hidden);
        assert_snapshot(snapshot, &screen);
    }
}

#[test]
fn d_toggles_drafts() {
    // Arrange: drafts are hidden by default
    let mut app = needs_me_app();
    assert!(!app.show_drafts, "drafts are not hidden by default");
    assert_hides(&render_needs_me(&app), &[DRAFT_TITLE]);

    // Act
    app.toggle_drafts();
    let with_drafts = render_needs_me(&app);

    // Assert
    assert!(app.show_drafts, "d did not show drafts");
    assert_shows(&with_drafts, &[DRAFT_TITLE]);
    assert_snapshot("needs_me_drafts_shown", &with_drafts);

    // Act
    app.toggle_drafts();
    let without_drafts = render_needs_me(&app);

    // Assert
    assert!(!app.show_drafts, "a second d did not hide drafts");
    assert_hides(&without_drafts, &[DRAFT_TITLE]);
    assert_snapshot("needs_me_drafts_hidden", &without_drafts);
}

/// The characters the sanitizer is meant to keep off the screen.
fn is_hidden(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{2028}' | '\u{2029}'
            | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn text_at<'a>(json: &'a serde_json::Value, pointer: &str) -> &'a str {
    json.pointer(pointer)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("the fixture lacks text at {pointer}"))
}

#[test]
fn escape_fixture_renders_clean_in_every_list_column() {
    // Arrange: every column's source text carries escapes, bidi controls or C1 bytes
    let hidden_host = "gh\u{1b}[31m.ex\u{202e}ample";
    let json = fs::read_to_string(HIDDEN_FIXTURE).expect("read the hidden-characters fixture");
    let rows = merge_needs_me(
        map_page(hidden_host, ACCOUNT, &json)
            .expect("map the fixture")
            .rows,
        Vec::new(),
    );
    let fixture: serde_json::Value = serde_json::from_str(&json).expect("parse the fixture");
    let node = "/data/search/nodes/0";
    let sources = [
        hidden_host,
        text_at(&fixture, &format!("{node}/title")),
        text_at(&fixture, &format!("{node}/author/login")),
        text_at(&fixture, &format!("{node}/repository/nameWithOwner")),
        text_at(&fixture, &format!("{node}/reviewDecision")),
        text_at(
            &fixture,
            &format!("{node}/commits/nodes/0/commit/statusCheckRollup/state"),
        ),
    ];
    assert!(
        sources.iter().all(|text| text.chars().any(is_hidden)),
        "a source text carries no hidden characters, so the test proves nothing: {sources:?}"
    );
    let hidden_host_row = rows.first().expect("one fixture row");
    let mut other_host_row = hidden_host_row.clone();
    other_host_row.key.host = OTHER_HOST.into();
    other_host_row.key.number += 1;
    let rows = [rows.clone(), vec![other_host_row]].concat();
    let hidden_host_row = rows.first().expect("one fixture row");
    assert_eq!(
        hidden_host_row.key.host, hidden_host,
        "the host no longer reaches the renderer raw"
    );
    let mut app = App::new();
    let generation = app.begin_refresh();
    app.apply(Loaded {
        generation,
        partial: false,
        host: hidden_host.into(),
        account: ACCOUNT.into(),
        result: Ok(rows),
    });

    // Act
    let screen = render_needs_me_at(&app, WIDE_WIDTH);

    // Assert
    let leaked: Vec<char> = screen
        .chars()
        .filter(|c| *c != '\n' && is_hidden(*c))
        .collect();
    assert!(
        leaked.is_empty(),
        "hidden characters reached the screen {leaked:?}:\n{screen:?}"
    );
    assert_shows(
        &screen,
        &[
            "gh.example",
            "octo/widgets",
            "#77",
            "Visible title",
            "eviluser",
            "review",
            "UNKNOWN",
            "FAILED",
        ],
    );
}

#[test]
fn failed_host_shows_its_state_while_the_other_host_rows_stay() {
    // Arrange: one host loaded its rows, the other failed in the same refresh
    let mut app = App::new();
    let generation = app.begin_refresh();
    let rows = merge_needs_me(fixture_rows(REVIEW_FIXTURE), fixture_rows(ASSIGNED_FIXTURE));
    app.apply(Loaded {
        generation,
        partial: false,
        host: HOST.into(),
        account: ACCOUNT.into(),
        result: Ok(rows),
    });

    // Act
    app.apply(Loaded {
        generation,
        partial: false,
        host: DOWN_HOST.into(),
        account: ACCOUNT.into(),
        result: Err(GithubError::Network),
    });
    let screen = render_needs_me(&app);

    // Assert
    assert_shows(&screen, &REVIEW_TITLES);
    assert_shows(&screen, &ASSIGNED_TITLES);
    assert_shows(&screen, &[DOWN_HOST, "could not reach GitHub"]);
    assert_snapshot("needs_me_partial_host", &screen);
}

#[test]
fn the_mine_view_ignores_the_tag_filter_and_shows_every_row() {
    // Arrange: a Review filter would hide the assigned rows on the Needs me list
    let mut app = needs_me_app();
    app.cycle_filter();
    assert_eq!(app.filter, ListFilter::Review);
    let mut terminal =
        Terminal::new(TestBackend::new(SCREEN_WIDTH, SCREEN_HEIGHT)).expect("test terminal");

    // Act
    terminal
        .draw(|frame| mine::render(frame, frame.area(), &app, &FixedClock))
        .expect("draw");
    let screen: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    // Assert
    assert_shows(&screen, &REVIEW_TITLES);
    assert_shows(&screen, &ASSIGNED_TITLES);
}

#[test]
fn a_cut_list_shows_a_partial_note() {
    // Arrange
    let mut app = needs_me_app();
    let generation = app.begin_refresh();
    app.apply(Loaded {
        generation,
        partial: true,
        host: HOST.into(),
        account: ACCOUNT.into(),
        result: Ok(merge_needs_me(
            fixture_rows(REVIEW_FIXTURE),
            fixture_rows(ASSIGNED_FIXTURE),
        )),
    });

    // Act
    let screen = render_needs_me(&app);

    // Assert
    assert_shows(&screen, &["Some rows are not shown"]);
}

#[test]
fn a_whole_list_shows_no_partial_note() {
    // Arrange
    let app = needs_me_app();

    // Act
    let screen = render_needs_me(&app);

    // Assert
    assert_hides(&screen, &["Some rows are not shown"]);
}

const LONG_REPO: &str = "amplifiedengineer/helios";
const TRIMMED_REPO: &str = "an-organisation-with-a-long-name/helios-tui";
const LONG_TITLE: &str = "Make the list columns give the spare width to the title column";
const NARROW_WIDTH: u16 = 80;
const MEDIUM_WIDTH: u16 = 110;
const WIDE_WIDTH: u16 = 160;
const OTHER_HOST: &str = "ghe.example.com";

fn wide_row(host: &str, repo: &str, number: u64) -> PrSummary {
    let mut row = fixture_rows(REVIEW_FIXTURE).remove(0);
    row.key.host = host.into();
    row.key.repo = repo.into();
    row.key.number = number;
    row.title = sanitize(LONG_TITLE);
    row
}

fn app_with(rows: Vec<PrSummary>) -> App {
    let mut app = App::new();
    let generation = app.begin_refresh();
    let host = rows
        .first()
        .map(|row| row.key.host.clone())
        .unwrap_or_default();
    app.apply(Loaded {
        generation,
        partial: false,
        host,
        account: ACCOUNT.into(),
        result: Ok(rows),
    });
    app
}

fn header_line(screen: &str) -> &str {
    screen.lines().next().expect("a header line")
}

#[test]
fn the_title_takes_the_spare_width_and_the_repo_stays_whole() {
    // Arrange
    let app = app_with(vec![wide_row(HOST, LONG_REPO, 7)]);
    let cases = [
        (NARROW_WIDTH, "list_width_80", true),
        (MEDIUM_WIDTH, "list_width_110", true),
        (WIDE_WIDTH, "list_width_160", true),
    ];

    for (width, snapshot, repo_whole) in cases {
        // Act
        let screen = render_needs_me_at(&app, width);

        // Assert
        assert_eq!(
            repo_whole,
            screen.contains(LONG_REPO),
            "at {width}:\n{screen}"
        );
        assert!(
            screen.contains(LONG_TITLE) || width < WIDE_WIDTH,
            "the title was cut at {width}:\n{screen}"
        );
        assert_snapshot(snapshot, &screen);
    }
}

#[test]
fn a_narrow_list_drops_the_low_value_columns_first() {
    // Arrange
    let app = app_with(vec![wide_row(HOST, LONG_REPO, 7)]);

    // Act
    let narrow = render_needs_me_at(&app, NARROW_WIDTH);
    let medium = render_needs_me_at(&app, MEDIUM_WIDTH);
    let wide = render_needs_me_at(&app, WIDE_WIDTH);

    // Assert
    for column in ["HOST", "DRAFT", "TAG"] {
        assert_hides(header_line(&medium), &[column]);
    }
    assert_shows(
        header_line(&medium),
        &["REPO", "TITLE", "REVIEW", "CHECKS", "UPDATED"],
    );
    assert_shows(header_line(&narrow), &["REPO", "TITLE", "UPDATED"]);
    assert_hides(header_line(&narrow), &["AUTHOR", "CHECKS"]);
    assert_shows(header_line(&wide), &["AUTHOR", "REVIEW", "CHECKS"]);
}

#[test]
fn the_host_column_shows_only_when_there_is_more_than_one_host() {
    // Arrange
    let one_host = app_with(vec![wide_row(HOST, LONG_REPO, 7)]);
    let two_hosts = app_with(vec![
        wide_row(HOST, LONG_REPO, 7),
        wide_row(OTHER_HOST, LONG_REPO, 8),
    ]);

    // Act
    let single = render_needs_me_at(&one_host, WIDE_WIDTH);
    let double = render_needs_me_at(&two_hosts, WIDE_WIDTH);

    // Assert
    assert_hides(header_line(&single), &["HOST"]);
    assert_shows(header_line(&double), &["HOST"]);
    assert_shows(&double, &[HOST, OTHER_HOST]);
}

#[test]
fn a_repo_that_is_too_long_loses_its_start_and_keeps_the_name() {
    // Arrange
    let app = app_with(vec![wide_row(HOST, TRIMMED_REPO, 7)]);

    // Act
    let screen = render_needs_me_at(&app, MEDIUM_WIDTH);

    // Assert
    assert_shows(&screen, &["\u{2026}", "/helios-tui"]);
    assert_hides(&screen, &["an-organisation"]);
}

/// How many leading characters of the long title the screen shows.
fn title_chars_shown(screen: &str) -> usize {
    LONG_TITLE
        .char_indices()
        .map(|(end, c)| end + c.len_utf8())
        .take_while(|end| screen.contains(&LONG_TITLE[..*end]))
        .count()
}

fn review_row(number: u64, decision: Option<ReviewDecision>) -> PrSummary {
    let mut row = wide_row(HOST, LONG_REPO, number);
    row.review_decision = decision;
    row
}

#[test]
fn a_review_column_that_is_blank_on_every_row_is_hidden_and_the_title_gets_its_width() {
    // Arrange
    let blank = app_with(vec![review_row(7, None), review_row(8, None)]);
    let reviewed = app_with(vec![
        review_row(7, None),
        review_row(8, Some(ReviewDecision::Approved)),
    ]);

    // Act
    let without = render_needs_me_at(&blank, MEDIUM_WIDTH);
    let with = render_needs_me_at(&reviewed, MEDIUM_WIDTH);

    // Assert
    assert_hides(header_line(&without), &["REVIEW"]);
    assert_shows(header_line(&with), &["REVIEW"]);
    assert!(
        title_chars_shown(&without) > title_chars_shown(&with),
        "the title got no extra room:\n{without}\n{with}"
    );
}

#[test]
fn one_non_blank_review_keeps_the_column() {
    // Arrange
    let app = app_with(vec![
        review_row(7, None),
        review_row(8, None),
        review_row(9, Some(ReviewDecision::ChangesRequested)),
    ]);

    // Act
    let screen = render_needs_me_at(&app, WIDE_WIDTH);

    // Assert
    assert_shows(header_line(&screen), &["REVIEW"]);
    assert_shows(&screen, &["changes"]);
}
