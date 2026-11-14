// SPDX-License-Identifier: Apache-2.0

//! The detail view: one tab each for overview, conversation, commits, files and checks.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use crate::app::refresh::Clock;
use crate::app::{App, DetailState, DetailTab};
use crate::github::octo::MAX_FILES;
use crate::markdown::from_remote;
use crate::model::{
    ChangedFile, CheckRun, CheckRunState, Comment, CommitSummary, DiffGap, FileDiff, PrDetail,
    PrKey, PrState, ReviewDecision, Shown,
};
use crate::remote::Remote;
use crate::tui::cache::RenderCache;
use crate::tui::list::{age_since, failure_message};
use crate::tui::markdown::render as render_markdown;

const COMMENTS_HEADING: &str = "Issue comments";
const THREADS_HEADING: &str = "Review threads";
const REPLY_INDENT: &str = "  ";
const LOADING_TEXT: &str = "Loading...";
const COMMENTS_NOUN: &str = "comments";
const THREADS_NOUN: &str = "review threads";
const THREAD_COMMENTS_NOUN: &str = "comments in this thread";
const COMMITS_NOUN: &str = "commits";
const CHECKS_NOUN: &str = "checks";
const FILES_CUT_NOTE_PREFIX: &str = "Showing the first";
const FILES_CUT_NOTE_SUFFIX: &str = "files";
const FILES_LOADING_TEXT: &str = "Loading files...";
const MARK_RENAMED_FROM: &str = "renamed from";
const MARK_TRUNCATED: &str = "truncated";
const MARK_CHANGED_NO_TEXT: &str = "binary or mode change";
const MARK_RENAMED: &str = "renamed";
const MARK_ADDED_OR_REMOVED_NO_TEXT: &str = "empty or binary";
const SHORT_OID_LEN: usize = 7;
const OVERVIEW_ITEM: &str = "overview";

/// Renders markdown bodies through the cache for one pull request at one width.
struct Bodies<'a> {
    cache: &'a RenderCache,
    pr_key: String,
    width: u16,
}

impl Bodies<'_> {
    /// The body as styled lines, each starting with `indent`.
    fn lines(&self, item: &str, body: &Remote, indent: &str) -> Vec<Line<'static>> {
        let mut hasher = DefaultHasher::new();
        body.as_str().hash(&mut hasher);
        let indent_width = u16::try_from(indent.chars().count()).unwrap_or(u16::MAX);
        let width = self.width.saturating_sub(indent_width);
        let lines = self
            .cache
            .lines(&self.pr_key, item, hasher.finish(), width, || {
                render_markdown(&from_remote(body), width)
            });
        if indent.is_empty() {
            return lines;
        }
        lines
            .into_iter()
            .map(|mut line| {
                line.spans.insert(0, indent.to_owned().into());
                line
            })
            .collect()
    }
}

/// Draws the selected tab, reusing rendered lines from `cache`.
pub fn render_cached(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    clock: &impl Clock,
    cache: &RenderCache,
) {
    let bodies = Bodies {
        cache,
        pr_key: app.selected.as_ref().map(pr_key_text).unwrap_or_default(),
        width: area.width,
    };
    let lines = match app.detail() {
        DetailState::Loading => vec![Line::from(LOADING_TEXT)],
        DetailState::Failed(error) => vec![Line::from(failure_message(error))],
        DetailState::Loaded(detail) => match app.tab {
            DetailTab::Overview => overview_lines(detail, &bodies),
            DetailTab::Conversation => conversation_lines(detail, clock, &bodies),
            DetailTab::Commits => with_count_note(
                detail.counts.commits,
                COMMITS_NOUN,
                commit_lines(&detail.commits, clock),
            ),
            DetailTab::Files => file_lines(app),
            DetailTab::Checks => with_count_note(
                detail.counts.checks,
                CHECKS_NOUN,
                check_lines(&detail.check_runs),
            ),
        },
    };
    let on_files = app.tab == DetailTab::Files;
    let files_offset = on_files.then(|| files_scroll(app, &lines, area));
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let scroll = if let Some(offset) = files_offset {
        offset
    } else {
        let rows = paragraph.line_count(area.width);
        app.set_detail_scroll_max(rows.saturating_sub(usize::from(area.height)));
        u16::try_from(app.detail_scroll()).unwrap_or(u16::MAX)
    };
    frame.render_widget(paragraph.scroll((scroll, 0)), area);
}

/// Draws the selected tab of the open pull request, with ages measured against `clock`.
pub fn render(frame: &mut Frame, area: Rect, app: &App, clock: &impl Clock) {
    render_cached(frame, area, app, clock, &RenderCache::new());
}

