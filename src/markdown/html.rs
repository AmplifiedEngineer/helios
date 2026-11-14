// SPDX-License-Identifier: Apache-2.0

use super::parse::{clean, url, Builder, Frame};
use super::{Block, MAX_NESTING};

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

/// Longest unfinished tag kept for a later fragment; a longer one is not a tag.
const MAX_PENDING_TAG_BYTES: usize = 4096;

const COMMENT_OPEN: &str = "<!--";
const COMMENT_CLOSE: &str = "-->";

/// Elements dropped together with their content.
const SKIPPED_ELEMENTS: [&str; 2] = ["script", "style"];

/// Turns the HTML fragments of one body into blocks, keeping state across fragments.
#[derive(Debug)]
pub struct HtmlTokenizer {
    builder: Builder,
}

impl Default for HtmlTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlTokenizer {
    pub fn new() -> Self {
        Self {
            builder: Builder::new(),
        }
    }

    pub fn feed(&mut self, fragment: &str) {
        self.builder.html(fragment);
    }

    /// Closes every open element and returns the blocks. A tag left unfinished is dropped.
    pub fn finish(self) -> Vec<Block> {
        self.builder.finish()
    }
}

/// One piece of an HTML fragment, in the order it appears.
#[derive(Debug)]
pub(super) enum Token {
    Open { name: String, href: Option<String> },
    Close(String),
    Text(String),
}

/// Splits HTML fragments into tokens, carrying unfinished input across fragments.
#[derive(Debug, Default)]
pub(super) struct Lexer {
    /// An unfinished `<...` carried over until a later fragment closes it.
    pending: String,
    /// Bytes of a pending comment already searched for its `-->`.
    comment_scanned: usize,
    /// The element whose content is being dropped, until its closing tag.
    skipping: Option<&'static str>,
}

/// A parsed tag: its lowercase name, whether it closes, and its raw attribute text.
struct Tag<'a> {
    name: String,
    closing: bool,
    attrs: &'a str,
}

impl Lexer {
    pub(super) fn feed(&mut self, fragment: &str) -> Vec<Token> {
        let mut tokens = Vec::new();
        let mut resume = std::mem::take(&mut self.comment_scanned);
        let mut rest = std::mem::take(&mut self.pending);
        rest.push_str(fragment);
        let mut rest = rest.as_str();
        while let Some(start) = rest.find('<') {
            self.text(&rest[..start], &mut tokens);
            let from_tag = &rest[start..];
            if !starts_tag(from_tag) {
                self.text("<", &mut tokens);
                rest = &from_tag[1..];
                continue;
            }
            if self.skipping.is_some() && from_tag.len() > 1 && !from_tag.starts_with("</") {
                rest = &from_tag[1..];
                continue;
            }
            if from_tag.starts_with(COMMENT_OPEN) {
                let scanned = if start == 0 { resume } else { 0 };
                match comment_end(from_tag, scanned) {
                    Some(end) => rest = &from_tag[end..],
                    None => {
                        // The whole input is at most MAX_BODY_BYTES, which bounds this buffer.
                        self.pending = from_tag.to_string();
                        self.comment_scanned = from_tag.len();
                        return tokens;
                    }
                }
                resume = 0;
                continue;
            }
            let Some(end) = tag_end(tag_window(from_tag)) else {
                if from_tag.len() > MAX_PENDING_TAG_BYTES {
                    self.text("<", &mut tokens);
                    rest = &from_tag[1..];
                    continue;
                }
                self.pending = from_tag.to_string();
                return tokens;
            };
            if let Some(tag) = parse_tag(&from_tag[1..end]) {
                self.tag(&tag, &mut tokens);
            }
            rest = &from_tag[end + 1..];
        }
        let split = partial_entity_start(rest).unwrap_or(rest.len());
        self.text(&rest[..split], &mut tokens);
        self.pending = rest[split..].to_string();
        tokens
    }

    /// Flushes a trailing partial entity as text. A tag left unfinished is dropped.
    pub(super) fn finish(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        let pending = std::mem::take(&mut self.pending);
        if pending.starts_with('&') {
            self.text(&pending, &mut tokens);
        }
        tokens
    }

    fn tag(&mut self, tag: &Tag<'_>, tokens: &mut Vec<Token>) {
        if let Some(skipped) = self.skipping {
            if tag.closing && tag.name == skipped {
                self.skipping = None;
            }
            return;
        }
        if tag.closing {
            tokens.push(Token::Close(tag.name.clone()));
            return;
        }
        if let Some(skipped) = SKIPPED_ELEMENTS
            .iter()
            .find(|skipped| **skipped == tag.name)
        {
            self.skipping = Some(skipped);
            return;
        }
        let href = if tag.name == "a" {
            attribute(tag.attrs, "href").and_then(|value| url(&value))
        } else {
            None
        };
        tokens.push(Token::Open {
            name: tag.name.clone(),
            href,
        });
    }

