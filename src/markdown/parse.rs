// SPDX-License-Identifier: Apache-2.0

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use super::{Block, Document, Inline, Table, MAX_BLOCKS, MAX_BODY_BYTES, MAX_NESTING};
use crate::remote::Remote;
use crate::sanitize::sanitize;

/// Link schemes kept in the document; anything else loses its URL.
const ALLOWED_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// Parses a sanitized body into a [`Document`].
pub fn from_remote(remote: &Remote) -> Document {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let (body, body_cut) = clip(remote.as_str());
    let mut builder = Builder::new();
    for event in Parser::new_ext(body, options) {
        builder.event(event);
    }
    let cut = body_cut || builder.cut;
    Document {
        blocks: builder.finish(),
        cut,
    }
}

/// Cuts `body` at the largest char boundary at or below the byte limit.
fn clip(body: &str) -> (&str, bool) {
    if body.len() <= MAX_BODY_BYTES {
        return (body, false);
    }
    let mut end = MAX_BODY_BYTES;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    (&body[..end], true)
}

/// One open element on the builder stack.
enum Frame {
    Root(Vec<Block>),
    Item(Vec<Block>),
    Quote(Vec<Block>),
    List(Vec<Vec<Block>>),
    Para(Vec<Inline>),
    Heading(u8, Vec<Inline>),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Link(Option<String>, Vec<Inline>),
    Image(Option<String>, String),
    Code(Option<String>, String),
    Table(Table, Vec<Vec<Inline>>),
    Cell(Vec<Inline>),
}

struct Builder {
    stack: Vec<Frame>,
    blocks_added: usize,
    cut: bool,
    /// Open list, quote and item tags that were flattened into their parent.
    flattened: usize,
}

impl Builder {
    fn new() -> Self {
        Self {
            stack: vec![Frame::Root(Vec::new())],
            blocks_added: 0,
            cut: false,
            flattened: 0,
        }
    }

