// SPDX-License-Identifier: Apache-2.0

use helios::markdown::{Block, Document, Inline, Table};
use helios::tui::cache::RenderCache;
use helios::tui::markdown::render;
use ratatui::style::Modifier;
use ratatui::text::Line;
use std::cell::Cell;

fn text(value: &str) -> Vec<Inline> {
    vec![Inline::Text(value.to_string())]
}

fn document(blocks: Vec<Block>) -> Document {
    Document { blocks, cut: false }
}

fn plain(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn plain_lines(lines: &[Line<'_>]) -> Vec<String> {
    lines.iter().map(plain).collect()
}

#[test]
fn headings_are_bold_and_lists_use_markers() {
    // Arrange
    let doc = document(vec![
        Block::Heading {
            level: 1,
            content: text("Title"),
        },
        Block::List(vec![
            vec![Block::Paragraph(text("alpha"))],
            vec![Block::Paragraph(text("beta"))],
        ]),
    ]);

    // Act
    let lines = render(&doc, 40);

    // Assert
    let heading = lines.first().expect("a heading line");
    let heading_text = heading
        .spans
        .iter()
        .find(|span| span.content.contains("Title"))
        .expect("a span with the heading text");
    assert!(heading_text.style.add_modifier.contains(Modifier::BOLD));
    let rendered: Vec<String> = plain_lines(&lines)
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(rendered, vec!["Title", "- alpha", "- beta"]);
}

#[test]
fn a_link_shows_its_text_then_the_url() {
    // Arrange
    let with_url = document(vec![Block::Paragraph(vec![Inline::Link {
        url: Some("https://example.com/x".to_string()),
        content: text("docs"),
    }])]);
    let without_url = document(vec![Block::Paragraph(vec![Inline::Link {
        url: None,
        content: text("docs"),
    }])]);

    // Act
    let with_url_lines = plain_lines(&render(&with_url, 80));
    let without_url_lines = plain_lines(&render(&without_url, 80));

    // Assert
    assert_eq!(with_url_lines, vec!["docs (https://example.com/x)"]);
    assert_eq!(without_url_lines, vec!["docs"]);
}

#[test]
fn a_table_aligns_its_columns() {
    // Arrange
    let doc = document(vec![Block::Table(Table {
        header: vec![text("Name"), text("Qty")],
        rows: vec![
            vec![text("apple"), text("3")],
            vec![text("fig"), text("12")],
        ],
    })]);

    // Act
    let lines = plain_lines(&render(&doc, 80));

    // Assert
    assert_eq!(
        lines,
        vec!["Name  | Qty", "----- | ---", "apple | 3  ", "fig   | 12 ",]
    );
}

#[test]
fn lines_wrap_at_the_given_width() {
    // Arrange
    let original = "the quick brown fox jumps over the lazy dog and keeps running far away";
    let doc = document(vec![Block::Paragraph(text(original))]);

    // Act
    let lines = plain_lines(&render(&doc, 20));

    // Assert
    assert!(lines.len() > 1, "expected the paragraph to wrap");
    for line in &lines {
        assert!(line.chars().count() <= 20, "line too wide: {line:?}");
    }
    assert_eq!(lines.join(" "), original);
}

#[test]
fn an_ordered_list_uses_numbers() {
    // Arrange
    let doc = document(vec![Block::OrderedList(vec![
        vec![Block::Paragraph(text("one"))],
        vec![Block::Paragraph(text("two"))],
    ])]);

    // Act
    let lines = render(&doc, 40);

    // Assert
    let rendered: Vec<String> = plain_lines(&lines)
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(rendered, vec!["1. one", "2. two"]);
}

/// The quote bar is ASCII so mono and ASCII-only terminals draw it the same.
const QUOTE_BAR: &str = "| ";
const CUT_NOTE: &str = "[showing part of this text]";
const MAX_URL_DISPLAY: usize = 80;
const HOST_PREFIX: &str = "https://host.example/";

fn link_to(url: &str) -> Document {
    document(vec![Block::Paragraph(vec![Inline::Link {
        url: Some(url.to_string()),
        content: text("docs"),
    }])])
}

fn url_of_length(length: usize) -> String {
    format!("{HOST_PREFIX}{}", "a".repeat(length - HOST_PREFIX.len()))
}

/// The URL shown between `docs (` and the closing `)` on the single line.
fn shown_url(doc: &Document) -> String {
    let lines = plain_lines(&render(doc, 400));
    assert_eq!(lines.len(), 1, "expected one line, got {lines:?}");
    lines[0]
        .strip_prefix("docs (")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or_else(|| panic!("unexpected link line: {:?}", lines[0]))
        .to_string()
}

#[test]
fn a_quote_has_a_bar_on_every_line() {
    // Arrange
    let words = "aaaa bbbb cccc dddd eeee ffff";
    let quote = document(vec![Block::Quote(vec![Block::Paragraph(text(words))])]);
    let nested = document(vec![Block::Quote(vec![Block::Quote(vec![
        Block::Paragraph(text("aaaa bbbb")),
    ])])]);

    // Act
    let quote_lines = plain_lines(&render(&quote, 12));
    let nested_lines = plain_lines(&render(&nested, 14));

    // Assert
    assert_eq!(
        quote_lines,
        vec![
            format!("{QUOTE_BAR}aaaa bbbb"),
            format!("{QUOTE_BAR}cccc dddd"),
            format!("{QUOTE_BAR}eeee ffff"),
        ]
    );
    assert_eq!(
        nested_lines,
        vec![format!("{QUOTE_BAR}{QUOTE_BAR}aaaa bbbb")]
    );
}

#[test]
fn a_cut_document_adds_its_note() {
    // Arrange
    let blocks = vec![Block::Paragraph(text("hello"))];
    let cut = Document {
        blocks: blocks.clone(),
        cut: true,
    };
    let whole = Document { blocks, cut: false };

    // Act
    let cut_lines = plain_lines(&render(&cut, 40));
    let whole_lines = plain_lines(&render(&whole, 40));

    // Assert
    assert_eq!(cut_lines.last().map(String::as_str), Some(CUT_NOTE));
    assert!(whole_lines.iter().all(|line| line != CUT_NOTE));
    assert_eq!(whole_lines, vec!["hello"]);
}

#[test]
fn a_long_url_is_shortened_but_keeps_its_host() {
    // Arrange
    let long = link_to(&url_of_length(200));
    let at_limit = url_of_length(MAX_URL_DISPLAY);
    let over_limit = url_of_length(MAX_URL_DISPLAY + 1);

    // Act
    let long_shown = shown_url(&long);
    let at_limit_shown = shown_url(&link_to(&at_limit));
    let over_limit_shown = shown_url(&link_to(&over_limit));

    // Assert
    assert!(long_shown.starts_with(HOST_PREFIX), "{long_shown}");
    assert!(long_shown.ends_with("..."), "{long_shown}");
    assert!(
        long_shown.chars().count() <= MAX_URL_DISPLAY,
        "{long_shown}"
    );
    assert_eq!(at_limit_shown, at_limit);
    assert_ne!(over_limit_shown, over_limit);
    assert!(over_limit_shown.starts_with(HOST_PREFIX));
    assert!(over_limit_shown.ends_with("..."));
    assert!(over_limit_shown.chars().count() <= MAX_URL_DISPLAY);
}

#[test]
fn a_link_whose_text_is_another_url_shows_both() {
    // Arrange
    let different = document(vec![Block::Paragraph(vec![Inline::Link {
        url: Some("https://evil.example/x".to_string()),
        content: text("https://good.example/"),
    }])]);
    let same = document(vec![Block::Paragraph(vec![Inline::Link {
        url: Some("https://good.example/".to_string()),
        content: text("https://good.example/"),
    }])]);

    // Act
    let different_lines = plain_lines(&render(&different, 80));
    let same_lines = plain_lines(&render(&same, 80));

    // Assert
    assert_eq!(
        different_lines,
        vec!["https://good.example/ (https://evil.example/x)"]
    );
    assert_eq!(same_lines, vec!["https://good.example/"]);
}

#[test]
fn wide_characters_count_by_display_width() {
    // Arrange
    let doc = document(vec![Block::Paragraph(text(&"漢".repeat(12)))]);

    // Act
    let lines = render(&doc, 10);

    // Assert
    assert!(lines.len() > 1, "expected the paragraph to wrap");
    for line in &lines {
        assert!(line.width() <= 10, "line too wide: {:?}", plain(line));
    }
}

#[test]
fn the_cache_builds_once_per_item_content_and_width() {
    // Arrange
    let cache = RenderCache::new();
    let calls = Cell::new(0_u32);
    let build = || {
        calls.set(calls.get() + 1);
        vec![
            Line::from("name  | kind"),
            Line::from("alpha | one"),
            Line::from("beta  | two"),
            Line::from("gamma | three"),
        ]
    };

    // Act
    let first = cache.lines("pr-1", "overview", 7, 80, build);
    let again = cache.lines("pr-1", "overview", 7, 80, build);

    // Assert
    assert_eq!(first.len(), 4);
    assert_eq!(first, again);
    assert_eq!(calls.get(), 1, "same item, content and width builds once");

    // Act
    cache.lines("pr-1", "overview", 8, 80, build);

    // Assert
    assert_eq!(calls.get(), 2, "a changed content hash builds again");

    // Act
    cache.lines("pr-1", "overview", 8, 60, build);

    // Assert
    assert_eq!(calls.get(), 3, "a changed width builds again");

    // Act
    cache.lines("pr-2", "overview", 8, 60, build);

    // Assert
    assert_eq!(calls.get(), 4, "a different PR clears the cache and builds");

    // Act
    cache.lines("pr-1", "overview", 8, 60, build);

    // Assert
    assert_eq!(calls.get(), 5, "the cache holds the selected PR only");
}
