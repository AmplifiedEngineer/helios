// SPDX-License-Identifier: Apache-2.0

/// Largest body, in bytes, that is parsed before the document is cut.
pub const MAX_BODY_BYTES: usize = 262_144;
/// Most blocks, nested ones included, a document may hold.
pub const MAX_BLOCKS: usize = 5000;
/// Deepest list and quote nesting a document may hold.
pub const MAX_NESTING: usize = 16;

/// A parsed body. `cut` is set when a limit stopped the parse early.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub cut: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Inline>),
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    /// Each item is a list of blocks.
    List(Vec<Vec<Block>>),
    Quote(Vec<Block>),
    Code {
        lang: Option<String>,
        text: String,
    },
    Table(Table),
    Rule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Code(String),
    Link {
        url: Option<String>,
        content: Vec<Inline>,
    },
    ImageAlt(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Table {
    pub header: Vec<Vec<Inline>>,
    pub rows: Vec<Vec<Vec<Inline>>>,
}

impl Document {
    /// Counts every block, nested ones included.
    pub fn block_count(&self) -> usize {
        count(&self.blocks)
    }

    /// Returns how deeply lists and quotes nest, 0 when there are none.
    pub fn nesting_depth(&self) -> usize {
        depth(&self.blocks)
    }
}

fn children(block: &Block) -> Vec<&[Block]> {
    match block {
        Block::List(items) => items.iter().map(Vec::as_slice).collect(),
        Block::Quote(inner) => vec![inner.as_slice()],
        _ => Vec::new(),
    }
}

fn count(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| 1 + children(block).into_iter().map(count).sum::<usize>())
        .sum()
}

fn depth(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            Block::List(_) | Block::Quote(_) => {
                1 + children(block).into_iter().map(depth).max().unwrap_or(0)
            }
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}
