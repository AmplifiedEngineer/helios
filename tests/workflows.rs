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

/// Write grants a job may hold, as (file, job, scope), each with the reason it needs it.
const ALLOWED_JOB_WRITES: [(&str, &str, &str, &str); 10] = [
    (
        "ci-auto-rerun.yml",
        "rerun",
        "actions",
        "reruns failed jobs",
    ),
    ("osv-scan.yml", "scan", "security-events", "uploads SARIF"),
    ("lock.yml", "lock", "issues", "locks closed issues"),
    (
        "lock.yml",
        "lock",
        "pull-requests",
        "locks closed pull requests",
    ),
    (
        "clanker-filter.yml",
        "clanker-filter",
        "pull-requests",
        "labels and comments on PRs",
    ),
    (
        "dependency-review.yml",
        "review",
        "pull-requests",
        "posts the failure summary comment",
    ),
    (
        "scorecard.yml",
        "analysis",
        "security-events",
        "uploads SARIF results",
    ),
    (
        "scorecard.yml",
        "analysis",
        "id-token",
        "publishes results to the Scorecard API",
    ),
    (
        "stale.yml",
        "stale",
        "issues",
        "labels and closes stale issues",
    ),
    (
        "stale.yml",
        "stale",
        "pull-requests",
        "labels and closes stale pull requests",
    ),
];

fn is_allowed_job_write(file: &str, job: &str, scope: &str) -> bool {
    ALLOWED_JOB_WRITES
        .iter()
        .any(|(f, j, s, _)| (*f, *j, *s) == (file, job, scope))
}

/// Every job-level write grant as (file, job, scope). Jobs without a mapping
/// are skipped here and reported by `write_violations`.
fn job_write_grants(workflows: &[(String, Yaml)]) -> Vec<(String, String, String)> {
    let mut grants = Vec::new();
    for (name, doc) in workflows {
        for (job_id, job) in doc["jobs"].as_hash().into_iter().flatten() {
            let job_id = job_id.as_str().unwrap_or("?");
            for scope in write_grants(&job["permissions"]) {
                grants.push((name.clone(), job_id.to_string(), scope));
            }
        }
    }
    grants
}

/// Returns each disallowed write grant: any top-level write, `write-all`
/// anywhere, job-level writes missing from `ALLOWED_JOB_WRITES`, and a
/// workflow whose jobs the check cannot read (it fails closed).
fn write_violations(workflows: &[(String, Yaml)]) -> Vec<String> {
    let mut violations = Vec::new();
    for (name, doc) in workflows {
        violations.extend(
            write_grants(&doc["permissions"])
                .into_iter()
                .map(|scope| format!("{name} top-level: {scope}")),
        );
        let Some(jobs) = doc["jobs"].as_hash() else {
            violations.push(format!("{name}: no jobs mapping"));
            continue;
        };
        for (job_id, job) in jobs {
            let job_id = job_id.as_str().unwrap_or("?");
            if job.as_hash().is_none() {
                violations.push(format!("{name} job {job_id}: not a mapping"));
                continue;
            }
            violations.extend(
                write_grants(&job["permissions"])
                    .into_iter()
                    .filter(|scope| {
                        scope == "write-all" || !is_allowed_job_write(name, job_id, scope)
                    })
                    .map(|scope| format!("{name} job {job_id}: {scope}")),
            );
        }
    }
    violations
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
    let violations = write_violations(&workflows);

    // Assert
    assert!(
        violations.is_empty(),
        "write permissions granted: {violations:?}"
    );
}

fn docs_from(name: &str, text: &str) -> Vec<(String, Yaml)> {
    let mut docs = YamlLoader::load_from_str(text).expect("test YAML should parse");
    vec![(name.to_string(), docs.remove(0))]
}

#[test]
fn write_check_accepts_listed_job_grant_and_rejects_the_rest() {
    // Arrange
    let listed = docs_from(
        "lock.yml",
        "permissions:\n  contents: read\njobs:\n  lock:\n    permissions:\n      issues: write\n",
    );
    let unlisted_scope = docs_from(
        "lock.yml",
        "permissions:\n  contents: read\njobs:\n  lock:\n    permissions:\n      contents: write\n",
    );
    let wrong_job = docs_from(
        "lock.yml",
        "permissions:\n  contents: read\njobs:\n  other:\n    permissions:\n      issues: write\n",
    );
    let wrong_file = docs_from(
        "other.yml",
        "permissions:\n  contents: read\njobs:\n  lock:\n    permissions:\n      issues: write\n",
    );
    let top_level = docs_from(
        "lock.yml",
        "permissions:\n  contents: write\njobs:\n  lock:\n    runs-on: x\n",
    );
    let write_all = docs_from(
        "lock.yml",
        "permissions:\n  contents: read\njobs:\n  lock:\n    permissions: write-all\n",
    );
    let no_jobs = docs_from("lock.yml", "permissions:\n  contents: read\n");
    let job_not_a_mapping = docs_from("lock.yml", "jobs:\n  lock: nope\n");

    // Act
    let results = [
        write_violations(&listed),
        write_violations(&unlisted_scope),
        write_violations(&wrong_job),
        write_violations(&wrong_file),
        write_violations(&top_level),
        write_violations(&write_all),
        write_violations(&no_jobs),
        write_violations(&job_not_a_mapping),
    ];

    // Assert
    assert_eq!(results[0], Vec::<String>::new(), "listed grant refused");
    assert_eq!(results[1], ["lock.yml job lock: contents"]);
    assert_eq!(results[2], ["lock.yml job other: issues"]);
    assert_eq!(results[3], ["other.yml job lock: issues"]);
    assert_eq!(results[4], ["lock.yml top-level: contents"]);
    assert_eq!(results[5], ["lock.yml job lock: write-all"]);
    assert_eq!(results[6], ["lock.yml: no jobs mapping"]);
    assert_eq!(results[7], ["lock.yml job lock: not a mapping"]);
}

#[test]
fn every_allowlist_entry_matches_a_real_job_grant() {
    // Arrange
    let held = job_write_grants(&all_workflows());

    // Act
    let stale: Vec<_> = ALLOWED_JOB_WRITES
        .iter()
        .filter(|(f, j, s, _)| !held.contains(&(f.to_string(), j.to_string(), s.to_string())))
        .collect();

    // Assert
    assert!(
        stale.is_empty(),
        "allowlist entries no workflow uses: {stale:?}"
    );
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
