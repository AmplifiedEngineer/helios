// SPDX-License-Identifier: Apache-2.0

use helios::markdown::{from_remote, Block, Document, Inline, Table, MAX_BLOCKS, MAX_BODY_BYTES};
use helios::sanitize::sanitize;

fn parse(body: &str) -> Document {
    from_remote(&sanitize(body))
}

fn text(s: &str) -> Inline {
    Inline::Text(s.to_string())
}

#[test]
fn a_paragraph_with_a_link_keeps_the_text_and_the_url() {
    // Arrange
    let body = "See [the docs](https://example.com/docs) now";

    // Act
    let document = parse(body);

    // Assert
    let expected = Document {
        blocks: vec![Block::Paragraph(vec![
            text("See "),
            Inline::Link {
                url: Some("https://example.com/docs".to_string()),
                content: vec![text("the docs")],
            },
            text(" now"),
        ])],
        cut: false,
    };
    assert_eq!(document, expected);
}

#[test]
fn a_fenced_code_block_keeps_its_language_and_text() {
    // Arrange
    let body = "```rust\nfn main() {}\n```\n";

    // Act
    let document = parse(body);

    // Assert
    let expected = Document {
        blocks: vec![Block::Code {
            lang: Some("rust".to_string()),
            text: "fn main() {}".to_string(),
        }],
        cut: false,
    };
    assert_eq!(document, expected);
}

#[test]
fn a_table_keeps_its_header_and_rows() {
    // Arrange
    let body = "| name | count |\n| --- | --- |\n| apples | 3 |\n| pears | 5 |\n";

    // Act
    let document = parse(body);

    // Assert
    let expected = Document {
        blocks: vec![Block::Table(Table {
            header: vec![vec![text("name")], vec![text("count")]],
            rows: vec![
                vec![vec![text("apples")], vec![text("3")]],
                vec![vec![text("pears")], vec![text("5")]],
            ],
        })],
        cut: false,
    };
    assert_eq!(document, expected);
}

#[test]
fn a_task_list_item_keeps_its_checked_state() {
    // Arrange
    let body = "- [x] done\n- [ ] todo\n";

    // Act
    let document = parse(body);

    // Assert
    let expected = Document {
        blocks: vec![Block::List(vec![
            vec![Block::Paragraph(vec![
                Inline::TaskMarker(true),
                text("done"),
            ])],
            vec![Block::Paragraph(vec![
                Inline::TaskMarker(false),
                text("todo"),
            ])],
        ])],
        cut: false,
    };
    assert_eq!(document, expected);
}

#[test]
fn an_image_becomes_its_alt_text_and_url() {
    // Arrange
    let body = "![a red dot](https://example.com/dot.png)";

    // Act
    let document = parse(body);

    // Assert
    let expected = Document {
        blocks: vec![Block::Paragraph(vec![Inline::Image {
            alt: "a red dot".to_string(),
            url: Some("https://example.com/dot.png".to_string()),
        }])],
        cut: false,
    };
    assert_eq!(document, expected);
}

fn inline_strings<'a>(inlines: &'a [Inline], out: &mut Vec<&'a str>) {
    for inline in inlines {
        match inline {
            Inline::Text(s) | Inline::Code(s) => out.push(s),
            Inline::Emphasis(content) | Inline::Strong(content) => inline_strings(content, out),
            Inline::Link { url, content } => {
                out.extend(url.as_deref());
                inline_strings(content, out);
            }
            Inline::Image { alt, url } => {
                out.push(alt);
                out.extend(url.as_deref());
            }
            Inline::TaskMarker(_) => {}
        }
    }
}

fn block_strings<'a>(blocks: &'a [Block], out: &mut Vec<&'a str>) {
    for block in blocks {
        match block {
            Block::Paragraph(content) | Block::Heading { content, .. } => {
                inline_strings(content, out);
            }
            Block::List(items) | Block::OrderedList(items) => {
                items.iter().for_each(|item| block_strings(item, out))
            }
            Block::Quote(inner) => block_strings(inner, out),
            Block::Code { lang, text } => {
                out.extend(lang.as_deref());
                out.push(text);
            }
            Block::Table(table) => {
                for cell in table.header.iter().chain(table.rows.iter().flatten()) {
                    inline_strings(cell, out);
                }
            }
            Block::Rule => {}
        }
    }
}

