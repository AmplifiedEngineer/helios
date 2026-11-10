// SPDX-License-Identifier: Apache-2.0

//! Turns a test terminal buffer into text a test can search.

use ratatui::buffer::Buffer;

/// The buffer as lines, each without its trailing blanks.
pub fn buffer_text(buffer: &Buffer) -> String {
    buffer
        .content()
        .chunks(usize::from(buffer.area.width))
        .map(|row| {
            let line: String = row.iter().map(|cell| cell.symbol()).collect();
            format!("{}\n", line.trim_end())
        })
        .collect()
}

/// The first line of `screen` that holds `needle`.
pub fn line_with<'a>(screen: &'a str, needle: &str) -> &'a str {
    screen
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no line holds {needle:?}:\n{screen}"))
}
