// SPDX-License-Identifier: Apache-2.0

//! The manual smoke script, run against a fake `gh` and the real binary.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

#[path = "support/fake_gh.rs"]
mod fake_gh;
use fake_gh::FakeGh;
#[path = "support/github.rs"]
mod github;
use github::planted_token;

const GH_ENV: &str = "HELIOS_GH";
const BIN_ENV: &str = "HELIOS_BIN";
const SYSTEM_PATH: &str = "/usr/bin:/bin";
const NO_ACCOUNTS_JSON: &str = r#"echo '{"hosts":{}}'"#;
const NO_ACCOUNTS_MESSAGE: &str = "no accounts found";
const GH_MISSING_MESSAGE: &str = "gh not found";
const GH_FAILURE_MESSAGE: &str = "running gh failed";
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

const EXECUTABLE_MODE: u32 = 0o755;
const MANIFEST_FLAG: &str = "--manifest-path";

#[test]
fn smoke_script_points_cargo_at_this_repo_from_any_directory() {
    // Arrange
    let gh = FakeGh::new(NO_ACCOUNTS_JSON);
    let dir = std::env::temp_dir().join(format!("helios-fake-cargo-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the fake cargo dir");
    let record = dir.join("args.txt");
    let cargo = dir.join("cargo");
    fs::write(
        &cargo,
        format!("#!/bin/sh\necho \"$@\" > {}\n", record.display()),
    )
    .expect("write the fake cargo");
    fs::set_permissions(&cargo, fs::Permissions::from_mode(EXECUTABLE_MODE)).expect("chmod");

    // Act
    let status = Command::new("/bin/sh")
        .arg(script())
        .env_clear()
        .env("PATH", format!("{}:{SYSTEM_PATH}", dir.display()))
        .env(GH_ENV, &gh.path)
        .current_dir(&dir)
        .stdin(Stdio::null())
        .status()
        .expect("smoke script runs");

    // Assert
    let args = fs::read_to_string(&record).expect("the fake cargo was called");
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let _ = fs::remove_dir_all(&dir);
    assert!(status.success());
    assert!(args.contains(MANIFEST_FLAG), "args: {args}");
    assert!(
        args.contains(&format!(
            "{}/scripts/../Cargo.toml",
            env!("CARGO_MANIFEST_DIR")
        )) && manifest.exists(),
        "args: {args}"
    );
}