fn all_strings(document: &Document) -> Vec<&str> {
    let mut out = Vec::new();
    block_strings(&document.blocks, &mut out);
    out
}

fn source_len(document: &Document) -> usize {
    all_strings(document).iter().map(|s| s.len()).sum()
}

#[test]
fn an_escape_sequence_never_reaches_the_document() {
    // Arrange
    let body = "before \x1b[31mred\x1b[0m middle \x1b]0;title\x07 after";

    // Act
    let document = parse(body);

    // Assert
    let strings = all_strings(&document);
    assert!(strings.iter().all(|s| !s.contains('\x1b')));
    let joined = strings.concat();
    assert!(joined.contains("before "));
    assert!(joined.contains("red"));
    assert!(joined.contains(" after"));
}

#[test]
fn an_oversized_body_is_cut_and_marked() {
    // Arrange
    let body = "a".repeat(MAX_BODY_BYTES * 2);

    // Act
    let document = parse(&body);

    // Assert
    assert!(document.cut);
    assert!(source_len(&document) <= MAX_BODY_BYTES);
}

#[test]
fn a_body_at_the_byte_limit_is_whole_and_one_byte_over_is_cut() {
    // Arrange
    let at_limit = "a".repeat(MAX_BODY_BYTES);
    let one_over = "a".repeat(MAX_BODY_BYTES + 1);

    // Act
    let whole = parse(&at_limit);
    let cut = parse(&one_over);

    // Assert
    assert!(!whole.cut);
    assert_eq!(source_len(&whole), MAX_BODY_BYTES);
    assert!(cut.cut);
}

#[test]
fn a_body_at_the_block_limit_is_whole_and_one_block_over_is_cut() {
    // Arrange
    let at_limit = "a\n\n".repeat(MAX_BLOCKS);
    let one_over = "a\n\n".repeat(MAX_BLOCKS + 1);

    // Act
    let whole = parse(&at_limit);
    let cut = parse(&one_over);

    // Assert
    assert_eq!(whole.block_count(), MAX_BLOCKS);
    assert!(!whole.cut);
    assert_eq!(cut.block_count(), MAX_BLOCKS);
    assert!(cut.cut);
}

const FORBIDDEN: [char; 6] = ['\x1b', '\x07', '\u{9b}', '\u{202e}', '\u{2066}', '\u{9d}'];

fn holds_forbidden(document: &Document) -> bool {
    all_strings(document)
        .iter()
        .any(|s| s.chars().any(|c| FORBIDDEN.contains(&c)))
}

fn first_link_url(document: &Document) -> Option<Option<String>> {
    fn find(inlines: &[Inline]) -> Option<Option<String>> {
        inlines.iter().find_map(|inline| match inline {
            Inline::Link { url, .. } => Some(url.clone()),
            Inline::Emphasis(c) | Inline::Strong(c) => find(c),
            _ => None,
        })
    }
    document.blocks.iter().find_map(|block| match block {
        Block::Paragraph(content) => find(content),
        _ => None,
    })
}

#[test]
fn a_link_url_with_control_or_bidi_characters_is_cleaned() {
    // Arrange
    let raw = "[raw](http://a.com/\x1b[31m\x07\u{202e}\u{2066}p)";
    let decoded = "[dec](http://a.com/&#x1b;&#7;&#x202e;&#x2066;q) and t&#x1b;&#x202e;ext";

    // Act
    let from_raw = parse(raw);
    let from_decoded = parse(decoded);

    // Assert
    assert!(!holds_forbidden(&from_raw));
    assert!(!holds_forbidden(&from_decoded));
    assert!(all_strings(&from_raw).concat().contains("raw"));
    assert_eq!(
        first_link_url(&from_decoded),
        Some(Some("http://a.com/q".to_string()))
    );
    assert!(all_strings(&from_decoded).concat().contains("text"));
}

