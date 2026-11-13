// SPDX-License-Identifier: Apache-2.0

//! The highlighter turns source lines into code roles.

use helios::highlight::{highlight, BlockHighlighter, LineRoles, MAX_HIGHLIGHT_LINE};
use helios::theme::Role;

/// Turns the roles of a line into (role, text) pairs.
fn spans(line: &str, roles: &LineRoles) -> Vec<(Role, String)> {
    roles
        .iter()
        .map(|(role, range)| (*role, line[range.clone()].to_string()))
        .collect()
}

fn has(spans: &[(Role, String)], role: Role, text: &str) -> bool {
    spans.iter().any(|(r, t)| *r == role && t.trim() == text)
}

fn is_plain(roles: &LineRoles) -> bool {
    roles.iter().all(|(role, _)| *role == Role::Plain)
}

#[test]
fn rust_source_gets_keyword_string_and_comment_roles() {
    // Arrange
    let line = "fn main() { let s = \"hi\"; // note";

    // Act
    let rows = highlight("rust", &[line]);

    // Assert
    let found = spans(line, &rows[0]);
    assert!(has(&found, Role::Keyword, "fn"), "{found:?}");
    assert!(has(&found, Role::String, "\"hi\""), "{found:?}");
    assert!(has(&found, Role::Comment, "// note"), "{found:?}");
}

#[test]
fn typescript_comes_from_the_extra_syntax_set() {
    // Arrange
    let line = "const x: number = 1;";

    // Act
    let rows = highlight("typescript", &[line]);

    // Assert
    let found = spans(line, &rows[0]);
    assert!(!is_plain(&rows[0]), "{found:?}");
    assert!(has(&found, Role::Keyword, "const"), "{found:?}");
    assert!(has(&found, Role::Number, "1"), "{found:?}");
}

#[test]
fn toml_and_dockerfile_are_known() {
    // Arrange
    let toml = "name = \"helios\"";
    let docker = "FROM alpine";

    // Act
    let toml_rows = highlight("toml", &[toml]);
    let docker_rows = highlight("Dockerfile", &[docker]);

    // Assert
    assert!(!is_plain(&toml_rows[0]), "{:?}", spans(toml, &toml_rows[0]));
    assert!(
        !is_plain(&docker_rows[0]),
        "{:?}",
        spans(docker, &docker_rows[0])
    );
}

#[test]
fn an_unknown_language_is_plain() {
    // Arrange
    let line = "fn main() {}";

    // Act
    let rows = highlight("no-such-language", &[line]);

    // Assert
    assert_eq!(rows, vec![vec![(Role::Plain, 0..line.len())]]);
}

#[test]
fn a_line_over_the_cap_is_returned_plain() {
    // Arrange
    let padding = " ".repeat(MAX_HIGHLIGHT_LINE - "fn".len());
    let at_cap = format!("fn{padding}");
    let over_cap = format!("fn{padding} ");

    // Act
    let rows = highlight("rust", &[&at_cap, &over_cap]);

    // Assert
    assert_eq!(at_cap.len(), MAX_HIGHLIGHT_LINE);
    assert!(has(&spans(&at_cap, &rows[0]), Role::Keyword, "fn"));
    assert_eq!(rows[1], vec![(Role::Plain, 0..over_cap.len())]);
}

#[test]
fn a_long_line_does_not_stop_later_lines_from_getting_roles() {
    // Arrange
    let long = format!("/* {}", "x".repeat(MAX_HIGHLIGHT_LINE));
    let next = "fn main() {}";

    // Act
    let rows = highlight("rust", &[&long, next]);

    // Assert
    assert!(has(&spans(next, &rows[1]), Role::Keyword, "fn"));
}

#[test]
fn cargo_lock_holds_one_syntect_entry() {
    // Arrange
    let lock = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock"))
        .expect("Cargo.lock should be readable");

    // Act
    let count = lock
        .lines()
        .filter(|line| *line == "name = \"syntect\"")
        .count();

    // Assert
    assert_eq!(count, 1);
}

#[test]
fn state_carries_across_the_lines_of_one_block() {
    // Arrange
    let lines = ["/* opened here", "still inside the comment", "*/ fn"];

    // Act
    let rows = highlight("rust", &lines);

    // Assert
    let middle = spans(lines[1], &rows[1]);
    assert!(has(&middle, Role::Comment, lines[1]), "{middle:?}");
}

#[test]
fn unbalanced_input_returns_one_role_row_per_line_with_the_same_text() {
    // Arrange
    let lines = [
        "let a = \"unterminated",
        "/*",
        "",
        "let s = \"héllo wörld 日本\"; // ünï",
        "}}}) ]] \"'",
    ];
    let cases = [("rust", true), ("no-such-language", false)];

    for (language, known) in cases {
        // Act
        let rows = highlight(language, &lines);

        // Assert
        assert_eq!(rows.len(), lines.len(), "{language}");
        for (line, roles) in lines.iter().zip(&rows) {
            let mut end = 0;
            for (_, range) in roles {
                assert_eq!(range.start, end, "gap in {line:?}");
                assert!(line.is_char_boundary(range.end), "{line:?}");
                end = range.end;
            }
            assert_eq!(end, line.len(), "{line:?}");
            let joined: String = spans(line, roles).into_iter().map(|(_, t)| t).collect();
            assert_eq!(&joined, line);
            if !known {
                assert!(is_plain(roles), "{line:?}");
            }
        }
    }
}

#[test]
fn jsx_and_tsx_map_to_their_base_language() {
    // Arrange
    let line = "const a = <div className=\"x\">{1}</div>;";
    let tokens = ["jsx", "tsx", "src/App.jsx", "src/App.tsx"];

    for token in tokens {
        // Act
        let rows = highlight(token, &[line]);

        // Assert
        let found = spans(line, &rows[0]);
        assert!(!is_plain(&rows[0]), "{token}: {found:?}");
        assert!(has(&found, Role::Keyword, "const"), "{token}: {found:?}");
    }
}

#[test]
fn feeding_line_by_line_equals_one_call_to_highlight() {
    // Arrange
    let long = format!("/* {}", "x".repeat(MAX_HIGHLIGHT_LINE));
    let lines = [
        "/* opened here",
        "still inside the comment",
        "*/ fn main() {",
        &long,
        "let s = \"hi\"; // note",
    ];
    let mut block = BlockHighlighter::new("rust");

    // Act
    let fed: Vec<LineRoles> = lines.iter().map(|line| block.feed(line)).collect();

    // Assert
    assert_eq!(fed, highlight("rust", &lines));
}

#[test]
fn a_cloned_highlighter_resumes_from_the_same_state() {
    // Arrange
    let mut block = BlockHighlighter::new("rust");
    block.feed("/* opened here");
    let mut copy = block.clone();

    // Act
    let original = block.feed("still inside the comment");
    let cloned = copy.feed("still inside the comment");

    // Assert
    assert_eq!(original, cloned);
    assert!(has(
        &spans("still inside the comment", &original),
        Role::Comment,
        "still inside the comment"
    ));
}

#[test]
fn an_unknown_language_feeds_plain_lines() {
    // Arrange
    let mut block = BlockHighlighter::new("no-such-language");

    // Act
    let roles = block.feed("fn main() {}");

    // Assert
    assert_eq!(roles, vec![(Role::Plain, 0.."fn main() {}".len())]);
}
