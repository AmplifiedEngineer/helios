// SPDX-License-Identifier: Apache-2.0

use forgebird::sanitize::sanitize;

/// Bidi marks, embeddings, isolates and line separators that `char::is_control` misses.
const INVISIBLE_SPOOFERS: [char; 15] = [
    '\u{200E}', '\u{200F}', '\u{061C}', '\u{2028}', '\u{2029}', '\u{202A}', '\u{202B}', '\u{202C}',
    '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}', '\u{2069}',
];

/// Independent of the implementation: what a reader must never see in the output.
fn must_be_removed(c: char) -> bool {
    (c.is_control() && c != '\n' && c != '\t') || INVISIBLE_SPOOFERS.contains(&c)
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
            "a\u{80}\u{85}\u{8E}\u{9C}b".into(),
            "ab",
        ),
        ("C1 CSI 0x9B".into(), "a\u{9B}31mb".into(), "ab"),
        ("C1 OSC 0x9D BEL".into(), "a\u{9D}0;title\x07b".into(), "ab"),
        (
            "C1 OSC 0x9D ST".into(),
            "a\u{9D}0;title\x1b\\b".into(),
            "ab",
        ),
        ("C1 DCS 0x90".into(), "a\u{90}q#payload\x1b\\b".into(), "ab"),
        ("C1 SOS 0x98".into(), "a\u{98}payload\x07b".into(), "ab"),
        ("C1 PM 0x9E".into(), "a\u{9E}payload\x07b".into(), "ab"),
        ("C1 APC 0x9F".into(), "a\u{9F}payload\x07b".into(), "ab"),
        ("PM".into(), "a\x1b^payload\x1b\\b".into(), "ab"),
        ("SOS".into(), "a\x1bXpayload\x1b\\b".into(), "ab"),
        ("charset ESC ( B".into(), "a\x1b(Bb".into(), "ab"),
        ("save cursor ESC 7".into(), "a\x1b7b".into(), "ab"),
        ("keypad ESC =".into(), "a\x1b=b".into(), "ab"),
        ("two intermediates".into(), "a\x1b #3b".into(), "ab"),
        ("DCS".into(), "a\x1bPq#payload\x1b\\b".into(), "ab"),
        ("APC".into(), "a\x1b_secret payload\x1b\\b".into(), "ab"),
        ("BEL".into(), "a\x07b".into(), "ab"),
        ("backspace".into(), "a\x08b".into(), "ab"),
        ("carriage return".into(), "a\rb".into(), "ab"),
        ("bare ESC".into(), "a\x1b\nb".into(), "a\nb"),
        ("NUL".into(), "a\0b".into(), "ab"),
        ("DEL".into(), "a\x7fb".into(), "ab"),
    ];
    for c in ('\u{01}'..='\u{1F}').filter(|c| !matches!(c, '\n' | '\t' | '\x1b')) {
        rows.push((format!("C0 {:#04x}", c as u32), format!("a{c}b"), "ab"));
    }
    for c in INVISIBLE_SPOOFERS {
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
fn sanitize_unterminated_sequences_drop_the_payload() {
    for (label, input) in [
        ("OSC", "text\x1b]0;never ends"),
        ("DCS", "text\x1bPq#never ends"),
        ("APC", "text\x1b_never ends"),
        ("PM", "text\x1b^never ends"),
        ("SOS", "text\x1bXnever ends"),
        ("CSI", "text\x1b[31"),
        ("lone ESC", "text\x1b"),
        ("ESC intermediate", "text\x1b("),
        ("C1 CSI", "text\u{9B}31"),
        ("C1 OSC", "text\u{9D}0;never ends"),
        ("C1 DCS", "text\u{90}never ends"),
        ("C1 SOS", "text\u{98}never ends"),
        ("C1 PM", "text\u{9E}never ends"),
        ("C1 APC", "text\u{9F}never ends"),
    ] {
        // Act
        let out = sanitize(input);

        // Assert
        assert_eq!(out.as_str(), "text", "class: {label}");
    }
}

#[test]
fn sanitize_output_never_contains_a_removable_char() {
    // Arrange
    let candidates: Vec<char> = ('\0'..='\u{20FF}')
        .filter(|c| must_be_removed(*c))
        .collect();
    let pieces = [
        "abc", "é", "日", "\n", "\t", "\x1b[", "\x1b]", "\u{9B}", "\u{9D}", "]0;t", "\x07",
    ];

    for (i, c) in candidates.iter().enumerate() {
        for (j, piece) in pieces.iter().enumerate() {
            let input = format!("{piece}{c}{}{c}{piece}", pieces[(i + j) % pieces.len()]);

            // Act
            let out = sanitize(&input);

            // Assert
            assert!(
                !out.as_str().chars().any(must_be_removed),
                "input {input:?} left {:?}",
                out.as_str()
            );
        }
    }
}
