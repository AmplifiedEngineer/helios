// SPDX-License-Identifier: Apache-2.0

//! The five themes: names, order, pinned values, depth detection and NO_COLOR.

use helios::theme::{effective_theme, no_color, ColorDepth, Ink, NamedColor, Palette, Role, Theme};
use helios::tui::style::ink_to_color;
use ratatui::style::Color;

const fn rgb(hex: u32) -> Ink {
    Ink::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |key| {
        pairs
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| (*v).to_string())
    }
}

/// Every plain field of a palette, then one ink per role.
fn all_inks(p: &Palette) -> Vec<Ink> {
    let mut inks = vec![
        p.title,
        p.accent,
        p.selection_fg,
        p.selection_bg,
        p.inactive,
        p.divider,
        p.live,
        p.caution,
        p.warn,
        p.error,
        p.added,
        p.removed,
    ];
    inks.extend(Role::ALL.map(|role| p.role(role)));
    inks
}

#[test]
fn the_five_themes_have_pbtop_names_and_order() {
    // Arrange
    let expected = [
        (Theme::Standard, "default"),
        (Theme::Spectrum, "spectrum"),
        (Theme::Dark, "dark"),
        (Theme::Light, "light"),
        (Theme::Mono, "mono"),
    ];

    // Act
    let listed: Vec<(Theme, &str)> = Theme::ALL.iter().map(|t| (*t, t.name())).collect();
    let cycled: Vec<Theme> = (0..6)
        .scan(Theme::Standard, |t, _| {
            let now = *t;
            *t = t.next();
            Some(now)
        })
        .collect();

    // Assert
    assert_eq!(listed, expected);
    assert_eq!(Theme::default(), Theme::Standard);
    assert_eq!(
        cycled,
        [
            Theme::Standard,
            Theme::Spectrum,
            Theme::Dark,
            Theme::Light,
            Theme::Mono,
            Theme::Standard
        ]
    );
}

#[test]
fn the_default_palette_pins_the_okabe_ito_values() {
    // Arrange
    // Okabe and Ito (2008) color universal design palette: orange e69f00,
    // sky blue 56b4e9, bluish green 009e73, yellow f0e442, vermillion d55e00,
    // reddish purple cc79a7. The grays and the selection blue 1f4e79 are
    // pbtop's own, and added and removed borrow bluish green and vermillion.

    // Act
    let p = Palette::of(Theme::Standard, ColorDepth::TrueColor);

    // Assert
    assert_eq!(p.title, rgb(0xeeeeee));
    assert_eq!(p.accent, rgb(0x56b4e9));
    assert_eq!(p.selection_fg, rgb(0xffffff));
    assert_eq!(p.selection_bg, rgb(0x1f4e79));
    assert_eq!(p.inactive, rgb(0x767676));
    assert_eq!(p.divider, rgb(0x6b6b6b));
    assert_eq!(p.live, rgb(0x009e73));
    assert_eq!(p.caution, rgb(0xf0e442));
    assert_eq!(p.warn, rgb(0xe69f00));
    assert_eq!(p.error, rgb(0xd55e00));
    assert_eq!(p.added, rgb(0x009e73));
    assert_eq!(p.removed, rgb(0xd55e00));
    assert_eq!(p.role(Role::Keyword), rgb(0xcc79a7));
    assert_eq!(p.role(Role::String), rgb(0x009e73));
    assert_eq!(p.role(Role::Number), rgb(0xe69f00));
    assert_eq!(p.role(Role::Comment), rgb(0x767676));
    assert_eq!(p.role(Role::Function), rgb(0x56b4e9));
    assert_eq!(p.role(Role::Type), rgb(0xf0e442));
    assert_eq!(p.role(Role::Plain), rgb(0xcccccc));
}

#[test]
fn the_other_four_themes_pin_their_named_values() {
    // Arrange
    // pbtop's `hi` is the helios accent. Values come from pbtop's theme.rs:
    // spectrum overrides only live, caution, warn and error (lines 343 to 355,
    // the HSV stops (0, 80, 95) give error 242, 48, 48) and inherits the rest
    // of default (hi line 302, sel_bg line 303). Dark is lines 356 to 398 (hi
    // 362, sel_bg 363, error 381). Light overrides hi at line 401 and inherits
    // sel_bg and error from dark (line 427). Mono is lines 429 to 449, with
    // every one of the three reset.
    let named = |c| Ink::Named(c);
    let table = [
        (
            Theme::Spectrum,
            rgb(0x56b4e9),
            rgb(0x1f4e79),
            Ink::Rgb(242, 48, 48),
        ),
        (
            Theme::Dark,
            named(NamedColor::Cyan),
            named(NamedColor::Blue),
            named(NamedColor::Red),
        ),
        (
            Theme::Light,
            named(NamedColor::Blue),
            named(NamedColor::Blue),
            named(NamedColor::Red),
        ),
        (Theme::Mono, Ink::Reset, Ink::Reset, Ink::Reset),
    ];

    for (theme, accent, selection_bg, error) in table {
        // Act
        let p = Palette::of(theme, ColorDepth::TrueColor);

        // Assert
        assert_eq!(p.accent, accent, "{} accent", theme.name());
        assert_eq!(
            p.selection_bg,
            selection_bg,
            "{} selection_bg",
            theme.name()
        );
        assert_eq!(p.error, error, "{} error", theme.name());
    }
}

