// SPDX-License-Identifier: Apache-2.0

//! A fake `gh`: a shell script in a unique temp dir.

// A shared helper: each test binary uses only part of it.
#![allow(dead_code)]

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

pub static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

const BUSY_RETRIES: u32 = 100;
const BUSY_RETRY_DELAY: Duration = Duration::from_millis(10);

/// Runs the script once so its first, slow run happens before a test's timeout starts.
///
/// On Linux a fork made by another test thread can briefly hold the script open for
/// writing, which fails the exec with "text file busy". The hold ends when that child
/// execs, so a short retry is enough.
fn warm_up(path: &Path) {
    for _ in 0..BUSY_RETRIES {
        match Command::new(path).arg("warm-up").status() {
            Err(error) if error.kind() == io::ErrorKind::ExecutableFileBusy => {
                thread::sleep(BUSY_RETRY_DELAY);
            }
            result => {
                result.expect("warm up fake gh");
                return;
            }
        }
    }
    Command::new(path)
        .arg("warm-up")
        .status()
        .expect("warm up fake gh");
}

/// A fake `gh` script, removed on drop.
pub struct FakeGh {
    pub dir: PathBuf,
    pub path: PathBuf,
}

impl FakeGh {
    pub fn new(script: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "helios-fake-gh-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("create fake gh dir");
        let path = dir.join("gh");
        fs::write(
            &path,
            format!("#!/bin/sh\n[ \"$1\" = warm-up ] && exit 0\n{script}\n"),
        )
        .expect("write fake gh");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod fake gh");
        warm_up(&path);
        Self { dir, path }
    }

    pub fn printing(file: &Path) -> Self {
        Self::new(&format!("cat '{}'", file.display()))
    }
}

impl Drop for FakeGh {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
