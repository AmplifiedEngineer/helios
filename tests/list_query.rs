// SPDX-License-Identifier: Apache-2.0

use forgebird::github::queries::{
    collect, filter_drafts, map_page, merge_needs_me, query_variables, ListError, ListKind,
    SearchPage, PAGE_SIZE, SEARCH_CAP,
};
use forgebird::model::{CheckState, PrSummary, ReviewDecision, Tag};
use std::fs;
use std::path::Path;

const HOST: &str = "github.com";
const ACCOUNT: &str = "octocat";

fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/list")
            .join(name),
    )
    .expect("read fixture")
}

fn rows(name: &str) -> Vec<PrSummary> {
    map_page(HOST, ACCOUNT, &fixture(name))
        .expect("map fixture")
        .rows
}

fn find(rows: &[PrSummary], number: u64) -> &PrSummary {
    rows.iter()
        .find(|row| row.key.number == number)
        .unwrap_or_else(|| panic!("no row for PR {number}"))
}

fn needs_me() -> Vec<PrSummary> {
    merge_needs_me(
        rows("synthetic-review.json"),
        rows("synthetic-assigned.json"),
    )
}

#[test]
fn a_pr_matching_both_searches_appears_once_tagged_review() {
    // Arrange
    let review = rows("synthetic-review.json");
    let assigned = rows("synthetic-assigned.json");

    // Act
    let merged = merge_needs_me(review, assigned);

    // Assert
    let shared: Vec<_> = merged.iter().filter(|row| row.key.number == 10).collect();
    assert_eq!(shared.len(), 1);
    assert_eq!(shared[0].tags, vec![Tag::Review]);
}

#[test]
fn a_pr_only_in_assigned_is_tagged_assigned() {
    // Arrange
    let review = rows("synthetic-review.json");
    let assigned = rows("synthetic-assigned.json");

    // Act
    let merged = merge_needs_me(review, assigned);

    // Assert
    assert_eq!(find(&merged, 20).tags, vec![Tag::Assigned]);
    assert_eq!(find(&merged, 11).tags, vec![Tag::Review]);
    assert_eq!(merged.len(), 6);
}

#[test]
fn the_search_strings_match_each_list() {
    // Arrange
    let expected = [
        (ListKind::Mine, "is:pr is:open author:@me"),
        (ListKind::Review, "is:pr is:open review-requested:@me"),
        (ListKind::Assigned, "is:pr is:open assignee:@me -author:@me"),
    ];

    for (kind, search) in expected {
        // Act
        let actual = kind.search();

        // Assert
        assert_eq!(actual, search);
    }
}

#[test]
fn a_draft_is_hidden_by_default_and_shown_when_asked() {
    // Arrange
    let merged = needs_me();

    // Act
    let hidden = filter_drafts(merged.clone(), false);
    let shown = filter_drafts(merged, true);

    // Assert
    assert!(hidden.iter().all(|row| !row.is_draft));
    assert!(hidden.iter().all(|row| row.key.number != 21));
    assert_eq!(hidden.len(), 5);
    assert!(find(&shown, 21).is_draft);
    assert_eq!(shown.len(), 6);
}

/// Unknown means `None` on `PrSummary::check_state`: GitHub reported no rollup,
/// as opposed to `Some(CheckState::Unknown(_))`, a state forgebird cannot name.
#[test]
fn a_missing_status_check_rollup_gives_unknown_check_state() {
    // Arrange
    let assigned = rows("synthetic-assigned.json");

    // Act
    let row = find(&assigned, 22);

    // Assert
    assert_eq!(row.check_state, None);
    assert_eq!(find(&assigned, 20).check_state, Some(CheckState::Success));
}

#[test]
fn null_author_and_null_review_decision_map() {
    // Arrange
    let assigned = rows("synthetic-assigned.json");

    // Act
    let row = find(&assigned, 23);

    // Assert
    assert_eq!(row.author.as_str(), "ghost");
    assert_eq!(row.review_decision, None);
}

