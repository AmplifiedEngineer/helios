// SPDX-License-Identifier: Apache-2.0

use helios::test_files::{is_test_path, TestPatterns};

const VITEST_AND_JEST: [(&str, bool); 5] = [
    ("a.test.ts", true),
    ("src/a.spec.tsx", true),
    ("web/x.test.js", true),
    ("b.spec.mjs", true),
    ("src/deep/widget.test.jsx", true),
];

const RUST_GO_AND_PYTHON: [(&str, bool); 4] = [
    ("src/foo_test.rs", true),
    ("pkg/x_test.go", true),
    ("test_models.py", true),
    ("app/test_views.py", true),
];

const TEST_DIRECTORIES: [(&str, bool); 6] = [
    ("tests/a.rs", true),
    ("src/tests/a.rs", true),
    ("a/b/test/c.py", true),
    ("pkg/__tests__/d.js", true),
    ("spec/x.rb", true),
    ("deep/er/spec/y.rb", true),
];

const SOURCE_FILES: [(&str, bool); 7] = [
    ("contest.rs", false),
    ("latest.ts", false),
    ("src/contest/a.rs", false),
    ("protest.py", false),
    ("attest_utils.go", false),
    ("src/testing.rs", false),
    ("src/test.rs", false),
];

fn assert_table(rows: &[(&str, bool)]) {
    // Arrange
    let patterns = TestPatterns::default();
    for (path, expected) in rows {
        // Act
        let actual = is_test_path(path, &patterns);

        // Assert
        assert_eq!(actual, *expected, "path {path}");
    }
}

#[test]
fn vitest_and_jest_names_are_test_files() {
    assert_table(&VITEST_AND_JEST);
}

#[test]
fn rust_go_and_python_test_names_are_test_files() {
    assert_table(&RUST_GO_AND_PYTHON);
}

#[test]
fn test_directories_match_at_any_depth() {
    assert_table(&TEST_DIRECTORIES);
}

#[test]
fn a_source_file_named_like_a_test_word_is_not_a_test_file() {
    assert_table(&SOURCE_FILES);
}
