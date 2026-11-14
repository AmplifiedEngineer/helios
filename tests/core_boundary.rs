// SPDX-License-Identifier: Apache-2.0

//! The core modules never name the terminal UI, so they stay reusable without it.

use std::fs;
use std::path::{Path, PathBuf};

const SINGLE_FILES: [&str; 4] = [
    "src/highlight.rs",
    "src/theme.rs",
    "src/test_files.rs",
    "src/config.rs",
];

fn names_the_ui(text: &str) -> bool {
    text.contains("ratatui") || text.contains("crate::tui")
}

fn rust_files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files_under(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn core_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    rust_files_under(&root.join("src/markdown"), &mut files);
    files.extend(
        SINGLE_FILES
            .iter()
            .map(|f| root.join(f))
            .filter(|p| p.is_file()),
    );
    files.sort();
    files
}

#[test]
fn core_modules_do_not_name_the_ui() {
    // Arrange
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = core_files(root);

    // Act
    let offenders: Vec<&PathBuf> = files
        .iter()
        .filter(|p| names_the_ui(&fs::read_to_string(p).unwrap_or_default()))
        .collect();

    // Assert
    assert!(!files.is_empty(), "the scan found no core files to check");
    assert!(
        offenders.is_empty(),
        "core files name the UI: {offenders:?}; scanned: {files:?}"
    );
}

#[test]
fn the_boundary_scan_rejects_a_file_that_names_the_ui() {
    // Arrange
    let ratatui_use = "use ratatui::Frame;\n";
    let tui_use = "use crate::tui::draw;\n";
    let clean = "use std::fmt;\npub fn render() {}\n";

    // Act
    let flags = (
        names_the_ui(ratatui_use),
        names_the_ui(tui_use),
        names_the_ui(clean),
    );

    // Assert
    assert_eq!(flags, (true, true, false));
}
