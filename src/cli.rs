// SPDX-License-Identifier: Apache-2.0

//! Command-line flags and environment, parsed from explicit inputs.

use std::collections::HashMap;
use std::path::PathBuf;

use thiserror::Error;

use crate::app::refresh::REFRESH_INTERVAL;
use crate::config::theme_named;

pub const REFRESH_SECS_MIN: u64 = 30;
pub const REFRESH_SECS_MAX: u64 = 3600;

const FLAG_PREFIX: &str = "--";
const HOSTS_FLAG: &str = "--hosts";
const REFRESH_FLAG: &str = "--refresh-secs";
const THEME_FLAG: &str = "--theme";
const THEME_ENV: &str = "HELIOS_THEME";
const DRAFTS_FLAG: &str = "--drafts";
const SMOKE_FLAG: &str = "--smoke";
const VERSION_FLAG: &str = "--version";
const VERSION_SHORT_FLAG: &str = "-V";
const HELP_FLAG: &str = "--help";
const HOSTS_ENV: &str = "HELIOS_HOSTS";
const REFRESH_ENV: &str = "HELIOS_REFRESH_SECS";
const GH_ENV: &str = "HELIOS_GH";
const DEFAULT_GH_PROGRAM: &str = "gh";
const TEST_PANIC_ENV: &str = "HELIOS_TEST_PANIC";
const FAKE_ACCOUNTS_ENV: &str = "HELIOS_FAKE_ACCOUNTS";
const HOST_SEPARATOR: char = ',';

/// Settings resolved from flags and environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub hosts: Vec<String>,
    pub refresh_secs: u64,
    pub drafts: bool,
    pub smoke: bool,
    /// Set to the crate version when `--version` was given.
    pub version: Option<&'static str>,
    pub help: bool,
    pub gh_program: PathBuf,
    /// The theme name from `--theme`, already checked.
    pub theme: Option<String>,
    /// The theme name from `HELIOS_THEME`, already checked.
    pub theme_env: Option<String>,
}

/// Test switches read from the environment. Every field is false in a release build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugHooks {
    pub panic_during_draw: bool,
    pub fake_accounts: bool,
}

