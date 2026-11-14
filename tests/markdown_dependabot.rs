// SPDX-License-Identifier: Apache-2.0

use helios::markdown::{from_remote, Block, Document, Inline};
use helios::sanitize::sanitize;

const DEPENDABOT_BODY: &str = include_str!("fixtures/markdown/dependabot.md");
const RAW_HTML_MARKERS: [&str; 4] = ["<details", "<a href", "<li>", "</"];

fn parse(body: &str) -> Document {
    from_remote(&sanitize(body))
}

fn text(value: &str) -> Inline {
    Inline::Text(value.to_string())
}

/// Every string the document would draw: text, inline code, code blocks and image alts.
fn all_text(blocks: &[Block], out: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Paragraph(content) | Block::Heading { content, .. } => inline_text(content, out),
            Block::List(items) => items.iter().for_each(|item| all_text(item, out)),
            Block::Quote(inner) => all_text(inner, out),
            Block::Code { text, .. } => out.push(text.clone()),
            Block::Table(table) => {
                let cells = table.header.iter().chain(table.rows.iter().flatten());
                cells.for_each(|cell| inline_text(cell, out));
            }
            Block::Rule => {}
        }
    }
}

fn inline_text(inlines: &[Inline], out: &mut Vec<String>) {
    for inline in inlines {
        match inline {
            Inline::Text(value) | Inline::Code(value) => out.push(value.clone()),
            Inline::Emphasis(content) | Inline::Strong(content) => inline_text(content, out),
            Inline::Link { content, .. } => inline_text(content, out),
            Inline::Image { alt, .. } => out.push(alt.clone()),
            Inline::TaskMarker(_) => {}
        }
    }
}

/// Every inline in the document, depth first, so a test can look one up by exact value.
fn all_inlines(blocks: &[Block], out: &mut Vec<Inline>) {
    fn walk(inlines: &[Inline], out: &mut Vec<Inline>) {
        for inline in inlines {
            out.push(inline.clone());
            if let Inline::Emphasis(content)
            | Inline::Strong(content)
            | Inline::Link { content, .. } = inline
            {
                walk(content, out);
            }
        }
    }
    for block in blocks {
        match block {
            Block::Paragraph(content) | Block::Heading { content, .. } => walk(content, out),
            Block::List(items) => items.iter().for_each(|item| all_inlines(item, out)),
            Block::Quote(inner) => all_inlines(inner, out),
            _ => {}
        }
    }
}

fn all_blocks<'a>(blocks: &'a [Block], out: &mut Vec<&'a Block>) {
    for block in blocks {
        out.push(block);
        match block {
            Block::List(items) => items.iter().for_each(|item| all_blocks(item, out)),
            Block::Quote(inner) => all_blocks(inner, out),
            _ => {}
        }
    }
}

#[test]
fn the_dependabot_body_has_no_raw_html_in_the_document() {
    // Arrange
    let mut texts = Vec::new();

    // Act
    let document = parse(DEPENDABOT_BODY);
    all_text(&document.blocks, &mut texts);

    // Assert
    let leaks: Vec<&String> = texts
        .iter()
        .filter(|text| RAW_HTML_MARKERS.iter().any(|marker| text.contains(marker)))
        .collect();
    assert!(leaks.is_empty(), "raw html reached the document: {leaks:?}");
    let joined = texts.concat();
    assert!(
        joined.contains("Fix a bug by"),
        "the text inside the html was dropped: {joined:?}"
    );
}

#[test]
fn the_dependabot_body_keeps_its_links_and_headings() {
    // Arrange
    let releases = Inline::Link {
        url: Some("https://github.com/serde-rs/serde/releases".to_string()),
        content: vec![text("serde's releases")],
    };
    let commit = Inline::Link {
        url: Some("https://github.com/serde-rs/serde/commit/abc".to_string()),
        content: vec![Inline::Code("abc1234".to_string())],
    };
    let release_heading = Block::Heading {
        level: 2,
        content: vec![text("v1.0.2")],
    };
    let summary = Block::Paragraph(vec![Inline::Strong(vec![text("Release notes")])]);

    // Act
    let document = parse(DEPENDABOT_BODY);
    let mut inlines = Vec::new();
    all_inlines(&document.blocks, &mut inlines);
    let mut blocks = Vec::new();
    all_blocks(&document.blocks, &mut blocks);

    // Assert
    assert!(inlines.contains(&releases), "missing the releases link");
    assert!(inlines.contains(&commit), "missing the commit link");
    assert!(
        blocks.contains(&&release_heading),
        "missing the v1.0.2 heading"
    );
    assert!(blocks.contains(&&summary), "missing the Release notes line");
}

#[test]
fn inline_html_across_events_keeps_its_nesting() {
    // Arrange
    let body = "press <em>hit <kbd>**Ctrl**</kbd></em> now";

    // Act
    let document = parse(body);

    // Assert
    let expected = Document {
        blocks: vec![Block::Paragraph(vec![
            text("press "),
            Inline::Emphasis(vec![text("hit "), Inline::Strong(vec![text("Ctrl")])]),
            text(" now"),
        ])],
        cut: false,
    };
    assert_eq!(document, expected);
}