    fn text(&self, text: &str, tokens: &mut Vec<Token>) {
        if !text.is_empty() && self.skipping.is_none() {
            tokens.push(Token::Text(decode_entities(text)));
        }
    }
}

impl Builder {
    /// Feeds one HTML fragment; the elements it opens stay open for later events.
    pub(super) fn html(&mut self, fragment: &str) {
        let tokens = self.lexer.feed(fragment);
        self.html_tokens(tokens);
    }

    pub(super) fn html_tokens(&mut self, tokens: Vec<Token>) {
        for token in tokens {
            match token {
                Token::Text(text) => self.html_text(&text),
                Token::Open { .. } | Token::Close(_) if self.skips_tag(&token) => {}
                Token::Open { name, href } => self.open_tag(&name, href),
                Token::Close(name) => self.close_tag(&name),
            }
        }
    }

    /// Inside `pre` and `code` every tag is dropped except the one that closes it.
    fn skips_tag(&self, token: &Token) -> bool {
        let closes = match (self.stack.last(), token) {
            (Some(Frame::Pre(_)), Token::Close(name)) => name == "pre",
            (Some(Frame::InlineCode(_)), Token::Close(name)) => name == "code",
            _ => false,
        };
        matches!(
            self.stack.last(),
            Some(Frame::Pre(_) | Frame::InlineCode(_))
        ) && !closes
    }

    fn html_text(&mut self, text: &str) {
        let loose = matches!(
            self.stack.last(),
            Some(Frame::Root(_) | Frame::Item(_) | Frame::Quote(_) | Frame::List(_))
        );
        if !(loose && text.trim().is_empty()) {
            self.text(text);
        }
    }

    fn open_tag(&mut self, name: &str, href: Option<String>) {
        if let Some(level) = heading_level(name) {
            self.open_block(Frame::Heading(level, Vec::new()));
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
            "em" => self.open_inline(Frame::Emphasis(Vec::new())),
            "strong" => self.open_inline(Frame::Strong(Vec::new())),
            "code" => self.stack.push(Frame::InlineCode(String::new())),
            "a" => self.open_inline(Frame::Link(href, Vec::new())),
            "br" => self.html_text("\n"),
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
            "em" => self.close_inline(|frame| matches!(frame, Frame::Emphasis(_))),
            "strong" => self.close_inline(|frame| matches!(frame, Frame::Strong(_))),
            "code" => self.close_if(|frame| matches!(frame, Frame::InlineCode(_))),
            "a" => self.close_inline(|frame| matches!(frame, Frame::Link(..))),
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
        if self.nesting() >= MAX_NESTING {
            self.flush_implicit_paragraph();
            self.flattened += 1;
        } else {
            self.open_block(frame);
        }
    }

    /// Opens an inline container, or drops its tag past `MAX_NESTING` and keeps the text.
    fn open_inline(&mut self, frame: Frame) {
        if self.inline_nesting() >= MAX_NESTING {
            self.dropped_inline += 1;
        } else {
            self.stack.push(frame);
        }
    }

    /// Closes an inline container, or consumes the end tag of a dropped one.
    fn close_inline(&mut self, matches_top: impl Fn(&Frame) -> bool) {
        if self.dropped_inline > 0 {
            self.dropped_inline -= 1;
        } else {
            self.close_if(matches_top);
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

/// Finds where the comment at the start of `from_tag` ends, searching from `scanned`
/// bytes in, backed up far enough to catch a `-->` split across fragments.
fn comment_end(from_tag: &str, scanned: usize) -> Option<usize> {
    let from = scanned
        .saturating_sub(COMMENT_CLOSE.len() - 1)
        .max(COMMENT_OPEN.len());
    from_tag
        .as_bytes()
        .get(from..)?
        .windows(COMMENT_CLOSE.len())
        .position(|window| window == COMMENT_CLOSE.as_bytes())
        .map(|at| from + at + COMMENT_CLOSE.len())
}

/// Cuts `from_tag` to the largest char boundary within `MAX_PENDING_TAG_BYTES`.
fn tag_window(from_tag: &str) -> &str {
    let mut end = from_tag.len().min(MAX_PENDING_TAG_BYTES);
    while !from_tag.is_char_boundary(end) {
        end -= 1;
    }
    &from_tag[..end]
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
