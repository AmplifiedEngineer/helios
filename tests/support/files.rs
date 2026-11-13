// SPDX-License-Identifier: Apache-2.0

//! Changed files for the screen and loop tests.

use helios::model::{ChangedFile, FileDiff};
use helios::sanitize::sanitize;

/// A file whose patch is `patch`, which must already be clean text.
pub fn file_with_patch(path: &str, patch: &str) -> ChangedFile {
    ChangedFile {
        path: sanitize(path),
        previous_path: None,
        diff: FileDiff::Patch(sanitize(patch)),
    }
}

/// A patch of one hunk: `count` added lines of `line_bytes` bytes each, then the added line `last`.
pub fn long_lines_patch(count: usize, line_bytes: usize, last: &str) -> String {
    let mut patch = String::from("@@ -1,1 +1,2 @@");
    for _ in 0..count {
        patch.push_str(&format!("\n+{:<line_bytes$}", "let a = 1;"));
    }
    patch.push_str(&format!("\n+{last}"));
    patch
}

/// A file whose patch adds one line, `added`.
pub fn patch_file(path: &str, added: &str) -> ChangedFile {
    ChangedFile {
        path: sanitize(path),
        previous_path: None,
        diff: FileDiff::Patch(sanitize(&format!("@@ -1,1 +1,2 @@\n context\n+{added}"))),
    }
}
