// SPDX-License-Identifier: Apache-2.0

//! Guards the structure of the GitHub Actions workflows under `.github/workflows`.

use std::path::PathBuf;
use std::process::Command;

use yaml_rust2::{Yaml, YamlLoader};

const REQUIRED_WORKFLOWS: [&str; 3] = ["ci.yml", "zizmor.yml", "dco.yml"];
const GUARD_SCRIPT: &str = r#"test -z "${UPDATE_SNAPSHOTS:-}""#;
const REDIRECT_FEATURE: &str = "--features follow-redirect";

fn workflows_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/workflows")
}

fn parse_workflow(name: &str) -> Yaml {
    let path = workflows_dir().join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} should be readable: {e}", path.display()));
    let mut docs = YamlLoader::load_from_str(&text)
        .unwrap_or_else(|e| panic!("{name} should be valid YAML: {e}"));
    assert_eq!(
        docs.len(),
        1,
        "{name} should hold exactly one YAML document"
    );
    docs.remove(0)
}

/// Asserts the directory and every required file exist, then parses all
/// `.yml` and `.yaml` files so no check can pass over an empty set.
fn all_workflows() -> Vec<(String, Yaml)> {
    let dir = workflows_dir();
    assert!(dir.is_dir(), "{} should exist", dir.display());
    for name in REQUIRED_WORKFLOWS {
        assert!(
            dir.join(name).is_file(),
            "{name} should exist in {}",
            dir.display()
        );
    }
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("workflow directory should be readable")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".yml") || name.ends_with(".yaml"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let doc = parse_workflow(&name);
            (name, doc)
        })
        .collect()
}

fn collect_uses(node: &Yaml, found: &mut Vec<String>) {
    match node {
        Yaml::Hash(map) => {
            for (key, value) in map {
                match (key.as_str(), value.as_str()) {
                    (Some("uses"), Some(uses)) => found.push(uses.to_string()),
                    _ => collect_uses(value, found),
                }
            }
        }
        Yaml::Array(items) => items.iter().for_each(|item| collect_uses(item, found)),
        _ => {}
    }
}

fn is_sha_pinned(uses: &str) -> bool {
    if uses.starts_with("./") {
        return true;
    }
    match uses.rsplit_once('@') {
        Some((_, rev)) => rev.len() == 40 && rev.chars().all(|c| c.is_ascii_hexdigit()),
        None => false,
    }
}

/// Returns the write grants in a `permissions` node, empty when none.
fn write_grants(permissions: &Yaml) -> Vec<String> {
    match permissions {
        Yaml::String(scope) if scope.contains("write") => vec![scope.clone()],
        Yaml::Hash(map) => map
            .iter()
            .filter(|(_, level)| level.as_str() == Some("write"))
            .map(|(scope, _)| scope.as_str().unwrap_or("?").to_string())
            .collect(),
        _ => vec![],
    }
}

fn ci_run_scripts() -> Vec<String> {
    let ci = parse_workflow("ci.yml");
    let mut scripts = Vec::new();
    let jobs = ci["jobs"]
        .as_hash()
        .expect("ci.yml should define a jobs mapping");
    for (_, job) in jobs {
        // A job that calls a reusable workflow has no steps.
        let Some(steps) = job["steps"].as_vec() else {
            continue;
        };
        scripts.extend(
            steps
                .iter()
                .filter_map(|step| step["run"].as_str())
                .map(|run| run.trim().to_string()),
        );
    }
    scripts
}

fn guard_script_from_ci() -> String {
    let guards: Vec<String> = ci_run_scripts()
        .into_iter()
        .filter(|script| script == GUARD_SCRIPT)
        .collect();
    assert_eq!(
        guards.len(),
        1,
        "ci.yml should have one UPDATE_SNAPSHOTS guard step"
    );
    guards.into_iter().next().expect("one guard checked above")
}

