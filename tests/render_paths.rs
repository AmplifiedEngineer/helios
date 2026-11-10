// SPDX-License-Identifier: Apache-2.0

use chrono::{TimeZone, Utc};
use helios::app::refresh::Clock;
use helios::app::worker::Loaded;
use helios::app::{App, DetailTab};
use helios::github::queries::{map_detail, map_files, map_page, merge_needs_me};
use helios::model::PrKey;
use helios::tui::{detail, diff, needs_me};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::{json, Value};
use std::time::SystemTime;

const ACCOUNT: &str = "octocat";
const PR_NUMBER: u64 = 5;
const SCREEN_WIDTH: u16 = 200;
const SCREEN_HEIGHT: u16 = 40;
const TIMESTAMP: &str = "2026-11-05T10:00:00Z";
const OID: &str = "1111111111111111111111111111111111111111";
const LEAKED_PAYLOAD: &str = "pwned";
const LEAKED_CSI_TAIL: &str = "[31m";

const HOST: &str = "host.test";
const REPO: &str = "octo/repo";
const TITLE: &str = "title";
const BODY: &str = "bodytext";
const AUTHOR: &str = "authorname";
const COMMENT_AUTHOR: &str = "talker";
const COMMENT_BODY: &str = "issuecomment";
const THREAD_PATH: &str = "src/threadpath.rs";
const REPLY_AUTHOR: &str = "replier";
const REPLY_BODY: &str = "replybody";
const COMMIT_HEADLINE: &str = "commitheadline";
const COMMIT_AUTHOR: &str = "committer";
const FILE_NAME: &str = "src/filename.rs";
const PREVIOUS_NAME: &str = "src/oldname.rs";
const CHECK_NAME: &str = "checkname";
const REMOVED_LINE: &str = "removedline";
const ADDED_LINE: &str = "addedline";

/// A clock stopped two hours after the fixtures' timestamps.
struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        Utc.with_ymd_and_hms(2026, 11, 5, 12, 0, 0)
            .single()
            .expect("a valid instant")
            .into()
    }
}

/// Wraps `visible` in a colour escape, an OSC title change, a bidi override, an isolate,
/// a C1 control, DEL, a line separator and a second bidi mark.
fn dirty(visible: &str) -> String {
    format!(
        "\u{1b}[31m{visible}\u{1b}[0m\u{1b}]0;{LEAKED_PAYLOAD}\u{7}\u{202e}\u{2066}\u{9b}2J\u{85}\u{7f}\u{2028}\u{200f}\u{61c}"
    )
}

