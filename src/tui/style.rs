// SPDX-License-Identifier: Apache-2.0

//! Turns palette inks into ratatui colors and styles.

use ratatui::style::{Color, Modifier, Style};

use crate::app::App;
use crate::theme::{ColorDepth, Ink, NamedColor, Palette, Theme};

/// The levels of one channel in the xterm 6x6x6 color cube.
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
const CUBE_START: u8 = 16;
const CUBE_SIDE: u8 = 6;
const GRAY_START: u8 = 232;
const GRAY_STEPS: u8 = 24;
const GRAY_FIRST_LEVEL: u8 = 8;
const GRAY_STEP: u8 = 10;

/// The 16 ANSI colors with the RGB values xterm shows for them.
const ANSI16: [(NamedColor, [u8; 3]); 16] = [
    (NamedColor::Black, [0, 0, 0]),
    (NamedColor::Red, [205, 0, 0]),
    (NamedColor::Green, [0, 205, 0]),
    (NamedColor::Yellow, [205, 205, 0]),
    (NamedColor::Blue, [0, 0, 238]),
    (NamedColor::Magenta, [205, 0, 205]),
    (NamedColor::Cyan, [0, 205, 205]),
    (NamedColor::Gray, [229, 229, 229]),
    (NamedColor::DarkGray, [127, 127, 127]),
    (NamedColor::LightRed, [255, 0, 0]),
    (NamedColor::LightGreen, [0, 255, 0]),
    (NamedColor::LightYellow, [255, 255, 0]),
    (NamedColor::LightBlue, [92, 92, 255]),
    (NamedColor::LightMagenta, [255, 0, 255]),
    (NamedColor::LightCyan, [0, 255, 255]),
    (NamedColor::White, [255, 255, 255]),
];

/// The ratatui color for `ink` on a terminal that shows `depth` colors.
pub fn ink_to_color(ink: Ink, depth: ColorDepth) -> Color {
    match (ink, depth) {
        (Ink::Reset, _) => Color::Reset,
        (Ink::Named(named), _) => named_color(named),
        (Ink::Rgb(r, g, b), ColorDepth::TrueColor) => Color::Rgb(r, g, b),
        (Ink::Rgb(r, g, b), ColorDepth::Ansi256) => Color::Indexed(nearest_indexed([r, g, b])),
        (Ink::Rgb(r, g, b), ColorDepth::Ansi16) => named_color(nearest_named([r, g, b])),
    }
}

fn named_color(named: NamedColor) -> Color {
    match named {
        NamedColor::Black => Color::Black,
        NamedColor::Red => Color::Red,
        NamedColor::Green => Color::Green,
        NamedColor::Yellow => Color::Yellow,
        NamedColor::Blue => Color::Blue,
        NamedColor::Magenta => Color::Magenta,
        NamedColor::Cyan => Color::Cyan,
        NamedColor::Gray => Color::Gray,
        NamedColor::DarkGray => Color::DarkGray,
        NamedColor::LightRed => Color::LightRed,
        NamedColor::LightGreen => Color::LightGreen,
        NamedColor::LightYellow => Color::LightYellow,
        NamedColor::LightBlue => Color::LightBlue,
        NamedColor::LightMagenta => Color::LightMagenta,
        NamedColor::LightCyan => Color::LightCyan,
        NamedColor::White => Color::White,
    }
}

fn distance(a: [u8; 3], b: [u8; 3]) -> u32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| u32::from(x.abs_diff(y)).pow(2))
        .sum()
}

fn nearest_named(rgb: [u8; 3]) -> NamedColor {
    ANSI16
        .iter()
        .min_by_key(|(_, reference)| distance(rgb, *reference))
        .map_or(NamedColor::White, |(named, _)| *named)
}

/// The index of the nearest color in the xterm cube or its gray ramp.
fn nearest_indexed(rgb: [u8; 3]) -> u8 {
    let cube_steps = rgb.map(|channel| {
        let nearest = CUBE_LEVELS
            .iter()
            .enumerate()
            .min_by_key(|(_, level)| level.abs_diff(channel))
            .map_or(0, |(step, _)| step);
        u8::try_from(nearest).unwrap_or(0)
    });
    let cube_rgb = cube_steps.map(|step| CUBE_LEVELS[usize::from(step)]);
    let cube_index = CUBE_START
        + cube_steps[0] * CUBE_SIDE * CUBE_SIDE
        + cube_steps[1] * CUBE_SIDE
        + cube_steps[2];

    let average = rgb.iter().map(|c| u16::from(*c)).sum::<u16>() / 3;
    let gray_step = ((average + u16::from(GRAY_STEP / 2))
        .saturating_sub(u16::from(GRAY_FIRST_LEVEL)))
        / u16::from(GRAY_STEP);
    let gray_step = u8::try_from(gray_step)
        .unwrap_or(GRAY_STEPS - 1)
        .min(GRAY_STEPS - 1);
    let gray_level = GRAY_FIRST_LEVEL + gray_step * GRAY_STEP;

    if distance(rgb, [gray_level; 3]) < distance(rgb, cube_rgb) {
        GRAY_START + gray_step
    } else {
        cube_index
    }
}

