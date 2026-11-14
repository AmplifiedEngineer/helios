// SPDX-License-Identifier: Apache-2.0

//! The render cache keeps the roles of diff lines per file, within a byte budget.

use helios::highlight::LineRoles;
use helios::theme::Role;
use helios::tui::cache::{
    HighlightBudget, RenderCache, MAX_HIGHLIGHT_BYTES_PER_FRAME, MAX_ROLE_ENTRIES,
};

const PR: &str = "github.com/octo/org/repo#1";
const OTHER_PR: &str = "github.com/octo/org/repo#2";
const PATH: &str = "src/a.rs";
const HUNK: &str = "@@ -1,1 +1,10 @@";
const LINE_BYTES: usize = 100;
const AMPLE: usize = 1_000_000;

/// A code line of exactly `LINE_BYTES` bytes that starts with a keyword.
fn code_line() -> String {
    format!("{:<LINE_BYTES$}", "let a = 1;")
}

/// A hunk header followed by `count` added lines of `LINE_BYTES` bytes each.
fn patch_of(count: usize) -> String {
    let mut patch = String::from(HUNK);
    for _ in 0..count {
        patch.push_str("\n+");
        patch.push_str(&code_line());
    }
    patch
}

fn roles(cache: &RenderCache, path: &str, patch: &str, budget: usize) -> Vec<LineRoles> {
    cache.diff_roles(PR, path, patch, &HighlightBudget::new(budget))
}

#[test]
fn the_frame_budget_is_65536_bytes() {
    // Arrange and Act
    let budget = HighlightBudget::per_frame();

    // Assert
    assert_eq!(MAX_HIGHLIGHT_BYTES_PER_FRAME, 65536);
    assert_eq!(budget.left(), 65536);
}

#[test]
fn roles_cover_the_code_after_the_marker_and_skip_the_hunk_header() {
    // Arrange
    let cache = RenderCache::new();
    let patch =
        format!("{HUNK}\n context\n-let old = 1;\n+fn main() {{}}\n\\ No newline at end of file");

    // Act
    let rows = roles(&cache, PATH, &patch, AMPLE);

    // Assert
    assert_eq!(rows.len(), 5, "one row of roles per patch line");
    assert!(rows[0].is_empty(), "the hunk header is not fed");
    assert_eq!(rows[1].iter().map(|(_, r)| r.end).max(), Some(7));
    assert_eq!(rows[2][0], (Role::Keyword, 0..3), "let after the minus");
    assert_eq!(rows[3][0], (Role::Keyword, 0..2), "fn after the plus");
    assert!(rows[4].is_empty(), "the no newline note is not fed");
}

#[test]
fn file_headers_before_the_first_hunk_are_not_fed() {
    // Arrange
    let cache = RenderCache::new();
    let patch = format!("--- a/src/a.rs\n+++ b/src/a.rs\n{HUNK}\n+fn main() {{}}");

    // Act
    let rows = roles(&cache, PATH, &patch, AMPLE);

    // Assert
    assert!(rows[0].is_empty());
    assert!(rows[1].is_empty());
    assert_eq!(rows[3][0], (Role::Keyword, 0..2));
}

#[test]
fn the_language_comes_from_the_path() {
    // Arrange
    let cache = RenderCache::new();
    let patch = format!("{HUNK}\n+def f(): pass");

    // Act
    let python = roles(&cache, "tool.py", &patch, AMPLE);
    let text = roles(&cache, "notes.txt", &patch, AMPLE);

    // Assert
    assert_eq!(python[1][0], (Role::Keyword, 0..3), "def is a keyword");
    assert!(text[1].iter().all(|(role, _)| *role == Role::Plain));
}

#[test]
fn a_budget_stops_the_work_and_the_next_call_resumes_it() {
    // Arrange
    let cache = RenderCache::new();
    let patch = patch_of(10);

    // Act
    let first = roles(&cache, PATH, &patch, LINE_BYTES * 2 + 50);
    let pending_after_first = cache.take_pending();
    let bytes_after_first = cache.highlighted_bytes();
    let second = roles(&cache, PATH, &patch, AMPLE);
    let bytes_after_second = cache.highlighted_bytes();

    // Assert
    assert_eq!(first.len(), 3, "the header and two lines fit the budget");
    assert!(pending_after_first);
    assert_eq!(bytes_after_first, LINE_BYTES * 2);
    assert_eq!(second.len(), 11);
    assert_eq!(second[..3], first[..], "the work done so far is kept");
    assert_eq!(bytes_after_second, LINE_BYTES * 10, "no line is fed twice");
    assert!(!cache.take_pending(), "a finished entry is not pending");
}

