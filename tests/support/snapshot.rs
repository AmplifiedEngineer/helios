// SPDX-License-Identifier: Apache-2.0

//! Snapshot comparison against files in `tests/fixtures/snapshots/`.
//!
//! Files are rewritten only when `UPDATE_SNAPSHOTS` is set, and never in CI.

// A shared helper: each test binary uses only part of it.
#![allow(dead_code)]

use std::env;
use std::fs;
use std::path::PathBuf;

pub const UPDATE_VAR: &str = "UPDATE_SNAPSHOTS";
pub const CI_VAR: &str = "CI";
pub const REFUSAL_MESSAGE: &str = "UPDATE_SNAPSHOTS is not allowed when CI is set";

const UPDATE_ENABLED: &str = "1";
const SNAPSHOT_DIR: &str = "tests/fixtures/snapshots";
const SNAPSHOT_EXTENSION: &str = "snap";

fn update_requested() -> bool {
    env::var(UPDATE_VAR).is_ok_and(|value| value == UPDATE_ENABLED)
}

fn snapshot_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(SNAPSHOT_DIR)
        .join(format!("{name}.{SNAPSHOT_EXTENSION}"))
}

/// Compares `actual` with the stored snapshot `name`.
///
/// With `UPDATE_SNAPSHOTS=1` the file is rewritten instead; that panics when `CI` is set.
pub fn assert_snapshot(name: &str, actual: &str) {
    let update = update_requested();
    if update && env::var_os(CI_VAR).is_some() {
        panic!("{REFUSAL_MESSAGE}");
    }

    let path = snapshot_path(name);
    if update {
        fs::write(&path, actual)
            .unwrap_or_else(|err| panic!("cannot write snapshot {}: {err}", path.display()));
        return;
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "cannot read snapshot {}: {err}; run with {UPDATE_VAR}={UPDATE_ENABLED} to create it",
            path.display()
        )
    });
    assert_eq!(
        expected, actual,
        "snapshot {name} differs; run with {UPDATE_VAR}={UPDATE_ENABLED} to accept the change"
    );
}
