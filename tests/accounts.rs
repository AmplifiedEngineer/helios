// SPDX-License-Identifier: Apache-2.0

// The fake gh is a sh script.
#![cfg(unix)]

use helios::accounts::{list, parse_accounts, Account, AccountState, AccountsError};
use helios::proc::{self, ProcError};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

#[path = "support/fake_gh.rs"]
mod fake_gh;
use fake_gh::{FakeGh, NEXT_DIR};

/// Short on purpose: the blocking gh test waits this long before the kill.
const BLOCKING_TIMEOUT: Duration = Duration::from_millis(200);
/// Room for a slow process start in every test that expects gh to finish.
const TIMEOUT: Duration = Duration::from_secs(5);
/// Far above the grace wait and far below the 30 s a stray sleep lasts.
const PROMPT_RETURN: Duration = Duration::from_secs(10);
/// A timeout that no test should ever reach.
const NEVER: Duration = Duration::from_secs(60);
/// Room for process start and teardown on a loaded machine.
const TIMEOUT_SLACK: Duration = Duration::from_millis(1500);

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/status")
        .join(name)
}

fn process_is_alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .output()
        .expect("run kill")
        .status
        .success()
}

fn fixture(name: &str) -> String {
    fs::read_to_string(fixture_path(name)).expect("read fixture")
}

fn find<'a>(accounts: &'a [Account], host: &str, login: &str) -> &'a Account {
    accounts
        .iter()
        .find(|a| a.host == host && a.login == login)
        .unwrap_or_else(|| panic!("no account {login} on {host} in {accounts:?}"))
}

#[test]
fn parses_two_hosts_and_two_accounts_on_one_host() {
    // Arrange
    let json = fixture("synthetic-two-accounts.json");

    // Act
    let accounts = parse_accounts(&json).expect("parse");

    // Assert
    assert_eq!(accounts.len(), 3);
    let on_github = accounts.iter().filter(|a| a.host == "github.com").count();
    assert_eq!(on_github, 2);
    let octocat = find(&accounts, "github.com", "octocat");
    assert!(octocat.active);
    assert_eq!(octocat.scopes, vec!["repo", "read:org", "workflow"]);
    assert_eq!(octocat.state, AccountState::Success);
    let hubot = find(&accounts, "github.com", "hubot");
    assert!(!hubot.active);
    assert_eq!(hubot.scopes, vec!["repo"]);
    assert_eq!(
        find(&accounts, "ghe.example.com", "octocat-ghe").host,
        "ghe.example.com"
    );
}

#[test]
fn parses_the_recorded_one_account_fixture() {
    // Arrange
    let json = fixture("recorded-one-account.json");

    // Act
    let accounts = parse_accounts(&json).expect("parse");

    // Assert
    assert_eq!(accounts.len(), 1);
    let account = &accounts[0];
    assert_eq!(account.host, "github.com");
    assert_eq!(account.login, "octocat");
    assert!(account.active);
    assert_eq!(account.state, AccountState::Success);
    assert_eq!(account.scopes, vec!["repo", "read:org"]);
}

#[test]
fn parses_the_timeout_state_and_keeps_an_unknown_one() {
    // Arrange
    let json = r#"{"hosts":{"github.com":[
        {"state":"timeout","active":true,"host":"github.com","login":"octocat"},
        {"state":"somethingnew","active":false,"host":"github.com","login":"hubot"}
    ]}}"#;

    // Act
    let accounts = parse_accounts(json).expect("parse");

    // Assert
    assert_eq!(
        find(&accounts, "github.com", "octocat").state,
        AccountState::Timeout
    );
    assert_eq!(
        find(&accounts, "github.com", "hubot").state,
        AccountState::Unknown("somethingnew".to_string())
    );
}

#[test]
fn a_non_success_account_stays_listed_with_its_state() {
    // Arrange
    let json = fixture("synthetic-two-accounts.json");

    // Act
    let accounts = parse_accounts(&json).expect("parse");

    // Assert
    let broken = find(&accounts, "ghe.example.com", "octocat-ghe");
    assert_eq!(broken.state, AccountState::Error);
    assert!(broken.scopes.is_empty());
}

