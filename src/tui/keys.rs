// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, DetailTab, ListView, Pane, Screen, Step};
use crate::config::{KeysConfig, SCOPE_ACTIONS};

const FOOTER_SEPARATOR: &str = "  ";
const LABEL_SEPARATOR: &str = "/";

const QUIT_KEY: &str = "q";
const HELP_KEY: &str = "?";
const ESCAPE_KEY: &str = "Esc";

const MOVE: (&str, &str) = ("move_down", "move_up");
const SCROLL: (&str, &str) = ("scroll_down", "scroll_up");

/// Where the user is, as far as which keys do something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Mine,
    NeedsMe,
    Detail,
    Files,
    Diff,
}

impl Scope {
    /// The scope the app is in now.
    pub fn of(app: &App) -> Self {
        let detail_focused = app.is_split() && app.pane == Pane::Detail;
        match (app.screen, app.view, app.tab) {
            (Screen::List, _, DetailTab::Files) if detail_focused => Self::Files,
            (Screen::List, _, _) if detail_focused => Self::Detail,
            (Screen::List, ListView::Mine, _) => Self::Mine,
            (Screen::List, ListView::NeedsMe, _) => Self::NeedsMe,
            (Screen::Detail, _, DetailTab::Files) => Self::Files,
            (Screen::Detail, _, _) => Self::Detail,
            (Screen::Diff, _, _) => Self::Diff,
        }
    }

    /// The name of the `[keys]` table that holds this scope's keys.
    fn table(self) -> &'static str {
        match self {
            Self::Mine | Self::NeedsMe => "list",
            Self::Detail => "detail",
            Self::Files => "files",
            Self::Diff => "diff",
        }
    }
}

const LISTS: &[Scope] = &[Scope::Mine, Scope::NeedsMe];
const ANYWHERE: &[Scope] = &[
    Scope::Mine,
    Scope::NeedsMe,
    Scope::Detail,
    Scope::Files,
    Scope::Diff,
];

/// One key binding: the default key label and what it does, shown in the footer and the help.
pub struct Binding {
    pub key: &'static str,
    pub action: &'static str,
    /// The `[keys]` actions the label stands for, in label order; none for a fixed label.
    pub actions: &'static [&'static str],
    /// Where the key does something; the footer and the help list it only there.
    pub scopes: &'static [Scope],
}

/// The table of default bindings the footer and the help are both drawn from.
pub const BINDINGS: &[Binding] = &[
    Binding {
        key: "j/k",
        action: "move",
        actions: &["move_down", "move_up"],
        scopes: &[Scope::Mine, Scope::NeedsMe, Scope::Files],
    },
    Binding {
        key: "j/k",
        action: "scroll",
        actions: &["scroll_down", "scroll_up"],
        scopes: &[Scope::Diff, Scope::Detail],
    },
    Binding {
        key: "g/G",
        action: "first/last",
        actions: &["first", "last"],
        scopes: ANYWHERE,
    },
    Binding {
        key: "Enter/l",
        action: "open",
        actions: &["open"],
        scopes: LISTS,
    },
    Binding {
        key: "Enter/Space",
        action: "toggle file",
        actions: &["toggle_file"],
        scopes: &[Scope::Files],
    },
    Binding {
        key: "e/E",
        action: "open/close all",
        actions: &["open_all", "close_all"],
        scopes: &[Scope::Files],
    },
    Binding {
        key: "x",
        action: "tests group",
        actions: &["toggle_tests"],
        scopes: &[Scope::Files],
    },
    Binding {
        key: "v",
        action: "full diff",
        actions: &["full_diff"],
        scopes: &[Scope::Files],
    },
    Binding {
        key: "Esc/h",
        action: "back",
        actions: &["back"],
        scopes: &[Scope::Detail, Scope::Files, Scope::Diff],
    },
    Binding {
        key: "m",
        action: "mine/needs me",
        actions: &["toggle_view"],
        scopes: LISTS,
    },
    Binding {
        key: "r",
        action: "refresh",
        actions: &["refresh"],
        scopes: LISTS,
    },
    Binding {
        key: "f",
        action: "filter",
        actions: &["filter"],
        scopes: &[Scope::NeedsMe],
    },
    Binding {
        key: "d",
        action: "toggle drafts",
        actions: &["toggle_drafts"],
        scopes: LISTS,
    },
    Binding {
        key: "Tab",
        action: "next tab",
        actions: &["next_tab"],
        scopes: &[Scope::Detail, Scope::Files],
    },
    Binding {
        key: "Shift-Tab",
        action: "previous tab",
        actions: &["previous_tab"],
        scopes: &[Scope::Detail, Scope::Files],
    },
    Binding {
        key: "t",
        action: "theme",
        actions: &["theme"],
        scopes: ANYWHERE,
    },
    Binding {
        key: "o",
        action: "open in browser",
        actions: &["open_in_browser"],
        scopes: ANYWHERE,
    },
    Binding {
        key: "?",
        action: "help",
        actions: &["help"],
        scopes: ANYWHERE,
    },
    Binding {
        key: "q",
        action: "quit",
        actions: &["quit"],
        scopes: ANYWHERE,
    },
    Binding {
        key: "Ctrl-C",
        action: "quit",
        actions: &[],
        scopes: ANYWHERE,
    },
];