#[test]
fn the_recorded_public_fixture_maps() {
    // Arrange
    let json = fixture("recorded-public-mine.json");

    // Act
    let page = map_page(HOST, ACCOUNT, &json).expect("map recorded page");

    // Assert
    assert_eq!(page.issue_count, 59);
    assert!(page.has_next_page);
    assert_eq!(page.end_cursor.as_deref(), Some("Y3Vyc29yOjM="));
    assert_eq!(page.rows.len(), 3);
    let first = &page.rows[0];
    assert_eq!(first.key.host, HOST);
    assert_eq!(first.key.account, ACCOUNT);
    assert_eq!(first.key.repo, "cli/cli");
    assert_eq!(first.key.number, 14964);
    assert_eq!(first.author.as_str(), "dependabot");
    assert_eq!(first.review_decision, Some(ReviewDecision::ReviewRequired));
    assert_eq!(first.check_state, Some(CheckState::Success));
    assert_eq!(first.updated_at.to_rfc3339(), "2026-11-07T23:18:39+00:00");
    assert!(first.tags.is_empty());
    assert!(page.rows[2].is_draft);
}

#[test]
fn mapped_text_is_sanitized() {
    // Arrange
    let json = fixture("synthetic-escapes.json");

    // Act
    let page = map_page(HOST, ACCOUNT, &json).expect("map escapes");

    // Assert
    let row = &page.rows[0];
    assert_eq!(row.title.as_str(), "Plain red title");
    assert_eq!(row.author.as_str(), "eviluser");
    assert!(!row.key.repo.contains('\u{1b}'));
}

/// Builds the response for result window `start..start + PAGE_SIZE` of `total` results.
fn generated_page(start: usize, total: usize) -> String {
    let end = (start + PAGE_SIZE).min(total);
    let nodes: Vec<String> = (start..end)
        .map(|n| {
            format!(
                r#"{{"number":{n},"title":"PR {n}","isDraft":false,"updatedAt":"2026-11-05T10:00:00Z","author":{{"login":"octocat"}},"repository":{{"nameWithOwner":"octo/widgets"}},"reviewDecision":null,"commits":{{"nodes":[]}}}}"#
            )
        })
        .collect();
    // The last page still claims a next one, as GitHub may at the cap.
    format!(
        r#"{{"data":{{"search":{{"issueCount":{total},"pageInfo":{{"hasNextPage":true,"endCursor":"c{end}"}},"nodes":[{}]}},"rateLimit":{{"remaining":1,"resetAt":"2026-11-05T12:00:00Z","cost":1}}}}}}"#,
        nodes.join(",")
    )
}

fn cursor_start(cursor: Option<&str>) -> usize {
    cursor.map_or(0, |c| c[1..].parse().expect("generated cursor"))
}

#[test]
fn exactly_1000_results_marks_the_list_partial_and_stops_paging() {
    // Arrange
    let mut calls = 0;
    let fetch = |cursor: Option<&str>| -> Result<SearchPage, ListError> {
        calls += 1;
        map_page(
            HOST,
            ACCOUNT,
            &generated_page(cursor_start(cursor), SEARCH_CAP),
        )
    };

    // Act
    let listing = collect(fetch).expect("collect");

    // Assert
    assert_eq!(listing.rows.len(), SEARCH_CAP);
    assert!(listing.partial);
    assert_eq!(calls, SEARCH_CAP / PAGE_SIZE);
}

#[test]
fn fewer_than_1000_results_is_not_partial() {
    // Arrange
    let total = 250;
    let fetch = |cursor: Option<&str>| -> Result<SearchPage, ListError> {
        let start = cursor_start(cursor);
        let mut page = map_page(HOST, ACCOUNT, &generated_page(start, total))?;
        page.has_next_page = start + PAGE_SIZE < total;
        Ok(page)
    };

    // Act
    let listing = collect(fetch).expect("collect");

    // Assert
    assert_eq!(listing.rows.len(), total);
    assert!(!listing.partial);
}

#[test]
fn a_bad_response_gives_a_parse_error_that_never_echoes_input() {
    // Arrange
    let bad = [
        r#"not json hunter2"#,
        r#"{"data":{"search":"hunter2"}}"#,
        r#"{"data":{"search":{"issueCount":0,"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[{"number":"hunter2"}]}}}"#,
    ];

    for json in bad {
        // Act
        let error = map_page(HOST, ACCOUNT, json).expect_err("bad response");

        // Assert
        assert!(matches!(error, ListError::Parse { .. }));
        assert!(!error.to_string().contains("hunter2"));
        assert!(!format!("{error:?}").contains("hunter2"));
    }
}

#[test]
fn query_variables_carry_search_page_size_and_cursor() {
    // Arrange
    let kind = ListKind::Review;

    // Act
    let first = query_variables(kind, None);
    let next = query_variables(kind, Some("c100"));

    // Assert
    assert_eq!(first["q"], "is:pr is:open review-requested:@me");
    assert_eq!(first["first"], PAGE_SIZE);
    assert!(first["after"].is_null());
    assert_eq!(next["after"], "c100");
}
