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

const COLUMN_SPACING: u16 = 1;
const MIN_TITLE_WIDTH: u16 = 30;
const REPO_WIDTH: u16 = 24;
const REPO_ELLIPSIS: char = '\u{2026}';

/// A table column. The title takes the width the others leave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    Host,
    Repo,
    Number,
    Title,
    Author,
    Tag,
    Draft,
    Review,
    Checks,
    Updated,
}

/// The columns in screen order.
const COLUMNS: [Column; 10] = [
    Column::Host,
    Column::Repo,
    Column::Number,
    Column::Title,
    Column::Author,
    Column::Tag,
    Column::Draft,
    Column::Review,
    Column::Checks,
    Column::Updated,
];
/// The columns a narrow terminal gives up, the first to go first. The host is only ever
/// in the table when rows come from more than one.
const DROP_ORDER: [Column; 6] = [
    Column::Draft,
    Column::Tag,
    Column::Host,
    Column::Author,
    Column::Checks,
    Column::Review,
];

impl Column {
    fn header(self) -> &'static str {
        match self {
            Self::Host => "HOST",
            Self::Repo => "REPO",
            Self::Number => "#",
            Self::Title => "TITLE",
            Self::Author => "AUTHOR",
            Self::Tag => "TAG",
            Self::Draft => "DRAFT",
            Self::Review => "REVIEW",
            Self::Checks => "CHECKS",
            Self::Updated => "UPDATED",
        }
    }

    /// The room the column needs; for the title, the least it is worth showing.
    fn width(self) -> u16 {
        match self {
            Self::Host => 16,
            Self::Repo => REPO_WIDTH,
            Self::Number => 6,
            Self::Title => MIN_TITLE_WIDTH,
            Self::Author => 12,
            Self::Tag | Self::Checks | Self::Updated => 8,
            Self::Draft => 5,
            Self::Review => 10,
        }
    }

    fn constraint(self) -> Constraint {
        match self {
            Self::Title => Constraint::Fill(1),
            other => Constraint::Length(other.width()),
        }
    }

    fn cell(self, row: &PrSummary, now: SystemTime) -> String {
        match self {
            Self::Host => sanitize(&row.key.host).to_string(),
            Self::Repo => trim_start(
                &sanitize(&row.key.repo).to_string(),
                usize::from(REPO_WIDTH),
            ),
            Self::Number => format!("#{}", row.key.number),
            Self::Title => row.title.to_string(),
            Self::Author => row.author.to_string(),
            Self::Tag => tags_text(&row.tags),
            Self::Draft => if row.is_draft { DRAFT_MARK } else { "" }.into(),
            Self::Review => decision_text(row.review_decision.as_ref()),
            Self::Checks => checks_text(row.check_state.as_ref()),
            Self::Updated => age_since(row.updated_at, now),
        }
    }
}

/// Whether the column has no text on any of `rows`. With no rows nothing is hidden.
fn is_blank_everywhere(column: Column, rows: &[&PrSummary], now: SystemTime) -> bool {
    !rows.is_empty() && rows.iter().all(|row| column.cell(row, now).is_empty())
}

/// The columns that fit in `width`: the host only when rows come from more than one, no
/// column that is blank on every row, then the low-value columns dropped in turn until the
/// title keeps its least width.
fn visible_columns(
    width: u16,
    several_hosts: bool,
    rows: &[&PrSummary],
    now: SystemTime,
) -> Vec<Column> {
    let mut columns: Vec<Column> = COLUMNS
        .into_iter()
        .filter(|column| several_hosts || *column != Column::Host)
        .filter(|column| !is_blank_everywhere(*column, rows, now))
        .collect();
    let mut drops = DROP_ORDER.into_iter();
    while needed_width(&columns) > width {
        let Some(dropped) = drops.next() else { break };
        columns.retain(|column| *column != dropped);
    }
    columns
}

fn needed_width(columns: &[Column]) -> u16 {
    let gaps = u16::try_from(columns.len().saturating_sub(1)).unwrap_or(u16::MAX);
    let widths: u16 = columns.iter().map(|column| column.width()).sum();
    widths.saturating_add(gaps.saturating_mul(COLUMN_SPACING))
}

/// `text` cut from the left to `max` characters, with an ellipsis where the start was.
fn trim_start(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().skip(count - max + 1).collect();
    format!("{REPO_ELLIPSIS}{kept}")
}
const PARTIAL_NOTE: &str = "Some rows are not shown: GitHub cut the list short.";
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
    let first_host = rows.first().map(|row| &row.key.host);
    let several_hosts = rows.iter().any(|row| Some(&row.key.host) != first_host);
    let columns = visible_columns(area.width, several_hosts, &rows, now);
    let rows = rows.into_iter().map(|row| table_row(row, &columns, now));
    let mut failures: Vec<String> = app
        .host_failures()
        .iter()
        .map(|((host, _), error)| format!("{}: {}", sanitize(host), failure_message(error)))
        .collect();
    if app.has_partial_list() {
        failures.insert(0, PARTIAL_NOTE.to_string());
    }
    let failure_height = u16::try_from(failures.len()).unwrap_or(u16::MAX);
    let [table_area, failure_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(failure_height)]).areas(area);
    let table = Table::new(rows, columns.iter().map(|column| column.constraint()))
        .column_spacing(COLUMN_SPACING)
        .header(Row::new(columns.iter().map(|column| column.header())))
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

fn table_row(row: &PrSummary, columns: &[Column], now: SystemTime) -> Row<'static> {
    Row::new(columns.iter().map(|column| column.cell(row, now)))
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
