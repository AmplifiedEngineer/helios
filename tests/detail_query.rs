// SPDX-License-Identifier: Apache-2.0

use helios::github::queries::{files_path, map_detail, map_files};
use helios::model::{
    ChangedFile, CheckRunState, DiffGap, FileDiff, PrDetail, PrState, ReviewDecision, Shown,
};
use std::fs;
use std::path::Path;

fn files() -> Vec<ChangedFile> {
    let json = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/detail/files-synthetic.json"),
    )
    .expect("read fixture");
    map_files(&json).expect("map files")
}

fn find<'a>(files: &'a [ChangedFile], path: &str) -> &'a ChangedFile {
    files
        .iter()
        .find(|file| file.path.as_str() == path)
        .unwrap_or_else(|| panic!("no file {path}"))
}

#[test]
fn truncated_patch_is_flagged_unavailable_with_its_web_url() {
    // Arrange
    let mapped = files();

    // Act
    let file = find(&mapped, "data/huge.json");

    // Assert
    match &file.diff {
        FileDiff::Unavailable { gap, web_url } => {
            assert_eq!(*gap, DiffGap::Truncated);
            assert_eq!(
                web_url.as_str(),
                "https://github.com/octo/widgets/blob/abc123/data/huge.json"
            );
        }
        other => panic!("expected an unavailable diff, got {other:?}"),
    }
}

#[test]
fn a_modified_file_keeps_its_patch_text() {
    // Arrange
    let mapped = files();

    // Act
    let file = find(&mapped, "src/lib.rs");

    // Assert
    match &file.diff {
        FileDiff::Patch(patch) => {
            assert_eq!(
                patch.as_str(),
                "@@ -1,2 +1,3 @@\n fn main() {}\n-old\n+new\n+newer"
            );
        }
        other => panic!("expected a patch, got {other:?}"),
    }
    assert_eq!(file.previous_path, None);
}

#[test]
fn a_modified_file_with_no_patch_and_no_changes_is_binary_or_mode_only() {
    // Arrange
    let mapped = files();

    // Act
    let file = find(&mapped, "assets/logo.png");

    // Assert
    match &file.diff {
        FileDiff::Unavailable { gap, web_url } => {
            assert_eq!(*gap, DiffGap::ChangedNoText);
            assert_eq!(
                web_url.as_str(),
                "https://github.com/octo/widgets/blob/abc123/assets/logo.png"
            );
        }
        other => panic!("expected an unavailable diff, got {other:?}"),
    }
}

#[test]
fn a_renamed_file_carries_its_previous_path() {
    // Arrange
    let mapped = files();

    // Act
    let file = find(&mapped, "src/renamed.rs");

    // Assert
    assert_eq!(
        file.previous_path.as_ref().map(|path| path.as_str()),
        Some("src/original.rs")
    );
}

#[test]
fn path_previous_path_and_patch_are_sanitized() {
    // Arrange
    let mapped = files();

    // Act
    let file = find(&mapped, "src/red.rs");

    // Assert
    assert_eq!(
        file.previous_path.as_ref().map(|path| path.as_str()),
        Some("src/before.rs")
    );
    match &file.diff {
        FileDiff::Patch(patch) => assert_eq!(patch.as_str(), "@@ -1 +1 @@\n-a\n+b"),
        other => panic!("expected a patch, got {other:?}"),
    }
}

// A file with no patch and no changes is told apart by status only: the REST entry has no binary
// flag, so the gap states what the status proves.
#[test]
fn pure_rename_without_patch_is_renamed_only() {
    // Arrange
    let mapped = files();

    // Act
    let file = find(&mapped, "src/moved.rs");

    // Assert
    assert_eq!(
        file.previous_path.as_ref().map(|path| path.as_str()),
        Some("src/old_home.rs")
    );
    match &file.diff {
        FileDiff::Unavailable { gap, web_url } => {
            assert_eq!(*gap, DiffGap::RenamedOnly);
            assert_eq!(
                web_url.as_str(),
                "https://github.com/octo/widgets/blob/abc123/src/moved.rs"
            );
        }
        other => panic!("expected an unavailable diff, got {other:?}"),
    }
}

