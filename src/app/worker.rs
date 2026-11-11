// SPDX-License-Identifier: Apache-2.0

//! What a background load sends back to the app.

use std::sync::mpsc;

use crate::github::errors::GithubError;
use crate::model::PrSummary;

/// One account's list load, tagged with the refresh that started it.
#[derive(Debug)]
pub struct Loaded {
    pub generation: u64,
    pub host: String,
    pub account: String,
    pub result: Result<Vec<PrSummary>, GithubError>,
}

pub fn channel() -> (mpsc::Sender<Loaded>, mpsc::Receiver<Loaded>) {
    mpsc::channel()
}
