// SPDX-License-Identifier: Apache-2.0

use helios::markdown::{Block, HtmlTokenizer, Inline, MAX_NESTING};

fn tokenize(html: &str) -> Vec<Block> {
    let mut tokenizer = HtmlTokenizer::new();
    tokenizer.feed(html);
    tokenizer.finish()
}

fn text(value: &str) -> Inline {
    Inline::Text(value.to_string())
}

#[test]
fn anchor_tags_become_links_with_their_url() {
    // Arrange
    let expected = vec![Block::Paragraph(vec![Inline::Link {
        url: Some("https://e.com/x".to_string()),
        content: vec![text("docs")],
    }])];

    // Act
    let double_quoted = tokenize(r#"<a href="https://e.com/x">docs</a>"#);
    let single_quoted = tokenize("<a href='https://e.com/x'>docs</a>");

    // Assert
    assert_eq!(double_quoted, expected);
    assert_eq!(single_quoted, expected);
}

#[test]
fn block_tags_become_blocks() {
    // Arrange
    let paragraph = vec![Block::Paragraph(vec![text("hello")])];
    let heading = vec![Block::Heading {
        level: 2,
        content: vec![text("Title")],
    }];
    let list = vec![Block::List(vec![
        vec![Block::Paragraph(vec![text("one")])],
        vec![Block::Paragraph(vec![text("two")])],
    ])];
    let quote = vec![Block::Quote(vec![Block::Paragraph(vec![text("said")])])];

    // Act
    let from_p = tokenize("<p>hello</p>");
    let from_h2 = tokenize("<h2>Title</h2>");
    let from_ul = tokenize("<ul><li>one</li><li>two</li></ul>");
    let from_blockquote = tokenize("<blockquote><p>said</p></blockquote>");

    // Assert
    assert_eq!(from_p, paragraph);
    assert_eq!(from_h2, heading);
    assert_eq!(from_ul, list);
    assert_eq!(from_blockquote, quote);
}

#[test]
fn summary_becomes_a_bold_line_and_details_stays_open() {
    // Arrange
    let expected = vec![
        Block::Paragraph(vec![Inline::Strong(vec![text("Release notes")])]),
        Block::Paragraph(vec![text("body")]),
    ];

    // Act
    let blocks = tokenize("<details><summary>Release notes</summary><p>body</p></details>");

    // Assert
    assert_eq!(blocks, expected);
}

#[test]
fn script_and_style_content_is_dropped() {
    // Arrange
    let expected = vec![Block::Paragraph(vec![text("beforeafter")])];

    // Act
    let blocks = tokenize("<script>alert(1)</script>before<style>p{color:red}</style>after");

    // Assert
    assert_eq!(blocks, expected);
}

#[test]
fn unknown_tags_are_removed_and_their_text_kept() {
    // Arrange
    let expected = vec![Block::Paragraph(vec![text("hi there")])];

    // Act
    let blocks = tokenize(r#"<span class="x">hi <blink>there</blink></span>"#);

    // Assert
    assert_eq!(blocks, expected);
}

#[test]
fn entities_decode_from_the_fixed_set() {
    // Arrange
    // `&nbsp;` decodes to a plain space, so the text ends in two spaces.
    let decoded = vec![Block::Paragraph(vec![text("& < > \" ' '  ")])];
    let unknown = vec![Block::Paragraph(vec![text("&bogus;")])];

    // Act
    let from_known = tokenize("<p>&amp; &lt; &gt; &quot; &#39; &#x27; &nbsp;</p>");
    let from_unknown = tokenize("<p>&bogus;</p>");

    // Assert
    assert_eq!(from_known, decoded);
    assert_eq!(from_unknown, unknown);
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
            Block::List(items) => items.iter().for_each(|item| block_strings(item, out)),
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

fn all_strings(blocks: &[Block]) -> Vec<&str> {
    let mut out = Vec::new();
    block_strings(blocks, &mut out);
    out
}

#[test]
fn numeric_entities_for_escape_and_bidi_characters_stay_out() {
    // Arrange
    // Only the fixed entity set decodes, so these references stay as literal
    // text and never become ESC or U+202E.
    let in_text = "<p>a&#x1b;b&#x202e;c</p>";
    let in_href = r#"<a href="&#x1b;https://e.com/&#x202e;x">x</a>"#;
    let in_title = r#"<a title="&#x1b;&#x202e;" href="https://e.com/y">x</a>"#;

    // Act
    let from_text = tokenize(in_text);
    let from_href = tokenize(in_href);
    let from_title = tokenize(in_title);

    // Assert
    assert_eq!(
        from_text,
        vec![Block::Paragraph(vec![text("a&#x1b;b&#x202e;c")])]
    );
    assert_eq!(
        from_href,
        vec![Block::Paragraph(vec![Inline::Link {
            url: None,
            content: vec![text("x")],
        }])]
    );
    for blocks in [&from_text, &from_href, &from_title] {
        for s in all_strings(blocks) {
            assert!(!s.contains('\u{1b}') && !s.contains('\u{202e}'), "{s:?}");
        }
    }
}

#[test]
fn an_html_anchor_with_a_javascript_href_drops_the_url() {
    // Arrange
    let expected = vec![Block::Paragraph(vec![Inline::Link {
        url: None,
        content: vec![text("x")],
    }])];

    // Act
    let plain = tokenize(r#"<a href="javascript:alert(1)">x</a>"#);
    let tab_split = tokenize(r#"<a href="java&#9;script:alert(1)">x</a>"#);

    // Assert
    assert_eq!(plain, expected);
    assert_eq!(tab_split, expected);
}

const DEPENDABOT_BODY: &str = concat!(
    "<details>\n<summary>Release notes</summary>\n",
    "<p><em>Sourced from <a href=\"https://github.com/o/r/releases/tag/v1\">o/r\'s releases</a>.</em></p>\n",
    "<blockquote>\n<h2>v1.2.0 &amp; friends</h2>\n<ul>\n<li>Fix <code>parse()</code> when a &lt; b</li>\n",
    "<li>Say &quot;hi&quot; &#39;twice&#39; &#x27;now&#x27;&nbsp;ok</li>\n</ul>\n</blockquote>\n</details>\n",
    "<p>Bumps <strong>o/r</strong> from 1.1.0 to 1.2.0.</p>"
);

fn visible_text(blocks: &[Block]) -> String {
    fn inlines(content: &[Inline], out: &mut String) {
        for inline in content {
            match inline {
                Inline::Text(s) => out.push_str(s),
                Inline::Emphasis(c) | Inline::Strong(c) | Inline::Link { content: c, .. } => {
                    inlines(c, out);
                }
                _ => {}
            }
        }
    }
    fn walk(blocks: &[Block], out: &mut String) {
        for block in blocks {
            match block {
                Block::Paragraph(c) | Block::Heading { content: c, .. } => inlines(c, out),
                Block::List(items) => items.iter().for_each(|item| walk(item, out)),
                Block::Quote(inner) => walk(inner, out),
                _ => {}
            }
        }
    }
    let mut out = String::new();
    walk(blocks, &mut out);
    out
}

fn text_nodes(blocks: &[Block]) -> Vec<String> {
    fn inlines(content: &[Inline], out: &mut Vec<String>) {
        for inline in content {
            match inline {
                Inline::Text(s) => out.push(s.clone()),
                Inline::Emphasis(c) | Inline::Strong(c) | Inline::Link { content: c, .. } => {
                    inlines(c, out);
                }
                _ => {}
            }
        }
    }
    fn walk(blocks: &[Block], out: &mut Vec<String>) {
        for block in blocks {
            match block {
                Block::Paragraph(c) | Block::Heading { content: c, .. } => inlines(c, out),
                Block::List(items) => items.iter().for_each(|item| walk(item, out)),
                Block::Quote(inner) => walk(inner, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(blocks, &mut out);
    out
}

fn has_tag_fragment(node: &str) -> bool {
    node.match_indices('<').any(|(at, _)| {
        node[at + 1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '/')
    })
}

/// A cut inside an entity leaves its first characters as text; drop them
/// before comparing, since the whole body shows the decoded character.
fn without_partial_entity(visible: &str) -> &str {
    match visible.rfind('&') {
        Some(at)
            if visible[at + 1..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '#') =>
        {
            &visible[..at]
        }
        _ => visible,
    }
}

#[test]
fn unclosed_and_misnested_tags_keep_their_text_and_leave_no_tag_fragments() {
    // Arrange
    let whole = visible_text(&tokenize(DEPENDABOT_BODY));
    let misnested = [
        ("<b><i>x</b></i>", "x"),
        ("</p>stray", "stray"),
        ("<p>a<p>b", "ab"),
        (r#"<a href="u">x"#, "x"),
        ("<li>x</ul>", "x"),
        ("<<<>>>", "<<<>>>"),
    ];

    // Act
    let results: Vec<(&str, String)> = misnested
        .iter()
        .map(|(input, _)| (*input, visible_text(&tokenize(input))))
        .collect();

    // Assert
    assert!(
        whole.contains("Release notes") && whole.contains("a < b"),
        "{whole:?}"
    );
    for cut in (0..=DEPENDABOT_BODY.len()).filter(|i| DEPENDABOT_BODY.is_char_boundary(*i)) {
        let prefix = &DEPENDABOT_BODY[..cut];
        let single = tokenize(prefix);
        for node in text_nodes(&single) {
            assert!(!has_tag_fragment(&node), "cut {cut}: {node:?}");
        }
        let visible = visible_text(&single);
        let kept = without_partial_entity(&visible);
        assert!(
            whole.starts_with(kept),
            "cut {cut}: {kept:?} is not a prefix of the whole text"
        );
        for split in (0..=cut).filter(|i| prefix.is_char_boundary(*i)) {
            let mut tokenizer = HtmlTokenizer::new();
            tokenizer.feed(&prefix[..split]);
            tokenizer.feed(&prefix[split..]);
            assert_eq!(tokenizer.finish(), single, "cut {cut}, split {split}");
        }
    }
    for (input, expected) in misnested {
        let (_, got) = results.iter().find(|(i, _)| *i == input).expect("ran");
        assert_eq!(got, expected, "input {input:?}");
    }
}

fn nested_quotes(levels: usize) -> Vec<Block> {
    let html = format!(
        "{}deep{}",
        "<blockquote>".repeat(levels),
        "</blockquote>".repeat(levels)
    );
    tokenize(&html)
}

fn quote_depth(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            Block::Quote(inner) => 1 + quote_depth(inner),
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

#[test]
fn nesting_at_the_limit_stays_and_one_deeper_is_flattened() {
    // Arrange
    let at_limit = MAX_NESTING;
    let past_limit = MAX_NESTING + 1;

    // Act
    let kept = nested_quotes(at_limit);
    let flattened = nested_quotes(past_limit);

    // Assert
    assert_eq!(quote_depth(&kept), MAX_NESTING);
    assert_eq!(visible_text(&kept), "deep");
    assert_eq!(quote_depth(&flattened), MAX_NESTING);
    assert_eq!(visible_text(&flattened), "deep");
}
