// SPDX-License-Identifier: Apache-2.0

// The theme names, their order and their color values come from pbtop's
// crates/telemetry/src/tui/theme.rs (same owner, Apache-2.0).

//! Themes and palettes for the terminal view, free of any UI crate.

const TRUECOLOR_VALUES: [&str; 2] = ["truecolor", "24bit"];
const TERM_256_MARK: &str = "256color";
const NO_COLOR_VAR: &str = "NO_COLOR";

/// A color theme. `t` cycles them in the order of `ALL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Theme {
    #[default]
    Standard,
    Spectrum,
    Dark,
    Light,
    Mono,
}

impl Theme {
    pub const ALL: [Theme; 5] = [
        Theme::Standard,
        Theme::Spectrum,
        Theme::Dark,
        Theme::Light,
        Theme::Mono,
    ];

    pub fn next(self) -> Theme {
        match self {
            Theme::Standard => Theme::Spectrum,
            Theme::Spectrum => Theme::Dark,
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Mono,
            Theme::Mono => Theme::Standard,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Theme::Standard => "default",
            Theme::Spectrum => "spectrum",
            Theme::Dark => "dark",
            Theme::Light => "light",
            Theme::Mono => "mono",
        }
    }
}

/// How many colors the terminal can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorDepth {
    #[default]
    TrueColor,
    Ansi256,
    Ansi16,
}

impl ColorDepth {
    /// From `COLORTERM`, then `TERM`. An unset or unknown `TERM`, such as
    /// `dumb`, gives 16 colors.
    pub fn detect(lookup: impl Fn(&str) -> Option<String>) -> ColorDepth {
        let truecolor = lookup("COLORTERM").is_some_and(|v| TRUECOLOR_VALUES.contains(&v.as_str()));
        if truecolor {
            ColorDepth::TrueColor
        } else if lookup("TERM").is_some_and(|v| v.contains(TERM_256_MARK)) {
            ColorDepth::Ansi256
        } else {
            ColorDepth::Ansi16
        }
    }
}

/// One of the 16 ANSI colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    White,
}

/// A color role value: the terminal's own color, a named color or RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Reset,
    Named(NamedColor),
    Rgb(u8, u8, u8),
}

/// What a token of source code is, shared with the highlighter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Keyword,
    String,
    Number,
    Comment,
    Function,
    Type,
    Plain,
}

impl Role {
    pub const ALL: [Role; 7] = [
        Role::Keyword,
        Role::String,
        Role::Number,
        Role::Comment,
        Role::Function,
        Role::Type,
        Role::Plain,
    ];
}

/// Every color the view uses, by role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub depth: ColorDepth,
    pub background: Ink,
    pub foreground: Ink,
    pub title: Ink,
    pub accent: Ink,
    pub selection_fg: Ink,
    pub selection_bg: Ink,
    pub inactive: Ink,
    pub divider: Ink,
    pub live: Ink,
    pub caution: Ink,
    pub warn: Ink,
    pub error: Ink,
    pub added: Ink,
    pub removed: Ink,
    code: [Ink; 7],
}