/// What the event loop does after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    Quit,
    /// Reload both lists now.
    Refresh,
    /// Open the current pull request in the web browser.
    OpenInBrowser,
}

/// The keys of one action in one scope.
#[derive(Debug, Clone)]
struct ActionKeys {
    keys: Vec<String>,
    /// Whether the file set these keys; the footer then shows them in place of the default label.
    overridden: bool,
}

/// The keys bound to each action in each scope, built from the settings.
#[derive(Debug, Clone)]
pub struct KeyMap {
    scopes: BTreeMap<&'static str, BTreeMap<&'static str, ActionKeys>>,
}

impl Default for KeyMap {
    fn default() -> Self {
        Self::new(&KeysConfig::default())
    }
}

impl KeyMap {
    /// The default keys, with each action the file binds taking the file's keys instead.
    pub fn new(config: &KeysConfig) -> Self {
        let scopes = SCOPE_ACTIONS
            .iter()
            .map(|(scope, actions)| {
                let bound = actions
                    .iter()
                    .map(|action| {
                        let keys = config.keys_for(scope, action);
                        let overridden = config.override_of(scope, action).is_some();
                        (*action, ActionKeys { keys, overridden })
                    })
                    .collect();
                (*scope, bound)
            })
            .collect();
        Self { scopes }
    }

    fn action(&self, scope: Scope, action: &str) -> Option<&ActionKeys> {
        self.scopes.get(scope.table())?.get(action)
    }

    /// Whether the key called `name` runs `action` in `scope`.
    fn is(&self, scope: Scope, action: &str, name: &str) -> bool {
        self.action(scope, action)
            .is_some_and(|bound| bound.keys.iter().any(|key| key == name))
    }

    /// The key label for `binding` in `scope`: the default label until the file rebinds an action.
    fn label(&self, binding: &Binding, scope: Scope) -> String {
        let defaults: Vec<&str> = binding.key.split(LABEL_SEPARATOR).collect();
        let parts: Vec<String> = binding
            .actions
            .iter()
            .enumerate()
            .filter_map(|(index, action)| {
                let bound = self.action(scope, action)?;
                Some(if bound.overridden {
                    bound.keys.join(LABEL_SEPARATOR)
                } else if binding.actions.len() == 1 {
                    binding.key.to_string()
                } else {
                    (*defaults.get(index)?).to_string()
                })
            })
            .filter(|part| !part.is_empty())
            .collect();
        if binding.actions.is_empty() {
            binding.key.to_string()
        } else {
            parts.join(LABEL_SEPARATOR)
        }
    }
}

/// The name a key goes by in the settings, or none for a key no setting can name.
fn key_name(key: KeyEvent) -> Option<String> {
    let name = match key.code {
        KeyCode::Char(ch) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            format!("Ctrl-{}", ch.to_ascii_uppercase())
        }
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(ch) => ch.to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "Shift-Tab".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Esc => ESCAPE_KEY.to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        _ => return None,
    };
    Some(name)
}

