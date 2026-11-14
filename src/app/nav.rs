// SPDX-License-Identifier: Apache-2.0

//! Moving around: the list cursor, the open pull request, its tabs, files and diff.

use super::{App, DetailTab, ListView, LoadRequest, Pane, Screen};
use crate::model::{ChangedFile, FileDiff, PrKey, PrSummary};
use crate::tui::diff::MAX_DIFF_LINES;

/// One move of a cursor or a scroll position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Next,
    Previous,
    First,
    Last,
}

impl Step {
    /// Where a position over `len` items lands; it never leaves `0..len`.
    fn land(self, position: usize, len: usize) -> usize {
        let last = len.saturating_sub(1);
        match self {
            Self::Next => position.saturating_add(1).min(last),
            Self::Previous => position.saturating_sub(1).min(last),
            Self::First => 0,
            Self::Last => last,
        }
    }
}

impl App {
    /// The rows of the list the screen shows, drafts and filter applied.
    pub fn list_rows(&self) -> Vec<&PrSummary> {
        match self.view {
            ListView::Mine => self.visible_mine_rows(),
            ListView::NeedsMe => self.visible_rows(true),
        }
    }

    /// The cursor of the list the screen shows.
    pub(super) fn cursor_slot(&mut self) -> &mut usize {
        match self.view {
            ListView::Mine => &mut self.mine_cursor,
            ListView::NeedsMe => &mut self.needs_me_cursor,
        }
    }

    /// The row index the cursor is on, kept inside the list.
    pub fn list_cursor(&self) -> usize {
        let cursor = match self.view {
            ListView::Mine => self.mine_cursor,
            ListView::NeedsMe => self.needs_me_cursor,
        };
        cursor.min(self.list_rows().len().saturating_sub(1))
    }

    /// The row the list cursor is on.
    pub fn highlighted(&self) -> Option<&PrSummary> {
        self.list_rows().get(self.list_cursor()).copied()
    }

    /// The pull request the user is looking at: the list row under the cursor on the list,
    /// the open pull request elsewhere.
    pub fn current_pr(&self) -> Option<&PrKey> {
        match self.screen {
            Screen::List => self.highlighted().map(|row| &row.key),
            Screen::Detail | Screen::Diff => self.selected.as_ref(),
        }
    }

    pub fn move_cursor(&mut self, step: Step) {
        let landed = step.land(self.list_cursor(), self.list_rows().len());
        *self.cursor_slot() = landed;
        self.cursor_moved = true;
    }

    /// Switches between the Mine and Needs me lists, with the cursor on the first row.
    pub fn toggle_view(&mut self) {
        self.view = self.view.other();
        *self.cursor_slot() = 0;
        self.cursor_moved = true;
    }

    /// Runs `change`, then puts the list cursor back on the row it was on, when that row remains.
    pub(super) fn keep_highlight(&mut self, change: impl FnOnce(&mut Self)) {
        let before = self.highlighted().map(|row| row.key.clone());
        change(self);
        let index = before.and_then(|key| self.list_rows().iter().position(|row| row.key == key));
        if let Some(index) = index {
            *self.cursor_slot() = index;
        }
    }

    /// Opens the pull request under the list cursor on its Overview tab: on the detail screen,
    /// or in the detail pane when the screen is split.
    pub fn open_highlighted(&mut self) {
        let Some(key) = self.highlighted().map(|row| row.key.clone()) else {
            return;
        };
        self.open(key);
        self.tab = DetailTab::Overview;
        if self.split {
            self.pane = Pane::Detail;
        } else {
            self.screen = Screen::Detail;
        }
    }

    /// Goes back one step: from the diff to the detail, from the detail to the list.
    pub fn back(&mut self) {
        match self.screen {
            Screen::Diff if self.split => {
                self.screen = Screen::List;
                self.pane = Pane::Detail;
            }
            Screen::Diff => self.screen = Screen::Detail,
            Screen::Detail | Screen::List => {
                self.screen = Screen::List;
                self.pane = Pane::List;
            }
        }
        self.show_help = false;
        self.detail_scroll = 0;
    }

    /// The first row of the open tab that is drawn; the Files tab scrolls with its cursor.
    pub fn detail_scroll(&self) -> usize {
        self.detail_scroll.min(self.detail_scroll_max.get())
    }

    /// Tells the app how far the open tab can scroll; the detail view calls it on every draw.
    pub fn set_detail_scroll_max(&self, max: usize) {
        self.detail_scroll_max.set(max);
    }

    /// Scrolls the open tab, which is not the Files tab.
    pub fn scroll_detail(&mut self, step: Step) {
        let max = self.detail_scroll_max.get();
        self.detail_scroll = step.land(self.detail_scroll(), max.saturating_add(1));
    }

    pub fn next_tab(&mut self) {
        self.tab = self.tab.next();
        self.detail_scroll = 0;
        self.ask_for_files();
    }

    pub fn previous_tab(&mut self) {
        self.tab = self.tab.previous();
        self.detail_scroll = 0;
        self.ask_for_files();
    }

    /// Asks for the changed files the first time the Files tab shows.
    fn ask_for_files(&mut self) {
        if self.tab != DetailTab::Files || self.files.is_some() || self.files_requested {
            return;
        }
        if let Some(key) = self.selected.clone() {
            self.files_requested = true;
            self.requests.push(LoadRequest::Files(key));
        }
    }

    /// The row index of the file the Files tab has selected.
    pub fn file_cursor(&self) -> usize {
        self.file_cursor
            .min(self.files().map_or(0, <[_]>::len).saturating_sub(1))
    }

    pub fn move_file(&mut self, step: Step) {
        let len = self.files().map_or(0, <[_]>::len);
        self.file_cursor = step.land(self.file_cursor(), len);
    }

    /// The file the Files tab has selected.
    pub fn selected_file(&self) -> Option<&ChangedFile> {
        self.files()?.get(self.file_cursor())
    }

    /// Opens the diff of the selected file.
    pub fn open_file(&mut self) {
        if self.tab == DetailTab::Files && self.selected_file().is_some() {
            self.screen = Screen::Diff;
            self.diff_scroll = 0;
        }
    }

    /// The first diff line shown.
    pub fn diff_scroll(&self) -> usize {
        self.diff_scroll.min(self.diff_last_line())
    }

    pub fn scroll_diff(&mut self, step: Step) {
        self.diff_scroll = step.land(self.diff_scroll(), self.diff_last_line() + 1);
    }

    /// The index of the last line the diff draws, the cut note included.
    fn diff_last_line(&self) -> usize {
        let Some(file) = self.selected_file() else {
            return 0;
        };
        let FileDiff::Patch(patch) = &file.diff else {
            return 0;
        };
        patch
            .as_str()
            .lines()
            .take(MAX_DIFF_LINES + 1)
            .count()
            .saturating_sub(1)
    }
}