#[test]
fn added_file_with_no_patch_is_empty_or_binary() {
    // Arrange
    let mapped = files();

    // Act
    let file = find(&mapped, "src/empty.rs");

    // Assert
    match &file.diff {
        FileDiff::Unavailable { gap, .. } => assert_eq!(*gap, DiffGap::AddedOrRemovedNoText),
        other => panic!("expected an unavailable diff, got {other:?}"),
    }
}

fn detail(name: &str) -> PrDetail {
    let json = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/detail")
            .join(name),
    )
    .expect("read fixture");
    map_detail(&json).expect("map detail")
}

fn recorded() -> PrDetail {
    detail("recorded-public-pr.json")
}

fn escapes() -> PrDetail {
    detail("synthetic-escapes.json")
}

#[test]
fn files_path_is_relative_with_page_size_and_page() {
    // Arrange
    let (owner, repo, number, page) = ("octo", "widgets", 42, 3);

    // Act
    let path = files_path(owner, repo, number, page);

    // Assert
    assert_eq!(
        path,
        "/repos/octo/widgets/pulls/42/files?per_page=100&page=3"
    );
}

#[test]
fn overview_maps_title_body_state_author_and_decision() {
    // Arrange
    let json = recorded();

    // Act
    let overview = (
        json.title.as_str(),
        json.body
            .as_str()
            .starts_with("<!--\nThank you for contributing"),
        json.state,
        json.author.as_str(),
        json.review_decision,
    );

    // Assert
    assert_eq!(
        overview,
        (
            "Require confirmation before `gh repo delete` deletes private forks",
            true,
            PrState::Merged,
            "BagToad",
            Some(ReviewDecision::Approved),
        )
    );
}

#[test]
fn overview_maps_labels_and_requested_reviewers_users_and_teams() {
    // Arrange
    let mapped = escapes();

    // Act
    let labels: Vec<_> = mapped.labels.iter().map(|l| l.as_str()).collect();
    let reviewers: Vec<_> = mapped
        .requested_reviewers
        .iter()
        .map(|r| r.as_str())
        .collect();

    // Assert
    assert_eq!(labels, vec!["bug!"]);
    assert_eq!(reviewers, vec!["reviewer", "coreteam"]);
}

#[test]
fn issue_comments_and_review_threads_are_separate_lists() {
    // Arrange
    let mapped = recorded();

    // Act
    let comment_authors: Vec<_> = mapped.comments.iter().map(|c| c.author.as_str()).collect();
    let thread_paths: Vec<_> = mapped.threads.iter().map(|t| t.path.as_str()).collect();
    let thread_authors: Vec<_> = mapped.threads[0]
        .comments
        .iter()
        .map(|c| c.author.as_str())
        .collect();

    // Assert
    assert_eq!(comment_authors, vec!["Ufktarimarge"]);
    assert_eq!(
        thread_paths,
        vec!["acceptance/testdata/repo/repo-delete-private-forks.txtar"]
    );
    assert_eq!(
        thread_authors,
        vec!["copilot-pull-request-reviewer", "BagToad"]
    );
}

#[test]
fn a_thread_carries_its_resolved_flag_and_line() {
    // Arrange
    let mapped = escapes();

    // Act
    let open = &mapped.threads[0];
    let resolved = &mapped.threads[1];

    // Assert
    assert_eq!((open.is_resolved, open.line), (false, Some(7)));
    assert_eq!((resolved.is_resolved, resolved.line), (true, None));
    assert!(recorded().threads[0].is_resolved);
    assert_eq!(recorded().threads[0].line, Some(15));
}

#[test]
fn commits_keep_their_order_with_oid_headline_author_and_date() {
    // Arrange
    let mapped = recorded();

    // Act
    let commits: Vec<_> = mapped
        .commits
        .iter()
        .map(|c| {
            (
                c.oid.as_str(),
                c.headline.as_str(),
                c.author.as_str(),
                c.date.to_rfc3339(),
            )
        })
        .collect();

    // Assert
    assert_eq!(
        commits,
        vec![
            (
                "7bfdc1b0811d2c8cfb61f792419f116a1ac6448c",
                "Require confirmation before deleting a private repo's forks",
                "BagToad",
                "2026-10-28T19:22:16+00:00".to_owned(),
            ),
            (
                "4948470b5a97a7fd839df5389527f575070ab2f8",
                "Accept cascading fork deletion in acceptance cleanup",
                "BagToad",
                "2026-10-28T19:22:17+00:00".to_owned(),
            ),
            (
                "b17c1554e635b794ff9dbb6607e15575bd9ea387",
                "Add acceptance test for deleting a private repo with forks",
                "BagToad",
                "2026-10-28T19:22:17+00:00".to_owned(),
            ),
        ]
    );
}