#[test]
fn a_javascript_link_keeps_its_text_and_drops_the_url() {
    // Arrange
    let cases = [
        ("[click](javascript:alert(1))", None),
        ("[x](/relative/path)", None),
        ("[x](ftp://h/f)", None),
        ("[x](http://h/f)", Some("http://h/f")),
        ("[x](https://h/f)", Some("https://h/f")),
        ("[x](mailto:a@b.c)", Some("mailto:a@b.c")),
    ];

    for (body, expected_url) in cases {
        // Act
        let document = parse(body);

        // Assert
        let label = body.split(']').next().unwrap_or("").trim_start_matches('[');
        assert_eq!(
            first_link_url(&document),
            Some(expected_url.map(str::to_string)),
            "{body}"
        );
        assert!(all_strings(&document).concat().contains(label), "{body}");
    }
}

#[test]
fn a_scheme_split_by_a_tab_entity_is_not_kept() {
    // Arrange
    let body = "[x](java&#9;script:alert(1))";

    // Act
    let document = parse(body);

    // Assert
    assert_eq!(first_link_url(&document), Some(None));
    assert!(all_strings(&document).concat().contains('x'));
}

#[test]
fn numeric_references_to_control_characters_never_reach_the_document() {
    // Arrange
    // Decoded, the references are ESC ESC C1-CSI and a bidi override. Where
    // the sanitizer sees them joined to the next letter (image alt, URLs, the
    // info string) it reads that letter as the CSI final byte and drops it, so
    // each leaf carries a sacrificial `X` right after the references. Plain
    // and link text arrive as separate events per reference, so their `X`
    // survives.
    let refs = "&#x1b;&#27;&#x9b;&#x202e;";
    let body = format!(
        "al{refs}Xpha `be{refs}ta`\n\n![de{refs}Xlta](http://a.com/i{refs}X.png) [li{refs}Xnk](http://a.com/u{refs}Xv)\n\n```ru{refs}Xst\nga{refs}mma\n```\n"
    );

    // Act
    let document = parse(&body);

    // Assert
    assert!(!holds_forbidden(&document));
    let strings = all_strings(&document);
    for (leaf, expected) in [
        ("plain text", "alXpha "),
        ("image alt", "delta"),
        ("image destination", "http://a.com/i.png"),
        ("link text", "liXnk"),
        ("link destination", "http://a.com/uv"),
        ("fenced info string", "rust"),
    ] {
        assert!(
            strings.contains(&expected),
            "{leaf} should be {expected:?}: {strings:?}"
        );
    }
    // pulldown-cmark does not decode references in code, so the code span and
    // the fenced body keep the literal `&#x1b;` text, which is not a control
    // character.
    for (leaf, expected) in [
        ("code span", format!("be{refs}ta")),
        ("fenced code body", format!("ga{refs}mma")),
    ] {
        assert!(
            strings.contains(&expected.as_str()),
            "{leaf} should keep the literal references as text: {strings:?}"
        );
    }
}

const UNFINISHED_TAG_TIME_LIMIT: std::time::Duration = std::time::Duration::from_secs(2);
const LATER_TAG_PAIRS: usize = 12_000;

#[test]
fn an_unfinished_tag_does_not_make_later_html_slow() {
    // Arrange
    let body = format!(
        "<div title='\n\n{}",
        "<strong>a</strong>".repeat(LATER_TAG_PAIRS)
    );
    assert!(body.len() < MAX_BODY_BYTES);
    let started = std::time::Instant::now();

    // Act
    let document = parse(&body);
    let elapsed = started.elapsed();

    // Assert
    assert!(
        elapsed < UNFINISHED_TAG_TIME_LIMIT,
        "parse took {elapsed:?}"
    );
    let strong_count = format!("{document:?}").matches("Strong(").count();
    assert_eq!(strong_count, LATER_TAG_PAIRS);
}

#[test]
fn an_ordered_list_parses_to_an_ordered_list_block() {
    // Arrange
    let ordered = "1. one\n2. two\n";
    let bullets = "- one\n- two\n";
    let items = vec![
        vec![Block::Paragraph(vec![text("one")])],
        vec![Block::Paragraph(vec![text("two")])],
    ];

    // Act
    let from_ordered = parse(ordered);
    let from_bullets = parse(bullets);

    // Assert
    assert_eq!(from_ordered.blocks, vec![Block::OrderedList(items.clone())]);
    assert_eq!(from_bullets.blocks, vec![Block::List(items)]);
}
