// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

use ratatui::text::Line;

/// The most rendered items the cache keeps for the selected pull request.
pub const MAX_CACHE_ENTRIES: usize = 2048;

/// Rendered lines of the selected pull request, keyed by item, content hash and width.
pub struct RenderCache {
    capacity: usize,
    state: RefCell<State>,
}

type Key = (String, u64, u16);

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
        }
    }

    /// Returns the cached lines for the key, or builds and stores them.
    /// A different `pr_key` than the last call clears every entry first.
    pub fn lines(
        &self,
        pr_key: &str,
        item: &str,
        content_hash: u64,
        width: u16,
        build: impl FnOnce() -> Vec<Line<'static>>,
    ) -> Vec<Line<'static>> {
        let key = (item.to_string(), content_hash, width);
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

    fn fetch(cache: &RenderCache, item: &str, builds: &Cell<u32>) -> Vec<Line<'static>> {
        cache.lines("pr", item, 1, 80, || {
            builds.set(builds.get() + 1);
            vec![Line::from(item.to_string())]
        })
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