/// Reads the test switches from `env`; a release build ignores them.
pub fn debug_hooks(env: &HashMap<String, String>) -> DebugHooks {
    let enabled = cfg!(debug_assertions);
    DebugHooks {
        panic_during_draw: enabled && env.get(TEST_PANIC_ENV).is_some_and(|value| value == "1"),
        fake_accounts: enabled && env.contains_key(FAKE_ACCOUNTS_ENV),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CliError {
    #[error("refresh interval {0} is outside {REFRESH_SECS_MIN} to {REFRESH_SECS_MAX} seconds")]
    RefreshSecsOutOfRange(u64),
    #[error("{name} needs a whole number of seconds")]
    RefreshSecsNotNumber { name: &'static str },
    #[error("{0} needs a value")]
    MissingValue(&'static str),
    #[error("{name} names an unknown theme `{value}`")]
    UnknownTheme { name: &'static str, value: String },
    #[error("unknown argument {0}")]
    UnknownArgument(String),
}

/// Parses `args` (without the program name) over the given environment.
/// A flag wins over its environment variable.
pub fn parse(args: &[String], env: &HashMap<String, String>) -> Result<Config, CliError> {
    let mut hosts = env.get(HOSTS_ENV).map(|raw| split_hosts(raw));
    let mut refresh = None;
    let mut config = Config {
        hosts: Vec::new(),
        refresh_secs: REFRESH_INTERVAL.as_secs(),
        drafts: false,
        smoke: false,
        version: None,
        help: false,
        gh_program: env
            .get(GH_ENV)
            .map_or_else(|| PathBuf::from(DEFAULT_GH_PROGRAM), PathBuf::from),
        theme: None,
        theme_env: None,
    };
    let mut theme = None;

    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            HOSTS_FLAG => {
                let raw = flag_value(&mut rest, HOSTS_FLAG)?;
                hosts = Some(split_hosts(raw));
            }
            REFRESH_FLAG => {
                let raw = flag_value(&mut rest, REFRESH_FLAG)?;
                refresh = Some(parse_refresh(raw, REFRESH_FLAG)?);
            }
            THEME_FLAG => {
                let raw = flag_value(&mut rest, THEME_FLAG)?;
                theme = Some(checked_theme(raw, THEME_FLAG)?);
            }
            DRAFTS_FLAG => config.drafts = true,
            SMOKE_FLAG => config.smoke = true,
            VERSION_FLAG | VERSION_SHORT_FLAG => config.version = Some(env!("CARGO_PKG_VERSION")),
            HELP_FLAG => config.help = true,
            other => return Err(CliError::UnknownArgument(other.to_string())),
        }
    }

    config.hosts = hosts.unwrap_or_default();
    let informational = config.help || config.version.is_some();
    if refresh.is_none() && !informational {
        refresh = env
            .get(REFRESH_ENV)
            .map(|raw| parse_refresh(raw, REFRESH_ENV))
            .transpose()?;
    }
    config.refresh_secs = refresh.unwrap_or(config.refresh_secs);
    if !informational {
        config.theme_env = env
            .get(THEME_ENV)
            .map(|raw| checked_theme(raw, THEME_ENV))
            .transpose()?;
    }
    config.theme = theme;
    Ok(config)
}

/// The value after `flag`; a following flag means the value is missing.
fn flag_value<'a>(
    rest: &mut std::slice::Iter<'a, String>,
    flag: &'static str,
) -> Result<&'a String, CliError> {
    rest.next()
        .filter(|value| !value.starts_with(FLAG_PREFIX))
        .ok_or(CliError::MissingValue(flag))
}

impl Config {
    /// Whether `host` is shown: every host when `--hosts` is not set.
    pub fn includes_host(&self, host: &str) -> bool {
        self.hosts.is_empty() || self.hosts.iter().any(|known| known == host)
    }
}

/// The usage text `--help` prints.
pub fn help() -> String {
    format!(
        "Usage: helios [options]\n\
         \n\
         Options:\n\
         \x20 {HOSTS_FLAG} <hosts>        Comma-separated hosts to show (env {HOSTS_ENV})\n\
         \x20 {REFRESH_FLAG} <secs>  Refresh interval, {REFRESH_SECS_MIN} to {REFRESH_SECS_MAX} (env {REFRESH_ENV})\n\
         \x20 {THEME_FLAG} <name>        Start theme: default, spectrum, dark, light or mono (env {THEME_ENV})\n\
         \x20 {DRAFTS_FLAG}               Show draft pull requests\n\
         \x20 {SMOKE_FLAG}                Print the Mine and Needs me counts and exit\n\
         \x20 {VERSION_FLAG}              Print the version and exit\n\
         \x20 {HELP_FLAG}                 Print this help and exit\n\
         \n\
         Environment:\n\
         \x20 {GH_ENV}  The gh program to run (default {DEFAULT_GH_PROGRAM})\n"
    )
}

fn split_hosts(raw: &str) -> Vec<String> {
    raw.split(HOST_SEPARATOR)
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(str::to_string)
        .collect()
}

fn checked_theme(raw: &str, name: &'static str) -> Result<String, CliError> {
    theme_named(raw)
        .map(|theme| theme.name().to_string())
        .ok_or_else(|| CliError::UnknownTheme {
            name,
            value: raw.to_string(),
        })
}

fn parse_refresh(raw: &str, name: &'static str) -> Result<u64, CliError> {
    let secs: u64 = raw
        .trim()
        .parse()
        .map_err(|_| CliError::RefreshSecsNotNumber { name })?;
    if (REFRESH_SECS_MIN..=REFRESH_SECS_MAX).contains(&secs) {
        Ok(secs)
    } else {
        Err(CliError::RefreshSecsOutOfRange(secs))
    }
}
