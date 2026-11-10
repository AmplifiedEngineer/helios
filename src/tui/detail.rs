// SPDX-License-Identifier: Apache-2.0

//! The detail view: one tab each for overview, conversation, commits, files and checks.

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use crate::app::refresh::Clock;
use crate::app::{App, DetailState, DetailTab};
use crate::model::{
    ChangedFile, CheckRun, CheckRunState, Comment, CommitSummary, DiffGap, FileDiff, PrDetail,
    PrState, ReviewDecision,
};
use crate::tui::list::{age_since, failure_message};

const COMMENTS_HEADING: &str = "Issue comments";
const THREADS_HEADING: &str = "Review threads";
const REPLY_INDENT: &str = "  ";
const LOADING_TEXT: &str = "Loading...";
const PARTIAL_NOTE: &str = "Some items are not shown: GitHub has more than were fetched.";
const FILES_LOADING_TEXT: &str = "Loading files...";
const MARK_RENAMED_FROM: &str = "renamed from";
const MARK_TRUNCATED: &str = "truncated";
const MARK_BINARY: &str = "binary";
const MARK_RENAMED: &str = "renamed";
const MARK_EMPTY: &str = "empty";
const SHORT_OID_LEN: usize = 7;

/// Draws the selected tab of the open pull request, with ages measured against `clock`.
pub fn render(frame: &mut Frame, area: Rect, app: &App, clock: &impl Clock) {
    let lines = match app.detail() {
        DetailState::Loading => vec![Line::from(LOADING_TEXT)],
        DetailState::Failed(error) => vec![Line::from(failure_message(error))],
        DetailState::Loaded(detail) => match app.tab {
            DetailTab::Overview => overview_lines(detail),
            DetailTab::Conversation => with_partial_note(detail, conversation_lines(detail, clock)),
            DetailTab::Commits => with_partial_note(detail, commit_lines(&detail.commits, clock)),
            DetailTab::Files => file_lines(app.files()),
            DetailTab::Checks => with_partial_note(detail, check_lines(&detail.check_runs)),
        },
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn with_partial_note(detail: &PrDetail, mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if detail.partial {
        lines.insert(0, Line::from(PARTIAL_NOTE));
    }
    lines
}

fn overview_lines(detail: &PrDetail) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(detail.title.to_string()),
        Line::from(format!("{} {}", detail.author, state_text(&detail.state))),
    ];
    if let Some(decision) = &detail.review_decision {
        lines.push(Line::from(review_text(decision)));
    }
    lines.push(Line::from(String::new()));
    lines.extend(
        detail
            .body
            .as_str()
            .lines()
            .map(|text| Line::from(text.to_owned())),
    );
    lines
}

fn state_text(state: &PrState) -> String {
    match state {
        PrState::Open => "Open".into(),
        PrState::Closed => "Closed".into(),
        PrState::Merged => "Merged".into(),
        PrState::Unknown(raw) => raw.to_string(),
    }
}

fn review_text(decision: &ReviewDecision) -> String {
    match decision {
        ReviewDecision::Approved => "Approved".into(),
        ReviewDecision::ChangesRequested => "Changes requested".into(),
        ReviewDecision::ReviewRequired => "Review required".into(),
        ReviewDecision::Unknown(raw) => raw.to_string(),
    }
}

fn commit_lines(commits: &[CommitSummary], clock: &impl Clock) -> Vec<Line<'static>> {
    commits
        .iter()
        .map(|commit| {
            let oid: String = commit.oid.as_str().chars().take(SHORT_OID_LEN).collect();
            let age = age_since(commit.date, clock.now());
            Line::from(format!("{oid} {} {} {age}", commit.headline, commit.author))
        })
        .collect()
}

fn file_lines(files: Option<&[ChangedFile]>) -> Vec<Line<'static>> {
    let Some(files) = files else {
        return vec![Line::from(FILES_LOADING_TEXT)];
    };
    files
        .iter()
        .map(|file| {
            let marks = file_marks(file);
            if marks.is_empty() {
                Line::from(file.path.to_string())
            } else {
                Line::from(format!("{} [{}]", file.path, marks.join(", ")))
            }
        })
        .collect()
}

fn file_marks(file: &ChangedFile) -> Vec<String> {
    let mut marks = Vec::new();
    if let Some(previous) = &file.previous_path {
        marks.push(format!("{MARK_RENAMED_FROM} {previous}"));
    }
    if let FileDiff::Unavailable { gap, .. } = &file.diff {
        match gap {
            DiffGap::Truncated => marks.push(MARK_TRUNCATED.into()),
            DiffGap::Binary => marks.push(MARK_BINARY.into()),
            DiffGap::RenamedOnly if file.previous_path.is_none() => marks.push(MARK_RENAMED.into()),
            DiffGap::RenamedOnly => {}
            DiffGap::Empty => marks.push(MARK_EMPTY.into()),
        }
    }
    marks
}

fn check_lines(runs: &[CheckRun]) -> Vec<Line<'static>> {
    runs.iter()
        .map(|run| Line::from(format!("{} {}", run.name, check_state_text(&run.state))))
        .collect()
}

fn check_state_text(state: &CheckRunState) -> String {
    match state {
        CheckRunState::Success => "Success".into(),
        CheckRunState::Failure => "Failure".into(),
        CheckRunState::Neutral => "Neutral".into(),
        CheckRunState::Cancelled => "Cancelled".into(),
        CheckRunState::Skipped => "Skipped".into(),
        CheckRunState::TimedOut => "Timed out".into(),
        CheckRunState::ActionRequired => "Action required".into(),
        CheckRunState::Queued => "Queued".into(),
        CheckRunState::InProgress => "In progress".into(),
        CheckRunState::Unknown(raw) => raw.to_string(),
    }
}

/// Issue comments first, then the review threads, each under its own heading.
fn conversation_lines(detail: &PrDetail, clock: &impl Clock) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(COMMENTS_HEADING)];
    for comment in &detail.comments {
        push_comment(&mut lines, comment, "", clock);
    }
    lines.push(Line::from(THREADS_HEADING));
    for thread in &detail.threads {
        lines.push(Line::from(thread.path.to_string()));
        for reply in &thread.comments {
            push_comment(&mut lines, reply, REPLY_INDENT, clock);
        }
    }
    lines
}

fn push_comment(
    lines: &mut Vec<Line<'static>>,
    comment: &Comment,
    indent: &str,
    clock: &impl Clock,
) {
    let age = age_since(comment.created_at, clock.now());
    lines.push(Line::from(format!("{indent}{} {age}", comment.author)));
    lines.extend(
        comment
            .body
            .as_str()
            .lines()
            .map(|text| Line::from(format!("{indent}{text}"))),
    );
}
