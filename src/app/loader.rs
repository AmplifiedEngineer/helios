// SPDX-License-Identifier: Apache-2.0

//! Runs the background loads on a tokio runtime and sends the results to the event loop.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use tokio::runtime::Handle;

use super::worker::{messages, Loaded, Message};
use super::{ListView, LoadRequest};
use crate::github::errors::GithubError;
use crate::github::queries::{merge_needs_me, ListKind, Listing};
use crate::github::GithubApi;
use crate::model::PrKey;

/// One account and the client that talks to its host.
pub struct Source<A> {
    pub host: String,
    pub login: String,
    pub api: Arc<A>,
}

impl<A> Clone for Source<A> {
    fn clone(&self) -> Self {
        Self {
            host: self.host.clone(),
            login: self.login.clone(),
            api: Arc::clone(&self.api),
        }
    }
}

pub struct Loader<A> {
    handle: Handle,
    sources: Vec<Source<A>>,
    sender: Sender<Message>,
    pending: Arc<AtomicUsize>,
}

impl<A: GithubApi + 'static> Loader<A> {
    /// A loader that spawns on `handle`, and the receiver its results arrive on.
    pub fn new(handle: Handle, sources: Vec<Source<A>>) -> (Self, Receiver<Message>) {
        let (sender, receiver) = messages();
        let loader = Self {
            handle,
            sources,
            sender,
            pending: Arc::new(AtomicUsize::new(0)),
        };
        (loader, receiver)
    }

    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// How many loads have started and not yet sent their result.
    pub fn pending(&self) -> usize {
        self.pending.load(Ordering::SeqCst)
    }

    /// Starts the Mine and Needs me loads of every account, tagged with `generation`.
    pub fn refresh(&self, generation: u64) {
        for source in &self.sources {
            let mine = Arc::clone(&source.api);
            self.spawn(list_message(
                ListView::Mine,
                generation,
                source.clone(),
                async move { mine.list(ListKind::Mine).await },
            ));
            let api = Arc::clone(&source.api);
            self.spawn(list_message(
                ListView::NeedsMe,
                generation,
                source.clone(),
                async move { needs_me(&*api).await },
            ));
        }
    }

    /// Starts the load `request` asks for.
    pub fn load(&self, request: LoadRequest) {
        match request {
            LoadRequest::Detail(key) => {
                let api = self.api_for(&key);
                self.spawn(async move {
                    let result = match (api, split_repo(&key)) {
                        (Some(api), Some((owner, repo))) => {
                            api.detail(&owner, &repo, key.number).await
                        }
                        _ => Err(GithubError::NotFoundOrHidden),
                    };
                    Message::Detail(key, Box::new(result))
                });
            }
            LoadRequest::Files(key) => {
                let api = self.api_for(&key);
                self.spawn(async move {
                    let result = match (api, split_repo(&key)) {
                        (Some(api), Some((owner, repo))) => {
                            api.files(&owner, &repo, key.number).await
                        }
                        _ => Err(GithubError::NotFoundOrHidden),
                    };
                    Message::Files(key, result)
                });
            }
        }
    }

    fn api_for(&self, key: &PrKey) -> Option<Arc<A>> {
        self.sources
            .iter()
            .find(|source| source.host == key.host && source.login == key.account)
            .map(|source| Arc::clone(&source.api))
    }

    fn spawn(&self, task: impl Future<Output = Message> + Send + 'static) {
        self.pending.fetch_add(1, Ordering::SeqCst);
        let sender = self.sender.clone();
        let pending = Arc::clone(&self.pending);
        self.handle.spawn(async move {
            let message = task.await;
            // The receiver is gone only when the loop has ended, so the result is not needed.
            let _ = sender.send(message);
            pending.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

/// The Review and Assigned searches of one account, merged and tagged.
async fn needs_me<A: GithubApi>(api: &A) -> Result<Listing, GithubError> {
    let (review, assigned) = tokio::join!(api.list(ListKind::Review), api.list(ListKind::Assigned));
    let (review, assigned) = (review?, assigned?);
    Ok(Listing {
        partial: review.partial || assigned.partial,
        rows: merge_needs_me(review.rows, assigned.rows),
    })
}

async fn list_message<A>(
    view: ListView,
    generation: u64,
    source: Source<A>,
    load: impl Future<Output = Result<Listing, GithubError>>,
) -> Message {
    let listing = load.await;
    Message::List(
        view,
        Loaded {
            generation,
            host: source.host,
            account: source.login,
            partial: listing.as_ref().is_ok_and(|listing| listing.partial),
            result: listing.map(|listing| listing.rows),
        },
    )
}

/// The owner and repository name of the pull request's `owner/name` repository.
fn split_repo(key: &PrKey) -> Option<(String, String)> {
    let (owner, repo) = key.repo.split_once('/')?;
    Some((owner.to_owned(), repo.to_owned()))
}
