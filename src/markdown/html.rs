// SPDX-License-Identifier: Apache-2.0

use super::parse::{clean, url};
use super::{Block, Inline, MAX_NESTING};

const HEADING_LEVELS: std::ops::RangeInclusive<u8> = 1..=6;

/// The only entities decoded; every other reference stays literal text.
const ENTITIES: [(&str, &str); 7] = [
    ("&amp;", "&"),
    ("&lt;", "<"),
    ("&gt;", ">"),
    ("&quot;", "\""),
    ("&#39;", "'"),
    ("&#x27;", "'"),
    ("&nbsp;", " "),
];

/// Elements dropped together with their content.
const SKIPPED_ELEMENTS: [&str; 2] = ["script", "style"];

/// Turns the HTML fragments of one body into blocks, keeping state across fragments.
#[derive(Debug)]
pub struct HtmlTokenizer {
    /// An unfinished `<...` carried over until a later fragment closes it.
    pending: String,
    /// The element whose content is being dropped, until its closing tag.
    skipping: Option<&'static str>,
    stack: Vec<Frame>,
    /// Lists and quotes opened past `MAX_NESTING`, whose closing tags are ignored.
    flattened: usize,
}

/// One open element on the tokenizer stack.
#[derive(Debug)]
enum Frame {
    Root(Vec<Block>),
    Item(Vec<Block>),
    Quote(Vec<Block>),
    List(Vec<Vec<Block>>),
    Para(Vec<Inline>),
    Heading(u8, Vec<Inline>),
    Summary(Vec<Inline>),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Link(Option<String>, Vec<Inline>),
    Pre(String),
    InlineCode(String),
}

/// A parsed tag: its lowercase name, whether it closes, and its raw attribute text.
struct Tag<'a> {
    name: String,
    closing: bool,
    attrs: &'a str,
}

impl Default for HtmlTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlTokenizer {
    pub fn new() -> Self {
        Self {
            pending: String::new(),
            skipping: None,
            stack: vec![Frame::Root(Vec::new())],
            flattened: 0,
        }
    }

    pub fn feed(&mut self, fragment: &str) {
        let mut rest = std::mem::take(&mut self.pending);
        rest.push_str(fragment);
        let mut rest = rest.as_str();
        while let Some(start) = rest.find('<') {
            self.text(&rest[..start]);
            let from_tag = &rest[start..];
            if !starts_tag(from_tag) {
                self.text("<");
                rest = &from_tag[1..];
                continue;
            }
            if self.skipping.is_some() && from_tag.len() > 1 && !from_tag.starts_with("</") {
                rest = &from_tag[1..];
                continue;
            }
            let Some(end) = tag_end(from_tag) else {
                self.pending = from_tag.to_string();
                return;
            };
            if let Some(tag) = parse_tag(&from_tag[1..end]) {
                self.tag(&tag);
            }
            rest = &from_tag[end + 1..];
        }
        let split = partial_entity_start(rest).unwrap_or(rest.len());
        self.text(&rest[..split]);
        self.pending = rest[split..].to_string();
    }

    /// Closes every open element and returns the blocks. A tag left unfinished is dropped.
    pub fn finish(mut self) -> Vec<Block> {
        let pending = std::mem::take(&mut self.pending);
        if pending.starts_with('&') {
            self.text(&pending);
        }
        while self.stack.len() > 1 {
            self.close();
        }
        match self.stack.pop() {
            Some(Frame::Root(blocks)) => blocks,
            _ => Vec::new(),
        }
    }

    fn tag(&mut self, tag: &Tag<'_>) {
        if let Some(skipped) = self.skipping {
            if tag.closing && tag.name == skipped {
                self.skipping = None;
            }
            return;
        }
        if self.in_literal() && !self.closes_literal(tag) {
            return;
        }
        if tag.closing {
            self.close_tag(&tag.name);
        } else {
            self.open_tag(tag);
        }
    }

    fn open_tag(&mut self, tag: &Tag<'_>) {
        let name = tag.name.as_str();
        if let Some(level) = heading_level(name) {
            self.open_block(Frame::Heading(level, Vec::new()));
            return;
        }
        if let Some(skipped) = SKIPPED_ELEMENTS.iter().find(|skipped| **skipped == name) {
            self.skipping = Some(skipped);
            return;
        }
        match name {
            "p" => self.open_block(Frame::Para(Vec::new())),
            "ul" | "ol" => self.open_container(Frame::List(Vec::new())),
            "blockquote" => self.open_container(Frame::Quote(Vec::new())),
            "pre" => self.open_block(Frame::Pre(String::new())),
            "summary" => self.open_block(Frame::Summary(Vec::new())),
            "li" if self.flattened == 0 => {
                self.flush_implicit_paragraph();
                self.close_open_item();
                self.stack.push(Frame::Item(Vec::new()));
            }
            "em" => self.stack.push(Frame::Emphasis(Vec::new())),
            "strong" => self.stack.push(Frame::Strong(Vec::new())),
            "code" => self.stack.push(Frame::InlineCode(String::new())),
            "a" => {
                let href = attribute(tag.attrs, "href").and_then(|value| url(&value));
                self.stack.push(Frame::Link(href, Vec::new()));
            }
            "br" => self.text("\n"),
            _ => {}
        }
    }

