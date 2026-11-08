// SPDX-License-Identifier: Apache-2.0

//! Lists the accounts `gh` is signed in to.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::proc::{self, ProcError};
use crate::secret::Secret;

/// `gh auth status` output is a few KiB at most.
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// `gh auth token` prints one short line.
const MAX_TOKEN_OUTPUT_BYTES: usize = 4096;

/// The state `gh auth status` reports for an account.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AccountState {
    Success,
    Timeout,
    Error,
    /// A state this version of forgebird does not know.
    Unknown(String),
}

impl AccountState {
    fn from_gh(state: String) -> Self {
        match state.as_str() {
            "success" => Self::Success,
            "timeout" => Self::Timeout,
            "error" => Self::Error,
            _ => Self::Unknown(state),
        }
    }
}

/// One account entry from `gh auth status --json hosts`.
///
/// The token `gh` can include in its output is deliberately not modelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub host: String,
    pub login: String,
    pub active: bool,
    pub scopes: Vec<String>,
    pub state: AccountState,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AccountsError {
    /// Carries no message text: serde_json echoes offending input values in it.
    #[error("could not parse gh auth status output ({category}) at line {line} column {column}")]
    Parse {
        category: &'static str,
        line: usize,
        column: usize,
    },
    #[error("gh printed output that is not valid text")]
    NotText,
    #[error("gh returned an empty or non-text token")]
    UnusableToken,
    #[error("{}", describe_run_failure(.0))]
    Proc(#[from] ProcError),
}

fn describe_run_failure(error: &ProcError) -> String {
    match error {
        ProcError::NotFound => "gh not found".to_owned(),
        other => format!("running gh failed: {other}"),
    }
}

impl AccountsError {
    fn from_json(error: serde_json::Error) -> Self {
        let category = match error.classify() {
            serde_json::error::Category::Io => "io",
            serde_json::error::Category::Syntax => "syntax",
            serde_json::error::Category::Data => "data",
            serde_json::error::Category::Eof => "unexpected end of input",
        };
        Self::Parse {
            category,
            line: error.line(),
            column: error.column(),
        }
    }
}

#[derive(Deserialize)]
struct Status {
    hosts: BTreeMap<String, Vec<Entry>>,
}

#[derive(Deserialize)]
struct Entry {
    state: String,
    active: bool,
    host: String,
    login: String,
    #[serde(default)]
    scopes: String,
}

impl From<Entry> for Account {
    fn from(entry: Entry) -> Self {
        Self {
            host: entry.host,
            login: entry.login,
            active: entry.active,
            scopes: entry
                .scopes
                .split(',')
                .map(str::trim)
                .filter(|scope| !scope.is_empty())
                .map(str::to_owned)
                .collect(),
            state: AccountState::from_gh(entry.state),
        }
    }
}

/// Parses `gh auth status --json hosts` output into accounts.
///
/// Accounts are ordered by host name, then in the order `gh` lists them.
pub fn parse_accounts(json: &str) -> Result<Vec<Account>, AccountsError> {
    let status: Status = serde_json::from_str(json).map_err(AccountsError::from_json)?;
    Ok(status
        .hosts
        .into_values()
        .flatten()
        .map(Account::from)
        .collect())
}

/// Runs `program auth status --json hosts` and parses its output.
pub fn list(program: &Path, timeout: Duration) -> Result<Vec<Account>, AccountsError> {
    let output = proc::run(
        program,
        &["auth", "status", "--json", "hosts"],
        timeout,
        MAX_OUTPUT_BYTES,
    )?;
    // The error type holds the bytes, so it is dropped unread.
    let text = std::str::from_utf8(&output.stdout).map_err(|_| AccountsError::NotText)?;
    parse_accounts(text)
}

/// Runs `program auth token --hostname <host> --user <login>` and returns its output as a secret.
pub fn token(
    program: &Path,
    host: &str,
    login: &str,
    timeout: Duration,
) -> Result<Secret, AccountsError> {
    let output = proc::run(
        program,
        &["auth", "token", "--hostname", host, "--user", login],
        timeout,
        MAX_TOKEN_OUTPUT_BYTES,
    )?;
    // from_utf8 reuses the buffer; its error type holds the bytes, so it is dropped unread.
    let mut text = String::from_utf8(output.stdout).map_err(|_| AccountsError::UnusableToken)?;
    text.truncate(text.trim_end().len());
    if text.is_empty() {
        return Err(AccountsError::UnusableToken);
    }
    Ok(Secret::new(text))
}
