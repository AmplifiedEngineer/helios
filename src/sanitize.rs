// SPDX-License-Identifier: Apache-2.0

use crate::remote::Remote;

const ESC: char = '\x1b';
const BEL: char = '\x07';
const CSI_INTRODUCER: char = '[';
const STRING_TERMINATOR_TAIL: char = '\\';

/// Introducers whose payload runs until BEL or ESC `\`: OSC, DCS, APC, PM and SOS.
fn is_string_introducer(c: char) -> bool {
    matches!(c, ']' | 'P' | '_' | '^' | 'X')
}

/// A CSI sequence ends at its first byte in this range; parameters and intermediates come before.
fn is_csi_final(c: char) -> bool {
    matches!(c, '\u{40}'..='\u{7E}')
}

/// Chars that can hide or spoof text: C0 (bar newline and tab), DEL, C1 and bidi overrides.
fn is_stripped(c: char) -> bool {
    matches!(c,
        '\u{00}'..='\u{08}' | '\u{0B}'..='\u{1F}' | '\u{7F}'..='\u{9F}'
        | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Strips terminal escape sequences, control characters and bidi overrides.
pub fn sanitize(input: &str) -> Remote {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == ESC {
            match chars.peek().copied() {
                Some(CSI_INTRODUCER) => {
                    chars.next();
                    // An unterminated CSI is dropped to the end of input.
                    for next in chars.by_ref() {
                        if is_csi_final(next) {
                            break;
                        }
                    }
                }
                Some(next) if is_string_introducer(next) => {
                    chars.next();
                    skip_string_payload(&mut chars);
                }
                // A bare ESC is dropped; the next char is judged on its own merits.
                _ => {}
            }
        } else if !is_stripped(c) {
            out.push(c);
        }
    }

    Remote::new(out)
}

fn skip_string_payload(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
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
