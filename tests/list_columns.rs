// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
mod support;
use support::clock::FixedClock;

use helios::app::App;
use helios::github::queries::map_page;
use helios::tui::list;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use std::fs;
use support::render::{buffer_text, line_with};

const HOST: &str = "github.com";
const ACCOUNT: &str = "octocat";
const REVIEW_FIXTURE: &str = "tests/fixtures/list/synthetic-review.json";
const FIXTURE_TITLE: &str = "Shared change";
const FIXTURE_REPO: &str = "octo/widgets";
const TITLE: &str = "Fix flaky retries!";
const LONG_REPO: &str = "org/long-repository-name";
const SHRUNK_REPO: &str = "\u{2026}sitory-name";
const UPDATED_HEADER: &str = "UPDATED";
const SCREEN_HEIGHT: u16 = 8;
const NARROW_PANE: u16 = 38;
const WIDE_PANE: u16 = 46;
const ROOMY_PANE: u16 = 80;

fn render_at(width: u16) -> String {
    let json = fs::read_to_string(REVIEW_FIXTURE)
        .expect("read the list fixture")
        .replace(FIXTURE_TITLE, TITLE)
        .replace(FIXTURE_REPO, LONG_REPO);
    let rows = map_page(HOST, ACCOUNT, &json)
        .expect("map the list fixture")
        .rows;
    let app = App::new();
    let mut terminal =
        Terminal::new(TestBackend::new(width, SCREEN_HEIGHT)).expect("test terminal");
    terminal
        .draw(|frame| {
            list::render_rows(
                frame,
                frame.area(),
                &app,
                rows.iter().collect(),
                &FixedClock::after_the_list_fixtures(),
                None,
            )
        })
        .expect("draw");
    buffer_text(terminal.backend().buffer())
}

#[test]
fn a_narrow_list_pane_keeps_the_title_readable() {
    for width in [NARROW_PANE, WIDE_PANE] {
        // Arrange: a title as long as the narrowest pane leaves room for

        // Act
        let screen = render_at(width);

        // Assert
        assert!(
            screen.contains(TITLE),
            "the title is cut at {width} columns:\n{screen}"
        );
        assert!(
            !screen.contains(UPDATED_HEADER),
            "UPDATED stays at {width} columns:\n{screen}"
        );
    }
}

#[test]
fn the_repo_shrinks_before_the_title_does() {
    for width in [NARROW_PANE, WIDE_PANE] {
        // Arrange: a repo name longer than the shrunk column

        // Act
        let screen = render_at(width);
        let row = line_with(&screen, TITLE);

        // Assert
        assert!(
            row.contains(SHRUNK_REPO),
            "the repo is not trimmed with an ellipsis at {width} columns:\n{screen}"
        );
        assert!(
            !row.contains(LONG_REPO),
            "the repo kept its full name at {width} columns:\n{screen}"
        );
    }
}

#[test]
fn a_pane_of_eighty_columns_keeps_every_column_it_had() {
    // Arrange: a width where the title already has its least room

    // Act
    let screen = render_at(ROOMY_PANE);

    // Assert
    assert!(screen.contains(UPDATED_HEADER), "{screen}");
    assert!(line_with(&screen, TITLE).contains(LONG_REPO), "{screen}");
}
