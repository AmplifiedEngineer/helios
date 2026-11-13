// SPDX-License-Identifier: Apache-2.0

//! Guards the exact-pin policy for every dependency in Cargo.toml.

use std::collections::BTreeMap;

const EXPECTED: [&str; 12] = [
    "ratatui",
    "crossterm",
    "tokio",
    "octocrab",
    "serde",
    "serde_json",
    "thiserror",
    "clap",
    "chrono",
    "pulldown-cmark",
    "syntect",
    "two-face",
];

const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// Returns dependency name to version requirement for every dependency table,
/// including `[dependencies.<name>]` sub-tables and `target.*` tables.
fn dependency_versions(manifest: &str) -> BTreeMap<String, String> {
    let root: toml::Table = manifest.parse().expect("Cargo.toml should be valid TOML");
    let mut tables: Vec<&toml::Table> = KINDS
        .iter()
        .filter_map(|kind| root.get(*kind).and_then(toml::Value::as_table))
        .collect();
    if let Some(targets) = root.get("target").and_then(toml::Value::as_table) {
        for target in targets.values().filter_map(toml::Value::as_table) {
            tables.extend(
                KINDS
                    .iter()
                    .filter_map(|kind| target.get(*kind).and_then(toml::Value::as_table)),
            );
        }
    }
    tables
        .into_iter()
        .flatten()
        .map(|(name, spec)| {
            let version = match spec {
                toml::Value::String(v) => v.as_str(),
                toml::Value::Table(t) => {
                    t.get("version").and_then(toml::Value::as_str).unwrap_or("")
                }
                _ => "",
            };
            (name.clone(), version.to_string())
        })
        .collect()
}

fn read_manifest() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("Cargo.toml should be readable")
}

#[test]
fn declares_every_expected_dependency() {
    // Arrange
    let deps = dependency_versions(&read_manifest());

    // Act
    let missing: Vec<&str> = EXPECTED
        .iter()
        .copied()
        .filter(|name| !deps.contains_key(*name))
        .collect();

    // Assert
    assert!(missing.is_empty(), "missing dependencies: {missing:?}");
}

#[test]
fn pins_every_dependency_version_exactly() {
    // Arrange
    let deps = dependency_versions(&read_manifest());

    // Act
    let unpinned: Vec<&str> = deps
        .iter()
        .filter(|(_, version)| !version.starts_with('='))
        .map(|(name, _)| name.as_str())
        .collect();

    // Assert
    assert!(!deps.is_empty(), "no dependencies found to check");
    assert!(
        unpinned.is_empty(),
        "dependencies not pinned with '=': {unpinned:?}"
    );
}
