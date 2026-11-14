// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::io;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use helios::app::loader::{Loader, Source};
use helios::app::App;
use helios::cli::{self, Config, DebugHooks};
use helios::config::{self, resolve_theme, Config as Settings};
use helios::github::clients::{build_clients, Client, ClientsError};
use helios::github::octo::{Octo, REQUEST_TIMEOUT};
use helios::smoke::{self, SmokeAccount};
use helios::theme::{effective_theme, ColorDepth};
use helios::tui::run::{Session, TerminalEvents};
use helios::tui::{self, SystemClock};

const PANIC_DURING_DRAW_MESSAGE: &str = "panic requested during the first draw";
const ACCOUNTS_FAILED_PREFIX: &str = "Could not read the gh accounts: ";

/// A client for every account `gh` knows on a host the config includes.
fn clients(config: &Config) -> Result<Vec<Client>, ClientsError> {
    build_clients(&config.gh_program, REQUEST_TIMEOUT, |host| {
        config.includes_host(host)
    })
}

fn run(config: &Config, settings: &Settings, hooks: DebugHooks) -> io::Result<()> {
    tui::install_panic_hook();
    let runtime = tokio::runtime::Runtime::new()?;
    let mut app = App::new();
    let lookup = |name: &str| std::env::var(name).ok();
    app.set_depth(ColorDepth::detect(lookup));
    let start_theme = resolve_theme(
        config.theme.as_deref(),
        config.theme_env.as_deref(),
        &settings.theme.name,
    );
    app.set_theme(effective_theme(start_theme, lookup));
    app.show_drafts = config.drafts;
    let clients = if hooks.fake_accounts {
        Vec::new()
    } else {
        clients(config).unwrap_or_else(|error| {
            app.set_notice(format!("{ACCOUNTS_FAILED_PREFIX}{error}"));
            Vec::new()
        })
    };
    let sources = clients
        .into_iter()
        .map(|client| Source {
            host: client.host,
            login: client.login,
            api: Arc::new(client.api),
        })
        .collect();
    let (loader, results) = Loader::new(runtime.handle().clone(), sources);
    let mut terminal = tui::init_terminal(hooks.fake_accounts)?;
    if hooks.panic_during_draw {
        terminal.draw(|_| panic!("{PANIC_DURING_DRAW_MESSAGE}"))?;
    }
    let mut session = Session::new(terminal, app, TerminalEvents, SystemClock, loader, results);
    session.set_refresh_interval(Duration::from_secs(config.refresh_secs));
    let outcome = session.run();
    tui::restore_terminal();
    outcome
}

fn smoke_run(config: &Config) -> Result<smoke::Outcome, String> {
    let entries: Vec<SmokeAccount<Octo>> = clients(config)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|client| SmokeAccount {
            host: client.host,
            account: client.login,
            api: client.api,
        })
        .collect();
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
    let real_env = |name: &str| env.get(name).cloned();
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
    let settings = match config::load(config::config_path(&real_env).as_deref()) {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("helios: {error}");
            return ExitCode::FAILURE;
        }
    };
    match run(&config, &settings, cli::debug_hooks(&env)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("helios: {error}");
            ExitCode::FAILURE
        }
    }
}
