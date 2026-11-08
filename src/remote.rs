// SPDX-License-Identifier: Apache-2.0

use std::fmt;

/// Text that came from GitHub and has passed through [`crate::sanitize::sanitize`].
///
/// Direct construction is a privacy error (expected code E0451):
///
/// ```compile_fail,E0451
/// use forgebird::remote::Remote;
///
/// let _remote = Remote { text: String::from("hello") };
/// ```
///
/// The only way to get one is `sanitize`:
///
/// ```
/// use forgebird::sanitize::sanitize;
///
/// let remote = sanitize("hello");
/// assert_eq!(remote.as_str(), "hello");
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Remote {
    text: String,
}

impl Remote {
    pub(crate) fn new(text: String) -> Self {
        Self { text }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for Remote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl fmt::Debug for Remote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.text, f)
    }
}
