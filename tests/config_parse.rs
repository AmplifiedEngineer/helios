// SPDX-License-Identifier: Apache-2.0

//! Parsing the settings file: defaults, valid values and every kind of rejection.

use std::collections::BTreeMap;

use helios::app::PREVIEW_DELAY_MS;
use helios::config::{
    parse, ColumnsConfig, Config, FilesConfig, KeysConfig, LayoutConfig, ThemeConfig,
};
use helios::test_files::TestPatterns;
use helios::tui::layout::{LIST_PERCENT, NARROW_BELOW};

fn actions(pairs: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
    pairs
        .iter()
        .map(|(action, keys)| {
            (
                (*action).to_string(),
                keys.iter().map(|key| (*key).to_string()).collect(),
            )
        })
        .collect()
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|item| (*item).to_string()).collect()
}

#[test]
fn an_empty_file_gives_the_defaults() {
    // Arrange
    let text = "";

    // Act
    let config = parse(text).expect("an empty file parses");

    // Assert
    assert_eq!(config, Config::default());
    assert_eq!(config.layout.list_percent, LIST_PERCENT);
    assert_eq!(config.layout.narrow_below, NARROW_BELOW);
    assert_eq!(config.layout.preview_delay_ms, PREVIEW_DELAY_MS);
    let patterns = TestPatterns::default();
    assert_eq!(
        patterns,
        TestPatterns::new(config.files.test_dirs, config.files.test_files)
    );
    assert!(config.files.collapsed);
}

#[test]
fn every_section_parses() {
    // Arrange
    let text = r#"
[layout]
list_percent = 55
narrow_below = 120
preview_delay_ms = 500

[columns]
mine = ["repo", "number", "title"]
needs_me = ["author", "title", "updated"]

[keys.list]
move_down = ["n", "Down"]

[keys.files]
toggle_file = ["Space", "Enter"]

[keys.diff]
scroll_down = ["ctrl-d"]

[theme]
name = "dark"

[files]
test_dirs = ["e2e"]
test_files = ["*.it.rs"]
collapsed = false
"#;

    // Act
    let config = parse(text).expect("a full file parses");

    // Assert
    let expected = Config {
        layout: LayoutConfig {
            list_percent: 55,
            narrow_below: 120,
            preview_delay_ms: 500,
        },
        columns: ColumnsConfig {
            mine: strings(&["repo", "number", "title"]),
            needs_me: strings(&["author", "title", "updated"]),
        },
        keys: KeysConfig {
            scopes: BTreeMap::from([
                (
                    "list".to_string(),
                    actions(&[("move_down", &["n", "Down"])]),
                ),
                (
                    "files".to_string(),
                    actions(&[("toggle_file", &["Space", "Enter"])]),
                ),
                ("diff".to_string(), actions(&[("scroll_down", &["Ctrl-D"])])),
            ]),
        },
        theme: ThemeConfig {
            name: "dark".to_string(),
        },
        files: FilesConfig {
            test_dirs: strings(&["e2e"]),
            test_files: strings(&["*.it.rs"]),
            collapsed: false,
        },
    };
    assert_eq!(config, expected);
}

#[test]
fn an_unknown_key_is_an_error_naming_the_key() {
    // Arrange
    let cases = [
        ("colour = 1\n", "colour"),
        ("[layout]\nlist_width = 30\n", "list_width"),
        ("[keys.files]\nteleport = [\"x\"]\n", "teleport"),
    ];

    for (text, name) in cases {
        // Act
        let error = parse(text).expect_err("an unknown key is rejected");

        // Assert
        assert!(
            error.to_string().contains(name),
            "`{error}` should name `{name}`"
        );
    }
}