fn pr_key_text(key: &PrKey) -> String {
    format!("{}/{}/{}#{}", key.host, key.account, key.repo, key.number)
}

/// How far the Files tab scrolls so the last row of the selected file stays in view, counting
/// the rows a long path wraps onto.
fn files_scroll(app: &App, lines: &[Line<'static>], area: Rect) -> u16 {
    let selected = app.file_cursor();
    let note_rows = usize::from(app.files_truncated());
    let through_selected: Vec<Line<'static>> = lines
        .iter()
        .take(selected + note_rows + 1)
        .cloned()
        .collect();
    let rows = Paragraph::new(through_selected)
        .wrap(Wrap { trim: false })
        .line_count(area.width);
    let hidden = rows.saturating_sub(usize::from(area.height));
    u16::try_from(hidden).unwrap_or(u16::MAX)
}

/// The "showing N of M" line for a cut list, or nothing when the list is whole.
fn count_note(count: Shown, noun: &str) -> Option<Line<'static>> {
    count
        .is_cut()
        .then(|| Line::from(format!("showing {} of {} {noun}", count.shown, count.total)))
}

fn with_count_note(count: Shown, noun: &str, mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if let Some(note) = count_note(count, noun) {
        lines.insert(0, note);
    }
    lines
}

fn overview_lines(detail: &PrDetail, bodies: &Bodies) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(detail.title.to_string()),
        Line::from(format!("{} {}", detail.author, state_text(&detail.state))),
    ];
    if let Some(decision) = &detail.review_decision {
        lines.push(Line::from(review_text(decision)));
    }
    lines.push(Line::from(String::new()));
    lines.extend(bodies.lines(OVERVIEW_ITEM, &detail.body, ""));
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

fn file_lines(app: &App) -> Vec<Line<'static>> {
    if let Some(error) = app.files_failure() {
        return vec![Line::from(failure_message(error))];
    }
    let Some(files) = app.files() else {
        return vec![Line::from(FILES_LOADING_TEXT)];
    };
    let selected = app.file_cursor();
    let note = app.files_truncated().then(|| {
        Line::from(format!(
            "{FILES_CUT_NOTE_PREFIX} {MAX_FILES} {FILES_CUT_NOTE_SUFFIX}"
        ))
    });
    note.into_iter()
        .chain(files.iter().enumerate().map(|(index, file)| {
            let marks = file_marks(file);
            let line = if marks.is_empty() {
                Line::from(file.path.to_string())
            } else {
                Line::from(format!("{} [{}]", file.path, marks.join(", ")))
            };
            if index == selected {
                line.style(Style::new().add_modifier(Modifier::REVERSED))
            } else {
                line
            }
        }))
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
            DiffGap::RenamedOnly if file.previous_path.is_none() => marks.push(MARK_RENAMED.into()),
            DiffGap::RenamedOnly => {}
            DiffGap::AddedOrRemovedNoText => marks.push(MARK_ADDED_OR_REMOVED_NO_TEXT.into()),
            DiffGap::ChangedNoText => marks.push(MARK_CHANGED_NO_TEXT.into()),
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
fn conversation_lines(
    detail: &PrDetail,
    clock: &impl Clock,
    bodies: &Bodies,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(COMMENTS_HEADING)];
    lines.extend(count_note(detail.counts.comments, COMMENTS_NOUN));
    for (index, comment) in detail.comments.iter().enumerate() {
        let item = format!("comment-{index}");
        push_comment(
            &mut lines,
            comment,
            "",
            clock,
            bodies.lines(&item, &comment.body, ""),
        );
    }
    lines.push(Line::from(THREADS_HEADING));
    lines.extend(count_note(detail.counts.threads, THREADS_NOUN));
    for (thread_index, thread) in detail.threads.iter().enumerate() {
        lines.push(Line::from(thread.path.to_string()));
        for (index, reply) in thread.comments.iter().enumerate() {
            let item = format!("thread-{thread_index}-{index}");
            let body = bodies.lines(&item, &reply.body, REPLY_INDENT);
            push_comment(&mut lines, reply, REPLY_INDENT, clock, body);
        }
        lines.extend(count_note(thread.comment_count, THREAD_COMMENTS_NOUN));
    }
    lines
}

fn push_comment(
    lines: &mut Vec<Line<'static>>,
    comment: &Comment,
    indent: &str,
    clock: &impl Clock,
    body: Vec<Line<'static>>,
) {
    let age = age_since(comment.created_at, clock.now());
    lines.push(Line::from(format!("{indent}{} {age}", comment.author)));
    lines.extend(body);
}
