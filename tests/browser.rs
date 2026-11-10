// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::Duration;

use helios::browser::{open_with, pr_url};
use helios::model::PrKey;
use helios::proc::ProcError;

const TIMEOUT: Duration = Duration::from_secs(5);

fn key(host: &str, repo: &str) -> PrKey {
    PrKey {
        host: host.into(),
        account: "octo".into(),
        repo: repo.into(),
        number: 42,
    }
}

#[test]
fn a_pull_request_url_is_https_with_host_repo_and_number() {
    // Act
    let github = pr_url(&key("github.com", "org/repo.rs"));
    let enterprise = pr_url(&key("ghe.example.com:8443", "my-org/my_repo"));

    // Assert
    assert_eq!(
        github.as_deref(),
        Some("https://github.com/org/repo.rs/pull/42")
    );
    assert_eq!(
        enterprise.as_deref(),
        Some("https://ghe.example.com:8443/my-org/my_repo/pull/42")
    );
}

#[test]
fn a_host_or_repo_that_is_not_plain_gives_no_url() {
    for (host, repo) in [
        ("github.com/evil", "org/repo"),
        ("user@github.com", "org/repo"),
        ("github.com ", "org/repo"),
        ("", "org/repo"),
        ("github.com:", "org/repo"),
        ("github.com:80a", "org/repo"),
        ("github..com", "org/repo"),
        ("github.com", "org"),
        ("github.com", "org/repo/extra"),
        ("github.com", "../repo"),
        ("github.com", "org/.."),
        ("github.com", "org/re po"),
        ("github.com", "org/repo?x=1"),
        ("github.com", "/repo"),
    ] {
        assert_eq!(pr_url(&key(host, repo)), None, "{host:?} {repo:?}");
    }
}

#[test]
fn only_an_https_address_reaches_the_opener() {
    for url in [
        "http://github.com/o/r/pull/1",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "github.com/o/r/pull/1",
        "-a Calculator",
    ] {
        // Act
        let result = open_with(Path::new("true"), url, TIMEOUT);

        // Assert
        assert!(matches!(result, Err(ProcError::Io { .. })), "{url}");
    }
}

#[test]
fn the_opener_runs_and_a_missing_one_is_reported() {
    // Act
    let ran = open_with(Path::new("true"), "https://github.com/o/r/pull/1", TIMEOUT);
    let missing = open_with(
        Path::new("helios-no-such-opener"),
        "https://github.com/o/r/pull/1",
        TIMEOUT,
    );

    // Assert
    assert!(ran.is_ok());
    assert!(matches!(missing, Err(ProcError::NotFound)));
}
