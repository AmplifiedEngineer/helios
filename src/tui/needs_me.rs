// SPDX-License-Identifier: Apache-2.0

//! The Needs me list view.

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::app::refresh::Clock;
use crate::app::App;
use crate::tui::list;

/// Draws the Needs me rows, with the updated column measured against `clock`.
pub fn render(frame: &mut Frame, area: Rect, app: &App, clock: &impl Clock) {
    list::render_rows(frame, area, app, app.visible_rows(true), clock, None);
}
