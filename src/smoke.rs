// SPDX-License-Identifier: Apache-2.0

//! `--smoke`: query every account once and print the list counts.

use crate::github::errors::GithubError;
use crate::github::queries::{filter_drafts, merge_needs_me, ListKind};
use crate::github::GithubApi;
use crate::sanitize::sanitize;

/// One account to query, with the client that talks to its host.
pub struct SmokeAccount<A> {
    pub host: String,
    pub account: String,
    pub api: A,
}

/// What `--smoke` prints and the process exit code it ends with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub text: String,
    pub exit_code: i32,
}

const EXIT_SUCCESS: i32 = 0;
const EXIT_FAILURE: i32 = 1;
const NO_ACCOUNTS: &str = "no accounts found";
const HOST_FAILED_PREFIX: &str = "Host failed";

/// Visible rows in the Mine and Needs me lists for one account.
struct Counts {
    mine: usize,
    needs_me: usize,
}

async fn count_account<A: GithubApi>(api: &A, show_drafts: bool) -> Result<Counts, GithubError> {
    let mine = filter_drafts(api.list(ListKind::Mine).await?.rows, show_drafts);
    let review = api.list(ListKind::Review).await?.rows;
    let assigned = api.list(ListKind::Assigned).await?.rows;
    let needs_me = filter_drafts(merge_needs_me(review, assigned), show_drafts);
    Ok(Counts {
        mine: mine.len(),
        needs_me: needs_me.len(),
    })
}

/// Counts the lists over every account that answers; a failing host is named, not fatal.
pub async fn run<A: GithubApi>(accounts: &[SmokeAccount<A>], show_drafts: bool) -> Outcome {
    if accounts.is_empty() {
        return Outcome {
            text: format!("{NO_ACCOUNTS}\n"),
            exit_code: EXIT_FAILURE,
        };
    }
    let mut text = String::new();
    let (mut mine, mut needs_me, mut succeeded) = (0, 0, 0);
    for entry in accounts {
        match count_account(&entry.api, show_drafts).await {
            Ok(counts) => {
                mine += counts.mine;
                needs_me += counts.needs_me;
                succeeded += 1;
            }
            Err(_) => {
                text.push_str(&format!(
                    "{HOST_FAILED_PREFIX}: {}\n",
                    sanitize(&entry.host)
                ));
            }
        }
    }
    text.push_str(&format!("Mine: {mine}\nNeeds me: {needs_me}\n"));
    let exit_code = if succeeded > 0 {
        EXIT_SUCCESS
    } else {
        EXIT_FAILURE
    };
    Outcome { text, exit_code }
}
