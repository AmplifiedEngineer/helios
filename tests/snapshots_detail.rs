// SPDX-License-Identifier: Apache-2.0

use chrono::{TimeZone, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use helios::app::{App, DetailState, DetailTab, LoadRequest, Screen};
use helios::github::errors::GithubError;
use helios::github::octo::MAX_FILES;
use helios::github::queries::{map_detail, map_files};
use helios::model::PrKey;
use helios::tui::cache::RenderCache;
use helios::tui::detail;
use helios::tui::draw::draw_cached;
use helios::tui::keys::{handle_key, BINDINGS};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use std::fs;

#[allow(dead_code)]
mod support;
use support::clock::FixedClock;
use support::render::{buffer_text, line_with};
use support::snapshot::assert_snapshot;

const HOST: &str = "github.com";
const ACCOUNT: &str = "octocat";
const REPO: &str = "cli/cli";
const PR_NUMBER: u64 = 1;
const FILES_FIXTURE: &str = "tests/fixtures/detail/files-synthetic.json";
const RECORDED_FIXTURE: &str = "tests/fixtures/detail/recorded-public-pr.json";
const SCREEN_WIDTH: u16 = 120;
const SCREEN_HEIGHT: u16 = 60;

const COMMENTS_HEADING: &str = "Issue comments";
const THREADS_HEADING: &str = "Review threads";
const ISSUE_AUTHOR: &str = "Ufktarimarge";
const ISSUE_BODY: &str = "Bana para laz";
const THREAD_PATH: &str = "repo-delete-private-forks.txtar";
const FIRST_REPLY_AUTHOR: &str = "copilot-pull-request-reviewer";
const SECOND_REPLY_AUTHOR: &str = "BagToad";
const SECOND_REPLY_BODY: &str = "gonna leave it";

fn open_detail(tab: DetailTab) -> App {
    let json = fs::read_to_string(RECORDED_FIXTURE).expect("read the detail fixture");
    let detail = map_detail(&json).expect("map the detail fixture");
    let key = PrKey {
        host: HOST.into(),
        account: ACCOUNT.into(),
        repo: REPO.into(),
        number: PR_NUMBER,
    };
    let mut app = App::new();
    app.select(key.clone());
    app.apply_detail(&key, Ok(detail));
    app.tab = tab;
    app
}

fn render_detail(app: &App) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(SCREEN_WIDTH, SCREEN_HEIGHT)).expect("test terminal");
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

fn position(screen: &str, needle: &str) -> usize {
    screen
        .find(needle)
        .unwrap_or_else(|| panic!("the screen lacks {needle:?}:\n{screen}"))
}

#[test]
fn conversation_keeps_issue_comments_and_review_threads_separate() {
    // Arrange
    let app = open_detail(DetailTab::Conversation);

    // Act
    let screen = render_detail(&app);

    // Assert: the issue comment sits under its own heading, before the threads
    let comments = position(&screen, COMMENTS_HEADING);
    let threads = position(&screen, THREADS_HEADING);
    assert!(
        comments < threads,
        "the sections are out of order:\n{screen}"
    );
    let issue_body = position(&screen, ISSUE_BODY);
    assert!(
        comments < issue_body && issue_body < threads,
        "the issue comment is outside its section:\n{screen}"
    );

    // Assert: the thread path and both replies sit under the threads heading
    for needle in [
        THREAD_PATH,
        FIRST_REPLY_AUTHOR,
        SECOND_REPLY_AUTHOR,
        SECOND_REPLY_BODY,
    ] {
        assert!(
            position(&screen, needle) > threads,
            "{needle:?} is outside the threads section:\n{screen}"
        );
    }

    // Assert: ages come from the fake clock and the fixture timestamps
    assert!(line_with(&screen, ISSUE_AUTHOR).contains("2h"), "{screen}");
    assert!(
        line_with(&screen, FIRST_REPLY_AUTHOR).contains("10d"),
        "{screen}"
    );
    assert!(
        line_with(&screen, SECOND_REPLY_AUTHOR).contains("9d"),
        "{screen}"
    );
}

const ALL_TABS: [DetailTab; 5] = [
    DetailTab::Overview,
    DetailTab::Conversation,
    DetailTab::Commits,
    DetailTab::Files,
    DetailTab::Checks,
];

