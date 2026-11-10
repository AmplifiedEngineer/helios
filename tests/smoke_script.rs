// SPDX-License-Identifier: Apache-2.0

//! The manual smoke script, run against a fake `gh` and the real binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

#[path = "support/fake_gh.rs"]
mod fake_gh;
use fake_gh::FakeGh;

const GH_ENV: &str = "HELIOS_GH";
const BIN_ENV: &str = "HELIOS_BIN";
const SYSTEM_PATH: &str = "/usr/bin:/bin";
const NO_ACCOUNTS_JSON: &str = r#"echo '{"hosts":{}}'"#;
const NO_ACCOUNTS_MESSAGE: &str = "no accounts found";
const GH_MISSING_MESSAGE: &str = "gh not found";
const GH_FAILURE_MESSAGE: &str = "running gh failed";
const TOKEN_PREFIX: &str = "ghp";
const TOKEN_BODY: &str = "PlantedTokenForTheLeakCheck0123456789";

/// Built at runtime so no token-shaped literal sits in the source.
fn planted_token() -> String {
    format!("{TOKEN_PREFIX}_{TOKEN_BODY}")
}
const FAILURE_EXIT_CODE: i32 = 1;

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/smoke.sh")
}

/// Runs the script with a cleared environment holding only `HELIOS_GH`, `HELIOS_BIN` and a
/// system `PATH`. `HELIOS_BIN` makes the script run the already built test binary instead of
/// calling cargo.
fn run_script(gh: &Path) -> Output {
    Command::new("/bin/sh")
        .arg(script())
        .env_clear()
        .env("PATH", SYSTEM_PATH)
        .env(GH_ENV, gh)
        .env(BIN_ENV, env!("CARGO_BIN_EXE_helios"))
        .stdin(Stdio::null())
        .output()
        .expect("smoke script runs")
}

#[test]
fn smoke_script_exits_nonzero_when_gh_is_missing() {
    // Arrange
    let missing = std::env::temp_dir().join("helios-smoke-script-no-such-gh");

    // Act
    let output = run_script(&missing);

    // Assert
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(FAILURE_EXIT_CODE));
    assert!(stderr.contains(GH_MISSING_MESSAGE), "stderr: {stderr}");
}

#[test]
fn smoke_script_runs_helios_smoke_and_passes_the_exit_code() {
    // Arrange
    let gh = FakeGh::new(NO_ACCOUNTS_JSON);

    // Act
    let output = run_script(&gh.path);

    // Assert
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(FAILURE_EXIT_CODE));
    assert!(stdout.contains(NO_ACCOUNTS_MESSAGE), "stdout: {stdout}");
}

#[test]
fn smoke_script_prints_no_token() {
    // Arrange
    let token = planted_token();
    let gh = FakeGh::new(&format!("echo '{token}'\necho '{token}' >&2\nexit 1"));

    // Act
    let output = run_script(&gh.path);

    // Assert
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(FAILURE_EXIT_CODE));
    assert!(!stdout.contains(&token), "stdout: {stdout}");
    assert!(!stderr.contains(&token), "stderr: {stderr}");
    assert!(stderr.contains(GH_FAILURE_MESSAGE), "stderr: {stderr}");
}