const fn hex(rgb: u32) -> Ink {
    Ink::Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

const fn named(color: NamedColor) -> Ink {
    Ink::Named(color)
}

// Okabe and Ito (2008) hues, as pbtop uses them.
const SKY: Ink = hex(0x56b4e9);
const ORANGE: Ink = hex(0xe69f00);
const BLUISH_GREEN: Ink = hex(0x009e73);
const YELLOW: Ink = hex(0xf0e442);
const VERMILLION: Ink = hex(0xd55e00);
const PURPLE: Ink = hex(0xcc79a7);
const INACTIVE_GRAY: Ink = hex(0x767676);
const FOREGROUND_GRAY: Ink = hex(0xcccccc);
// pbtop's light theme amber for caution and warn.
const AMBER: Ink = hex(0xa06400);

impl Palette {
    pub fn of(theme: Theme, depth: ColorDepth) -> Palette {
        match theme {
            Theme::Standard => standard(depth),
            Theme::Spectrum => spectrum(depth),
            Theme::Dark => dark(depth),
            Theme::Light => light(depth),
            Theme::Mono => mono(depth),
        }
    }

    pub fn role(&self, role: Role) -> Ink {
        self.code[role as usize]
    }
}

/// Code roles in the order of `Role::ALL`.
const fn code(
    keyword: Ink,
    string: Ink,
    number: Ink,
    comment: Ink,
    function: Ink,
    type_: Ink,
    plain: Ink,
) -> [Ink; 7] {
    [keyword, string, number, comment, function, type_, plain]
}

// pbtop's default palette. Added and removed borrow its live and error
// colors, and the code roles borrow its series colors.
fn standard(depth: ColorDepth) -> Palette {
    Palette {
        depth,
        background: hex(0x000000),
        foreground: FOREGROUND_GRAY,
        title: hex(0xeeeeee),
        accent: SKY,
        selection_fg: hex(0xffffff),
        selection_bg: hex(0x1f4e79),
        inactive: INACTIVE_GRAY,
        divider: hex(0x6b6b6b),
        live: BLUISH_GREEN,
        caution: YELLOW,
        warn: ORANGE,
        error: VERMILLION,
        added: BLUISH_GREEN,
        removed: VERMILLION,
        code: code(
            PURPLE,
            BLUISH_GREEN,
            ORANGE,
            INACTIVE_GRAY,
            SKY,
            YELLOW,
            FOREGROUND_GRAY,
        ),
    }
}

// pbtop's spectrum overrides the four severity colors with HSV stops
// (130, 85, 75), (55, 85, 95), (30, 90, 95) and (0, 80, 95).
fn spectrum(depth: ColorDepth) -> Palette {
    let live = hex(0x1dbf38);
    let error = hex(0xf23030);
    Palette {
        live,
        caution: hex(0xf2e124),
        warn: hex(0xf28518),
        error,
        added: live,
        removed: error,
        ..standard(depth)
    }
}

// pbtop's dark theme. Added and removed borrow green and red, and the code
// roles borrow its series colors.
fn dark(depth: ColorDepth) -> Palette {
    Palette {
        depth,
        background: Ink::Reset,
        foreground: Ink::Reset,
        title: Ink::Reset,
        accent: named(NamedColor::Cyan),
        selection_fg: named(NamedColor::White),
        selection_bg: named(NamedColor::Blue),
        inactive: named(NamedColor::DarkGray),
        divider: named(NamedColor::DarkGray),
        live: named(NamedColor::Green),
        caution: named(NamedColor::Yellow),
        warn: named(NamedColor::Yellow),
        error: named(NamedColor::Red),
        added: named(NamedColor::Green),
        removed: named(NamedColor::Red),
        code: code(
            named(NamedColor::Magenta),
            named(NamedColor::Green),
            named(NamedColor::Yellow),
            named(NamedColor::DarkGray),
            named(NamedColor::Cyan),
            named(NamedColor::LightBlue),
            Ink::Reset,
        ),
    }
}

// pbtop's light theme: dark colors for a white ground, from its series colors.
fn light(depth: ColorDepth) -> Palette {
    Palette {
        accent: named(NamedColor::Blue),
        inactive: named(NamedColor::Gray),
        divider: named(NamedColor::Gray),
        caution: AMBER,
        warn: AMBER,
        code: code(
            named(NamedColor::Magenta),
            named(NamedColor::Green),
            AMBER,
            named(NamedColor::Gray),
            named(NamedColor::Blue),
            named(NamedColor::Cyan),
            Ink::Reset,
        ),
        ..dark(depth)
    }
}

fn mono(depth: ColorDepth) -> Palette {
    Palette {
        depth,
        background: Ink::Reset,
        foreground: Ink::Reset,
        title: Ink::Reset,
        accent: Ink::Reset,
        selection_fg: Ink::Reset,
        selection_bg: Ink::Reset,
        inactive: Ink::Reset,
        divider: Ink::Reset,
        live: Ink::Reset,
        caution: Ink::Reset,
        warn: Ink::Reset,
        error: Ink::Reset,
        added: Ink::Reset,
        removed: Ink::Reset,
        code: [Ink::Reset; 7],
    }
}

/// True when `NO_COLOR` is set to a non-empty value, as no-color.org asks.
pub fn no_color(lookup: impl Fn(&str) -> Option<String>) -> bool {
    lookup(NO_COLOR_VAR).is_some_and(|v| !v.is_empty())
}

/// The theme to draw with: mono when `NO_COLOR` is set, else the request.
pub fn effective_theme(requested: Theme, lookup: impl Fn(&str) -> Option<String>) -> Theme {
    if no_color(lookup) {
        Theme::Mono
    } else {
        requested
    }
}
