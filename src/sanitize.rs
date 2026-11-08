// SPDX-License-Identifier: Apache-2.0

use crate::remote::Remote;

const ESC: char = '\x1b';
const BEL: char = '\x07';
const CSI_INTRODUCER: char = '[';
const STRING_TERMINATOR_TAIL: char = '\\';
const C1_CSI: char = '\u{9B}';

/// Introducers whose payload runs until BEL or ESC `\`: OSC, DCS, APC, PM and SOS.
fn is_string_introducer(c: char) -> bool {
    matches!(c, ']' | 'P' | '_' | '^' | 'X')
}

/// OSC, DCS, SOS, PM and APC in their single-char C1 form.
fn is_c1_string_introducer(c: char) -> bool {
    matches!(c, '\u{90}' | '\u{98}' | '\u{9D}' | '\u{9E}' | '\u{9F}')
}

/// Intermediate bytes of a short escape such as ESC `(` `B`.
fn is_escape_intermediate(c: char) -> bool {
    matches!(c, '\u{20}'..='\u{2F}')
}

/// The byte that ends a short escape such as ESC `7` or ESC `=`.
fn is_escape_final(c: char) -> bool {
    matches!(c, '\u{30}'..='\u{7E}')
}

/// A CSI sequence ends at its first byte in this range; parameters and intermediates come before.
fn is_csi_final(c: char) -> bool {
    matches!(c, '\u{40}'..='\u{7E}')
}

/// Chars that can hide or spoof text: C0 (bar newline and tab), DEL, C1 and bidi overrides.
fn is_stripped(c: char) -> bool {
    matches!(c,
        '\u{00}'..='\u{08}' | '\u{0B}'..='\u{1F}' | '\u{7F}'..='\u{9F}'
        | '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{2028}' | '\u{2029}'
        | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Strips terminal escape sequences, control characters and bidi overrides.
pub fn sanitize(input: &str) -> Remote {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            ESC => skip_escape(&mut chars),
            C1_CSI => skip_csi(&mut chars),
            c if is_c1_string_introducer(c) => skip_string_payload(&mut chars),
            c if !is_stripped(c) => out.push(c),
            _ => {}
        }
    }

    Remote::new(out)
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

/// Drops the rest of an escape sequence; `chars` sits just after the ESC.
fn skip_escape(chars: &mut Chars<'_>) {
    match chars.peek().copied() {
        Some(CSI_INTRODUCER) => {
            chars.next();
            skip_csi(chars);
        }
        Some(next) if is_string_introducer(next) => {
            chars.next();
            skip_string_payload(chars);
        }
        Some(next) if is_escape_intermediate(next) || is_escape_final(next) => {
            while chars.next_if(|c| is_escape_intermediate(*c)).is_some() {}
            chars.next_if(|c| is_escape_final(*c));
        }
        // A bare ESC is dropped; the next char is judged on its own merits.
        _ => {}
    }
}

/// An unterminated CSI is dropped to the end of input.
fn skip_csi(chars: &mut Chars<'_>) {
    for next in chars.by_ref() {
        if is_csi_final(next) {
            break;
        }
    }
}

fn skip_string_payload(chars: &mut Chars<'_>) {
    while let Some(c) = chars.next() {
        match c {
            BEL => return,
            ESC if chars.peek() == Some(&STRING_TERMINATOR_TAIL) => {
                chars.next();
                return;
            }
            _ => {}
        }
    }
}
