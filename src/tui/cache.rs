// SPDX-License-Identifier: Apache-2.0

use std::cell::{Cell, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

use ratatui::text::Line;

use crate::highlight::{BlockHighlighter, LineRoles, MAX_HIGHLIGHT_LINE};
use crate::theme::{ColorDepth, Theme};
use crate::tui::diff::{CodeLines, MAX_DIFF_LINES};

/// The most rendered items the cache keeps for the selected pull request.
pub const MAX_CACHE_ENTRIES: usize = 2048;

/// The most diff files whose roles the cache keeps for the selected pull request.
pub const MAX_ROLE_ENTRIES: usize = 512;

/// The most bytes of diff code one frame highlights, across all open files.
pub const MAX_HIGHLIGHT_BYTES_PER_FRAME: usize = 65536;

/// The bytes of diff code a frame may still highlight.
#[derive(Debug)]
pub struct HighlightBudget(Cell<usize>);

impl HighlightBudget {
    pub fn new(bytes: usize) -> Self {
        Self(Cell::new(bytes))
    }

    /// A fresh budget for one frame.
    pub fn per_frame() -> Self {
        Self::new(MAX_HIGHLIGHT_BYTES_PER_FRAME)
    }

    pub fn left(&self) -> usize {
        self.0.get()
    }

    fn spend(&self, bytes: usize) {
        self.0.set(self.0.get().saturating_sub(bytes));
    }
}

/// Rendered lines of the selected pull request, keyed by item, content hash, width and theme.
pub struct RenderCache {
    capacity: usize,
    state: RefCell<State>,
    builds: Cell<usize>,
    roles: RefCell<Roles>,
    pending: Cell<bool>,
    highlighted: Cell<usize>,
}

type Key = (String, u64, u16, (Theme, ColorDepth));

/// A diff file's roles are keyed by its path and the hash of its patch.
type RoleKey = (String, u64);

/// The highlighting done so far for one patch, which later calls continue.
struct RoleEntry {
    highlighter: BlockHighlighter,
    walk: CodeLines,
    rows: Vec<LineRoles>,
    /// The patch has no more lines to highlight.
    ended: bool,
    /// The line count the latest call asked for.
    wanted: usize,
}

impl RoleEntry {
    /// Whether the latest call has all the lines it asked for.
    fn done(&self) -> bool {
        self.ended || self.rows.len() >= self.wanted
    }
}

#[derive(Default)]
struct Roles {
    pr_key: String,
    entries: HashMap<RoleKey, RoleEntry>,
    order: VecDeque<RoleKey>,
}

#[derive(Default)]
struct State {
    pr_key: String,
    entries: HashMap<Key, Vec<Line<'static>>>,
    order: VecDeque<Key>,
}

impl Default for RenderCache {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderCache {
    pub fn new() -> Self {
        Self::with_capacity(MAX_CACHE_ENTRIES)
    }

    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            state: RefCell::default(),
            builds: Cell::new(0),
            roles: RefCell::default(),
            pending: Cell::new(false),
            highlighted: Cell::new(0),
        }
    }

    /// Total bytes of diff code the highlighter has parsed.
    pub fn highlighted_bytes(&self) -> usize {
        self.highlighted.get()
    }

    /// Reads and clears the flag that says a diff still has lines to highlight.
    pub fn take_pending(&self) -> bool {
        self.pending.replace(false)
    }

    pub fn clear_pending(&self) {
        self.pending.set(false);
    }

    /// The roles of each patch line highlighted so far, one per line in order.
    ///
    /// Work continues only while `budget` has bytes left, so the result can hold fewer rows than
    /// the patch has lines; the pending flag then says more is to come.
    pub fn diff_roles(
        &self,
        pr_key: &str,
        path: &str,
        patch: &str,
        budget: &HighlightBudget,
    ) -> Vec<LineRoles> {
        self.diff_roles_to(pr_key, path, patch, MAX_DIFF_LINES, budget)
    }

    /// Like `diff_roles`, highlighting no further than the first `limit` patch lines. The entry
    /// keeps its progress, so a later call with a larger limit resumes it.
    pub fn diff_roles_to(
        &self,
        pr_key: &str,
        path: &str,
        patch: &str,
        limit: usize,
        budget: &HighlightBudget,
    ) -> Vec<LineRoles> {
        let mut hasher = DefaultHasher::new();
        patch.hash(&mut hasher);
        let key = (path.to_owned(), hasher.finish());
        let mut roles = self.roles.borrow_mut();
        if roles.pr_key != pr_key {
            *roles = Roles {
                pr_key: pr_key.to_owned(),
                ..Roles::default()
            };
        }
        if !roles.entries.contains_key(&key) {
            if roles.entries.len() >= MAX_ROLE_ENTRIES {
                if let Some(oldest) = roles.order.pop_front() {
                    roles.entries.remove(&oldest);
                }
            }
            roles.order.push_back(key.clone());
            let entry = RoleEntry {
                highlighter: BlockHighlighter::new(path),
                walk: CodeLines::default(),
                rows: Vec::new(),
                ended: false,
                wanted: 0,
            };
            roles.entries.insert(key.clone(), entry);
        }
        let Some(entry) = roles.entries.get_mut(&key) else {
            return Vec::new();
        };
        self.advance(entry, patch, limit, budget);
        let (rows, unfinished) = (entry.rows.clone(), !entry.done());
        self.update_pending(&roles, unfinished);
        rows
    }

    /// Highlights the next patch lines of `entry` until `limit` lines are done, the patch ends or
    /// `budget` runs out.
    /// Added and removed lines feed one stream in patch order, an approximation as old and new interleave.
    fn advance(&self, entry: &mut RoleEntry, patch: &str, limit: usize, budget: &HighlightBudget) {
        entry.wanted = limit;
        if entry.done() {
            return;
        }
        for line in patch.lines().take(MAX_DIFF_LINES).skip(entry.rows.len()) {
            if entry.rows.len() >= limit {
                return;
            }
            let mut walk = entry.walk;
            let code = walk.code_of(line);
            // A line over the cap is not parsed, so it costs nothing.
            let cost = code
                .map(str::len)
                .filter(|len| *len <= MAX_HIGHLIGHT_LINE)
                .unwrap_or(0);
            if cost > budget.left() {
                return;
            }
            budget.spend(cost);
            self.highlighted.set(self.highlighted.get() + cost);
            entry.walk = walk;
            let row = code.map_or_else(Vec::new, |code| entry.highlighter.feed(code));
            entry.rows.push(row);
        }
        entry.ended = true;
    }

    /// Raises the flag when the entry just drawn is unfinished, and lowers it once every entry is.
    fn update_pending(&self, roles: &Roles, drawn_unfinished: bool) {
        if drawn_unfinished {
            self.pending.set(true);
        } else if roles.entries.values().all(RoleEntry::done) {
            self.pending.set(false);
        }
    }

    /// How many times a builder ran, that is, how many lookups missed.
    pub fn builds(&self) -> usize {
        self.builds.get()
    }

    /// Returns the cached lines for the key, or builds and stores them.
    /// A different `pr_key` than the last call clears every entry first.
    pub fn lines(
        &self,
        pr_key: &str,
        item: &str,
        content_hash: u64,
        width: u16,
        look: (Theme, ColorDepth),
        build: impl FnOnce() -> Vec<Line<'static>>,
    ) -> Vec<Line<'static>> {
        let key = (item.to_string(), content_hash, width, look);
        {
            let mut state = self.state.borrow_mut();
            if state.pr_key != pr_key {
                *state = State {
                    pr_key: pr_key.to_string(),
                    ..State::default()
                };
            }
            if let Some(lines) = state.entries.get(&key) {
                return lines.clone();
            }
        }

        // The borrow is released so `build` can use the cache itself.
        self.builds.set(self.builds.get() + 1);
        let lines = build();

        let mut state = self.state.borrow_mut();
        if self.capacity > 0 {
            if state.entries.len() >= self.capacity {
                if let Some(oldest) = state.order.pop_front() {
                    state.entries.remove(&oldest);
                }
            }
            if state.entries.insert(key.clone(), lines.clone()).is_none() {
                state.order.push_back(key);
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const LOOK: (Theme, ColorDepth) = (Theme::Standard, ColorDepth::TrueColor);

    fn fetch(cache: &RenderCache, item: &str, builds: &Cell<u32>) -> Vec<Line<'static>> {
        cache.lines("pr", item, 1, 80, LOOK, || {
            builds.set(builds.get() + 1);
            vec![Line::from(item.to_string())]
        })
    }

    #[test]
    fn a_changed_theme_or_depth_builds_again() {
        // Arrange
        let cache = RenderCache::new();
        let builds = Cell::new(0);
        let build = |look| {
            cache.lines("pr", "a", 1, 80, look, || {
                builds.set(builds.get() + 1);
                vec![Line::from("a")]
            })
        };
        build(LOOK);

        // Act
        build(LOOK);
        let after_same = builds.get();
        build((Theme::Dark, ColorDepth::TrueColor));
        let after_theme = builds.get();
        build((Theme::Standard, ColorDepth::Ansi16));

        // Assert
        assert_eq!(after_same, 1, "the same look stays cached");
        assert_eq!(after_theme, 2, "another theme builds again");
        assert_eq!(builds.get(), 3, "another color depth builds again");
    }

    #[test]
    fn the_oldest_entry_is_dropped_when_the_cache_is_full() {
        // Arrange
        let cache = RenderCache::with_capacity(2);
        let builds = Cell::new(0);
        fetch(&cache, "a", &builds);
        fetch(&cache, "b", &builds);
        fetch(&cache, "c", &builds);
        assert_eq!(builds.get(), 3);

        // Act
        fetch(&cache, "c", &builds);
        let after_cached = builds.get();
        fetch(&cache, "a", &builds);

        // Assert
        assert_eq!(after_cached, 3, "c stays cached");
        assert_eq!(builds.get(), 4, "a was evicted and builds again");
    }
}
