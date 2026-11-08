// SPDX-License-Identifier: Apache-2.0

//! Scans recorded and synthetic fixtures for credentials.
//!
//! A value counts as redacted, and so is clean, when it is empty or is one of
//! `[redacted]`, `<redacted>` or `REDACTED` (case-insensitive), optionally after
//! an auth scheme (`Bearer`, `token`, `Basic`). Any other value after `Bearer`
//! or an `Authorization` header or JSON field is a finding.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Shortest body after `github_pat_` that GitHub issues for a fine-grained token.
const GITHUB_PAT_MIN_BODY_LEN: usize = 22;
/// Exact body length after `ghp_`-style prefixes for a classic token.
const GITHUB_TOKEN_BODY_LEN: usize = 36;
const CLASSIC_TOKEN_PREFIXES: [&str; 5] = ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"];
/// A JSON `token` key, quotes included so `tokenSource` does not match.
const TOKEN_KEY: &str = "\"token\"";
const AUTH_SCHEMES: [&str; 3] = ["bearer", "token", "basic"];
const REDACTION_MARKERS: [&str; 3] = ["[redacted]", "<redacted>", "redacted"];

#[derive(Debug, PartialEq, Eq)]
struct Finding {
    path: PathBuf,
    line: usize,
    kind: &'static str,
}

/// Walks every file under `dir` and reports each credential found.
fn scan(dir: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    scan_dir(dir, &mut findings);
    findings
}

fn scan_dir(dir: &Path, findings: &mut Vec<Finding>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .expect("read fixture dir")
        .map(|entry| entry.expect("read dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            scan_dir(&path, findings);
        } else {
            let bytes = fs::read(&path).expect("read fixture file");
            let text = String::from_utf8_lossy(&bytes);
            for (index, line) in text.lines().enumerate() {
                for kind in scan_line(line) {
                    findings.push(Finding {
                        path: path.clone(),
                        line: index + 1,
                        kind,
                    });
                }
            }
        }
    }
}

fn scan_line(line: &str) -> Vec<&'static str> {
    let mut kinds = Vec::new();
    let fine_grained =
        github_token_starts(line, "github_pat_", GITHUB_PAT_MIN_BODY_LEN, true).count();
    kinds.extend(std::iter::repeat_n("github_pat", fine_grained));
    for prefix in CLASSIC_TOKEN_PREFIXES {
        let classic = github_token_starts(line, prefix, GITHUB_TOKEN_BODY_LEN, false).count();
        kinds.extend(std::iter::repeat_n("github_token", classic));
    }
    let lower = line.to_ascii_lowercase();
    for (at, _) in lower.match_indices("bearer ") {
        if !is_redacted(&value_at(&line[at + "bearer ".len()..])) {
            kinds.push("bearer");
        }
    }
    for (at, _) in lower.match_indices("authorization") {
        if let Some(value) = authorization_value(&line[at + "authorization".len()..]) {
            if !is_redacted(&value) {
                kinds.push("authorization");
            }
        }
    }
    for (at, _) in lower.match_indices(TOKEN_KEY) {
        if let Some(value) = authorization_value(&line[at + TOKEN_KEY.len() - 1..]) {
            if !is_redacted(&value) {
                kinds.push("token_field");
            }
        }
    }
    kinds
}

/// Yields each occurrence of `prefix` followed by at least `min_len` token
/// characters (alphanumerics, plus `_` when `allow_underscore`).
fn github_token_starts<'a>(
    line: &'a str,
    prefix: &'a str,
    min_len: usize,
    allow_underscore: bool,
) -> impl Iterator<Item = usize> + 'a {
    line.match_indices(prefix).filter_map(move |(at, _)| {
        let body = &line[at + prefix.len()..];
        let len = body
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || (allow_underscore && *c == '_'))
            .count();
        (len >= min_len).then_some(at)
    })
}

/// The value after an `Authorization` name, in a header (`: value`) or JSON
/// (`": "value"`) form; `None` when no separator follows the name.
fn authorization_value(rest: &str) -> Option<String> {
    let rest = rest.strip_prefix('"').unwrap_or(rest);
    let rest = rest.trim_start().strip_prefix(':')?;
    let rest = rest.trim_start().trim_start_matches('"');
    let end = rest.find('"').unwrap_or(rest.len());
    Some(rest[..end].trim().to_string())
}

/// The run of characters up to the next whitespace or quote.
fn value_at(rest: &str) -> String {
    rest.chars()
        .take_while(|c| !c.is_whitespace() && *c != '"' && *c != '\'')
        .collect()
}

/// Drops a leading `Bearer`, `token` or `Basic` scheme so only the credential
/// is judged; recordings keep the scheme and redact the rest.
fn strip_auth_scheme(value: &str) -> &str {
    let value = value.trim();
    let (scheme, rest) = value.split_once(char::is_whitespace).unwrap_or((value, ""));
    if AUTH_SCHEMES
        .iter()
        .any(|known| scheme.eq_ignore_ascii_case(known))
    {
        rest.trim_start()
    } else {
        value
    }
}