#[test]
fn bad_json_gives_a_parse_error() {
    // Arrange
    let json = "{not json";

    // Act
    let result = parse_accounts(json);

    // Assert
    assert!(
        matches!(result, Err(AccountsError::Parse { .. })),
        "{result:?}"
    );
}

#[test]
fn a_token_field_in_the_output_is_ignored() {
    // Arrange
    let secret = format!("{}_{}", "gho", "s3cr3t".repeat(4));
    let json = format!(
        r#"{{"hosts":{{"github.com":[{{"state":"success","active":true,"host":"github.com","login":"octocat","tokenSource":"keyring","token":"{secret}","scopes":"repo","gitProtocol":"https"}}]}}}}"#
    );

    // Act
    let accounts = parse_accounts(&json).expect("parse");

    // Assert
    assert_eq!(accounts.len(), 1);
    assert!(!format!("{accounts:?}").contains(&secret));
}

#[test]
fn a_parse_error_never_echoes_the_input() {
    // Arrange
    let sentinel = format!("{}_{}", "sentinel", "zq9x".repeat(3));
    let json = format!(
        r#"{{"hosts":{{"github.com":[{{"state":"success","active":"{sentinel}","host":"github.com","login":"octocat"}}]}}}}"#
    );

    // Act
    let error = parse_accounts(&json).expect_err("wrong type must not parse");

    // Assert
    assert!(matches!(error, AccountsError::Parse { .. }), "{error:?}");
    for text in [
        error.to_string(),
        format!("{error:?}"),
        format!("{error:#}"),
        format!("{error:#?}"),
    ] {
        assert!(!text.contains(&sentinel), "leaked: {text}");
    }
}

#[test]
fn lists_accounts_from_a_fake_gh() {
    // Arrange
    let gh = FakeGh::printing(&fixture_path("synthetic-two-accounts.json"));

    // Act
    let accounts = list(&gh.path, TIMEOUT).expect("list");

    // Assert
    assert_eq!(accounts.len(), 3);
    assert_eq!(find(&accounts, "github.com", "hubot").scopes, vec!["repo"]);
}

