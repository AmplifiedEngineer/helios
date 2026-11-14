// SPDX-License-Identifier: Apache-2.0

//! `--print-config` shows the effective settings as TOML that parses back to the same config.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use helios::config::{parse, to_toml, Config};

const PRINT_FLAG: &str = "--print-config";
const CONFIG_ENV: &str = "HELIOS_CONFIG";
const FILE_TEXT: &str =
    "[theme]\nname = \"dark\"\n\n[keys.files]\ntoggle_tests = [\"z\"]\n\n[layout]\nlist_percent = 55\n";
const ESCAPE_BYTE: u8 = 0x1b;

/// A fresh, empty directory unique to the test.
fn temp_dir(test: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("helios-print-config-{}-{test}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir is created");
    dir
}

/// The text with every `# action = [...]` line inside a `[keys.*]` table uncommented.
fn uncomment_key_lines(text: &str) -> String {
    let mut in_keys = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let header = line.trim_start_matches("# ");
        if header.starts_with('[') {
            in_keys = header.starts_with("[keys.");
        }
        let is_key_line = in_keys && line.starts_with("# ") && line.contains(" = [");
        let is_table = line.starts_with("# [keys.");
        out.push(if is_key_line || is_table {
            &line[2..]
        } else {
            line
        });
    }
    out.join("\n")
}

#[test]
fn every_printed_default_key_line_loads_when_uncommented() {
    // Arrange
    let printed = to_toml(&Config::default());
    let uncommented = uncomment_key_lines(&printed);
    let lines: Vec<&str> = uncommented
        .lines()
        .filter(|line| line.contains(" = [") && !printed.lines().any(|p| p == *line))
        .collect();

    // Act
    let all_together = parse(&uncommented);
    let each_alone: Vec<_> = lines
        .iter()
        .map(|line| {
            let table = printed
                .lines()
                .take_while(|p| *p != format!("# {line}"))
                .filter(|p| p.starts_with("# [keys."))
                .last()
                .map(|p| &p[2..]);
            (
                *line,
                table.map(|table| parse(&format!("{table}\n{line}\n"))),
            )
        })
        .collect();

    // Assert
    assert!(!lines.is_empty(), "some default key lines are printed");
    assert!(all_together.is_ok(), "all lines together: {all_together:?}");
    for (line, result) in each_alone {
        assert!(
            result.is_some_and(|result| result.is_ok()),
            "`{line}` alone does not load"
        );
    }
}

#[test]
fn print_config_shows_the_effective_settings_and_round_trips() {
    // Arrange
    let dir = temp_dir("round-trip");
    let file = dir.join("config.toml");
    fs::write(&file, FILE_TEXT).expect("the config file is written");
    let expected = parse(FILE_TEXT).expect("the file parses");

    // Act
    let output = Command::new(env!("CARGO_BIN_EXE_helios"))
        .arg(PRINT_FLAG)
        .env(CONFIG_ENV, &file)
        .output()
        .expect("helios runs");
    let printed = Config::default();
    let default_round_trip = parse(&to_toml(&printed));

    // Assert
    assert!(output.status.success(), "exit status {:?}", output.status);
    let stdout = String::from_utf8(output.stdout).expect("the output is text");
    assert!(
        !stdout.as_bytes().contains(&ESCAPE_BYTE),
        "no terminal escapes"
    );
    assert!(
        stdout.contains("name = \"dark\""),
        "the theme shows: {stdout}"
    );
    assert_eq!(parse(&stdout), Ok(expected));
    assert_eq!(default_round_trip, Ok(Config::default()));
    for section in ["[layout]", "[columns]", "[theme]", "[files]"] {
        assert!(stdout.contains(section), "{section} shows");
    }
    assert!(stdout.contains("toggle_tests = [\"z\"]"));
    assert!(stdout.contains("move_down"), "the default keys show");
}