fn key() -> PrKey {
    PrKey {
        host: HOST.into(),
        account: ACCOUNT.into(),
        repo: REPO.into(),
        number: PR_NUMBER,
    }
}

fn assert_has_all(screen: &str, needles: &[&str]) {
    for needle in needles {
        assert!(
            screen.contains(needle),
            "the screen lacks {needle:?}:\n{screen}"
        );
    }
}

#[test]
fn overview_tab_shows_title_author_state_review_and_body() {
    // Arrange
    let app = open_detail(DetailTab::Overview);

    // Act
    let screen = render_detail(&app);

    // Assert
    assert_has_all(
        &screen,
        &[
            "Require confirmation before `gh repo delete` deletes private forks",
            "BagToad",
            "Merged",
            "Approved",
        ],
    );
    assert_snapshot("detail_overview", &screen);
}

#[test]
fn commits_tab_lists_the_commits_in_order() {
    // Arrange
    let app = open_detail(DetailTab::Commits);

    // Act
    let screen = render_detail(&app);

    // Assert
    let first = position(
        &screen,
        "Require confirmation before deleting a private repo's forks",
    );
    let second = position(
        &screen,
        "Accept cascading fork deletion in acceptance cleanup",
    );
    let third = position(
        &screen,
        "Add acceptance test for deleting a private repo with forks",
    );
    assert!(first < second && second < third, "{screen}");
    assert_has_all(&screen, &["7bfdc1b", "4948470", "b17c155", "BagToad"]);
    assert_snapshot("detail_commits", &screen);
}

#[test]
fn files_tab_marks_truncated_binary_and_renamed_files() {
    // Arrange
    let mut app = open_detail(DetailTab::Files);
    let json = fs::read_to_string(FILES_FIXTURE).expect("read the files fixture");
    app.apply_files(&key(), map_files(&json).expect("map the files fixture"));

    // Act
    let screen = render_detail(&app);

    // Assert
    assert!(
        line_with(&screen, "data/huge.json").contains("truncated"),
        "{screen}"
    );
    assert!(
        line_with(&screen, "assets/logo.png").contains("binary or mode change"),
        "{screen}"
    );
    let renamed = line_with(&screen, "src/renamed.rs");
    assert!(
        renamed.contains("renamed") && renamed.contains("src/original.rs"),
        "{screen}"
    );
    assert!(
        line_with(&screen, "src/moved.rs").contains("renamed"),
        "{screen}"
    );
    assert!(
        !line_with(&screen, "src/lib.rs").contains("truncated"),
        "{screen}"
    );
    assert_snapshot("detail_files", &screen);
}

#[test]
fn checks_tab_shows_each_check_run_with_its_state() {
    // Arrange
    let app = open_detail(DetailTab::Checks);

    // Act
    let screen = render_detail(&app);

    // Assert
    assert!(
        line_with(&screen, "label-external")
            .to_lowercase()
            .contains("skipped"),
        "{screen}"
    );
    assert!(
        line_with(&screen, "lint")
            .to_lowercase()
            .contains("success"),
        "{screen}"
    );
    assert_snapshot("detail_checks", &screen);
}

#[test]
fn loading_state_renders_a_loading_notice() {
    // Arrange
    let mut app = App::new();
    app.select(key());

    // Act
    let screen = render_detail(&app);

    // Assert
    assert_has_all(&screen, &["Loading"]);
    assert_snapshot("detail_loading", &screen);
}

#[test]
fn not_found_or_hidden_renders_a_fixed_message_inline() {
    // Arrange
    let mut app = App::new();
    app.select(key());
    app.apply_detail(&key(), Err(GithubError::NotFoundOrHidden));

    // Act
    let screen = render_detail(&app);

    // Assert
    assert_has_all(&screen, &["not found or hidden"]);
    assert!(
        !screen.contains("NotFoundOrHidden"),
        "raw error text:\n{screen}"
    );
    assert_snapshot("detail_hidden", &screen);
}

fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    handle_key(app, KeyEvent::new(code, modifiers));
}

#[test]
fn tab_cycles_forward_through_the_five_tabs_and_wraps() {
    // Arrange
    let mut app = open_detail(DetailTab::Overview);
    app.screen = Screen::Detail;
    let mut seen = vec![app.tab];

    // Act
    for _ in 0..ALL_TABS.len() {
        press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        seen.push(app.tab);
    }

    // Assert
    let mut expected = ALL_TABS.to_vec();
    expected.push(DetailTab::Overview);
    assert_eq!(seen, expected);
}

