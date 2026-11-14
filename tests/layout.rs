// SPDX-License-Identifier: Apache-2.0

use helios::tui::layout::{
    panes, LayoutSettings, Pane, Panes, LIST_PERCENT, MIN_PANE_WIDTH, NARROW_BELOW,
};
use ratatui::layout::Rect;

const AREA_HEIGHT: u16 = 30;
const MAX_SWEPT_WIDTH: u16 = 300;
const WIDE_WIDTH: u16 = 120;
const CUSTOM_NARROW_BELOW: u16 = 40;
const TINY_LIST_PERCENT: u16 = 1;
const HUGE_LIST_PERCENT: u16 = 99;
const ORIGINS: [(u16, u16); 3] = [(0, 0), (5, 3), (17, 9)];

fn split(area: Rect, settings: &LayoutSettings) -> Option<(Rect, Rect)> {
    match panes(area, Pane::List, settings) {
        Panes::Split { list, detail } => Some((list, detail)),
        Panes::Single(_) => None,
    }
}

#[test]
fn a_wide_area_splits_into_list_and_detail() {
    // Arrange
    let area = Rect::new(0, 0, WIDE_WIDTH, AREA_HEIGHT);

    // Act
    let result = split(area, &LayoutSettings::default());

    // Assert
    let (list, detail) = result.expect("a wide area splits");
    assert_eq!(list.width, WIDE_WIDTH * LIST_PERCENT / 100);
    assert_eq!(detail.width, WIDE_WIDTH - list.width);
    assert_eq!(list.x, 0);
}

#[test]
fn an_area_under_the_narrow_limit_gives_one_pane() {
    // Arrange
    let narrow = Rect::new(0, 0, NARROW_BELOW - 1, AREA_HEIGHT);
    let limit = Rect::new(0, 0, NARROW_BELOW, AREA_HEIGHT);

    // Act
    let under = panes(narrow, Pane::List, &LayoutSettings::default());
    let at = panes(limit, Pane::List, &LayoutSettings::default());

    // Assert
    assert_eq!(under, Panes::Single(narrow));
    assert!(matches!(at, Panes::Split { .. }));
}

#[test]
fn the_split_never_leaves_a_pane_with_zero_width() {
    // Arrange
    let default = LayoutSettings::default();
    let extremes = [TINY_LIST_PERCENT, HUGE_LIST_PERCENT].map(|list_percent| LayoutSettings {
        list_percent,
        narrow_below: CUSTOM_NARROW_BELOW,
    });
    let cases = std::iter::once((default, NARROW_BELOW)).chain(
        extremes
            .into_iter()
            .map(|settings| (settings, CUSTOM_NARROW_BELOW)),
    );

    for (settings, first_width) in cases {
        for width in first_width..=MAX_SWEPT_WIDTH {
            let area = Rect::new(0, 0, width, AREA_HEIGHT);

            // Act
            let result = split(area, &settings);

            // Assert
            let (list, detail) = result.expect("the area is at the narrow limit or wider");
            assert!(list.width > 0 && detail.width > 0, "zero pane at {width}");
            if width >= 2 * MIN_PANE_WIDTH {
                assert!(list.width >= MIN_PANE_WIDTH, "list too thin at {width}");
                assert!(detail.width >= MIN_PANE_WIDTH, "detail too thin at {width}");
            }
        }
    }
}

#[test]
fn the_panes_cover_the_area_without_overlap() {
    // Arrange
    let settings = LayoutSettings::default();

    for (x, y) in ORIGINS {
        for width in NARROW_BELOW..=MAX_SWEPT_WIDTH {
            let area = Rect::new(x, y, width, AREA_HEIGHT);

            // Act
            let result = split(area, &settings);

            // Assert
            let (list, detail) = result.expect("the area is at the narrow limit or wider");
            assert_eq!(list.width + detail.width, area.width);
            assert_eq!((list.y, list.height), (area.y, area.height));
            assert_eq!((detail.y, detail.height), (area.y, area.height));
            assert_eq!(list.x, area.x);
            assert_eq!(detail.x, list.x + list.width);
        }
    }
}
