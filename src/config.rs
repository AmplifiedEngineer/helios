// SPDX-License-Identifier: Apache-2.0

//! The settings file: its model, defaults and parsing.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::RangeInclusive;

use serde::Deserialize;

use crate::test_files::{DEFAULT_DIRECTORIES, DEFAULT_FILE_PATTERNS};
use crate::theme::Theme;

pub const DEFAULT_LIST_PERCENT: u16 = 40;
pub const DEFAULT_NARROW_BELOW: u16 = 100;
pub const DEFAULT_PREVIEW_DELAY_MS: u64 = 300;
pub const DEFAULT_COLLAPSED: bool = true;

pub const LIST_PERCENT_BOUNDS: RangeInclusive<u16> = 20..=70;
pub const NARROW_BELOW_BOUNDS: RangeInclusive<u16> = 40..=400;
pub const PREVIEW_DELAY_MS_BOUNDS: RangeInclusive<u64> = 0..=5000;

/// Column names in screen order, which is also the default for both lists.
pub const COLUMN_NAMES: [&str; 10] = [
    "host", "repo", "number", "title", "author", "tag", "draft", "review", "checks", "updated",
];

pub const LIST_ACTIONS: [&str; 13] = [
    "move_down",
    "move_up",
    "first",
    "last",
    "open",
    "toggle_view",
    "refresh",
    "filter",
    "toggle_drafts",
    "theme",
    "open_in_browser",
    "help",
    "quit",
];
pub const DETAIL_ACTIONS: [&str; 11] = [
    "scroll_down",
    "scroll_up",
    "first",
    "last",
    "back",
    "next_tab",
    "previous_tab",
    "theme",
    "open_in_browser",
    "help",
    "quit",
];
pub const FILES_ACTIONS: [&str; 16] = [
    "move_down",
    "move_up",
    "first",
    "last",
    "toggle_file",
    "open_all",
    "close_all",
    "toggle_tests",
    "full_diff",
    "back",
    "next_tab",
    "previous_tab",
    "theme",
    "open_in_browser",
    "help",
    "quit",
];
pub const DIFF_ACTIONS: [&str; 9] = [
    "scroll_down",
    "scroll_up",
    "first",
    "last",
    "back",
    "theme",
    "open_in_browser",
    "help",
    "quit",
];

/// Each scope name with the action names valid in it.
pub const SCOPE_ACTIONS: [(&str, &[&str]); 4] = [
    ("list", &LIST_ACTIONS),
    ("detail", &DETAIL_ACTIONS),
    ("files", &FILES_ACTIONS),
    ("diff", &DIFF_ACTIONS),
];

/// Key names written out in full, in their canonical spelling.
pub const NAMED_KEYS: [&str; 10] = [
    "Enter",
    "Space",
    "Tab",
    "Shift-Tab",
    "Up",
    "Down",
    "Esc",
    "Backspace",
    "Home",
    "End",
];
const CTRL_PREFIX: &str = "Ctrl-";

/// Keys that keep their built-in meaning and cannot be rebound.
pub const RESERVED_KEYS: [&str; 4] = ["q", "Ctrl-C", "?", "Esc"];

/// The settings, one field per file section.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Config {
    pub layout: LayoutConfig,
    pub columns: ColumnsConfig,
    pub keys: KeysConfig,
    pub theme: ThemeConfig,
    pub files: FilesConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutConfig {
    pub list_percent: u16,
    pub narrow_below: u16,
    pub preview_delay_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnsConfig {
    pub mine: Vec<String>,
    pub needs_me: Vec<String>,
}

/// Scope name to action name to the canonical keys bound to it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeysConfig {
    pub scopes: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeConfig {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesConfig {
    pub test_dirs: Vec<String>,
    pub test_files: Vec<String>,
    pub collapsed: bool,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            list_percent: DEFAULT_LIST_PERCENT,
            narrow_below: DEFAULT_NARROW_BELOW,
            preview_delay_ms: DEFAULT_PREVIEW_DELAY_MS,
        }
    }
}

impl Default for ColumnsConfig {
    fn default() -> Self {
        Self {
            mine: names(&COLUMN_NAMES),
            needs_me: names(&COLUMN_NAMES),
        }
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            name: Theme::default().name().to_string(),
        }
    }
}

impl Default for FilesConfig {
    fn default() -> Self {
        Self {
            test_dirs: names(&DEFAULT_DIRECTORIES),
            test_files: names(&DEFAULT_FILE_PATTERNS),
            collapsed: DEFAULT_COLLAPSED,
        }
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_string()).collect()
}