fn run_guard(script: &str, snapshots: Option<&str>) -> std::process::ExitStatus {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(script)
        .env_clear()
        .env("PATH", "/usr/bin:/bin");
    if let Some(value) = snapshots {
        command.env("UPDATE_SNAPSHOTS", value);
    }
    command.status().expect("sh should be runnable")
}

#[test]
fn every_action_is_pinned_to_a_commit_sha() {
    // Arrange
    let workflows = all_workflows();

    // Act
    let mut unpinned = Vec::new();
    let mut total = 0;
    for (name, doc) in &workflows {
        let mut uses = Vec::new();
        collect_uses(doc, &mut uses);
        total += uses.len();
        unpinned.extend(
            uses.into_iter()
                .filter(|u| !is_sha_pinned(u))
                .map(|u| format!("{name}: {u}")),
        );
    }

    // Assert
    assert!(total > 0, "workflows should use at least one action");
    assert!(
        unpinned.is_empty(),
        "actions not pinned to a 40-character SHA: {unpinned:?}"
    );
}

#[test]
fn no_workflow_or_job_grants_write_permission() {
    // Arrange
    let workflows = all_workflows();

    // Act
    let mut grants = Vec::new();
    for (name, doc) in &workflows {
        grants.extend(
            write_grants(&doc["permissions"])
                .into_iter()
                .map(|g| format!("{name}: {g}")),
        );
        for (job_id, job) in doc["jobs"].as_hash().into_iter().flatten() {
            grants.extend(
                write_grants(&job["permissions"])
                    .into_iter()
                    .map(|g| format!("{name} job {}: {g}", job_id.as_str().unwrap_or("?"))),
            );
        }
    }

    // Assert
    assert!(grants.is_empty(), "write permissions granted: {grants:?}");
}

#[test]
fn every_workflow_declares_top_level_contents_read() {
    // Arrange
    let workflows = all_workflows();

    // Act
    let missing: Vec<&str> = workflows
        .iter()
        .filter(|(_, doc)| doc["permissions"]["contents"].as_str() != Some("read"))
        .map(|(name, _)| name.as_str())
        .collect();

    // Assert
    assert!(
        missing.is_empty(),
        "no top-level `permissions: contents: read`: {missing:?}"
    );
}

#[test]
fn ci_runs_tests_with_and_without_follow_redirect() {
    // Arrange
    all_workflows();
    let scripts = ci_run_scripts();

    // Act
    let test_steps: Vec<&String> = scripts
        .iter()
        .filter(|s| s.contains("cargo test"))
        .collect();
    let with_feature = test_steps
        .iter()
        .filter(|s| s.contains(REDIRECT_FEATURE))
        .count();
    let without_feature = test_steps.len() - with_feature;

    // Assert
    assert_eq!(
        without_feature, 1,
        "expected one `cargo test` step without the feature"
    );
    assert_eq!(
        with_feature, 1,
        "expected one `cargo test` step with {REDIRECT_FEATURE}"
    );
}

#[test]
fn ci_has_update_snapshots_guard_step() {
    // Arrange
    all_workflows();

    // Act
    let guard = guard_script_from_ci();

    // Assert
    assert_eq!(guard, GUARD_SCRIPT);
}

#[test]
fn guard_script_fails_when_update_snapshots_is_set() {
    // Arrange
    all_workflows();
    let guard = guard_script_from_ci();

    // Act
    let status = run_guard(&guard, Some("1"));

    // Assert
    assert!(
        !status.success(),
        "guard should exit non-zero when UPDATE_SNAPSHOTS is set"
    );
}

#[test]
fn guard_script_passes_when_update_snapshots_is_unset() {
    // Arrange
    all_workflows();
    let guard = guard_script_from_ci();

    // Act
    let status = run_guard(&guard, None);

    // Assert
    assert!(
        status.success(),
        "guard should exit zero when UPDATE_SNAPSHOTS is unset"
    );
}
