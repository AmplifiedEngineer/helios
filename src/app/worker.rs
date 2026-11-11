// SPDX-License-Identifier: Apache-2.0

//! What a background load sends back to the app.

use std::sync::mpsc;

use super::ListView;
use crate::github::errors::GithubError;
use crate::model::{ChangedFile, PrDetail, PrKey, PrSummary};

/// One account's list load, tagged with the refresh that started it.
#[derive(Debug)]
pub struct Loaded {
    pub generation: u64,
    /// GitHub cut the list short, so `result` may hold fewer rows than exist.
    pub partial: bool,
    pub host: String,
    pub account: String,
    pub result: Result<Vec<PrSummary>, GithubError>,
}

pub fn channel() -> (mpsc::Sender<Loaded>, mpsc::Receiver<Loaded>) {
    mpsc::channel()
}

/// What any background load sends back to the event loop.
#[derive(Debug)]
pub enum Message {
    /// One account's rows for one list.
    List(ListView, Loaded),
    Detail(PrKey, Box<Result<PrDetail, GithubError>>),
    Files(PrKey, Result<Vec<ChangedFile>, GithubError>),
    /// The system opener could not show a pull request in the browser.
    OpenFailed,
}

pub fn messages() -> (mpsc::Sender<Message>, mpsc::Receiver<Message>) {
    mpsc::channel()
}