/// The step a movement key asks for, with `(down, up)` naming the scope's two movement actions.
fn step_for(keys: &KeyMap, scope: Scope, name: &str, (down, up): (&str, &str)) -> Option<Step> {
    [
        (down, Step::Next),
        (up, Step::Previous),
        ("first", Step::First),
        ("last", Step::Last),
    ]
    .into_iter()
    .find_map(|(action, step)| keys.is(scope, action, name).then_some(step))
}

/// Applies one key press to the app with the default keys.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Outcome {
    handle_key_with(app, key, &KeyMap::default())
}

/// Applies one key press to the app; `q`, `Ctrl-C`, `?` and `Esc` keep their meaning in any `keys`.
pub fn handle_key_with(app: &mut App, key: KeyEvent, keys: &KeyMap) -> Outcome {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Outcome::Quit;
    }
    if app.show_help {
        app.show_help = false;
        return Outcome::Continue;
    }
    let Some(name) = key_name(key) else {
        return Outcome::Continue;
    };
    let scope = Scope::of(app);
    if name == QUIT_KEY || keys.is(scope, "quit", &name) {
        return Outcome::Quit;
    }
    if name == HELP_KEY || keys.is(scope, "help", &name) {
        app.show_help = true;
        return Outcome::Continue;
    }
    if keys.is(scope, "open_in_browser", &name) {
        return Outcome::OpenInBrowser;
    }
    if keys.is(scope, "theme", &name) {
        app.cycle_theme();
        return Outcome::Continue;
    }
    match scope {
        Scope::Mine | Scope::NeedsMe => list_key(app, keys, scope, &name),
        Scope::Detail | Scope::Files => detail_key(app, keys, scope, &name),
        Scope::Diff => diff_key(app, keys, &name),
    }
}

fn is_back(keys: &KeyMap, scope: Scope, name: &str) -> bool {
    name == ESCAPE_KEY || keys.is(scope, "back", name)
}

fn list_key(app: &mut App, keys: &KeyMap, scope: Scope, name: &str) -> Outcome {
    let is = |action| keys.is(scope, action, name);
    if is("open") {
        app.open_highlighted();
    } else if is("toggle_view") {
        app.toggle_view();
    } else if is("refresh") {
        return Outcome::Refresh;
    } else if is("filter") {
        if app.view == ListView::NeedsMe {
            app.cycle_filter();
        }
    } else if is("toggle_drafts") {
        app.toggle_drafts();
    } else if let Some(step) = step_for(keys, scope, name, MOVE) {
        app.move_cursor(step);
    }
    Outcome::Continue
}

fn detail_key(app: &mut App, keys: &KeyMap, scope: Scope, name: &str) -> Outcome {
    let is = |action| keys.is(scope, action, name);
    if is("next_tab") {
        app.next_tab();
    } else if is("previous_tab") {
        app.previous_tab();
    } else if is("toggle_file") {
        app.toggle_selected_row();
    } else if is("open_all") {
        app.open_all_files();
    } else if is("close_all") {
        app.close_all_files();
    } else if is("toggle_tests") {
        app.toggle_tests();
    } else if is("full_diff") {
        app.open_file();
    } else if is_back(keys, scope, name) {
        app.back();
    } else if scope == Scope::Files {
        if let Some(step) = step_for(keys, scope, name, MOVE) {
            app.move_file(step);
        }
    } else if let Some(step) = step_for(keys, scope, name, SCROLL) {
        app.scroll_detail(step);
    }
    Outcome::Continue
}

fn diff_key(app: &mut App, keys: &KeyMap, name: &str) -> Outcome {
    if is_back(keys, Scope::Diff, name) {
        app.back();
    } else if let Some(step) = step_for(keys, Scope::Diff, name, SCROLL) {
        app.scroll_diff(step);
    }
    Outcome::Continue
}

/// A key label and what it does, as the footer and the help show them.
struct Shown {
    key: String,
    action: &'static str,
}

