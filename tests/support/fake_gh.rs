// SPDX-License-Identifier: Apache-2.0

//! A fake `gh`: a shell script in a unique temp dir.

// A shared helper: each test binary uses only part of it.
#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

pub static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

/// A fake `gh` script, removed on drop.
pub struct FakeGh {
    pub dir: PathBuf,
    pub path: PathBuf,
}

impl FakeGh {
    pub fn new(script: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "forgebird-fake-gh-{}-{}",
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
        // The first run of a new script is slow on macOS; pay that cost before a test's timeout starts.
        Command::new(&path)
            .arg("warm-up")
            .status()
            .expect("warm up fake gh");
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
