// SPDX-License-Identifier: Apache-2.0

use helios::markdown::{Block, Document, Inline};

fn paragraph(text: &str) -> Block {
    Block::Paragraph(vec![Inline::Text(text.to_string())])
}

#[test]
fn a_document_counts_its_blocks_for_the_limit() {
    // Arrange: three top level blocks, the last one a list holding a
    // paragraph and a nested list that holds one more paragraph.
    let nested_list = Block::List(vec![vec![paragraph("deep")]]);
    let outer_list = Block::List(vec![vec![paragraph("item"), nested_list]]);
    let document = Document {
        blocks: vec![paragraph("first"), paragraph("second"), outer_list],
        cut: false,
    };

    // Act
    let block_count = document.block_count();
    let nesting_depth = document.nesting_depth();

    // Assert: 3 top level blocks, plus "item", the nested list and "deep".
    assert_eq!(block_count, 6);
    // Assert: the outer list is depth 1 and the nested list is depth 2.
    assert_eq!(nesting_depth, 2);
    assert!(!document.cut);
}
