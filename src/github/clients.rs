// SPDX-License-Identifier: Apache-2.0

//! One GitHub client for every `gh` account on the hosts the caller wants.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;

use crate::accounts::{self, AccountsError};
use crate::github::errors::GithubError;
use crate::github::octo::{api_base, GhTokens, Octo};

/// One `gh` account and the client for its host.
pub struct Client {
    pub host: String,
    pub login: String,
    pub api: Octo,
}

#[derive(Debug, Error)]
pub enum ClientsError {
    #[error("{0}")]
    Accounts(#[from] AccountsError),
    #[error("{0}")]
    Github(#[from] GithubError),
}

/// A client for every account `program` lists on a host `include` accepts.
pub fn build_clients(
    program: &Path,
    timeout: Duration,
    include: impl Fn(&str) -> bool,
) -> Result<Vec<Client>, ClientsError> {
    let mut clients = Vec::new();
    for account in accounts::list(program, timeout)?
        .iter()
        .filter(|account| include(&account.host))
    {
        let tokens = GhTokens {
            program: program.to_path_buf(),
            host: account.host.clone(),
            login: account.login.clone(),
            timeout,
        };
        let api = Octo::new(account, &api_base(&account.host), timeout, Arc::new(tokens))?;
        clients.push(Client {
            host: account.host.clone(),
            login: account.login.clone(),
            api,
        });
    }
    Ok(clients)
}
