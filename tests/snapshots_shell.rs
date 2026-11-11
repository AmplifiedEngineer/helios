// SPDX-License-Identifier: Apache-2.0

#[allow(dead_code)]
mod support;

use crossterm::cursor::Show;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::Command;
use helios::tui::keys::{render_footer, render_help, BINDINGS};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::{Frame, Terminal};
use std::env;
use std::process::{Command as Process, Stdio};
use support::snapshot::{assert_snapshot, CI_VAR, REFUSAL_MESSAGE, UPDATE_VAR};

const PROBE_TEST: &str = "harness_probe_for_ci_refusal";
const ENABLED: &str = "1";

/// Runs only inside the child process of `snapshot_harness_refuses_update_in_ci`.
#[test]
#[ignore = "run by snapshot_harness_refuses_update_in_ci in a child process"]
fn harness_probe_for_ci_refusal() {
    assert_snapshot("ci_refusal_probe", "unused");
}

#[test]
fn snapshot_harness_refuses_update_in_ci() {
    // Arrange: a child process with an explicit environment, so no parallel test sees it
    let mut child = Process::new(env::current_exe().expect("test binary path"));
    child
        .env_clear()
        .env(UPDATE_VAR, ENABLED)
        .env(CI_VAR, ENABLED)
        .args(["--exact", PROBE_TEST, "--ignored", "--nocapture"]);

    // Act
    let output = child.output().expect("run the probe child");

    // Assert
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "the harness accepted UPDATE_SNAPSHOTS in CI"
    );
    assert!(
        stderr.contains(REFUSAL_MESSAGE),
        "the child did not fail with the refusal message: {stderr}"
    );
}

const SCREEN_WIDTH: u16 = 120;
const SCREEN_HEIGHT: u16 = 24;

fn render_to_text(draw: fn(&mut Frame, Rect)) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(SCREEN_WIDTH, SCREEN_HEIGHT)).expect("test terminal");
    terminal
        .draw(|frame| draw(frame, frame.area()))
        .expect("draw");
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn footer_and_help_list_every_binding_in_the_table() {
    // Arrange
    assert!(!BINDINGS.is_empty(), "the binding table is empty");

    // Act
    let footer = render_to_text(render_footer);
    let help = render_to_text(render_help);

    // Assert
    for binding in BINDINGS {
        for (name, screen) in [("footer", &footer), ("help", &help)] {
            assert!(
                screen.contains(binding.key),
                "{name} lacks the key {:?}",
                binding.key
            );
            assert!(
                screen.contains(binding.action),
                "{name} lacks the action {:?}",
                binding.action
            );
        }
    }
}

const PANIC_VAR: &str = "HELIOS_TEST_PANIC";
const FAKE_ACCOUNTS_VAR: &str = "HELIOS_FAKE_ACCOUNTS";
const FAKE_ACCOUNTS_FILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/status/synthetic-two-accounts.json"
);
const PANIC_EXIT_CODE: i32 = 101;

fn ansi(command: impl Command) -> String {
    let mut sequence = String::new();
    command.write_ansi(&mut sequence).expect("write ansi");
    sequence
}

#[test]
fn terminal_is_restored_after_a_panic() {
    // Arrange: piped stdin and stdout, so the binary needs no TTY, gh or network
    let mut child = Process::new(env!("CARGO_BIN_EXE_helios"));
    child
        .env_clear()
        .env(PANIC_VAR, ENABLED)
        .env(FAKE_ACCOUNTS_VAR, FAKE_ACCOUNTS_FILE)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Act
    let output = child.output().expect("run the helios binary");

    // Assert
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(PANIC_EXIT_CODE),
        "the binary did not exit with the panic code"
    );
    for (name, sequence) in [
        ("enter-alternate-screen", ansi(EnterAlternateScreen)),
        ("leave-alternate-screen", ansi(LeaveAlternateScreen)),
        ("show-cursor", ansi(Show)),
    ] {
        assert!(
            stdout.contains(&sequence),
            "stdout lacks the {name} sequence: {stdout:?}"
        );
    }
}