#[test]
fn a_value_out_of_range_is_an_error() {
    // Arrange
    let rejected = [
        "[layout]\nlist_percent = 19\n",
        "[layout]\nlist_percent = 71\n",
        "[layout]\nlist_percent = 0\n",
        "[layout]\nlist_percent = 101\n",
    ];
    let accepted = [
        "[layout]\nlist_percent = 20\n",
        "[layout]\nlist_percent = 70\n",
    ];

    for text in rejected {
        // Act
        let error = parse(text).expect_err("an out of range percent is rejected");

        // Assert
        assert_eq!(error.key.as_deref(), Some("layout.list_percent"));
        assert!(
            error.to_string().contains("20"),
            "`{error}` names the bounds"
        );
        assert!(
            error.to_string().contains("70"),
            "`{error}` names the bounds"
        );
    }
    for text in accepted {
        assert!(parse(text).is_ok(), "{text:?} is inside the bounds");
    }
    assert!(parse("[layout]\npreview_delay_ms = 5001\n").is_err());
    assert!(parse("[layout]\npreview_delay_ms = 5000\n").is_ok());
}

#[test]
fn an_error_carries_the_line_of_the_key() {
    // Arrange
    let text = "[theme]\nname = \"dark\"\n\n[layout]\nnarrow_below = 90\nlist_percent = 19\n";

    // Act
    let error = parse(text).expect_err("an out of range percent is rejected");

    // Assert
    assert_eq!(error.line, Some(6));
    assert_eq!(
        error.to_string(),
        format!("{} (key layout.list_percent, line 6)", error.message)
    );
}

#[test]
fn a_duplicate_key_binding_in_one_scope_is_an_error() {
    // Arrange
    let twice = "[keys.files]\nopen_all = [\"z\"]\nclose_all = [\"z\"]\n";
    let two_scopes = "[keys.files]\nopen_all = [\"z\"]\n[keys.diff]\nback = [\"z\"]\n";

    // Act
    let error = parse(twice).expect_err("one key for two actions is rejected");
    let fine = parse(two_scopes);

    // Assert
    assert!(error.to_string().contains('z'), "`{error}` names the key");
    assert!(
        error.to_string().contains("files"),
        "`{error}` names the scope"
    );
    assert!(fine.is_ok());
}

#[test]
fn a_reserved_key_cannot_be_rebound() {
    // Arrange
    let reserved = ["q", "Ctrl-C", "?", "Esc"];

    for key in reserved {
        let text = format!("[keys.list]\nrefresh = [\"{key}\"]\n");

        // Act
        let error = parse(&text).expect_err("a reserved key is rejected");

        // Assert
        assert!(
            error.to_string().contains(key),
            "`{error}` should name `{key}`"
        );
    }
}

#[test]
fn a_rebind_onto_a_default_key_of_another_action_is_an_error() {
    // Arrange
    let clash = "[keys.list]\nmove_down = [\"r\"]\n";
    let both = "[keys.list]\nmove_down = [\"r\"]\nrefresh = [\"R\"]\n";

    // Act
    let error = parse(clash).expect_err("the rebind leaves refresh without its key");
    let fine = parse(both);

    // Assert
    let message = error.to_string();
    for part in ["`r`", "list", "refresh"] {
        assert!(message.contains(part), "`{message}` should name {part}");
    }
    assert!(fine.is_ok(), "rebinding both actions parses: {fine:?}");
}

#[test]
fn a_key_listed_twice_for_one_action_is_an_error_with_its_own_message() {
    // Arrange
    let text = "[keys.list]\nmove_down = [\"j\", \"j\"]\n";

    // Act
    let error = parse(text).expect_err("a repeated key is rejected");

    // Assert
    assert!(
        error
            .message
            .contains("`j` is listed twice for `move_down`"),
        "`{}` has the repeat message",
        error.message
    );
}

#[test]
fn a_rebind_onto_a_global_key_is_an_error() {
    // Arrange
    let text = "[keys.list]\nmove_down = [\"o\"]\n";

    // Act
    let error = parse(text).expect_err("`o` runs open_in_browser first");

    // Assert
    let message = error.to_string();
    for part in ["`o`", "list", "open_in_browser"] {
        assert!(message.contains(part), "`{message}` should name {part}");
    }
}
