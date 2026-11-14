// SPDX-License-Identifier: Apache-2.0

//! Syntax highlighting: splits source lines into runs of code roles.

use std::ops::Range;
use std::path::Path;
use std::sync::OnceLock;

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

use crate::theme::Role;

/// Longest line, in bytes, that is highlighted.
pub const MAX_HIGHLIGHT_LINE: usize = 2000;

/// The roles of one line: contiguous byte ranges that cover the whole line.
pub type LineRoles = Vec<(Role, Range<usize>)>;

/// Scope prefixes and their roles. The innermost scope with a match wins.
// `storage.type` is Keyword because Rust gives `let` and `u32` the same scope.
const SCOPE_ROLES: [(&str, Role); 12] = [
    ("comment", Role::Comment),
    ("string", Role::String),
    ("constant.numeric", Role::Number),
    ("entity.name.function", Role::Function),
    ("support.function", Role::Function),
    ("variable.function", Role::Function),
    ("entity.name.type", Role::Type),
    ("entity.name.class", Role::Type),
    ("support.type", Role::Type),
    ("support.class", Role::Type),
    ("keyword", Role::Keyword),
    ("storage", Role::Keyword),
];

static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static ROLE_SCOPES: OnceLock<Vec<(Scope, Role)>> = OnceLock::new();

fn syntaxes() -> &'static SyntaxSet {
    SYNTAXES.get_or_init(two_face::syntax::extra_newlines)
}

fn role_scopes() -> &'static [(Scope, Role)] {
    ROLE_SCOPES.get_or_init(|| {
        SCOPE_ROLES
            .iter()
            .filter_map(|(prefix, role)| Scope::new(prefix).ok().map(|scope| (scope, *role)))
            .collect()
    })
}

fn plain(line: &str) -> LineRoles {
    if line.is_empty() {
        Vec::new()
    } else {
        vec![(Role::Plain, 0..line.len())]
    }
}

/// The grammar set has no JSX syntax, so JSX names fall back to JavaScript.
fn base_name(name: &str) -> &str {
    if name == "jsx" {
        "js"
    } else {
        name
    }
}

fn find_syntax(language_or_path: &str) -> Option<&'static SyntaxReference> {
    let set = syntaxes();
    let path = Path::new(language_or_path);
    let by_extension = || {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| set.find_syntax_by_extension(base_name(ext)))
    };
    let by_file_name = || {
        path.file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| set.find_syntax_by_extension(name))
    };
    set.find_syntax_by_token(base_name(language_or_path))
        .or_else(by_extension)
        .or_else(by_file_name)
}

fn role_of(stack: &ScopeStack) -> Role {
    let table = role_scopes();
    stack
        .as_slice()
        .iter()
        .rev()
        .find_map(|scope| {
            table
                .iter()
                .find(|(prefix, _)| prefix.is_prefix_of(*scope))
                .map(|(_, role)| *role)
        })
        .unwrap_or(Role::Plain)
}

/// Adds a run to the row, joining it to the previous run when the role matches.
fn push_run(row: &mut LineRoles, role: Role, range: Range<usize>) {
    if range.is_empty() {
        return;
    }
    match row.last_mut() {
        Some((last_role, last_range)) if *last_role == role => last_range.end = range.end,
        _ => row.push((role, range)),
    }
}

/// Parses one line and returns its roles, or `None` when the parser fails.
fn line_roles(line: &str, state: &mut ParseState, stack: &mut ScopeStack) -> Option<LineRoles> {
    // The grammar set expects every line to end in a newline.
    let text = format!("{line}\n");
    let ops = state.parse_line(&text, syntaxes()).ok()?;
    let mut row = LineRoles::new();
    let mut start = 0;
    for (index, op) in ops {
        let end = index.min(line.len());
        push_run(&mut row, role_of(stack), start..end);
        start = start.max(end);
        stack.apply(&op).ok()?;
    }
    push_run(&mut row, role_of(stack), start..line.len());
    Some(row)
}

/// Highlights one block a line at a time, keeping the parser state between lines.
///
/// An unknown language gives plain lines. A line over `MAX_HIGHLIGHT_LINE`
/// is plain and is not fed to the parser, so the state for later lines stays valid.
#[derive(Clone)]
pub struct BlockHighlighter {
    parser: Option<(ParseState, ScopeStack)>,
}

impl BlockHighlighter {
    /// A highlighter for a language token or file path.
    pub fn new(language_or_path: &str) -> Self {
        Self {
            parser: find_syntax(language_or_path)
                .map(|syntax| (ParseState::new(syntax), ScopeStack::new())),
        }
    }

    /// The roles of the next line of the block.
    pub fn feed(&mut self, line: &str) -> LineRoles {
        let Some((state, stack)) = &mut self.parser else {
            return plain(line);
        };
        if line.len() > MAX_HIGHLIGHT_LINE {
            return plain(line);
        }
        line_roles(line, state, stack).unwrap_or_else(|| {
            *stack = ScopeStack::new();
            plain(line)
        })
    }
}

/// Returns one `LineRoles` per input line for a language token or file path.
pub fn highlight(language_or_path: &str, lines: &[&str]) -> Vec<LineRoles> {
    let mut block = BlockHighlighter::new(language_or_path);
    lines.iter().map(|line| block.feed(line)).collect()
}