    fn close_tag(&mut self, name: &str) {
        if let Some(level) = heading_level(name) {
            self.close_if(|frame| matches!(frame, Frame::Heading(open, _) if *open == level));
            return;
        }
        if matches!(name, "ul" | "ol" | "blockquote") && self.flattened > 0 {
            self.flattened -= 1;
            return;
        }
        match name {
            "p" => self.close_if(|frame| matches!(frame, Frame::Para(_))),
            "ul" | "ol" => {
                self.flush_implicit_paragraph();
                self.close_open_item();
                self.close_if(|frame| matches!(frame, Frame::List(_)));
            }
            "li" if self.flattened == 0 => {
                self.flush_implicit_paragraph();
                self.close_open_item();
            }
            "blockquote" => {
                self.flush_implicit_paragraph();
                self.close_if(|frame| matches!(frame, Frame::Quote(_)));
            }
            "pre" => self.close_if(|frame| matches!(frame, Frame::Pre(_))),
            "summary" => self.close_if(|frame| matches!(frame, Frame::Summary(_))),
            "em" => self.close_if(|frame| matches!(frame, Frame::Emphasis(_))),
            "strong" => self.close_if(|frame| matches!(frame, Frame::Strong(_))),
            "code" => self.close_if(|frame| matches!(frame, Frame::InlineCode(_))),
            "a" => self.close_if(|frame| matches!(frame, Frame::Link(..))),
            _ => {}
        }
    }

    /// Opens a block element, first closing the paragraph that loose text opened.
    fn open_block(&mut self, frame: Frame) {
        self.flush_implicit_paragraph();
        self.stack.push(frame);
    }

    /// Opens a list or quote, or flattens it into its parent past `MAX_NESTING`.
    fn open_container(&mut self, frame: Frame) {
        let depth = self
            .stack
            .iter()
            .filter(|open| matches!(open, Frame::List(_) | Frame::Quote(_)))
            .count();
        if depth >= MAX_NESTING {
            self.flush_implicit_paragraph();
            self.flattened += 1;
        } else {
            self.open_block(frame);
        }
    }

    fn close_if(&mut self, matches_top: impl Fn(&Frame) -> bool) {
        if self.stack.last().is_some_and(matches_top) {
            self.close();
        }
    }

    fn close_open_item(&mut self) {
        self.close_if(|frame| matches!(frame, Frame::Item(_)));
    }

    /// Closes the paragraph that text outside any paragraph tag opened.
    fn flush_implicit_paragraph(&mut self) {
        self.close_if(|frame| matches!(frame, Frame::Para(_)));
    }

    fn in_literal(&self) -> bool {
        matches!(
            self.stack.last(),
            Some(Frame::Pre(_) | Frame::InlineCode(_))
        )
    }

    /// Inside `pre` and `code` every tag is text except the one that closes it.
    fn closes_literal(&self, tag: &Tag<'_>) -> bool {
        tag.closing
            && matches!(
                (self.stack.last(), tag.name.as_str()),
                (Some(Frame::Pre(_)), "pre") | (Some(Frame::InlineCode(_)), "code")
            )
    }

    /// Pops the top frame and hands the finished element to its parent.
    fn close(&mut self) {
        if self.stack.len() < 2 {
            return;
        }
        let Some(frame) = self.stack.pop() else {
            return;
        };
        match frame {
            Frame::Root(_) => {}
            Frame::Item(blocks) => match self.stack.last_mut() {
                Some(Frame::List(items)) => items.push(blocks),
                _ => blocks.into_iter().for_each(|block| self.add_block(block)),
            },
            Frame::Quote(blocks) => self.add_block(Block::Quote(blocks)),
            Frame::List(items) => self.add_block(Block::List(items)),
            Frame::Para(content) => self.add_block(Block::Paragraph(content)),
            Frame::Heading(level, content) => self.add_block(Block::Heading { level, content }),
            Frame::Summary(content) => {
                self.add_block(Block::Paragraph(vec![Inline::Strong(content)]));
            }
            Frame::Pre(text) => self.add_block(Block::Code {
                lang: None,
                text: clean(&text),
            }),
            Frame::Emphasis(content) => self.push_inline(Inline::Emphasis(content)),
            Frame::Strong(content) => self.push_inline(Inline::Strong(content)),
            Frame::Link(url, content) => self.push_inline(Inline::Link { url, content }),
            Frame::InlineCode(text) => self.push_inline(Inline::Code(clean(&text))),
        }
    }