fn is_redacted(value: &str) -> bool {
    let value = strip_auth_scheme(value);
    value.is_empty()
        || REDACTION_MARKERS
            .iter()
            .any(|marker| value.eq_ignore_ascii_case(marker))
}

/// A unique temporary directory that is removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "helios-fixture-scan-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            label
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn write(&self, name: &str, body: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().expect("fixture has a parent")).expect("create subdir");
        fs::write(path, body).expect("write fixture");
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Planted credentials are assembled at runtime so this source never holds a
/// string that a secret scanner would flag.
fn planted_cases() -> Vec<(String, String)> {
    let classic_body = "0123456789abcdefghijklmnopqrstuvwxyz"; // GITHUB_TOKEN_BODY_LEN chars
    let mut cases: Vec<(String, String)> = ['p', 'o', 'u', 's', 'r']
        .into_iter()
        .map(|kind| {
            let token = format!("gh{kind}_{classic_body}");
            (format!("gh{kind}"), format!(r#"{{"note": "{token}"}}"#))
        })
        .collect();
    let fine_grained = ["github", "_pat_", "11ABCDEFG0123456789_abcdefghijklmnop"].concat();
    cases.push((
        "github_pat".into(),
        format!(r#"{{"note": "{fine_grained}"}}"#),
    ));
    cases.push(("bearer".into(), "curl -H 'Bearer abc123def456'\n".into()));
    cases.push((
        "authorization_header".into(),
        "Authorization: token abc123def456\n".into(),
    ));
    let plain_value = ["abc123", "def456"].concat();
    cases.push((
        "token_json_key".into(),
        format!(r#"{{"token": "{plain_value}"}}"#),
    ));
    cases.push((
        "authorization_json".into(),
        r#"{"Authorization": "abc123def456"}"#.into(),
    ));
    cases
}

#[test]
fn scan_finds_a_planted_token() {
    let mut missed = Vec::new();
    for (label, body) in planted_cases() {
        // Arrange
        let dir = TempDir::new(&label);
        dir.write("planted.json", &body);

        // Act
        let findings = scan(&dir.0);

        if findings.len() != 1 {
            missed.push(label);
        }
    }

    // Assert
    assert!(missed.is_empty(), "scan missed or duplicated: {missed:?}");
}

#[test]
fn scan_finds_a_token_in_a_nested_directory() {
    // Arrange
    let dir = TempDir::new("nested");
    let token = ["ghp", "_", "0123456789abcdefghijklmnopqrstuvwxyz"].concat();
    dir.write("clean.json", "{}\n");
    dir.write(
        "a/b/planted.json",
        &format!("{{}}\n{{\"note\": \"{token}\"}}"),
    );

    // Act
    let findings = scan(&dir.0);

    // Assert
    let expected = Finding {
        path: dir.0.join("a/b/planted.json"),
        line: 2,
        kind: "github_token",
    };
    assert_eq!(findings, vec![expected]);
}

#[test]
fn scan_passes_a_clean_fixture() {
    // Arrange
    let dir = TempDir::new("clean");
    dir.write(
        "clean.json",
        r#"{"token": "[redacted]", "authorization": "<redacted>", "Authorization": "REDACTED", "login": "octocat"}"#,
    );

    // Act
    let findings = scan(&dir.0);

    // Assert
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn scan_passes_a_redacted_credential_after_an_auth_scheme() {
    let bodies = [
        "Authorization: Bearer [redacted]\n",
        "Authorization: token [redacted]\n",
        "authorization: BASIC <redacted>\n",
        r#"{"Authorization": "Bearer [redacted]"}"#,
        r#"{"Authorization": "token REDACTED"}"#,
    ];
    for body in bodies {
        // Arrange
        let dir = TempDir::new("scheme");
        dir.write("recorded.txt", body);

        // Act
        let findings = scan(&dir.0);

        // Assert
        assert!(findings.is_empty(), "{body:?} flagged: {findings:?}");
    }
}

#[test]
fn scan_flags_a_real_looking_credential_after_an_auth_scheme() {
    // Arrange
    let dir = TempDir::new("scheme-leak");
    let credential = ["abc123", "def456", "ghi789"].concat();
    dir.write("leak.txt", &format!("Authorization: Bearer {credential}\n"));

    // Act
    let findings = scan(&dir.0);

    // Assert
    assert!(
        !findings.is_empty(),
        "credential after a scheme went unflagged"
    );
}

#[test]
fn real_fixtures_hold_no_credentials() {
    // Arrange
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");

    // Act
    let findings = scan(&dir);

    // Assert
    assert!(dir.is_dir(), "tests/fixtures is missing");
    assert!(
        fs::read_dir(&dir).expect("read fixtures").next().is_some(),
        "tests/fixtures is empty"
    );
    assert!(findings.is_empty(), "credentials in fixtures: {findings:?}");
}
