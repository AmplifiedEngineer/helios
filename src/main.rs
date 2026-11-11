// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::io;
use std::process::ExitCode;
use std::sync::Arc;

use helios::accounts;
use helios::cli::{self, Config};
use helios::github::octo::{api_base, GhTokens, Octo, REQUEST_TIMEOUT};
use helios::smoke::{self, SmokeAccount};
use helios::tui;

#[cfg(debug_assertions)]
fn debug_hooks(config: &Config) -> (bool, bool) {
    (config.fake_accounts, config.panic_during_draw)
}

#[cfg(not(debug_assertions))]
fn debug_hooks(_config: &Config) -> (bool, bool) {
    (false, false)
}

fn run(config: &Config) -> io::Result<()> {
    tui::install_panic_hook();
    let (fake_accounts, panic_during_draw) = debug_hooks(config);
    let mut terminal = tui::init_terminal(fake_accounts)?;
    let drawn = tui::draw_first_frame(&mut terminal, panic_during_draw);
    tui::restore_terminal();
    drawn
}

fn smoke_run(config: &Config) -> Result<smoke::Outcome, String> {
    let found = accounts::list(&config.gh_program, REQUEST_TIMEOUT).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    for account in found.iter().filter(|a| config.includes_host(&a.host)) {
        let tokens = GhTokens {
            program: config.gh_program.clone(),
            host: account.host.clone(),
            login: account.login.clone(),
            timeout: REQUEST_TIMEOUT,
        };
        let api = Octo::new(
            account,
            &api_base(&account.host),
            REQUEST_TIMEOUT,
            Arc::new(tokens),
        )
        .map_err(|e| e.to_string())?;
        entries.push(SmokeAccount {
            host: account.host.clone(),
            account: account.login.clone(),
            api,
        });
    }
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    Ok(runtime.block_on(smoke::run(&entries, config.drafts)))
}

fn smoke_main(config: &Config) -> ExitCode {
    match smoke_run(config) {
        Ok(outcome) => {
            print!("{}", outcome.text);
            u8::try_from(outcome.exit_code).map_or(ExitCode::FAILURE, ExitCode::from)
        }
        Err(message) => {
            eprintln!("helios: {message}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env: HashMap<String, String> = std::env::vars().collect();
    let config = match cli::parse(&args, &env) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("helios: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(version) = config.version {
        println!("helios {version}");
        return ExitCode::SUCCESS;
    }
    if config.help {
        print!("{}", cli::help());
        return ExitCode::SUCCESS;
    }
    if config.smoke {
        return smoke_main(&config);
    }
    match run(&config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("helios: {error}");
            ExitCode::FAILURE
        }
    }
}
