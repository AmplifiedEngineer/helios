// SPDX-License-Identifier: Apache-2.0

//! Opening a pull request in the web browser.

use std::path::Path;
use std::time::Duration;

use crate::model::PrKey;
use crate::proc::{self, ProcError};

/// How long the opener program may run before it is stopped.
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

const MAX_OPENER_OUTPUT_BYTES: usize = 4096;
const SCHEME: &str = "https";

#[cfg(target_os = "macos")]
pub const OPENER: &str = "open";
#[cfg(not(target_os = "macos"))]
pub const OPENER: &str = "xdg-open";

/// The web address of a pull request, or `None` when the host or repository would not make
/// a plain `https` address.
pub fn pr_url(key: &PrKey) -> Option<String> {
    let (owner, name) = key.repo.split_once('/')?;
    let valid = valid_host(&key.host) && valid_segment(owner) && valid_segment(name);
    valid.then(|| format!("{SCHEME}://{}/{owner}/{name}/pull/{}", key.host, key.number))
}

fn valid_host(host: &str) -> bool {
    let (name, port) = host.split_once(':').unwrap_or((host, "1"));
    !name.is_empty()
        && name.split('.').all(|label| {
            !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
        && !port.is_empty()
        && port.chars().all(|c| c.is_ascii_digit())
}

fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Runs `program` with `url` as its only argument, without a shell. Only an `https` address
/// is passed on.
pub fn open_with(program: &Path, url: &str, timeout: Duration) -> Result<(), ProcError> {
    if !url.starts_with(&format!("{SCHEME}://")) {
        return Err(ProcError::Io {
            kind: std::io::ErrorKind::InvalidInput,
        });
    }
    proc::run(program, &[url], timeout, MAX_OPENER_OUTPUT_BYTES).map(|_| ())
}

/// Opens `url` with the system opener.
pub fn open(url: &str) -> Result<(), ProcError> {
    open_with(Path::new(OPENER), url, OPEN_TIMEOUT)
}