#[test]
fn shift_tab_cycles_backward_through_the_five_tabs_and_wraps() {
    // Arrange
    let mut app = open_detail(DetailTab::Overview);
    app.screen = Screen::Detail;
    let mut seen = vec![app.tab];

    // Act
    for _ in 0..ALL_TABS.len() {
        press(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
        seen.push(app.tab);
    }

    // Assert
    let mut expected: Vec<DetailTab> = ALL_TABS.iter().rev().copied().collect();
    expected.rotate_right(1);
    expected.push(DetailTab::Overview);
    assert_eq!(seen, expected);
}

#[test]
fn the_key_table_lists_the_tab_keys() {
    // Arrange
    let wanted = [("Tab", "next tab"), ("Shift-Tab", "previous tab")];

    // Act
    let listed: Vec<(&str, &str)> = BINDINGS.iter().map(|b| (b.key, b.action)).collect();

    // Assert
    for pair in wanted {
        assert!(
            listed.contains(&pair),
            "BINDINGS lacks {pair:?}: {listed:?}"
        );
    }
}

#[test]
fn opening_a_pull_request_shows_loading_and_requests_one_detail_load() {
    // Arrange
    let mut app = open_detail(DetailTab::Overview);
    let other = PrKey {
        number: PR_NUMBER + 1,
        ..key()
    };

    // Act
    app.open(other.clone());

    // Assert
    assert!(matches!(app.detail(), DetailState::Loading));
    assert_eq!(app.selected, Some(other.clone()));
    assert_eq!(app.requests(), [LoadRequest::Detail(other)]);
}

const CUT_TOTAL: u64 = 240;

/// The recorded detail opened in an app, with `totalCount` set at the JSON pointer `pointer`.
fn app_with_total(pointer: &str) -> App {
    let json = fs::read_to_string(RECORDED_FIXTURE).expect("read the detail fixture");
    let mut value: serde_json::Value = serde_json::from_str(&json).expect("fixture json");
    value["data"]["repository"]["pullRequest"]
        .pointer_mut(pointer)
        .expect("the connection is in the fixture")["totalCount"] = CUT_TOTAL.into();
    let detail = map_detail(&value.to_string()).expect("map the detail");
    let key = PrKey {
        host: HOST.into(),
        account: ACCOUNT.into(),
        repo: REPO.into(),
        number: PR_NUMBER,
    };
    let mut app = App::new();
    app.select(key.clone());
    app.apply_detail(&key, Ok(detail));
    app
}

#[test]
fn a_cut_list_says_how_many_are_shown_of_the_total_on_its_tab() {
    // Arrange
    let contexts = "/checkRuns/nodes/0/commit/statusCheckRollup/contexts";
    let cases = [
        (
            "/comments",
            DetailTab::Conversation,
            "showing 1 of 240 comments",
        ),
        (
            "/reviewThreads",
            DetailTab::Conversation,
            "showing 1 of 240 review threads",
        ),
        (
            "/reviewThreads/nodes/0/comments",
            DetailTab::Conversation,
            "showing 2 of 240 comments in this thread",
        ),
        ("/commits", DetailTab::Commits, "showing 3 of 240 commits"),
        (contexts, DetailTab::Checks, "showing 25 of 240 checks"),
    ];

    for (pointer, tab, note) in cases {
        let mut app = app_with_total(pointer);

        // Act
        app.tab = tab;
        let screen = render_detail(&app);

        // Assert
        assert!(screen.contains(note), "{note}:\n{screen}");
    }
}

#[test]
fn a_whole_detail_shows_no_count_note_on_any_tab() {
    // Arrange
    let json = fs::read_to_string(RECORDED_FIXTURE).expect("read the detail fixture");
    let detail = map_detail(&json).expect("map the detail");
    let key = PrKey {
        host: HOST.into(),
        account: ACCOUNT.into(),
        repo: REPO.into(),
        number: PR_NUMBER,
    };
    let mut app = App::new();
    app.select(key.clone());
    app.apply_detail(&key, Ok(detail));

    for tab in [
        DetailTab::Overview,
        DetailTab::Conversation,
        DetailTab::Commits,
        DetailTab::Checks,
    ] {
        // Act
        app.tab = tab;
        let screen = render_detail(&app);

        // Assert
        assert!(!screen.contains("showing "), "{tab:?}:\n{screen}");
    }
}

/// A pull request with `count` invented commits, open on the Commits tab.
fn app_with_commits(count: usize) -> App {
    let json = fs::read_to_string(RECORDED_FIXTURE).expect("read the detail fixture");
    let mut detail = map_detail(&json).expect("map the detail fixture");
    detail.commits = (0..count)
        .map(|index| helios::model::CommitSummary {
            oid: helios::sanitize::sanitize(&format!("{index:07}abcdef")),
            headline: helios::sanitize::sanitize(&format!("commit number {index}")),
            author: helios::sanitize::sanitize("octocat"),
            date: Utc
                .with_ymd_and_hms(2026, 11, 7, 18, 0, 0)
                .single()
                .expect("a valid instant"),
        })
        .collect();
    let mut app = App::new();
    app.select(key());
    app.apply_detail(&key(), Ok(detail));
    app.tab = DetailTab::Commits;
    app.screen = Screen::Detail;
    app
}

#[test]
fn commits_past_the_screen_are_reachable_by_scrolling() {
    // Arrange: more commits than rows, so the tail is below the fold
    let mut app = app_with_commits(150);
    let first = render_detail(&app);
    assert!(first.contains("commit number 0 "), "{first}");
    assert!(!first.contains("commit number 149 "), "{first}");

    // Act: jump to the end, as a reviewer would with G
    press(&mut app, KeyCode::Char('G'), KeyModifiers::NONE);
    let last = render_detail(&app);

    // Assert: the last commit shows and the first has scrolled away
    assert!(
        last.contains("commit number 149 "),
        "tail unreachable:\n{last}"
    );
    assert!(!last.contains("commit number 0 "), "{last}");

    // Act: one line back up, then back to the top
    press(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);
    assert!(render_detail(&app).contains("commit number 148 "));
    press(&mut app, KeyCode::Char('g'), KeyModifiers::NONE);
    assert!(render_detail(&app).contains("commit number 0 "));
}

#[test]
fn leaving_a_tab_resets_its_scroll() {
    // Arrange: scrolled to the end of a long commit list
    let mut app = app_with_commits(150);
    press(&mut app, KeyCode::Char('G'), KeyModifiers::NONE);

    // Act: move to the next tab and back
    press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    press(&mut app, KeyCode::BackTab, KeyModifiers::NONE);

    // Assert: the commits start at the top again
    assert!(render_detail(&app).contains("commit number 0 "));
}

#[test]
fn a_selected_file_with_a_long_wrapped_path_stays_on_screen() {
    // Arrange: ten files whose paths each wrap onto two rows of a 40 by 10 area
    let nodes: Vec<serde_json::Value> = (0..10)
        .map(|index| {
            serde_json::json!({
                "filename": format!("dir/{index}/{}", "x".repeat(50)),
                "previous_filename": null,
                "status": "modified",
                "changes": 1,
                "blob_url": "https://github.com/o/r/blob/a/f",
                "patch": "@@ -1 +1 @@\n-a\n+b"
            })
        })
        .collect();
    let files = map_files(&serde_json::Value::Array(nodes).to_string()).expect("map files");
    let mut app = open_detail(DetailTab::Files);
    app.apply_files(&key(), files);
    app.screen = Screen::Detail;
    for _ in 0..9 {
        press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    }

    // Act
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).expect("test terminal");
    terminal
        .draw(|frame| {
            detail::render(
                frame,
                frame.area(),
                &app,
                &FixedClock::at(2026, 11, 7, 20, 30, 46),
            )
        })
        .expect("draw");
    let screen = buffer_text(terminal.backend().buffer());

    // Assert: the last file, selected, is on screen
    assert!(
        screen.contains("dir/9/"),
        "selected file off screen:\n{screen}"
    );
}

