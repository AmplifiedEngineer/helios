// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use helios::app::refresh::REFRESH_INTERVAL;
use helios::cli::{self, Config, REFRESH_SECS_MAX, REFRESH_SECS_MIN};
use helios::tui::keys::BINDINGS;

#[path = "support/fake_gh.rs"]
mod fake_gh;
use fake_gh::FakeGh;

const REFRESH_FLAG: &str = "--refresh-secs";
const HOSTS_FLAG: &str = "--hosts";
const REFRESH_ENV: &str = "HELIOS_REFRESH_SECS";
const HOSTS_ENV: &str = "HELIOS_HOSTS";

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_string()).collect()
}

fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn parse_refresh(value: u64) -> Result<Config, cli::CliError> {
    cli::parse(&args(&[REFRESH_FLAG, &value.to_string()]), &env(&[]))
}

#[test]
fn refresh_secs_bounds() {
    // Arrange
    let accepted = [REFRESH_SECS_MIN, REFRESH_SECS_MAX];
    let rejected = [REFRESH_SECS_MIN - 1, REFRESH_SECS_MAX + 1, 0];

    // Act
    let accepted_results: Vec<_> = accepted.iter().map(|v| parse_refresh(*v)).collect();
    let rejected_results: Vec<_> = rejected.iter().map(|v| parse_refresh(*v)).collect();

    // Assert
    for (value, result) in accepted.iter().zip(&accepted_results) {
        assert_eq!(
            result.as_ref().map(|config| config.refresh_secs),
            Ok(*value),
            "{value} should be accepted"
        );
    }
    for (value, result) in rejected.iter().zip(&rejected_results) {
        assert_eq!(
            result,
            &Err(cli::CliError::RefreshSecsOutOfRange(*value)),
            "{value} should be rejected"
        );
    }
}

#[test]
fn flags_beat_environment() {
    // Arrange
    let environment = env(&[(REFRESH_ENV, "100"), (HOSTS_ENV, "env.example.com")]);
    let from_flags = args(&[REFRESH_FLAG, "200", HOSTS_FLAG, "flag.example.com"]);

    // Act
    let with_flags = cli::parse(&from_flags, &environment);
    let env_only = cli::parse(&[], &environment);

    // Assert
    let with_flags = with_flags.expect("flags and environment parse");
    assert_eq!(with_flags.refresh_secs, 200);
    assert_eq!(with_flags.hosts, vec!["flag.example.com".to_string()]);
    let env_only = env_only.expect("environment alone parses");
    assert_eq!(env_only.refresh_secs, 100);
    assert_eq!(env_only.hosts, vec!["env.example.com".to_string()]);
}

const GH_ENV: &str = "HELIOS_GH";
const TEST_PANIC_ENV: &str = "HELIOS_TEST_PANIC";
const FAKE_ACCOUNTS_ENV: &str = "HELIOS_FAKE_ACCOUNTS";