/// Why a settings file was rejected, with where in the file when known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub message: String,
    pub key: Option<String>,
    /// 1-based.
    pub line: Option<usize>,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        match (&self.key, self.line) {
            (Some(key), Some(line)) => write!(f, " (key {key}, line {line})"),
            (Some(key), None) => write!(f, " (key {key})"),
            (None, Some(line)) => write!(f, " (line {line})"),
            (None, None) => Ok(()),
        }
    }
}

impl std::error::Error for ConfigError {}

/// The file as serde reads it: shape only, with every value optional.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct RawConfig {
    layout: RawLayout,
    columns: RawColumns,
    keys: RawKeys,
    theme: RawTheme,
    files: RawFiles,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct RawLayout {
    list_percent: Option<i64>,
    narrow_below: Option<i64>,
    preview_delay_ms: Option<i64>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct RawColumns {
    mine: Option<Vec<String>>,
    needs_me: Option<Vec<String>>,
}

type RawActions = BTreeMap<String, Vec<String>>;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct RawKeys {
    list: Option<RawActions>,
    detail: Option<RawActions>,
    files: Option<RawActions>,
    diff: Option<RawActions>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct RawTheme {
    name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct RawFiles {
    test_dirs: Option<Vec<String>>,
    test_files: Option<Vec<String>>,
    collapsed: Option<bool>,
}

/// Reads and checks a settings file; any problem is an error naming what and where.
pub fn parse(text: &str) -> Result<Config, ConfigError> {
    let raw: RawConfig = toml::from_str(text).map_err(|error| ConfigError {
        message: error.message().to_string(),
        key: None,
        line: error.span().map(|span| line_at(text, span.start)),
    })?;
    let source = Source(text);
    Ok(Config {
        layout: layout(&raw.layout, &source)?,
        columns: columns(&raw.columns, &source)?,
        keys: keys(raw.keys, &source)?,
        theme: theme(&raw.theme, &source)?,
        files: files(raw.files),
    })
}

fn line_at(text: &str, offset: usize) -> usize {
    text.get(..offset)
        .map_or(1, |head| head.matches('\n').count() + 1)
}

/// The settings text, used to find the line of a key that serde already accepted.
struct Source<'a>(&'a str);

impl Source<'_> {
    /// An error for `leaf` inside `table`; the line is the first line in that table that starts with the key.
    fn error(&self, table: &str, leaf: &str, message: String) -> ConfigError {
        let key = if table.is_empty() {
            leaf.to_string()
        } else {
            format!("{table}.{leaf}")
        };
        ConfigError {
            message,
            key: Some(key),
            line: self.line_of(table, leaf),
        }
    }

    fn line_of(&self, table: &str, leaf: &str) -> Option<usize> {
        let mut current = "";
        for (index, line) in self.0.lines().enumerate() {
            let trimmed = line.trim();
            let header = trimmed
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'));
            if let Some(name) = header {
                current = name.trim();
            } else if current == table
                && trimmed
                    .strip_prefix(leaf)
                    .is_some_and(|rest| rest.trim_start().starts_with('='))
            {
                return Some(index + 1);
            }
        }
        None
    }
}

fn layout(raw: &RawLayout, source: &Source) -> Result<LayoutConfig, ConfigError> {
    Ok(LayoutConfig {
        list_percent: bounded(
            raw.list_percent,
            DEFAULT_LIST_PERCENT,
            LIST_PERCENT_BOUNDS,
            ("layout", "list_percent"),
            source,
        )?,
        narrow_below: bounded(
            raw.narrow_below,
            DEFAULT_NARROW_BELOW,
            NARROW_BELOW_BOUNDS,
            ("layout", "narrow_below"),
            source,
        )?,
        preview_delay_ms: bounded(
            raw.preview_delay_ms,
            DEFAULT_PREVIEW_DELAY_MS,
            PREVIEW_DELAY_MS_BOUNDS,
            ("layout", "preview_delay_ms"),
            source,
        )?,
    })
}

fn bounded<T>(
    value: Option<i64>,
    default: T,
    bounds: RangeInclusive<T>,
    (table, leaf): (&str, &str),
    source: &Source,
) -> Result<T, ConfigError>
where
    T: TryFrom<i64> + PartialOrd + fmt::Display + Copy,
{
    let Some(value) = value else {
        return Ok(default);
    };
    match T::try_from(value) {
        Ok(converted) if bounds.contains(&converted) => Ok(converted),
        _ => Err(source.error(
            table,
            leaf,
            format!(
                "value {value} is outside the allowed range {} to {}",
                bounds.start(),
                bounds.end()
            ),
        )),
    }
}

fn columns(raw: &RawColumns, source: &Source) -> Result<ColumnsConfig, ConfigError> {
    Ok(ColumnsConfig {
        mine: column_list(raw.mine.as_deref(), "mine", source)?,
        needs_me: column_list(raw.needs_me.as_deref(), "needs_me", source)?,
    })
}

/// An absent or empty list means the default order.
fn column_list(
    list: Option<&[String]>,
    leaf: &str,
    source: &Source,
) -> Result<Vec<String>, ConfigError> {
    let list = list.unwrap_or_default();
    if let Some(unknown) = list
        .iter()
        .find(|name| !COLUMN_NAMES.contains(&name.as_str()))
    {
        return Err(source.error(
            "columns",
            leaf,
            format!(
                "unknown column `{unknown}`, expected one of {}",
                COLUMN_NAMES.join(", ")
            ),
        ));
    }
    Ok(if list.is_empty() {
        names(&COLUMN_NAMES)
    } else {
        list.to_vec()
    })
}

fn theme(raw: &RawTheme, source: &Source) -> Result<ThemeConfig, ConfigError> {
    let Some(name) = &raw.name else {
        return Ok(ThemeConfig::default());
    };
    if Theme::ALL.iter().any(|theme| theme.name() == name) {
        return Ok(ThemeConfig { name: name.clone() });
    }
    let valid: Vec<&str> = Theme::ALL.iter().map(|theme| theme.name()).collect();
    Err(source.error(
        "theme",
        "name",
        format!(
            "unknown theme `{name}`, expected one of {}",
            valid.join(", ")
        ),
    ))
}

fn files(raw: RawFiles) -> FilesConfig {
    let defaults = FilesConfig::default();
    FilesConfig {
        test_dirs: raw.test_dirs.unwrap_or(defaults.test_dirs),
        test_files: raw.test_files.unwrap_or(defaults.test_files),
        collapsed: raw.collapsed.unwrap_or(defaults.collapsed),
    }
}

fn keys(raw: RawKeys, source: &Source) -> Result<KeysConfig, ConfigError> {
    let raw_scopes = [raw.list, raw.detail, raw.files, raw.diff];
    let mut scopes = BTreeMap::new();
    for ((scope, valid_actions), actions) in SCOPE_ACTIONS.iter().zip(raw_scopes) {
        if let Some(actions) = actions {
            scopes.insert(
                (*scope).to_string(),
                scope_bindings(scope, valid_actions, actions, source)?,
            );
        }
    }
    Ok(KeysConfig { scopes })
}

fn scope_bindings(
    scope: &str,
    valid_actions: &[&str],
    actions: RawActions,
    source: &Source,
) -> Result<BTreeMap<String, Vec<String>>, ConfigError> {
    let table = format!("keys.{scope}");
    let mut owner_of: BTreeMap<String, String> = BTreeMap::new();
    let mut bound = BTreeMap::new();
    for (action, keys) in actions {
        if !valid_actions.contains(&action.as_str()) {
            return Err(source.error(
                &table,
                &action,
                format!(
                    "unknown action `{action}` in scope {scope}, expected one of {}",
                    valid_actions.join(", ")
                ),
            ));
        }
        let mut canonical = Vec::new();
        for key in &keys {
            let key =
                normalize_key(key).map_err(|message| source.error(&table, &action, message))?;
            if RESERVED_KEYS.contains(&key.as_str()) {
                return Err(source.error(
                    &table,
                    &action,
                    format!("key `{key}` is reserved and cannot be rebound"),
                ));
            }
            if let Some(other) = owner_of.insert(key.clone(), action.clone()) {
                return Err(source.error(
                    &table,
                    &action,
                    format!(
                        "key `{key}` is bound to both `{other}` and `{action}` in scope {scope}"
                    ),
                ));
            }
            canonical.push(key);
        }
        bound.insert(action, canonical);
    }
    Ok(bound)
}

/// Checks a key string and returns its canonical spelling.
pub fn normalize_key(key: &str) -> Result<String, String> {
    if let Some(named) = NAMED_KEYS
        .iter()
        .find(|named| named.eq_ignore_ascii_case(key))
    {
        return Ok((*named).to_string());
    }
    let invalid = || {
        format!(
            "invalid key `{key}`, expected one character, Ctrl-<character> or one of {}",
            NAMED_KEYS.join(", ")
        )
    };
    let single = |text: &str| {
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(ch), None) if !ch.is_whitespace() && !ch.is_control() => Some(ch),
            _ => None,
        }
    };
    match key.get(..CTRL_PREFIX.len()) {
        Some(prefix) if prefix.eq_ignore_ascii_case(CTRL_PREFIX) => key
            .get(CTRL_PREFIX.len()..)
            .and_then(single)
            .map(|ch| format!("{CTRL_PREFIX}{}", ch.to_ascii_uppercase()))
            .ok_or_else(invalid),
        _ => single(key).map(String::from).ok_or_else(invalid),
    }
}
