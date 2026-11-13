// SPDX-License-Identifier: Apache-2.0

//! Fenced code blocks with a known language are colored by token role.

#[allow(dead_code)]
mod support;

use helios::markdown::{from_remote, Document};
use helios::sanitize::sanitize;
use helios::theme::{ColorDepth, Theme};
use helios::tui::markdown::{render, render_styled};
use helios::tui::style::Styles;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};
use support::render::cell_colors;

const WIDTH: u16 = 40;
const HEIGHT: u16 = 3;
const SOURCE: &str = "fn main() { let s = \"hi\"; }";
/// Columns a code line sits to the right of the block's left edge.
const CODE_INDENT: u16 = 4;
/// The `f` of `fn` in the code line.
const KEYWORD_X: u16 = CODE_INDENT;
/// The `h` inside the string `"hi"`: the indent plus `fn main() { let s = "`.
const STRING_X: u16 = CODE_INDENT + 21;
/// The standard theme's keyword color, 0xcc79a7.
const STANDARD_KEYWORD: Color = Color::Rgb(0xcc, 0x79, 0xa7);
/// The standard theme's string color, 0x009e73.
const STANDARD_STRING: Color = Color::Rgb(0x00, 0x9e, 0x73);

fn parse(body: &str) -> Document {
    from_remote(&sanitize(body))
}

fn fenced(language: &str) -> Document {
    parse(&format!("```{language}\n{SOURCE}\n```"))
}

fn styles(theme: Theme) -> Styles {
    Styles::new(theme, ColorDepth::TrueColor)
}

fn draw(lines: Vec<Line<'static>>) -> Buffer {
    let area = Rect::new(0, 0, WIDTH, HEIGHT);
    let mut buffer = Buffer::empty(area);
    Paragraph::new(lines).render(area, &mut buffer);
    buffer
}

fn plain(lines: &[Line<'_>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect()
}

#[test]
fn a_fenced_rust_block_is_colored_by_role() {
    // Arrange
    let document = fenced("rust");

    // Act
    let colored = draw(render_styled(&document, WIDTH, &styles(Theme::Standard)));
    let mono = draw(render_styled(&document, WIDTH, &styles(Theme::Mono)));

    // Assert
    assert_eq!(
        cell_colors(&colored, KEYWORD_X, 0).0,
        STANDARD_KEYWORD,
        "the f of fn"
    );
    assert_eq!(
        cell_colors(&colored, STRING_X, 0).0,
        STANDARD_STRING,
        "the h inside the string"
    );
    assert_eq!(cell_colors(&mono, KEYWORD_X, 0).0, Color::Reset);
    assert_eq!(cell_colors(&mono, STRING_X, 0).0, Color::Reset);
}

#[test]
fn a_block_with_no_language_is_plain() {
    // Arrange
    let document = parse(&format!("```\n{SOURCE}\n```"));

    // Act
    let lines = render_styled(&document, WIDTH, &styles(Theme::Standard));
    let buffer = draw(lines.clone());

    // Assert
    assert_eq!(plain(&lines), plain(&render(&document, WIDTH)));
    for x in 0..WIDTH {
        assert_eq!(
            cell_colors(&buffer, x, 0),
            (Color::Reset, Color::Reset),
            "column {x}"
        );
    }
}

#[test]
fn mono_keeps_code_blocks_uncolored() {
    // Arrange
    let document = fenced("rust");

    // Act
    let buffer = draw(render_styled(&document, WIDTH, &styles(Theme::Mono)));

    // Assert
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            assert_eq!(
                cell_colors(&buffer, x, y),
                (Color::Reset, Color::Reset),
                "cell {x},{y}"
            );
        }
    }
}

#[test]
fn highlighting_leaves_the_text_of_every_line_unchanged() {
    // Arrange
    let document = fenced("rust");

    // Act
    let colored = render_styled(&document, WIDTH, &styles(Theme::Standard));

    // Assert
    assert_eq!(plain(&colored), plain(&render(&document, WIDTH)));
}