    fn finish(mut self) -> Vec<Block> {
        match self.stack.pop() {
            Some(Frame::Root(blocks)) => blocks,
            _ => Vec::new(),
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => self.push_inline(Inline::Code(clean(&code))),
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.text("\n"),
            Event::TaskListMarker(checked) => self.push_inline(Inline::TaskMarker(checked)),
            Event::Rule => {
                self.flush_implicit_paragraph();
                self.add_block(Block::Rule);
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        if is_block(&tag) {
            self.flush_implicit_paragraph();
        }
        if self.flatten(&tag) {
            self.flattened += 1;
            return;
        }
        let frame = match tag {
            Tag::Paragraph => Frame::Para(Vec::new()),
            Tag::Heading { level, .. } => Frame::Heading(level as u8, Vec::new()),
            Tag::BlockQuote(_) => Frame::Quote(Vec::new()),
            Tag::CodeBlock(kind) => Frame::Code(code_lang(&kind), String::new()),
            Tag::List(_) => Frame::List(Vec::new()),
            Tag::Item => Frame::Item(Vec::new()),
            Tag::Table(_) => Frame::Table(Table::default(), Vec::new()),
            Tag::TableCell => Frame::Cell(Vec::new()),
            Tag::Emphasis => Frame::Emphasis(Vec::new()),
            Tag::Strong => Frame::Strong(Vec::new()),
            Tag::Link { dest_url, .. } => Frame::Link(url(&dest_url), Vec::new()),
            Tag::Image { dest_url, .. } => Frame::Image(url(&dest_url), String::new()),
            _ => return,
        };
        self.stack.push(frame);
    }

    /// Whether a container tag is too deep to keep as its own frame.
    fn flatten(&self, tag: &Tag<'_>) -> bool {
        match tag {
            Tag::Item => self.flattened > 0,
            Tag::List(_) | Tag::BlockQuote(_) => {
                self.flattened > 0 || self.nesting() >= MAX_NESTING
            }
            _ => false,
        }
    }

    fn nesting(&self) -> usize {
        self.stack
            .iter()
            .filter(|frame| matches!(frame, Frame::List(_) | Frame::Quote(_)))
            .count()
    }

    fn end(&mut self, tag: TagEnd) {
        if self.flattened > 0
            && matches!(tag, TagEnd::Item | TagEnd::List(_) | TagEnd::BlockQuote(_))
        {
            self.flattened -= 1;
            self.flush_implicit_paragraph();
            return;
        }
        match tag {
            TagEnd::Item | TagEnd::BlockQuote(_) => {
                self.flush_implicit_paragraph();
                self.close();
            }
            TagEnd::TableHead => self.end_table_head(),
            TagEnd::TableRow => self.end_table_row(),
            TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::CodeBlock
            | TagEnd::List(_)
            | TagEnd::Table
            | TagEnd::TableCell
            | TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Link
            | TagEnd::Image => self.close(),
            _ => {}
        }
    }

    fn end_table_head(&mut self) {
        if let Some(Frame::Table(table, row)) = self.stack.last_mut() {
            table.header = std::mem::take(row);
        }
    }

    fn end_table_row(&mut self) {
        if let Some(Frame::Table(table, row)) = self.stack.last_mut() {
            table.rows.push(std::mem::take(row));
        }
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
            Frame::Item(blocks) => {
                if let Some(Frame::List(items)) = self.stack.last_mut() {
                    items.push(blocks);
                }
            }
            Frame::Quote(blocks) => self.add_block(Block::Quote(blocks)),
            Frame::List(items) => self.add_block(Block::List(items)),
            Frame::Para(content) => self.add_block(Block::Paragraph(content)),
            Frame::Heading(level, content) => self.add_block(Block::Heading { level, content }),
            Frame::Code(lang, text) => self.add_block(Block::Code {
                lang,
                text: clean(text.strip_suffix('\n').unwrap_or(&text)),
            }),
            Frame::Table(table, _) => self.add_block(Block::Table(table)),
            Frame::Cell(content) => {
                if let Some(Frame::Table(_, row)) = self.stack.last_mut() {
                    row.push(content);
                }
            }
            Frame::Emphasis(content) => self.push_inline(Inline::Emphasis(content)),
            Frame::Strong(content) => self.push_inline(Inline::Strong(content)),
            Frame::Link(url, content) => self.push_inline(Inline::Link { url, content }),
            Frame::Image(url, alt) => self.push_inline(Inline::Image {
                alt: clean(&alt),
                url,
            }),
        }
    }

    /// Closes the paragraph a tight list item opens without a paragraph tag.
    fn flush_implicit_paragraph(&mut self) {
        if matches!(self.stack.last(), Some(Frame::Para(_))) {
            self.close();
        }
    }

    fn add_block(&mut self, block: Block) {
        self.blocks_added += 1;
        if self.blocks_added > MAX_BLOCKS {
            self.cut = true;
            return;
        }
        if let Some(Frame::Root(blocks) | Frame::Item(blocks) | Frame::Quote(blocks)) =
            self.stack.last_mut()
        {
            blocks.push(block);
        }
    }

    fn text(&mut self, text: &str) {
        match self.stack.last_mut() {
            Some(Frame::Code(_, buffer) | Frame::Image(_, buffer)) => buffer.push_str(text),
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
            push_merging_text(inlines, inline);
        }
    }

    fn inlines_mut(&mut self) -> Option<&mut Vec<Inline>> {
        match self.stack.last_mut() {
            Some(
                Frame::Para(inlines)
                | Frame::Heading(_, inlines)
                | Frame::Emphasis(inlines)
                | Frame::Strong(inlines)
                | Frame::Link(_, inlines)
                | Frame::Cell(inlines),
            ) => Some(inlines),
            _ => None,
        }
    }
}

fn push_merging_text(inlines: &mut Vec<Inline>, inline: Inline) {
    match (inlines.last_mut(), inline) {
        (Some(Inline::Text(last)), Inline::Text(next)) => last.push_str(&next),
        (_, inline) => inlines.push(inline),
    }
}

fn is_block(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::CodeBlock(_)
            | Tag::List(_)
            | Tag::Table(_)
    )
}

fn code_lang(kind: &CodeBlockKind<'_>) -> Option<String> {
    match kind {
        CodeBlockKind::Fenced(info) => info.split_whitespace().next().map(clean),
        CodeBlockKind::Indented => None,
    }
}

/// Sanitizes again at the leaf: numeric references decode after the first pass.
pub(super) fn clean(text: &str) -> String {
    sanitize(text).as_str().to_string()
}

/// Keeps a URL only when its scheme, read with all whitespace and controls removed, is allowed.
pub(super) fn url(dest: &str) -> Option<String> {
    let cleaned = clean(dest);
    let probe: String = cleaned
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .flat_map(char::to_lowercase)
        .collect();
    let (scheme, _) = probe.split_once(':')?;
    ALLOWED_SCHEMES.contains(&scheme).then_some(cleaned)
}
