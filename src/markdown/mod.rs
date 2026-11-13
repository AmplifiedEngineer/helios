// SPDX-License-Identifier: Apache-2.0

//! Markdown rendering for GitHub bodies, starting with a plain data model.

mod html;
mod model;
mod parse;

pub use html::HtmlTokenizer;
pub use model::{Block, Document, Inline, Table, MAX_BLOCKS, MAX_BODY_BYTES, MAX_NESTING};
pub use parse::from_remote;