    fn add_block(&mut self, block: Block) {
        if let Some(Frame::Root(blocks) | Frame::Item(blocks) | Frame::Quote(blocks)) =
            self.stack.last_mut()
        {
            blocks.push(block);
        }
    }

    fn text(&mut self, text: &str) {
        if text.is_empty() || self.skipping.is_some() {
            return;
        }
        let decoded = decode_entities(text);
        let text = decoded.as_str();
        match self.stack.last_mut() {
            Some(Frame::Pre(buffer) | Frame::InlineCode(buffer)) => buffer.push_str(text),
            Some(Frame::Root(_) | Frame::Item(_) | Frame::Quote(_) | Frame::List(_))
                if text.trim().is_empty() => {}
            _ => self.push_inline(Inline::Text(clean(text))),
        }
    }

    fn push_inline(&mut self, inline: Inline) {
        if matches!(
            self.stack.last(),
            Some(Frame::Root(_) | Frame::Item(_) | Frame::Quote(_))
        ) {
            self.stack.push(Frame::Para(Vec::new()));
        }
        if let Some(inlines) = self.inlines_mut() {
            match (inlines.last_mut(), inline) {
                (Some(Inline::Text(last)), Inline::Text(next)) => last.push_str(&next),
                (_, inline) => inlines.push(inline),
            }
        }
    }

    fn inlines_mut(&mut self) -> Option<&mut Vec<Inline>> {
        match self.stack.last_mut() {
            Some(
                Frame::Para(inlines)
                | Frame::Heading(_, inlines)
                | Frame::Summary(inlines)
                | Frame::Emphasis(inlines)
                | Frame::Strong(inlines)
                | Frame::Link(_, inlines),
            ) => Some(inlines),
            _ => None,
        }
    }
}

/// Replaces each entity in `ENTITIES` with its character and leaves all other text as is.
fn decode_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        decoded.push_str(&rest[..at]);
        rest = &rest[at..];
        match ENTITIES.iter().find(|(name, _)| rest.starts_with(name)) {
            Some((name, character)) => {
                decoded.push_str(character);
                rest = &rest[name.len()..];
            }
            None => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

/// Finds a trailing `&...` that could still grow into a known entity once more text arrives.
fn partial_entity_start(text: &str) -> Option<usize> {
    let start = text.rfind('&')?;
    let tail = &text[start..];
    ENTITIES
        .iter()
        .any(|(name, _)| name.len() > tail.len() && name.starts_with(tail))
        .then_some(start)
}

fn heading_level(name: &str) -> Option<u8> {
    let digits = name.strip_prefix('h')?;
    if digits.len() != 1 {
        return None;
    }
    digits
        .parse::<u8>()
        .ok()
        .filter(|level| HEADING_LEVELS.contains(level))
}

/// Finds the `>` that ends the tag starting at `from_tag[0]`, skipping quoted values.
fn tag_end(from_tag: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (index, c) in from_tag.char_indices() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(index),
            (None, _) => {}
        }
    }
    None
}

/// Reads the text between `<` and `>`; comments and declarations yield no tag.
fn parse_tag(inner: &str) -> Option<Tag<'_>> {
    let (closing, body) = match inner.strip_prefix('/') {
        Some(body) => (true, body),
        None => (false, inner),
    };
    if !body.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let name_end = body
        .find(|c: char| c.is_whitespace() || c == '/')
        .unwrap_or(body.len());
    let (name, attrs) = body.split_at(name_end);
    Some(Tag {
        name: name.to_ascii_lowercase(),
        closing,
        attrs,
    })
}

/// Returns the value of the named attribute, quoted with either quote or bare.
fn attribute(attrs: &str, wanted: &str) -> Option<String> {
    let mut rest = attrs;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        if rest.is_empty() {
            return None;
        }
        let name_end = rest
            .find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(rest.len());
        let (name, after_name) = rest.split_at(name_end);
        let Some(after_equals) = after_name.trim_start().strip_prefix('=') else {
            rest = after_name;
            continue;
        };
        let (value, remaining) = split_value(after_equals.trim_start());
        if name.eq_ignore_ascii_case(wanted) {
            return Some(clean(&decode_entities(value)));
        }
        rest = remaining;
    }
}

/// Splits an attribute value from what follows it.
fn split_value(input: &str) -> (&str, &str) {
    if let Some(quote) = input.chars().next().filter(|c| matches!(c, '"' | '\'')) {
        let body = &input[quote.len_utf8()..];
        return match body.find(quote) {
            Some(end) => (&body[..end], &body[end + quote.len_utf8()..]),
            None => (body, ""),
        };
    }
    let end = input.find(char::is_whitespace).unwrap_or(input.len());
    input.split_at(end)
}

/// Whether the `<` opening `from_tag` can start a tag; any other `<` is plain text.
/// A lone `<` at the end is held back until more input arrives.
fn starts_tag(from_tag: &str) -> bool {
    from_tag[1..]
        .chars()
        .next()
        .is_none_or(|c| c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?'))
}