#[test]
fn check_runs_map_their_state_and_url() {
    // Arrange
    let mapped = recorded();

    // Act
    let first = &mapped.check_runs[0];
    let second = &mapped.check_runs[1];

    // Assert
    assert_eq!(mapped.check_runs.len(), 25);
    assert_eq!(first.name.as_str(), "label-external");
    assert_eq!(first.state, CheckRunState::Skipped);
    assert_eq!(
        first.url.as_ref().map(|url| url.as_str()),
        Some("https://github.com/cli/cli/actions/runs/40755671685/job/123053838439")
    );
    assert_eq!(second.state, CheckRunState::Success);
    assert_eq!(
        second.url.as_ref().map(|url| url.as_str()),
        Some("https://github.com/cli/cli/actions/runs/40754336109/job/123049100414")
    );
}

#[test]
fn an_unfinished_check_run_reports_its_status() {
    // Arrange
    let mapped = escapes();

    // Act
    let running = &mapped.check_runs[2];

    // Assert
    assert_eq!(running.name.as_str(), "running");
    assert_eq!(running.state, CheckRunState::InProgress);
}

#[test]
fn an_unknown_check_state_is_kept_raw_not_shown_as_success() {
    // Arrange
    let mapped = escapes();

    // Act
    let mystery = &mapped.check_runs[1];

    // Assert
    match &mystery.state {
        CheckRunState::Unknown(raw) => assert_eq!(raw.as_str(), "FUTURE_STATE"),
        other => panic!("expected an unknown state, got {other:?}"),
    }
    assert_eq!(mystery.url, None);
}

#[test]
fn a_status_context_is_not_listed_as_a_check_run() {
    // Arrange
    let mapped = escapes();

    // Act
    let names: Vec<_> = mapped.check_runs.iter().map(|c| c.name.as_str()).collect();

    // Assert
    assert_eq!(names, vec!["build (linux)", "mystery", "running"]);
}

#[test]
fn every_remote_text_field_is_sanitized() {
    // Arrange
    let mapped = escapes();

    // Act
    let fields = [
        mapped.title.as_str(),
        mapped.body.as_str(),
        mapped.labels[0].as_str(),
        mapped.comments[0].body.as_str(),
        mapped.threads[0].comments[0].body.as_str(),
        mapped.commits[0].headline.as_str(),
        mapped.check_runs[0].name.as_str(),
    ];

    // Assert
    assert_eq!(
        fields,
        [
            "Plain red title",
            "Body with escapes\nsecond line",
            "bug!",
            "Issue comment text",
            "Thread comment text",
            "Fix the thing",
            "build (linux)",
        ]
    );
}

#[test]
fn a_null_author_maps_to_the_ghost_login() {
    // Arrange
    let mapped = escapes();

    // Act
    let author = mapped.author.as_str();

    // Assert
    assert_eq!(author, "ghost");
}

const CUT_TOTAL: u64 = 10_000;

/// The recorded detail with `totalCount` replaced at the JSON pointer `pointer` inside the pull
/// request.
fn recorded_with_total(pointer: &str, total: u64) -> PrDetail {
    let file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/detail/recorded-public-pr.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(file).expect("read fixture"))
            .expect("fixture json");
    let node = json["data"]["repository"]["pullRequest"]
        .pointer_mut(pointer)
        .expect("the connection is in the fixture");
    node["totalCount"] = total.into();
    map_detail(&json.to_string()).expect("map detail")
}

fn whole(shown: usize) -> Shown {
    Shown {
        shown,
        total: shown,
    }
}

fn cut(shown: usize) -> Shown {
    Shown {
        shown,
        total: CUT_TOTAL as usize,
    }
}