#[test]
fn files_tab_notes_when_the_list_is_cut() {
    // Arrange
    let mut app = open_detail(DetailTab::Files);
    let json = fs::read_to_string(FILES_FIXTURE).expect("read the files fixture");
    app.apply_files(&key(), map_files(&json).expect("map the files fixture"));
    app.mark_files_truncated(&key());

    // Act
    let screen = render_detail(&app);

    // Assert
    let note = format!("Showing the first {MAX_FILES} files");
    assert!(screen.contains(&note), "{screen}");
    assert!(position(&screen, &note) < position(&screen, "data/huge.json"));
}

#[test]
fn files_tab_has_no_note_when_the_list_is_whole() {
    // Arrange
    let mut app = open_detail(DetailTab::Files);
    let json = fs::read_to_string(FILES_FIXTURE).expect("read the files fixture");
    app.apply_files(&key(), map_files(&json).expect("map the files fixture"));

    // Act
    let screen = render_detail(&app);

    // Assert
    assert!(!screen.contains("Showing the first"), "{screen}");
}

const DEPENDABOT_FIXTURE: &str = "tests/fixtures/markdown/dependabot.md";
const QUOTED_AND_FENCED_COMMENT: &str = "Looks good.\n\n> quoted\n\n```rust\nlet x = 1;\n```\n";

