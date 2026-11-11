// SPDX-License-Identifier: Apache-2.0

use std::io;

use clap::Parser;
use helios::tui;

#[cfg(debug_assertions)]
const TEST_PANIC_VAR: &str = "HELIOS_TEST_PANIC";
#[cfg(debug_assertions)]
const FAKE_ACCOUNTS_VAR: &str = "HELIOS_FAKE_ACCOUNTS";

/// Gives `--version` and `--help` to the binary.
#[derive(Parser)]
#[command(version)]
struct Cli {}

/// Test hooks, read only in debug builds.
#[derive(Default)]
struct TestHooks {
    panic_during_draw: bool,
    fake_accounts: bool,
}

impl TestHooks {
    #[cfg(debug_assertions)]
    fn from_env() -> Self {
        Self {
            panic_during_draw: std::env::var_os(TEST_PANIC_VAR).is_some_and(|value| value == "1"),
            fake_accounts: std::env::var_os(FAKE_ACCOUNTS_VAR).is_some(),
        }
    }

    #[cfg(not(debug_assertions))]
    fn from_env() -> Self {
        Self::default()
    }
}

fn run(hooks: &TestHooks) -> io::Result<()> {
    tui::install_panic_hook();
    let mut terminal = tui::init_terminal(hooks.fake_accounts)?;
    let drawn = tui::draw_first_frame(&mut terminal, hooks.panic_during_draw);
    tui::restore_terminal();
    drawn
}

fn main() -> io::Result<()> {
    Cli::parse();
    run(&TestHooks::from_env())
}
