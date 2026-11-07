// SPDX-License-Identifier: Apache-2.0

use forgebird::sanitize::sanitize;

fn is_stripped(c: char) -> bool {
    matches!(c,
        '\u{00}'..='\u{08}' | '\u{0B}'..='\u{1F}' | '\u{7F}'..='\u{9F}'
        | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

#[test]
fn sanitize_strips_every_control_and_bidi_class() {
    // Arrange
    let mut rows: Vec<(String, String, &str)> = vec![
        ("CSI".into(), "a\x1b[31;1mred\x1b[0mb".into(), "aredb"),
        ("CSI cursor".into(), "a\x1b[2Jb".into(), "ab"),
        ("OSC BEL".into(), "a\x1b]0;evil title\x07b".into(), "ab"),
        (
            "OSC ST".into(),
            "a\x1b]8;;http://x\x1b\\link\x1b]8;;\x1b\\b".into(),
            "alinkb",
        ),
        (
            "C1 controls".into(),
            "a\u{80}\u{85}\u{8E}\u{9F}b".into(),
            "ab",
        ),
        ("C1 CSI 0x9B".into(), "a\u{9B}31mb".into(), "a31mb"),
        ("DCS".into(), "a\x1bPq#payload\x1b\\b".into(), "ab"),
        ("APC".into(), "a\x1b_secret payload\x1b\\b".into(), "ab"),
        ("BEL".into(), "a\x07b".into(), "ab"),
        ("backspace".into(), "a\x08b".into(), "ab"),
        ("carriage return".into(), "a\rb".into(), "ab"),
        ("bare ESC".into(), "a\x1bb".into(), "ab"),
        ("NUL".into(), "a\0b".into(), "ab"),
        ("DEL".into(), "a\x7fb".into(), "ab"),
    ];
    for c in ('\u{01}'..='\u{1F}').filter(|c| !matches!(c, '\n' | '\t' | '\x1b')) {
        rows.push((format!("C0 {:#04x}", c as u32), format!("a{c}b"), "ab"));
    }
    for c in ('\u{202A}'..='\u{202E}').chain('\u{2066}'..='\u{2069}') {
        rows.push((format!("bidi U+{:04X}", c as u32), format!("a{c}b"), "ab"));
    }

    for (label, input, expected) in rows {
        // Act
        let out = sanitize(&input);

        // Assert
        assert_eq!(out.as_str(), expected, "class: {label}");
    }
}

#[test]
fn sanitize_keeps_newline_and_tab() {
    // Arrange
    let input = "line one\n\tline two";

    // Act
    let out = sanitize(input);

    // Assert
    assert_eq!(out.as_str(), input);
}

#[test]
fn sanitize_keeps_plain_and_non_ascii_text() {
    // Arrange
    let input = "Plain café naïve 日本語 émoji 🎉";

    // Act
    let out = sanitize(input);

    // Assert
    assert_eq!(out.as_str(), input);
}

#[test]
fn sanitize_unterminated_sequences_leave_no_escape_byte() {
    for (label, input) in [
        ("OSC", "text\x1b]0;never ends"),
        ("CSI", "text\x1b[31"),
        ("lone ESC", "text\x1b"),
    ] {
        // Act
        let out = sanitize(input);

        // Assert
        assert!(!out.as_str().contains('\x1b'), "class: {label}");
        assert!(out.as_str().starts_with("text"), "class: {label}");
    }
}

#[test]
fn sanitize_output_never_contains_a_stripped_char() {
    // Arrange
    let stripped: Vec<char> = ('\0'..='\u{2069}').filter(|c| is_stripped(*c)).collect();
    let pieces = ["abc", "é", "日", "\n", "\t", "\x1b[", "\x1b]", "]0;t"];

    for (i, c) in stripped.iter().enumerate() {
        for (j, piece) in pieces.iter().enumerate() {
            let input = format!("{piece}{c}{}{c}{piece}", pieces[(i + j) % pieces.len()]);

            // Act
            let out = sanitize(&input);

            // Assert
            assert!(
                !out.as_str().chars().any(is_stripped),
                "input {input:?} left {:?}",
                out.as_str()
            );
        }
    }
}