fn rust_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("source directory reads") {
        let path = entry.expect("directory entry reads").path();
        if path.is_dir() {
            rust_sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn the_default_refresh_matches_the_scheduler_interval() {
    // Arrange
    let no_flags = args(&[]);
    let no_environment = env(&[]);

    // Act
    let config = cli::parse(&no_flags, &no_environment).expect("defaults parse");

    // Assert
    assert_eq!(
        Duration::from_secs(config.refresh_secs),
        REFRESH_INTERVAL,
        "the default refresh derives from the scheduler interval"
    );
}

#[test]
fn helios_gh_names_the_gh_program() {
    // Arrange
    let named = env(&[(GH_ENV, "/opt/fake/gh")]);
    let unnamed = env(&[]);

    // Act
    let with_env = cli::parse(&[], &named).expect("HELIOS_GH parses");
    let without_env = cli::parse(&[], &unnamed).expect("defaults parse");

    // Assert
    assert_eq!(with_env.gh_program, PathBuf::from("/opt/fake/gh"));
    assert_eq!(without_env.gh_program, PathBuf::from("gh"));
}

#[test]
fn helios_gh_is_read_only_in_cli() {
    // Arrange
    let mut sources = Vec::new();
    rust_sources(&source_root(), &mut sources);

    // Act
    let mut naming: Vec<_> = sources
        .iter()
        .filter(|path| {
            fs::read_to_string(path)
                .expect("source reads")
                .contains(GH_ENV)
        })
        .map(|path| path.file_name().expect("file name").to_owned())
        .collect();
    naming.sort();

    // Assert
    assert_eq!(naming, vec![std::ffi::OsString::from("cli.rs")]);
}

#[cfg(debug_assertions)]
#[test]
fn the_debug_hooks_come_from_the_environment() {
    // Arrange
    let hooked = env(&[(TEST_PANIC_ENV, "1"), (FAKE_ACCOUNTS_ENV, "1")]);
    let unhooked = env(&[]);

    // Act
    let with_hooks = cli::debug_hooks(&hooked);
    let without_hooks = cli::debug_hooks(&unhooked);

    // Assert
    assert!(with_hooks.panic_during_draw);
    assert!(with_hooks.fake_accounts);
    assert!(!without_hooks.panic_during_draw);
    assert!(!without_hooks.fake_accounts);
}

#[cfg(not(debug_assertions))]
#[test]
fn the_debug_hooks_are_off_in_a_release_build() {
    // Arrange
    let hooked = env(&[(TEST_PANIC_ENV, "1"), (FAKE_ACCOUNTS_ENV, "1")]);

    // Act
    let hooks = cli::debug_hooks(&hooked);

    // Assert
    assert!(!hooks.panic_during_draw);
    assert!(!hooks.fake_accounts);
}

#[test]
fn config_has_the_same_fields_in_every_build() {
    // Arrange
    let expected = Config {
        hosts: Vec::new(),
        refresh_secs: REFRESH_SECS_MIN,
        drafts: false,
        smoke: false,
        version: None,
        help: false,
        gh_program: PathBuf::from("gh"),
    };

    // Act
    let parsed = cli::parse(
        &args(&[REFRESH_FLAG, &REFRESH_SECS_MIN.to_string()]),
        &env(&[]),
    );

    // Assert
    assert_eq!(parsed, Ok(expected));
}

#[test]
fn the_public_tui_api_holds_no_panic_switch() {
    // Arrange
    let tui_mod = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/mod.rs");

    // Act
    let text = fs::read_to_string(tui_mod).expect("tui/mod.rs reads");

    // Assert
    assert!(!text.contains("panic!"), "tui/mod.rs holds a panic switch");
}

#[test]
fn main_no_longer_names_the_debug_variables() {
    // Arrange
    let main_rs = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");

    // Act
    let text = fs::read_to_string(main_rs).expect("main.rs reads");

    // Assert
    assert!(!text.contains(TEST_PANIC_ENV));
    assert!(!text.contains(FAKE_ACCOUNTS_ENV));
}

#[test]
fn version_carries_the_crate_version() {
    // Arrange
    let version_flag = args(&["--version"]);

    // Act
    let config = cli::parse(&version_flag, &env(&[])).expect("--version parses");

    // Assert
    assert_eq!(config.version, Some(env!("CARGO_PKG_VERSION")));
}

const HELP_FLAG: &str = "--help";
const SMOKE_FLAG: &str = "--smoke";
const KNOWN_FLAGS: [&str; 6] = [
    "--hosts",
    "--refresh-secs",
    "--drafts",
    "--smoke",
    "--version",
    "--help",
];
const NO_ACCOUNTS_JSON: &str = r#"echo '{"hosts":{}}'"#;
const NO_ACCOUNTS_MESSAGE: &str = "no accounts found";
const TOKEN_PREFIX: &str = "ghp";
const TOKEN_BODY: &str = "PlantedTokenForTheLeakCheck0123456789";

/// Built at runtime so no token-shaped literal sits in the source.
fn planted_token() -> String {
    format!("{TOKEN_PREFIX}_{TOKEN_BODY}")
}
const GH_FAILURE_MESSAGE: &str = "running gh failed";
const FAILURE_EXIT_CODE: i32 = 1;

/// Runs the real binary with a cleared environment holding only `HELIOS_GH`.
fn run_helios(arg: &str, gh: &Path) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_helios"))
        .arg(arg)
        .env_clear()
        .env(GH_ENV, gh)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("helios starts");
    drop(child.stdin.take());
    child.wait_with_output().expect("helios finishes")
}

/// Long flags in `text`: a `--` followed by lowercase letters and hyphens.
fn long_flags(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter_map(|word| {
            let start = word.find("--")?;
            let flag: String = word[start..]
                .chars()
                .take_while(|c| *c == '-' || c.is_ascii_lowercase())
                .collect();
            (flag.len() > 2).then_some(flag)
        })
        .collect()
}

#[test]
fn readme_flags_match_help() {
    // Arrange
    let readme = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md"))
        .expect("README reads");
    let no_gh = Path::new("gh");

    // Act
    let help = String::from_utf8(run_helios(HELP_FLAG, no_gh).stdout).expect("help is text");
    let listed = long_flags(&help);

    // Assert
    for flag in KNOWN_FLAGS {
        assert!(listed.iter().any(|f| f == flag), "--help lists {flag}");
    }
    for flag in &listed {
        assert!(readme.contains(flag.as_str()), "README names {flag}");
    }
    for binding in BINDINGS {
        assert!(
            readme.contains(binding.key),
            "README keys table names {}",
            binding.key
        );
    }
}

#[test]
fn smoke_through_the_binary_with_no_accounts_fails_cleanly() {
    // Arrange
    let gh = FakeGh::new(NO_ACCOUNTS_JSON);

    // Act
    let output = run_helios(SMOKE_FLAG, &gh.path);

    // Assert
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(FAILURE_EXIT_CODE));
    assert!(stdout.contains(NO_ACCOUNTS_MESSAGE), "stdout: {stdout}");
}

