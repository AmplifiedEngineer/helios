// SPDX-License-Identifier: Apache-2.0

//! Builds the app a run starts with from the settings, the flag and the environment.

use super::App;
use crate::config::{resolve_theme, Config, COLUMN_NAMES};
use crate::test_files::TestPatterns;
use crate::theme::{effective_theme, no_color, ColorDepth};
use crate::tui::layout::LayoutSettings;

/// The app as it starts: the settings applied, the theme from the flag, the environment or the file.
pub fn startup_app(
    settings: &Config,
    cli_theme: Option<&str>,
    env_theme: Option<&str>,
    lookup: impl Fn(&str) -> Option<String> + Copy,
) -> App {
    let mut app = App::new();
    app.set_depth(ColorDepth::detect(lookup));
    let theme = resolve_theme(cli_theme, env_theme, &settings.theme.name);
    app.set_theme(effective_theme(theme, lookup));
    if no_color(lookup) {
        app.lock_theme();
    }
    app.set_layout(LayoutSettings::from(&settings.layout));
    app.set_preview_delay_ms(settings.layout.preview_delay_ms);
    app.set_columns(
        explicit_columns(&settings.columns.mine),
        explicit_columns(&settings.columns.needs_me),
    );
    app.set_test_patterns(TestPatterns::new(
        settings.files.test_dirs.clone(),
        settings.files.test_files.clone(),
    ));
    app.set_files_start_open(!settings.files.collapsed);
    app
}

/// The default order stands for the automatic choice of columns, so it becomes an empty list.
fn explicit_columns(names: &[String]) -> Vec<String> {
    if names.iter().map(String::as_str).eq(COLUMN_NAMES) {
        Vec::new()
    } else {
        names.to_vec()
    }
}