#[test]
fn a_whole_detail_reports_shown_equal_to_total_for_every_list() {
    // Arrange
    let detail = recorded();

    // Act
    let counts = detail.counts;

    // Assert
    assert_eq!(counts.comments, whole(1));
    assert_eq!(counts.threads, whole(1));
    assert_eq!(counts.commits, whole(3));
    assert_eq!(counts.checks, whole(25));
    assert_eq!(detail.threads[0].comment_count, whole(2));
    assert!(!counts.comments.is_cut());
}

#[test]
fn a_cut_list_reports_how_many_are_shown_of_the_total() {
    // Arrange
    let contexts = "/checkRuns/nodes/0/commit/statusCheckRollup/contexts";

    // Act
    let comments = recorded_with_total("/comments", CUT_TOTAL).counts.comments;
    let thread_list = recorded_with_total("/reviewThreads", CUT_TOTAL)
        .counts
        .threads;
    let commits = recorded_with_total("/commits", CUT_TOTAL).counts.commits;
    let checks = recorded_with_total(contexts, CUT_TOTAL).counts.checks;

    // Assert
    assert_eq!(comments, cut(1));
    assert_eq!(thread_list, cut(1));
    assert_eq!(commits, cut(3));
    assert_eq!(checks, cut(25));
    assert!(comments.is_cut());
}

#[test]
fn a_cut_thread_reports_its_own_comment_total() {
    // Arrange
    let pointer = "/reviewThreads/nodes/0/comments";

    // Act
    let detail = recorded_with_total(pointer, CUT_TOTAL);

    // Assert
    assert_eq!(detail.threads[0].comment_count, cut(2));
    assert!(!detail.counts.threads.is_cut());
}

#[test]
fn the_detail_query_asks_for_the_totals() {
    // Arrange
    let query = helios::github::queries::DETAIL_QUERY;

    // Act
    let count = query.matches("totalCount").count();

    // Assert
    assert!(
        count >= 5,
        "comments, threads, thread comments, commits and checks need totals: {count}"
    );
}

#[test]
fn a_detail_with_a_bad_time_or_bad_json_is_a_typed_error() {
    // Arrange
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/detail/recorded-public-pr.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(path).expect("read fixture"))
            .expect("fixture json");
    value["data"]["repository"]["pullRequest"]["comments"]["nodes"][0]["createdAt"] =
        "not a time".into();

    // Act
    let bad_time = map_detail(&value.to_string());
    let bad_json = map_detail("{");

    // Assert
    assert!(matches!(
        bad_time,
        Err(helios::github::queries::DetailError::Timestamp)
    ));
    assert!(matches!(
        bad_json,
        Err(helios::github::queries::DetailError::Parse)
    ));
}

#[test]
fn a_files_body_that_is_not_an_array_is_a_typed_error() {
    // Arrange
    let bodies = ["{}", "[{\"filename\": 1}]", ""];

    for body in bodies {
        // Act
        let result = map_files(body);

        // Assert
        assert!(
            matches!(result, Err(helios::github::queries::FilesError::Parse)),
            "{body:?}"
        );
    }
}

#[test]
fn no_patch_gap_follows_status_and_changes() {
    // Arrange: each status and changes pair, with no patch
    let cases = [
        ("added", 0, DiffGap::AddedOrRemovedNoText),
        ("removed", 0, DiffGap::AddedOrRemovedNoText),
        ("modified", 0, DiffGap::ChangedNoText),
        ("changed", 0, DiffGap::ChangedNoText),
        ("copied", 0, DiffGap::ChangedNoText),
        ("renamed", 0, DiffGap::RenamedOnly),
        ("added", 7, DiffGap::Truncated),
        ("removed", 7, DiffGap::Truncated),
        ("modified", 7, DiffGap::Truncated),
        ("renamed", 7, DiffGap::Truncated),
    ];

    for (status, changes, expected) in cases {
        let json = serde_json::json!([{
            "filename": "f",
            "status": status,
            "changes": changes,
            "blob_url": "https://github.com/o/r/blob/a/f"
        }])
        .to_string();

        // Act
        let mapped = map_files(&json).expect("map the files");

        // Assert
        match &mapped[0].diff {
            FileDiff::Unavailable { gap, .. } => {
                assert_eq!(*gap, expected, "{status} with {changes} changes");
            }
            other => panic!("expected an unavailable diff, got {other:?}"),
        }
    }
}