/// The bindings to show for `scope` with the keys in `keys`; with no scope, the defaults.
/// An action with no key bound is left out.
fn shown(scope: Option<Scope>, keys: &KeyMap) -> Vec<Shown> {
    BINDINGS
        .iter()
        .filter_map(|binding| {
            let key = match scope {
                Some(scope) if binding.scopes.contains(&scope) => keys.label(binding, scope),
                Some(_) => return None,
                None => binding.key.to_string(),
            };
            (!key.is_empty()).then_some(Shown {
                key,
                action: binding.action,
            })
        })
        .collect()
}

/// The bold key label followed by its action, shared by the footer and the help.
fn binding_spans(shown: &Shown) -> [Span<'static>; 2] {
    [
        Span::styled(shown.key.clone(), Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(format!(" {}", shown.action)),
    ]
}

/// The footer text for `scope` packed into lines no wider than `width`.
pub fn footer_lines(scope: Option<Scope>, width: u16) -> Vec<Line<'static>> {
    footer_lines_with(scope, width, &KeyMap::default())
}

/// The footer text for `scope` with the keys in `keys`.
pub fn footer_lines_with(scope: Option<Scope>, width: u16, keys: &KeyMap) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let mut lines = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for binding in shown(scope, keys) {
        let item = binding_spans(&binding);
        let item_width: usize = item.iter().map(Span::width).sum();
        let separator = if spans.is_empty() {
            0
        } else {
            FOOTER_SEPARATOR.len()
        };
        if !spans.is_empty() && used + separator + item_width > width {
            lines.push(Line::from(std::mem::take(&mut spans)));
            used = 0;
        } else if !spans.is_empty() {
            spans.push(Span::raw(FOOTER_SEPARATOR));
            used += separator;
        }
        used += item_width;
        spans.extend(item);
    }
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    lines
}

/// How many rows the footer for `scope` needs at `width`.
pub fn footer_height(scope: Scope, width: u16) -> u16 {
    footer_height_with(scope, width, &KeyMap::default())
}

/// How many rows the footer for `scope` needs at `width` with the keys in `keys`.
pub fn footer_height_with(scope: Scope, width: u16, keys: &KeyMap) -> u16 {
    u16::try_from(footer_lines_with(Some(scope), width, keys).len()).unwrap_or(u16::MAX)
}

/// How many keys the help lists for `scope`.
pub fn help_height(scope: Scope) -> u16 {
    help_height_with(scope, &KeyMap::default())
}

/// How many keys the help lists for `scope` with the keys in `keys`.
pub fn help_height_with(scope: Scope, keys: &KeyMap) -> u16 {
    u16::try_from(shown(Some(scope), keys).len()).unwrap_or(u16::MAX)
}

/// Draws the footer for `scope` from `BINDINGS`; with no scope, every binding.
pub fn render_footer_for(frame: &mut Frame, area: Rect, scope: Option<Scope>) {
    render_footer_for_with(frame, area, scope, &KeyMap::default());
}

/// Draws the footer for `scope` with the keys in `keys`.
pub fn render_footer_for_with(frame: &mut Frame, area: Rect, scope: Option<Scope>, keys: &KeyMap) {
    frame.render_widget(
        Paragraph::new(footer_lines_with(scope, area.width, keys)),
        area,
    );
}

/// Draws the help for `scope` from `BINDINGS`; with no scope, every binding.
pub fn render_help_for(frame: &mut Frame, area: Rect, scope: Option<Scope>) {
    render_help_for_with(frame, area, scope, &KeyMap::default());
}

/// Draws the help for `scope` with the keys in `keys`.
pub fn render_help_for_with(frame: &mut Frame, area: Rect, scope: Option<Scope>, keys: &KeyMap) {
    let lines: Vec<Line> = shown(scope, keys)
        .iter()
        .map(|binding| Line::from(binding_spans(binding).to_vec()))
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

/// Draws the footer with every binding, wrapped to the width.
pub fn render_footer(frame: &mut Frame, area: Rect) {
    render_footer_for(frame, area, None);
}

/// Draws the help overlay with every binding.
pub fn render_help(frame: &mut Frame, area: Rect) {
    render_help_for(frame, area, None);
}
