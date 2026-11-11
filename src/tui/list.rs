// SPDX-License-Identifier: Apache-2.0

//! The row table the Mine and Needs me views share.

use std::time::SystemTime;

use chrono::{DateTime, Utc};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Paragraph, Row, Table, TableState};
use ratatui::Frame;

use crate::app::refresh::Clock;
use crate::app::App;
use crate::github::errors::GithubError;
use crate::model::{CheckState, PrSummary, ReviewDecision, Tag};
use crate::sanitize::sanitize;

const HEADER: [&str; 10] = [
    "HOST", "REPO", "#", "TITLE", "AUTHOR", "TAG", "DRAFT", "REVIEW", "CHECKS", "UPDATED",
];
const COLUMN_WIDTHS: [Constraint; 10] = [
    Constraint::Length(12),
    Constraint::Length(22),
    Constraint::Length(6),
    Constraint::Min(16),
    Constraint::Length(12),
    Constraint::Length(8),
    Constraint::Length(5),
    Constraint::Length(10),
    Constraint::Length(8),
    Constraint::Length(8),
];
const DRAFT_MARK: &str = "draft";
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3_600;
const SECONDS_PER_DAY: u64 = 86_400;

/// Draws the rows that pass the draft toggle, and the tag filter when `by_tag` is set.
pub fn render(frame: &mut Frame, area: Rect, app: &App, clock: &impl Clock, by_tag: bool) {
    render_rows(frame, area, app, app.visible_rows(by_tag), clock, None);
}

/// Draws `rows`, with the one at `selected` highlighted, and a line for each failed account.
pub fn render_rows(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    rows: Vec<&PrSummary>,
    clock: &impl Clock,
    selected: Option<usize>,
) {
    let now = clock.now();
    let rows = rows.into_iter().map(|row| table_row(row, now));
    let failures: Vec<String> = app
        .host_failures()
        .iter()
        .map(|((host, _), error)| format!("{}: {}", sanitize(host), failure_message(error)))
        .collect();
    let failure_height = u16::try_from(failures.len()).unwrap_or(u16::MAX);
    let [table_area, failure_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(failure_height)]).areas(area);
    let table = Table::new(rows, COLUMN_WIDTHS)
        .header(Row::new(HEADER))
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut state = TableState::default().with_selected(selected);
    frame.render_stateful_widget(table, table_area, &mut state);
    frame.render_widget(Paragraph::new(failures.join("\n")), failure_area);
}

/// A fixed message per failure kind, so no text from GitHub reaches the screen.
pub(crate) fn failure_message(error: &GithubError) -> &'static str {
    match error {
        GithubError::AuthRequired => "GitHub rejected the credentials",
        GithubError::PermissionDenied => "the token is not allowed to do this",
        GithubError::SsoBlocked => "single sign-on is required for this token",
        GithubError::NotFoundOrHidden => "not found or hidden from this token",
        GithubError::RateLimited { .. } => "GitHub rate limit reached",
        GithubError::Network => "could not reach GitHub",
        GithubError::Timeout => "GitHub did not answer in time",
        GithubError::Api { .. } => "GitHub answered with an error",
        GithubError::Decode => "GitHub sent a response helios could not read",
    }
}

fn table_row(row: &PrSummary, now: SystemTime) -> Row<'static> {
    Row::new([
        sanitize(&row.key.host).to_string(),
        sanitize(&row.key.repo).to_string(),
        format!("#{}", row.key.number),
        row.title.to_string(),
        row.author.to_string(),
        tags_text(&row.tags),
        if row.is_draft { DRAFT_MARK } else { "" }.into(),
        decision_text(row.review_decision.as_ref()),
        checks_text(row.check_state.as_ref()),
        age_since(row.updated_at, now),
    ])
}

fn tags_text(tags: &[Tag]) -> String {
    let names: Vec<&str> = tags
        .iter()
        .map(|tag| match tag {
            Tag::Review => "review",
            Tag::Assigned => "assigned",
        })
        .collect();
    names.join(",")
}

fn decision_text(decision: Option<&ReviewDecision>) -> String {
    match decision {
        None => String::new(),
        Some(ReviewDecision::Approved) => "approved".into(),
        Some(ReviewDecision::ChangesRequested) => "changes".into(),
        Some(ReviewDecision::ReviewRequired) => "required".into(),
        Some(ReviewDecision::Unknown(raw)) => raw.to_string(),
    }
}

fn checks_text(state: Option<&CheckState>) -> String {
    match state {
        None => String::new(),
        Some(CheckState::Success) => "pass".into(),
        Some(CheckState::Failure) => "fail".into(),
        Some(CheckState::Error) => "error".into(),
        Some(CheckState::Pending) => "pending".into(),
        Some(CheckState::Expected) => "expected".into(),
        Some(CheckState::Unknown(raw)) => raw.to_string(),
    }
}

/// How long before `now` the instant was, as the largest whole unit.
pub(crate) fn age_since(instant: DateTime<Utc>, now: SystemTime) -> String {
    let seconds = now
        .duration_since(SystemTime::from(instant))
        .map_or(0, |elapsed| elapsed.as_secs());
    match seconds {
        s if s >= SECONDS_PER_DAY => format!("{}d", s / SECONDS_PER_DAY),
        s if s >= SECONDS_PER_HOUR => format!("{}h", s / SECONDS_PER_HOUR),
        s => format!("{}m", s / SECONDS_PER_MINUTE),
    }
}
