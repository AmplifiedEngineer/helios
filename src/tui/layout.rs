// SPDX-License-Identifier: Apache-2.0

//! Splits the terminal area into the list and detail panes.

use ratatui::layout::Rect;

/// Share of the width the list pane takes, in percent.
pub const LIST_PERCENT: u16 = 40;
/// Areas narrower than this many columns show one pane.
pub const NARROW_BELOW: u16 = 100;
/// Narrowest a pane gets when the area is wide enough for two.
pub const MIN_PANE_WIDTH: u16 = 30;
/// Narrowest terminal the interface draws in.
pub const MIN_TERMINAL_WIDTH: u16 = 40;
/// Shortest terminal the interface draws in.
pub const MIN_TERMINAL_HEIGHT: u16 = 8;

/// The pane that has the keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    List,
    Detail,
}

/// The tunable layout values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutSettings {
    pub list_percent: u16,
    pub narrow_below: u16,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            list_percent: LIST_PERCENT,
            narrow_below: NARROW_BELOW,
        }
    }
}

/// The areas the panes occupy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panes {
    Single(Rect),
    Split { list: Rect, detail: Rect },
}

/// Lays out the panes inside `area`.
pub fn panes(area: Rect, _focus: Pane, settings: &LayoutSettings) -> Panes {
    if area.width < settings.narrow_below {
        return Panes::Single(area);
    }
    let list_width = list_width(area.width, settings.list_percent);
    let list = Rect {
        width: list_width,
        ..area
    };
    let detail = Rect {
        x: area.x + list_width,
        width: area.width - list_width,
        ..area
    };
    Panes::Split { list, detail }
}

/// Keeps both panes at least `MIN_PANE_WIDTH` wide, or halves an area too small for that.
fn list_width(total: u16, percent: u16) -> u16 {
    if total < 2 * MIN_PANE_WIDTH {
        return total / 2;
    }
    let wanted = u32::from(total) * u32::from(percent) / 100;
    let wanted = u16::try_from(wanted).unwrap_or(total);
    wanted.clamp(MIN_PANE_WIDTH, total - MIN_PANE_WIDTH)
}
