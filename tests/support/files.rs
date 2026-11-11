// SPDX-License-Identifier: Apache-2.0

//! Changed files for the screen and loop tests.

use helios::model::{ChangedFile, FileDiff};
use helios::sanitize::sanitize;

/// A file whose patch adds one line, `added`.
pub fn patch_file(path: &str, added: &str) -> ChangedFile {
    ChangedFile {
        path: sanitize(path),
        previous_path: None,
        diff: FileDiff::Patch(sanitize(&format!("@@ -1,1 +1,2 @@\n context\n+{added}"))),
    }
}
