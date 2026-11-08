// SPDX-License-Identifier: Apache-2.0

// The fake gh is a sh script.
#![cfg(unix)]

use forgebird::accounts::{token, AccountsError};
use forgebird::secret::Secret;
use std::fs;
use std::time::Duration;

#[path = "support/fake_gh.rs"]
mod fake_gh;
use fake_gh::FakeGh;

/// Room for a slow process start in every test that expects gh to finish.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Built at runtime so no literal in this file looks like a real token.
fn sentinel() -> String {
    format!("{}_{}", "gho", "q4zx".repeat(9))
}

#[derive(Debug)]
#[allow(dead_code)]
struct Holder {
    name: &'static str,
    secret: Secret,
}

#[test]
fn sentinel_token_from_gh_stderr_never_reaches_any_formatting() {
    // Arrange
    let sentinel = sentinel();
    let gh = FakeGh::new(&format!("echo {sentinel} >&2\nexit 1"));
    let holder = Holder {
        name: "holder",
        secret: Secret::new(sentinel.clone()),
    };

    // Act
    let error =
        token(&gh.path, "github.com", "octocat", TIMEOUT).expect_err("a failing gh must fail");

    // Assert
    for text in [
        error.to_string(),
        format!("{error:?}"),
        format!("{error:#}"),
        format!("{error:#?}"),
        format!("{holder:?}"),
        format!("{holder:#?}"),
    ] {
        assert!(!text.contains(&sentinel), "leaked: {text}");
    }
}

#[test]
fn secret_debug_and_display_are_redacted() {
    // Arrange
    let secret = Secret::new(sentinel());

    // Act
    let debug = format!("{secret:?}");
    let display = format!("{secret}");

    // Assert
    assert_eq!(debug, "[redacted]");
    assert_eq!(display, "[redacted]");
}

#[test]
fn expose_returns_the_value() {
    // Arrange
    let value = sentinel();
    let secret = Secret::new(value.clone());

    // Act
    let exposed = secret.expose();

    // Assert
    assert_eq!(exposed, value);
}

#[test]
fn token_returns_the_trimmed_stdout_as_a_secret() {
    // Arrange
    let sentinel = sentinel();
    let gh = FakeGh::new(&format!("echo {sentinel}"));

    // Act
    let secret = token(&gh.path, "github.com", "octocat", TIMEOUT).expect("token");

    // Assert
    assert_eq!(secret.expose(), sentinel);
}

#[test]
fn token_passes_host_and_user_as_separate_arguments() {
    // Arrange
    let dir = std::env::temp_dir().join(format!("forgebird-argv-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create argv dir");
    let argv_file = dir.join("argv");
    let gh = FakeGh::new(&format!(
        "for a in \"$@\"; do echo \"$a\"; done > '{}'\necho {}",
        argv_file.display(),
        sentinel()
    ));

    // Act
    let result = token(&gh.path, "ghe.example.com", "octo cat", TIMEOUT);

    // Assert
    let argv = fs::read_to_string(&argv_file).unwrap_or_default();
    let _ = fs::remove_dir_all(&dir);
    result.expect("token");
    let lines: Vec<&str> = argv.lines().collect();
    assert_eq!(
        lines,
        [
            "auth",
            "token",
            "--hostname",
            "ghe.example.com",
            "--user",
            "octo cat"
        ]
    );
}

#[test]
fn token_refuses_output_that_is_empty_blank_or_not_text() {
    // Arrange
    let sentinel = sentinel();
    let scripts = [
        ("prints nothing", "true".to_string()),
        ("prints a newline", "echo".to_string()),
        ("prints invalid utf-8", r"printf '\377'".to_string()),
        (
            "prints invalid utf-8 after text",
            format!(r"printf '{sentinel}\377'"),
        ),
    ];

    for (label, script) in scripts {
        let gh = FakeGh::new(&script);

        // Act
        let result = token(&gh.path, "github.com", "octocat", TIMEOUT);

        // Assert
        let error = result.expect_err(label);
        assert!(matches!(error, AccountsError::UnusableToken), "{label}");
        for text in [error.to_string(), format!("{error:?}")] {
            assert!(!text.contains(&sentinel), "{label} leaked: {text}");
            assert!(!text.contains('\u{FFFD}'), "{label} leaked bytes: {text}");
        }
    }
}