/// A style that sets the foreground to `ink`, or none when `ink` is the terminal's own color.
fn foreground(ink: Ink, depth: ColorDepth) -> Style {
    match ink {
        Ink::Reset => Style::new(),
        ink => Style::new().fg(ink_to_color(ink, depth)),
    }
}

/// The style of the selected row: the palette's colors, or reversed video without color.
fn selection(palette: &Palette) -> Style {
    let depth = palette.depth;
    match (palette.selection_fg, palette.selection_bg) {
        (Ink::Reset, Ink::Reset) => Style::new().add_modifier(Modifier::REVERSED),
        (fg, bg) => foreground(fg, depth).bg(ink_to_color(bg, depth)),
    }
}

/// Every style the screens draw with, built once from the app's theme and color depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Styles {
    pub border: Style,
    pub border_focused: Style,
    pub title: Style,
    pub selection: Style,
    pub tab_current: Style,
    pub inactive: Style,
    pub error: Style,
    pub added: Style,
    pub removed: Style,
    pub check_pass: Style,
    pub check_fail: Style,
    pub check_pending: Style,
    pub review_approved: Style,
    pub review_changes: Style,
    pub review_required: Style,
}

impl Styles {
    pub fn of(app: &App) -> Self {
        Self::new(app.theme(), app.depth())
    }

    pub fn new(theme: Theme, depth: ColorDepth) -> Self {
        let palette = Palette::of(theme, depth);
        let ink = |ink| foreground(ink, depth);
        Self {
            border: ink(palette.divider),
            border_focused: ink(palette.accent),
            title: ink(palette.title),
            selection: selection(&palette),
            tab_current: ink(palette.accent).add_modifier(Modifier::BOLD),
            inactive: ink(palette.inactive),
            error: ink(palette.error),
            added: ink(palette.added),
            removed: ink(palette.removed),
            check_pass: ink(palette.live),
            check_fail: ink(palette.error),
            check_pending: ink(palette.caution),
            review_approved: ink(palette.live),
            review_changes: ink(palette.warn),
            review_required: ink(palette.caution),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::NamedColor;

    #[test]
    fn reset_and_named_inks_keep_their_meaning_at_every_depth() {
        // Arrange
        let depths = [
            ColorDepth::TrueColor,
            ColorDepth::Ansi256,
            ColorDepth::Ansi16,
        ];

        for depth in depths {
            // Act
            let reset = ink_to_color(Ink::Reset, depth);
            let named = ink_to_color(Ink::Named(NamedColor::DarkGray), depth);

            // Assert
            assert_eq!(reset, Color::Reset);
            assert_eq!(named, Color::DarkGray);
        }
    }

    #[test]
    fn rgb_stays_rgb_on_a_true_color_terminal() {
        // Arrange
        let ink = Ink::Rgb(0x56, 0xb4, 0xe9);

        // Act
        let color = ink_to_color(ink, ColorDepth::TrueColor);

        // Assert
        assert_eq!(color, Color::Rgb(0x56, 0xb4, 0xe9));
    }

    #[test]
    fn rgb_becomes_the_nearest_cube_or_gray_index_on_a_256_color_terminal() {
        // Arrange
        let cases = [
            (Ink::Rgb(255, 0, 0), Color::Indexed(196)),
            (Ink::Rgb(0, 0, 0), Color::Indexed(16)),
            (Ink::Rgb(128, 128, 128), Color::Indexed(244)),
        ];

        for (ink, expected) in cases {
            // Act
            let color = ink_to_color(ink, ColorDepth::Ansi256);

            // Assert
            assert_eq!(color, expected, "{ink:?}");
        }
    }

    #[test]
    fn rgb_becomes_the_nearest_named_color_on_a_16_color_terminal() {
        // Arrange
        let cases = [
            (Ink::Rgb(250, 10, 10), Color::LightRed),
            (Ink::Rgb(0, 0, 0), Color::Black),
            (Ink::Rgb(255, 255, 255), Color::White),
            (Ink::Rgb(0, 150, 0), Color::Green),
        ];

        for (ink, expected) in cases {
            // Act
            let color = ink_to_color(ink, ColorDepth::Ansi16);

            // Assert
            assert_eq!(color, expected, "{ink:?}");
        }
    }
}