fn is_hidden(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{61c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}'
            | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Every remote field of the fixture, by name, with its raw value.
fn raw_fields() -> Vec<(&'static str, String)> {
    [
        ("host", HOST),
        ("repo", REPO),
        ("title", TITLE),
        ("body", BODY),
        ("author", AUTHOR),
        ("issue comment author", COMMENT_AUTHOR),
        ("issue comment body", COMMENT_BODY),
        ("thread path", THREAD_PATH),
        ("reply author", REPLY_AUTHOR),
        ("reply body", REPLY_BODY),
        ("commit message", COMMIT_HEADLINE),
        ("commit author", COMMIT_AUTHOR),
        ("file name", FILE_NAME),
        ("renamed from path", PREVIOUS_NAME),
        ("check name", CHECK_NAME),
        ("removed diff line", REMOVED_LINE),
        ("added diff line", ADDED_LINE),
    ]
    .into_iter()
    .map(|(name, visible)| (name, dirty(visible)))
    .collect()
}

fn comment(author: &str, body: &str) -> Value {
    json!({ "author": { "login": dirty(author) }, "body": dirty(body), "createdAt": TIMESTAMP })
}

fn list_json() -> String {
    json!({ "data": { "search": {
        "issueCount": 1,
        "pageInfo": { "hasNextPage": false, "endCursor": null },
        "nodes": [{
            "number": PR_NUMBER,
            "title": dirty(TITLE),
            "isDraft": false,
            "updatedAt": TIMESTAMP,
            "author": { "login": dirty(AUTHOR) },
            "repository": { "nameWithOwner": dirty(REPO) },
            "reviewDecision": null,
            "commits": { "nodes": [] }
        }]
    }}})
    .to_string()
}

fn detail_json() -> String {
    json!({ "data": { "repository": { "pullRequest": {
        "title": dirty(TITLE),
        "body": dirty(BODY),
        "state": "OPEN",
        "author": { "login": dirty(AUTHOR) },
        "labels": { "nodes": [{ "name": dirty("label") }] },
        "reviewRequests": { "nodes": [] },
        "reviewDecision": null,
        "comments": { "nodes": [comment(COMMENT_AUTHOR, COMMENT_BODY)] },
        "reviewThreads": { "nodes": [{
            "isResolved": false,
            "path": dirty(THREAD_PATH),
            "line": 3,
            "comments": { "nodes": [comment(REPLY_AUTHOR, REPLY_BODY)] }
        }]},
        "commits": { "nodes": [{ "commit": {
            "oid": OID,
            "messageHeadline": dirty(COMMIT_HEADLINE),
            "committedDate": TIMESTAMP,
            "author": { "name": dirty(COMMIT_AUTHOR), "user": null }
        }}]},
        "checkRuns": { "nodes": [{ "commit": { "statusCheckRollup": { "contexts": { "nodes": [{
            "__typename": "CheckRun",
            "name": dirty(CHECK_NAME),
            "status": "COMPLETED",
            "conclusion": "SUCCESS",
            "detailsUrl": null
        }]}}}}]}
    }}}})
    .to_string()
}

fn files_json() -> String {
    let patch = format!(
        "@@ -1 +1 @@\n-{}\n+{}",
        dirty(REMOVED_LINE),
        dirty(ADDED_LINE)
    );
    json!([{
        "filename": dirty(FILE_NAME),
        "previous_filename": dirty(PREVIOUS_NAME),
        "status": "renamed",
        "changes": 2,
        "blob_url": "https://github.com/octo/repo/blob/abc/file.rs",
        "patch": patch
    }])
    .to_string()
}

fn pr_key() -> PrKey {
    PrKey {
        host: dirty(HOST),
        account: ACCOUNT.into(),
        repo: String::new(),
        number: PR_NUMBER,
    }
}

fn list_app() -> App {
    let page = map_page(&dirty(HOST), ACCOUNT, &list_json()).expect("map the list fixture");
    let mut app = App::new();
    let generation = app.begin_refresh();
    let applied = app.apply(Loaded {
        generation,
        host: dirty(HOST),
        account: ACCOUNT.into(),
        result: Ok(merge_needs_me(page.rows, Vec::new())),
    });
    assert!(applied, "the fixture rows were not applied");
    app
}

fn detail_app(tab: DetailTab) -> App {
    let key = pr_key();
    let mut app = App::new();
    app.select(key.clone());
    app.apply_detail(
        &key,
        Ok(map_detail(&detail_json()).expect("map the detail fixture")),
    );
    app.apply_files(
        &key,
        map_files(&files_json()).expect("map the files fixture"),
    );
    app.tab = tab;
    app
}

/// Draws one frame and returns every cell symbol, one string per row.
fn draw(render: impl FnOnce(&mut ratatui::Frame)) -> Vec<String> {
    let mut terminal =
        Terminal::new(TestBackend::new(SCREEN_WIDTH, SCREEN_HEIGHT)).expect("test terminal");
    terminal.draw(render).expect("draw");
    terminal
        .backend()
        .buffer()
        .content()
        .chunks(usize::from(SCREEN_WIDTH))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect()
}

fn assert_clean(screen_name: &str, rows: &[String], visible: &[&str]) {
    let screen = rows.join("\n");
    if let Some(found) = rows
        .iter()
        .flat_map(|row| row.chars())
        .find(|c| is_hidden(*c))
    {
        panic!("the {screen_name} screen holds {found:?}:\n{screen}");
    }
    for leaked in [LEAKED_PAYLOAD, LEAKED_CSI_TAIL] {
        assert!(
            !screen.contains(leaked),
            "the {screen_name} screen leaks {leaked:?}:\n{screen}"
        );
    }
    for text in visible {
        assert!(
            screen.contains(text),
            "the {screen_name} screen lost {text:?}:\n{screen}"
        );
    }
}

#[test]
fn escapes_in_every_remote_field_render_clean_on_every_screen() {
    // Arrange: every remote field of the fixture holds hidden characters before any render
    for (field, raw) in raw_fields() {
        assert!(
            raw.chars().any(is_hidden),
            "the raw {field} holds no hidden character"
        );
    }
    let list = list_app();
    let diff_file = map_files(&files_json())
        .expect("map the files fixture")
        .remove(0);
    let tabs: [(DetailTab, &[&str]); 5] = [
        (DetailTab::Overview, &[TITLE, BODY, AUTHOR]),
        (
            DetailTab::Conversation,
            &[
                COMMENT_AUTHOR,
                COMMENT_BODY,
                THREAD_PATH,
                REPLY_AUTHOR,
                REPLY_BODY,
            ],
        ),
        (DetailTab::Commits, &[COMMIT_HEADLINE, COMMIT_AUTHOR]),
        (DetailTab::Files, &[FILE_NAME, PREVIOUS_NAME]),
        (DetailTab::Checks, &[CHECK_NAME]),
    ];

    // Act: render the list, every detail tab and the diff
    let list_rows = draw(|frame| needs_me::render(frame, frame.area(), &list, &FixedClock));
    let tab_rows: Vec<(DetailTab, Vec<String>)> = tabs
        .iter()
        .map(|(tab, _)| {
            let app = detail_app(*tab);
            let rows = draw(|frame| detail::render(frame, frame.area(), &app, &FixedClock));
            (*tab, rows)
        })
        .collect();
    let diff_rows = draw(|frame| diff::render(frame, frame.area(), &diff_file));

    // Assert: no hidden character reaches any screen, and the visible text stays
    assert_clean("list", &list_rows, &[HOST, REPO, TITLE, AUTHOR]);
    for ((tab, visible), (_, rows)) in tabs.iter().zip(&tab_rows) {
        assert_clean(&format!("{tab:?} tab"), rows, visible);
    }
    assert_clean("diff", &diff_rows, &[FILE_NAME, REMOVED_LINE, ADDED_LINE]);
}
