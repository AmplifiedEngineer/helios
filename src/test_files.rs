// SPDX-License-Identifier: Apache-2.0

//! Decides whether a changed file is a test file, from its path alone.

pub const DEFAULT_DIRECTORIES: [&str; 4] = ["tests", "test", "__tests__", "spec"];
pub const DEFAULT_FILE_PATTERNS: [&str; 5] = [
    "*.test.*",
    "*.spec.*",
    "*_test.rs",
    "*_test.go",
    "test_*.py",
];

/// Directory names and file name globs that mark a path as a test file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestPatterns {
    directories: Vec<String>,
    files: Vec<String>,
}

impl TestPatterns {
    pub fn new(directories: Vec<String>, files: Vec<String>) -> Self {
        Self { directories, files }
    }
}

impl Default for TestPatterns {
    fn default() -> Self {
        Self::new(
            DEFAULT_DIRECTORIES.map(String::from).to_vec(),
            DEFAULT_FILE_PATTERNS.map(String::from).to_vec(),
        )
    }
}

pub fn is_test_path(path: &str, patterns: &TestPatterns) -> bool {
    let mut components = path.split('/');
    let Some(file_name) = components.next_back() else {
        return false;
    };
    components.any(|dir| patterns.directories.iter().any(|name| name == dir))
        || patterns
            .files
            .iter()
            .any(|pattern| glob_matches(pattern, file_name))
}

/// Matches `text` against `pattern`, where `*` stands for any run of characters.
fn glob_matches(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, rest)) => text.strip_prefix(head).is_some_and(|tail| {
            (0..=tail.len()).any(|skip| {
                tail.get(skip..)
                    .is_some_and(|suffix| glob_matches(rest, suffix))
            })
        }),
    }
}
