// SPDX-License-Identifier: Apache-2.0

//! Guards the root documentation: the files exist, their relative links resolve,
//! and they name no CLI flag beyond the documented ones.

use std::fs;
use std::path::{Path, PathBuf};

const DOCS: [&str; 4] = ["README.md", "SECURITY.md", "CONTRIBUTING.md", "AGENTS.md"];
/// Long flags the CLI supports and the docs may name.
const DOCUMENTED_FLAGS: [&str; 1] = ["--version"];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Lines outside fenced code blocks, with 1-based line numbers.
fn prose_lines(text: &str) -> Vec<(usize, &str)> {
    let mut in_fence = false;
    let mut lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence {
            lines.push((index + 1, line));
        }
    }
    lines
}

/// Link targets on one line: inline `](target)` and reference `[label]: target`.
fn targets_in(line: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("](") {
        rest = &rest[start + 2..];
        if let Some(end) = rest.find(')') {
            targets.push(
                rest[..end]
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string(),
            );
            rest = &rest[end..];
        }
    }
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() <= 3 && trimmed.starts_with('[') {
        if let Some((_, after)) = trimmed.split_once("]:") {
            targets.push(after.split_whitespace().next().unwrap_or("").to_string());
        }
    }
    targets
}

fn is_relative(target: &str) -> bool {
    !(target.is_empty()
        || target.starts_with('#')
        || target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:"))
}

/// Broken relative links in `text`, as `line: target`, resolved against `base`.
fn broken_links(base: &Path, text: &str) -> Vec<String> {
    let mut broken = Vec::new();
    for (line_no, line) in prose_lines(text) {
        for target in targets_in(line).into_iter().filter(|t| is_relative(t)) {
            let path = target.split('#').next().unwrap_or("");
            if !path.is_empty() && !base.join(path).exists() {
                broken.push(format!("{line_no}: {target}"));
            }
        }
    }
    broken
}

/// Long flags (`--` then a lowercase letter) anywhere in `text`, code blocks included.
fn flags_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let bytes = line.as_bytes();
        for i in 0..bytes.len().saturating_sub(2) {
            if &bytes[i..i + 2] == b"--" && bytes[i + 2].is_ascii_lowercase() {
                found.push(format!("{}: {}", index + 1, &line[i..]));
            }
        }
    }
    found
}

fn read_doc(name: &str) -> String {
    fs::read_to_string(root().join(name)).unwrap_or_else(|e| panic!("cannot read {name}: {e}"))
}

#[test]
fn docs_exist() {
    // Arrange
    let missing_or_empty: Vec<&str> = DOCS
        .iter()
        .copied()
        .filter(|name| fs::read_to_string(root().join(name)).map_or(true, |t| t.trim().is_empty()))
        .collect();

    // Act / Assert
    assert!(
        missing_or_empty.is_empty(),
        "missing or empty docs: {missing_or_empty:?}"
    );
}

#[test]
fn relative_links_resolve() {
    // Arrange
    let mut broken = Vec::new();

    // Act
    for name in DOCS {
        for entry in broken_links(&root(), &read_doc(name)) {
            broken.push(format!("{name}:{entry}"));
        }
    }

    // Assert
    assert!(
        broken.is_empty(),
        "broken relative links:\n{}",
        broken.join("\n")
    );
}

// Strict except for the documented flags; a later step checks the docs against `--help` instead.
#[test]
fn docs_name_only_documented_flags() {
    // Arrange
    let mut flags = Vec::new();

    // Act
    for name in DOCS {
        for entry in flags_in(&read_doc(name)) {
            let flag = entry.split_once(": ").map_or("", |(_, flag)| flag);
            if DOCUMENTED_FLAGS.iter().any(|known| flag.starts_with(known)) {
                continue;
            }
            flags.push(format!("{name}:{entry}"));
        }
    }

    // Assert
    assert!(
        flags.is_empty(),
        "flags named in docs:\n{}",
        flags.join("\n")
    );
}

#[test]
fn checks_detect_what_they_claim() {
    // Arrange
    let text = "[ok](Cargo.toml#top)\n\
                [bad](missing/file.md)\n\
                [ext](https://example.com) [anchor](#x)\n\
                ```\n\
                [hidden](nowhere.md)\n\
                ```\n\
                [ref]: also-missing.md\n\
                run it with --verbose\n";

    // Act
    let broken = broken_links(&root(), text);
    let flags = flags_in(text);

    // Assert
    assert_eq!(broken, vec!["2: missing/file.md", "7: also-missing.md"]);
    assert_eq!(flags, vec!["8: --verbose"]);
}