#[test]
fn no_color_forces_mono() {
    // Arrange
    let set = [("NO_COLOR", "1")];
    let empty = [("NO_COLOR", "")];

    // Act
    let forced: Vec<Theme> = Theme::ALL
        .iter()
        .map(|t| effective_theme(*t, env(&set)))
        .collect();
    let kept = effective_theme(Theme::Dark, env(&empty));
    let mono = Palette::of(Theme::Mono, ColorDepth::TrueColor);

    // Assert
    assert!(no_color(env(&set)));
    assert!(!no_color(env(&empty)));
    assert!(!no_color(env(&[])));
    assert_eq!(forced, [Theme::Mono; 5]);
    assert_eq!(kept, Theme::Dark);
    for ink in all_inks(&mono) {
        assert_eq!(ink, Ink::Reset);
    }
}

#[test]
fn color_depth_comes_from_colorterm_and_term() {
    // Arrange
    let cases: [(&[(&str, &str)], ColorDepth); 7] = [
        (&[("COLORTERM", "truecolor")], ColorDepth::TrueColor),
        (
            &[("COLORTERM", "24bit"), ("TERM", "dumb")],
            ColorDepth::TrueColor,
        ),
        (&[("TERM", "xterm-256color")], ColorDepth::Ansi256),
        (&[("TERM", "linux")], ColorDepth::Ansi16),
        (&[("TERM", "dumb")], ColorDepth::Ansi16),
        (&[], ColorDepth::Ansi16),
        (
            &[("COLORTERM", "yes"), ("TERM", "xterm")],
            ColorDepth::Ansi16,
        ),
    ];

    for (pairs, expected) in cases {
        // Act
        let depth = ColorDepth::detect(env(pairs));

        // Assert
        assert_eq!(depth, expected, "{pairs:?}");
    }
}

#[test]
fn every_role_has_a_color_in_all_five_themes() {
    for theme in Theme::ALL {
        // Arrange
        let p = Palette::of(theme, ColorDepth::TrueColor);

        for role in Role::ALL {
            // Act
            let ink = p.role(role);

            // Assert
            if theme == Theme::Mono {
                assert_eq!(ink, Ink::Reset, "mono {role:?}");
            } else if role != Role::Plain {
                assert_ne!(ink, Ink::Reset, "{} {role:?}", theme.name());
            }
        }
    }
}

#[test]
fn light_caution_and_warn_are_not_the_error_color_at_16_colors() {
    // Arrange
    let true_color = Palette::of(Theme::Light, ColorDepth::TrueColor);
    let reduced = Palette::of(Theme::Light, ColorDepth::Ansi16);

    // Act
    let caution = ink_to_color(reduced.caution, ColorDepth::Ansi16);
    let warn = ink_to_color(reduced.warn, ColorDepth::Ansi16);
    let error = ink_to_color(reduced.error, ColorDepth::Ansi16);

    // Assert
    assert_ne!(caution, error, "caution against error");
    assert_ne!(warn, error, "warn against error");
    assert_eq!(caution, Color::Yellow);
    assert_eq!(warn, Color::Yellow);
    assert_eq!(true_color.caution, rgb(0xa06400));
    assert_eq!(true_color.warn, rgb(0xa06400));
}

#[test]
fn keywords_and_comments_differ_at_16_colors() {
    // Arrange
    let depths = [ColorDepth::Ansi16, ColorDepth::Ansi256];

    for theme in Theme::ALL.into_iter().filter(|t| *t != Theme::Mono) {
        for depth in depths {
            let palette = Palette::of(theme, depth);

            // Act
            let keyword = ink_to_color(palette.role(Role::Keyword), depth);
            let comment = ink_to_color(palette.role(Role::Comment), depth);

            // Assert
            assert_ne!(keyword, comment, "{} at {depth:?}", theme.name());
        }
    }
}