fn open_detail_with(tab: DetailTab, edit: impl FnOnce(&mut serde_json::Value)) -> App {
    let json = fs::read_to_string(RECORDED_FIXTURE).expect("read the detail fixture");
    let mut value: serde_json::Value = serde_json::from_str(&json).expect("parse the fixture");
    edit(&mut value["data"]["repository"]["pullRequest"]);
    let detail = map_detail(&value.to_string()).expect("map the edited fixture");
    let mut app = App::new();
    app.select(key());
    app.apply_detail(&key(), Ok(detail));
    app.tab = tab;
    app
}

fn render_cached_detail(app: &App, cache: &RenderCache) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(SCREEN_WIDTH, SCREEN_HEIGHT)).expect("test terminal");
    terminal
        .draw(|frame| {
            detail::render_cached(
                frame,
                frame.area(),
                app,
                &FixedClock::at(2026, 11, 7, 20, 30, 46),
                cache,
            )
        })
        .expect("draw");
    buffer_text(terminal.backend().buffer())
}

fn draw_app(app: &App, cache: &RenderCache, width: u16) {
    let mut terminal = Terminal::new(TestBackend::new(width, SCREEN_HEIGHT)).expect("terminal");
    terminal
        .draw(|frame| draw_cached(frame, app, &FixedClock::at(2026, 11, 7, 20, 30, 46), cache))
        .expect("draw");
}

#[test]
fn the_overview_renders_a_dependabot_body_as_text() {
    // Arrange
    let body = fs::read_to_string(DEPENDABOT_FIXTURE).expect("read the dependabot body");
    let app = open_detail_with(DetailTab::Overview, |pull| pull["body"] = body.into());
    let cache = RenderCache::new();

    // Act
    let screen = render_cached_detail(&app, &cache);

    // Assert
    for raw in ["<details", "**", "]("] {
        assert!(!screen.contains(raw), "the screen shows {raw:?}:\n{screen}");
    }
    assert_snapshot("detail_dependabot", &screen);
}

#[test]
fn two_draws_parse_the_body_once() {
    // Arrange
    let mut app = open_detail(DetailTab::Overview);
    app.screen = Screen::Detail;
    let cache = RenderCache::new();

    // Act
    draw_app(&app, &cache, SCREEN_WIDTH);
    draw_app(&app, &cache, SCREEN_WIDTH);

    // Assert
    assert_eq!(cache.builds(), 1, "the second draw reuses the body lines");

    // Act
    draw_app(&app, &cache, SCREEN_WIDTH - 20);

    // Assert
    assert_eq!(cache.builds(), 2, "a new width renders the body again");
}

#[test]
fn a_comment_body_renders_markdown() {
    // Arrange
    let app = open_detail_with(DetailTab::Conversation, |pull| {
        pull["comments"]["nodes"][0]["body"] = QUOTED_AND_FENCED_COMMENT.into();
    });
    let cache = RenderCache::new();

    // Act
    let screen = render_cached_detail(&app, &cache);

    // Assert
    assert!(!screen.contains("```"), "a fence marker shows:\n{screen}");
    assert!(
        !screen
            .lines()
            .any(|line| line.trim_start().starts_with('>')),
        "quote syntax shows:\n{screen}"
    );
    assert!(line_with(&screen, "quoted").contains("| "), "{screen}");
    assert_snapshot("detail_conversation_markdown", &screen);
}
