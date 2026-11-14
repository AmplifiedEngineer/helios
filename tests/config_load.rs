// SPDX-License-Identifier: Apache-2.0

//! Finding, loading and layering the settings file.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use helios::config::{config_path, load, resolve_theme, Config};
use helios::theme::Theme;

const HELIOS_CONFIG: &str = "HELIOS_CONFIG";
const XDG_CONFIG_HOME: &str = "XDG_CONFIG_HOME";
const HOME: &str = "HOME";
const BAD_FILE: &str = "[layout]\nlist_percent = 99\n";
const GOOD_FILE: &str = "[theme]\nname = \"dark\"\n";
const ESCAPE_BYTE: u8 = 0x1b;

/// A fresh, empty directory unique to the test.
fn temp_dir(test: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("helios-config-load-{}-{test}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir is created");
    dir
}

fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |name| map.get(name).cloned()
}

#[test]
fn the_path_comes_from_helios_config_then_xdg_then_home() {
    // Arrange
    let all = lookup(&[
        (HELIOS_CONFIG, "/c/own.toml"),
        (XDG_CONFIG_HOME, "/x"),
        (HOME, "/h"),
    ]);
    let no_own = lookup(&[(XDG_CONFIG_HOME, "/x"), (HOME, "/h")]);
    let only_home = lookup(&[(HOME, "/h")]);
    let nothing = lookup(&[]);
    let empties = lookup(&[(HELIOS_CONFIG, ""), (XDG_CONFIG_HOME, ""), (HOME, "/h")]);
    let all_empty = lookup(&[(HELIOS_CONFIG, ""), (XDG_CONFIG_HOME, ""), (HOME, "")]);

    // Act
    let paths = [
        config_path(&all),
        config_path(&no_own),
        config_path(&only_home),
        config_path(&nothing),
        config_path(&empties),
        config_path(&all_empty),
    ];

    // Assert
    assert_eq!(
        paths,
        [
            Some(PathBuf::from("/c/own.toml")),
            Some(PathBuf::from("/x/helios/config.toml")),
            Some(PathBuf::from("/h/.config/helios/config.toml")),
            None,
            Some(PathBuf::from("/h/.config/helios/config.toml")),
            None,
        ]
    );
}

#[test]
fn a_missing_file_means_defaults() {
    // Arrange
    let missing = temp_dir("missing").join("config.toml");

    // Act
    let from_missing = load(Some(&missing));
    let from_none = load(None);

    // Assert
    assert_eq!(
        from_missing.expect("a missing file loads"),
        Config::default()
    );
    assert_eq!(from_none.expect("no path loads"), Config::default());
}

#[test]
fn a_bad_file_stops_startup_with_file_line_and_key() {
    // Arrange
    let file = temp_dir("bad").join("config.toml");
    fs::write(&file, BAD_FILE).expect("file is written");

    // Act
    let output = Command::new(env!("CARGO_BIN_EXE_helios"))
        .env_clear()
        .env(HELIOS_CONFIG, &file)
        .stdin(Stdio::null())
        .output()
        .expect("helios starts");

    // Assert
    let stderr = String::from_utf8(output.stderr).expect("stderr is text");
    assert!(!output.status.success(), "a bad file must fail startup");
    assert!(stderr.contains(&file.display().to_string()), "{stderr}");
    assert!(stderr.contains("line 2"), "{stderr}");
    assert!(stderr.contains("layout.list_percent"), "{stderr}");
    assert!(!output.stdout.contains(&ESCAPE_BYTE), "no terminal setup");
}

#[test]
fn flag_beats_env_beats_file_beats_default() {
    // Arrange
    let default = Config::default().theme.name;

    // Act
    let all = resolve_theme(Some("spectrum"), Some("light"), "dark");
    let no_flag = resolve_theme(None, Some("light"), "dark");
    let file_only = resolve_theme(None, None, "dark");
    let nothing = resolve_theme(None, None, &default);

    // Assert
    assert_eq!(all, Theme::Spectrum);
    assert_eq!(no_flag, Theme::Light);
    assert_eq!(file_only, Theme::Dark);
    assert_eq!(nothing, Theme::Standard);
}

#[test]
fn helios_never_writes_the_file() {
    // Arrange
    let file = temp_dir("readonly").join("config.toml");
    fs::write(&file, GOOD_FILE).expect("file is written");
    let before = snapshot(&file);

    // Act
    let loaded = load(Some(&file)).expect("the file loads");
    let status = Command::new(env!("CARGO_BIN_EXE_helios"))
        .arg("--version")
        .env_clear()
        .env(HELIOS_CONFIG, &file)
        .stdin(Stdio::null())
        .status()
        .expect("helios starts");

    // Assert
    assert_eq!(loaded.theme.name, "dark");
    assert!(status.success());
    assert_eq!(snapshot(&file), before);
}

/// The file's bytes and modified time.
fn snapshot(path: &Path) -> (Vec<u8>, std::time::SystemTime) {
    let modified = fs::metadata(path)
        .and_then(|meta| meta.modified())
        .expect("modified time reads");
    (fs::read(path).expect("file reads"), modified)
}