// The failing `gh` path needs no network: `gh auth status` fails after printing the token.
#[test]
fn smoke_through_the_binary_never_prints_a_token_when_gh_fails() {
    // Arrange
    let token = planted_token();
    let gh = FakeGh::new(&format!("echo '{token}'\necho '{token}' >&2\nexit 1"));

    // Act
    let output = run_helios(SMOKE_FLAG, &gh.path);

    // Assert
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(&token), "stdout: {stdout}");
    assert!(!stderr.contains(&token), "stderr: {stderr}");
    assert!(
        stderr.contains(GH_FAILURE_MESSAGE),
        "the smoke run reports the gh failure: {stderr}"
    );
}

#[test]
fn a_bad_refresh_env_value_is_ignored_when_the_flag_is_given() {
    // Arrange
    let environment = env(&[("HELIOS_REFRESH_SECS", "abc")]);

    // Act
    let flagged = cli::parse(&args(&[REFRESH_FLAG, "60"]), &environment);
    let helped = cli::parse(&args(&["--help"]), &environment);
    let bare = cli::parse(&args(&[]), &environment);

    // Assert
    assert_eq!(flagged.map(|c| c.refresh_secs), Ok(60));
    assert!(helped.is_ok(), "--help must not trip over the env value");
    assert!(bare.is_err(), "a bad env value alone is still an error");
}

#[test]
fn a_flag_is_never_taken_as_the_value_of_the_flag_before_it() {
    // Arrange
    let cases = [
        (
            vec!["--hosts", "--smoke"],
            cli::CliError::MissingValue("--hosts"),
        ),
        (
            vec!["--refresh-secs", "--drafts"],
            cli::CliError::MissingValue("--refresh-secs"),
        ),
    ];

    for (input, expected) in cases {
        // Act
        let result = cli::parse(&args(&input), &env(&[]));

        // Assert
        assert_eq!(result, Err(expected), "{input:?}");
    }
}

#[test]
fn an_empty_host_list_includes_every_host_and_a_set_list_only_its_hosts() {
    // Arrange
    let all = cli::parse(&args(&[]), &env(&[])).expect("defaults parse");
    let one = cli::parse(&args(&["--hosts", "ghe.example.com"]), &env(&[])).expect("flag parses");

    // Act
    let results = [
        all.includes_host("github.com"),
        one.includes_host("github.com"),
        one.includes_host("ghe.example.com"),
    ];

    // Assert
    assert_eq!(results, [true, false, true]);
}