#[test]
fn a_finished_entry_costs_nothing_on_the_next_call() {
    // Arrange
    let cache = RenderCache::new();
    let patch = patch_of(3);
    roles(&cache, PATH, &patch, AMPLE);
    let bytes = cache.highlighted_bytes();

    // Act
    let budget = HighlightBudget::new(AMPLE);
    cache.diff_roles(PR, PATH, &patch, &budget);

    // Assert
    assert_eq!(cache.highlighted_bytes(), bytes);
    assert_eq!(budget.left(), AMPLE);
}

#[test]
fn the_bytes_of_each_fed_line_are_taken_from_the_budget() {
    // Arrange
    let cache = RenderCache::new();
    let budget = HighlightBudget::new(LINE_BYTES * 5);

    // Act
    cache.diff_roles(PR, PATH, &patch_of(3), &budget);
    cache.diff_roles(PR, "b.rs", &patch_of(3), &budget);

    // Assert
    assert_eq!(budget.left(), 0, "two files share one budget");
    assert_eq!(cache.highlighted_bytes(), LINE_BYTES * 5);
}

#[test]
fn clear_pending_and_take_pending_reset_the_flag() {
    // Arrange
    let cache = RenderCache::new();
    roles(&cache, PATH, &patch_of(4), LINE_BYTES);

    // Act
    let first = cache.take_pending();
    let second = cache.take_pending();
    roles(&cache, PATH, &patch_of(4), 0);
    cache.clear_pending();
    let after_clear = cache.take_pending();

    // Assert
    assert!(first, "the cut entry is pending");
    assert!(!second, "take_pending clears the flag");
    assert!(!after_clear, "clear_pending clears it too");
}

#[test]
fn a_changed_patch_is_highlighted_again() {
    // Arrange
    let cache = RenderCache::new();
    roles(&cache, PATH, &patch_of(2), AMPLE);

    // Act
    let reloaded = roles(&cache, PATH, &patch_of(3), AMPLE);

    // Assert
    assert_eq!(reloaded.len(), 4);
    assert_eq!(cache.highlighted_bytes(), LINE_BYTES * 5);
}

#[test]
fn the_entries_clear_when_the_pull_request_changes() {
    // Arrange
    let cache = RenderCache::new();
    let patch = patch_of(2);
    let budget = HighlightBudget::new(AMPLE);
    cache.diff_roles(PR, PATH, &patch, &budget);

    // Act
    cache.diff_roles(OTHER_PR, PATH, &patch, &budget);
    cache.diff_roles(PR, PATH, &patch, &budget);

    // Assert
    assert_eq!(
        cache.highlighted_bytes(),
        LINE_BYTES * 2 * 3,
        "each switch highlights the file again"
    );
}

#[test]
fn the_oldest_entry_is_dropped_past_the_cap_and_eviction_sets_no_pending_flag() {
    // Arrange
    let cache = RenderCache::new();
    let patch = patch_of(1);
    roles(&cache, "first.rs", &patch_of(4), LINE_BYTES);
    cache.take_pending();

    // Act
    for index in 0..MAX_ROLE_ENTRIES {
        roles(&cache, &format!("file{index}.rs"), &patch, AMPLE);
    }
    let pending = cache.take_pending();
    let before = cache.highlighted_bytes();
    roles(&cache, "first.rs", &patch_of(4), AMPLE);
    let after = cache.highlighted_bytes();

    // Assert
    assert!(!pending, "eviction does not raise the flag");
    assert_eq!(
        after - before,
        LINE_BYTES * 4,
        "the dropped file starts over"
    );
}

#[test]
fn the_newest_entries_stay_cached_at_the_cap() {
    // Arrange
    let cache = RenderCache::new();
    let patch = patch_of(1);
    for index in 0..=MAX_ROLE_ENTRIES {
        roles(&cache, &format!("file{index}.rs"), &patch, AMPLE);
    }
    let before = cache.highlighted_bytes();

    // Act
    roles(&cache, &format!("file{MAX_ROLE_ENTRIES}.rs"), &patch, AMPLE);

    // Assert
    assert_eq!(cache.highlighted_bytes(), before);
}
