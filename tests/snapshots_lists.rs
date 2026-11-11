// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
mod support;

use chrono::{TimeZone, Utc};
use helios::app::refresh::Clock;
use helios::app::worker::Loaded;
use helios::app::{App, ListFilter};
use helios::github::errors::GithubError;
use helios::github::queries::{map_page, merge_needs_me};
use helios::model::PrSummary;
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
        host: HOST.into(),
        account: ACCOUNT.into(),
        result: Ok(rows),
    });
    assert!(applied, "the fixture rows were not applied");
    app
}

fn render_needs_me(app: &App) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(SCREEN_WIDTH, SCREEN_HEIGHT)).expect("test terminal");
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
    assert_eq!(
        hidden_host_row.key.host, hidden_host,
        "the host no longer reaches the renderer raw"
    );
    let mut app = App::new();
    let generation = app.begin_refresh();
    app.apply(Loaded {
        generation,
        host: hidden_host.into(),
        account: ACCOUNT.into(),
        result: Ok(rows),
    });

    // Act
    let screen = render_needs_me(&app);

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
        host: HOST.into(),
        account: ACCOUNT.into(),
        result: Ok(rows),
    });

    // Act
    app.apply(Loaded {
        generation,
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