#[test]
fn no_login_gives_an_empty_list() {
    // Arrange
    let gh = FakeGh::new(r#"echo '{"hosts":{}}'"#);

    // Act
    let accounts = list(&gh.path, TIMEOUT).expect("list");

    // Assert
    assert!(accounts.is_empty());
}

#[test]
fn bad_json_from_gh_gives_a_parse_error() {
    // Arrange
    let gh = FakeGh::new("echo '{not json'");

    // Act
    let result = list(&gh.path, TIMEOUT);

    // Assert
    assert!(
        matches!(result, Err(AccountsError::Parse { .. })),
        "{result:?}"
    );
}

#[test]
fn a_blocking_gh_times_out() {
    // Arrange
    let pid_file_dir = std::env::temp_dir().join(format!(
        "helios-pid-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&pid_file_dir).expect("create pid dir");
    let pid_file = pid_file_dir.join("pid");
    let gh = FakeGh::new(&format!(
        "echo $$ > '{}'\nexec sleep 30",
        pid_file.display()
    ));

    // Act
    let started = Instant::now();
    let result = list(&gh.path, BLOCKING_TIMEOUT);
    let elapsed = started.elapsed();

    // Assert
    let pid = fs::read_to_string(&pid_file).unwrap_or_default();
    let alive = !pid.trim().is_empty() && process_is_alive(pid.trim());
    let _ = fs::remove_dir_all(&pid_file_dir);
    assert!(
        matches!(result, Err(AccountsError::Proc(ProcError::Timeout))),
        "{result:?}"
    );
    assert!(
        elapsed < BLOCKING_TIMEOUT + TIMEOUT_SLACK,
        "took {elapsed:?}"
    );
    assert!(!pid.trim().is_empty(), "the fake gh never started");
    assert!(!alive, "child {} is still running", pid.trim());
}

#[test]
fn output_over_the_cap_is_refused() {
    // Arrange
    let gh = FakeGh::new("yes helios | head -c 5000");

    // Act
    let result = proc::run(&gh.path, &[], TIMEOUT, 100);

    // Assert
    assert!(
        matches!(result, Err(ProcError::OutputTooLarge)),
        "{result:?}"
    );
}

#[test]
fn a_missing_gh_gives_not_found() {
    // Arrange
    let missing = std::env::temp_dir().join("helios-no-such-dir/gh");

    // Act
    let result = list(&missing, TIMEOUT);

    // Assert
    let error = result.expect_err("a missing program must fail");
    assert!(
        matches!(error, AccountsError::Proc(ProcError::NotFound)),
        "{error:?}"
    );
    assert!(error.to_string().contains("gh not found"), "{error}");
}

#[test]
fn a_non_zero_exit_is_an_error() {
    // Arrange
    let gh = FakeGh::new("exit 3");

    // Act
    let result = list(&gh.path, TIMEOUT);

    // Assert
    assert!(
        matches!(result, Err(AccountsError::Proc(ProcError::Failed { .. }))),
        "{result:?}"
    );
}

#[test]
fn a_failing_gh_never_leaks_stderr() {
    // Arrange
    let sentinel = format!("{}_{}", "sentinel", "k7vw".repeat(3));
    let gh = FakeGh::new(&format!("echo {sentinel} >&2\nexit 1"));

    // Act
    let error = list(&gh.path, TIMEOUT).expect_err("a failing gh must fail");

    // Assert
    assert!(
        matches!(error, AccountsError::Proc(ProcError::Failed { .. })),
        "{error:?}"
    );
    for text in [
        error.to_string(),
        format!("{error:?}"),
        format!("{error:#}"),
        format!("{error:#?}"),
    ] {
        assert!(!text.contains(&sentinel), "leaked: {text}");
    }
}

#[test]
fn a_grandchild_holding_the_pipes_does_not_hang_run() {
    // Arrange
    let gh = FakeGh::new("sleep 30 &\nexit 0");

    // Act
    let started = Instant::now();
    let result = proc::run(&gh.path, &[], TIMEOUT, 1024);
    let elapsed = started.elapsed();

    // Assert
    assert!(matches!(result, Err(ProcError::Timeout)), "{result:?}");
    assert!(elapsed < PROMPT_RETURN, "took {elapsed:?}");
}

#[test]
fn endless_stdout_hits_the_cap_long_before_the_timeout() {
    // Arrange
    let gh = FakeGh::new("exec yes");

    // Act
    let started = Instant::now();
    let result = proc::run(&gh.path, &[], NEVER, 100);
    let elapsed = started.elapsed();

    // Assert
    assert!(
        matches!(result, Err(ProcError::OutputTooLarge)),
        "{result:?}"
    );
    assert!(elapsed < PROMPT_RETURN, "took {elapsed:?}");
}

#[test]
fn endless_stderr_hits_the_cap_long_before_the_timeout() {
    // Arrange
    let gh = FakeGh::new("exec yes >&2");

    // Act
    let started = Instant::now();
    let result = proc::run(&gh.path, &[], NEVER, 100);
    let elapsed = started.elapsed();

    // Assert
    assert!(
        matches!(result, Err(ProcError::OutputTooLarge)),
        "{result:?}"
    );
    assert!(elapsed < PROMPT_RETURN, "took {elapsed:?}");
}

#[test]
fn non_text_output_from_gh_is_an_error_without_the_bytes() {
    // Arrange
    let gh = FakeGh::new(r"printf '\377'");

    // Act
    let error = list(&gh.path, TIMEOUT).expect_err("non-text output must fail");

    // Assert
    assert!(matches!(error, AccountsError::NotText), "{error:?}");
    assert!(!error.to_string().contains('\u{FFFD}'), "{error}");
}
