// SPDX-License-Identifier: Apache-2.0

//! Shared fixtures for the GitHub client tests.

use helios::accounts::{Account, AccountState};
use helios::github::errors::GithubError;
use helios::github::octo::TokenSource;
use helios::secret::Secret;

const TOKEN_PREFIX: &str = "tok";
const TOKEN_BODY: &str = "planted";

/// Planted token, built at runtime so no token-shaped literal sits in the source.
pub fn planted_token() -> String {
    format!("{TOKEN_PREFIX}_{TOKEN_BODY}")
}

/// A token source that always returns the same token.
pub struct FixedToken(pub String);

impl TokenSource for FixedToken {
    fn fetch(&self) -> Result<Secret, GithubError> {
        Ok(Secret::new(self.0.clone()))
    }
}

pub fn account() -> Account {
    Account {
        host: "github.test".into(),
        login: "octo".into(),
        active: true,
        scopes: vec![],
        state: AccountState::Success,
    }
}
