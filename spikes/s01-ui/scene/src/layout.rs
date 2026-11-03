//! Where everything is, in logical pixels (doc 05 §4: 4 px grid, nav rail 240 px). Both
//! prototypes place their elements here, so [`crate::check`] can judge either from pixels.

use std::ops::Range;

use crate::data::{self, Nav};

/// The window's client area. The pixel snapshots and the headless frame benchmark use it at
/// scale 1.0 and 1.5; the interactive G1 runs shrink it to fit the runner's 1024×768 screen.
pub const WIDTH: f32 = 1280.0;
pub const HEIGHT: f32 = 800.0;

pub const TITLE_H: f32 = 48.0;
pub const NAV_W: f32 = 240.0;
pub const NAV_TOP: f32 = 8.0;
pub const NAV_ITEM_H: f32 = 36.0;
pub const NAV_GROUP_H: f32 = 28.0;
pub const NAV_SEPARATOR_H: f32 = 17.0;
pub const PAD: f32 = 24.0;
pub const GAP: f32 = 16.0;
pub const PAGE_TITLE_H: f32 = 28.0;
pub const HEADER_H: f32 = 36.0;
pub const ROW_H: f32 = 32.0;
pub const DRAWER_W: f32 = 460.0;
pub const CHANGE_H: f32 = 40.0;
pub const BUTTON_H: f32 = 32.0;
pub const BADGE_H: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    #[must_use]
    pub const fn right(self) -> f32 {
        self.x + self.w
    }

    #[must_use]
    pub const fn bottom(self) -> f32 {
        self.y + self.h
    }

    #[must_use]
    pub const fn inset(self, dx: f32, dy: f32) -> Self {
        Self::new(
            self.x + dx,
            self.y + dy,
            self.w - 2.0 * dx,
            self.h - 2.0 * dy,
        )
    }

    #[must_use]
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    #[must_use]
    pub fn scaled(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.w * s, self.h * s)
    }

    #[must_use]
    pub fn intersect(self, o: Self) -> Option<Self> {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        (r > x && b > y).then(|| Self::new(x, y, r - x, b - y))
    }
}

pub const TITLE_BAR: Rect = Rect::new(0.0, 0.0, WIDTH, TITLE_H);
pub const APP_NAME: Rect = Rect::new(16.0, 0.0, 112.0, TITLE_H);
pub const PILL: Rect = Rect::new(136.0, 10.0, 268.0, 28.0);
pub const SEARCH: Rect = Rect::new(476.0, 8.0, 340.0, 32.0);
pub const ACTIVITY: Rect = Rect::new(WIDTH - 132.0, 8.0, 116.0, 32.0);
pub const NAV: Rect = Rect::new(0.0, TITLE_H, NAV_W, HEIGHT - TITLE_H);
pub const CONTENT: Rect = Rect::new(NAV_W, TITLE_H, WIDTH - NAV_W, HEIGHT - TITLE_H);
pub const PAGE_TITLE: Rect = Rect::new(NAV_W + PAD, TITLE_H + 16.0, 400.0, PAGE_TITLE_H);

/// Overview: four summary cards in a row, then the recent-activity card.
#[must_use]
pub fn cards() -> [Rect; 4] {
    let w = (CONTENT.w - 2.0 * PAD - 3.0 * GAP) / 4.0;
    let y = PAGE_TITLE.bottom() + GAP;
    std::array::from_fn(|i| Rect::new(CONTENT.x + PAD + i as f32 * (w + GAP), y, w, 104.0))
}

#[must_use]
pub fn recent() -> Rect {
    let top = cards()[0].bottom() + GAP;
    Rect::new(
        CONTENT.x + PAD,
        top,
        CONTENT.w - 2.0 * PAD,
        52.0 + 5.0 * 44.0,
    )
}

#[must_use]
pub fn recent_row(i: usize) -> Rect {
    let r = recent();
    Rect::new(r.x + 16.0, r.y + 48.0 + i as f32 * 44.0, r.w - 32.0, 44.0)
}

/// The nav rail's entries and their rows.
pub fn nav_entries() -> impl Iterator<Item = (usize, Nav, Rect)> {
    let mut y = NAV.y + NAV_TOP;
    data::NAV.into_iter().enumerate().map(move |(i, e)| {
        let h = match e {
            Nav::Item(_) => NAV_ITEM_H,
            Nav::Group(_) => NAV_GROUP_H,
            Nav::Separator => NAV_SEPARATOR_H,
        };
        let r = Rect::new(8.0, y, NAV_W - 16.0, h);
        y += h;
        (i, e, r)
    })
}

#[must_use]
pub fn nav_item(index: usize) -> Rect {
    nav_entries().nth(index).map_or(NAV, |(_, _, r)| r)
}

/// The list's card: header, then rows.
pub const TABLE: Rect = Rect::new(
    NAV_W + PAD,
    TITLE_H + 16.0 + PAGE_TITLE_H + GAP,
    WIDTH - NAV_W - 2.0 * PAD,
    HEIGHT - (TITLE_H + 16.0 + PAGE_TITLE_H + GAP) - PAD,
);
pub const HEADER: Rect = Rect::new(TABLE.x, TABLE.y, TABLE.w, HEADER_H);
/// The scrolling viewport of the rows.
pub const VIEWPORT: Rect = Rect::new(TABLE.x, TABLE.y + HEADER_H, TABLE.w, TABLE.h - HEADER_H);

/// Column x offsets (from [`TABLE`]'s left) and widths; Size is right-aligned.
pub const COLUMNS: [(f32, f32, bool); 5] = [
    (16.0, 330.0, false),
    (354.0, 170.0, false),
    (532.0, 92.0, false),
    (632.0, 92.0, true),
    (744.0, 216.0, false),
];

/// Name cells are clipped here, leaving room for the row badge.
pub const NAME_CLIP: f32 = 258.0;

/// Row `i`'s TD-0015 badge (`New`/`Changed`/`Orphan`), at the right end of the name column.
#[must_use]
pub fn badge(i: usize, scroll: f32) -> Rect {
    let r = row(i, scroll);
    Rect::new(
        r.x + COLUMNS[0].0 + COLUMNS[0].1 - 64.0,
        r.y + (ROW_H - BADGE_H) / 2.0,
        60.0,
        BADGE_H,
    )
}

/// The furthest the list scrolls.
#[must_use]
pub fn max_scroll(rows: usize) -> f32 {
    (rows as f32 * ROW_H - VIEWPORT.h).max(0.0)
}

/// Rows that intersect the viewport at `scroll`.
#[must_use]
pub fn visible_rows(scroll: f32, rows: usize) -> Range<usize> {
    let first = (scroll / ROW_H).floor().max(0.0) as usize;
    let last = ((scroll + VIEWPORT.h) / ROW_H).ceil() as usize;
    first.min(rows)..last.min(rows)
}

/// Row `i`'s rectangle at `scroll` (unclipped; the viewport clips it).
#[must_use]
pub fn row(i: usize, scroll: f32) -> Rect {
    Rect::new(
        VIEWPORT.x,
        VIEWPORT.y + i as f32 * ROW_H - scroll,
        VIEWPORT.w,
        ROW_H,
    )
}

/// The preview drawer, flush right under the title bar.
pub const DRAWER: Rect = Rect::new(WIDTH - DRAWER_W, TITLE_H, DRAWER_W, HEIGHT - TITLE_H);
pub const DRAWER_TITLE: Rect =
    Rect::new(DRAWER.x + PAD, DRAWER.y + 20.0, DRAWER_W - 2.0 * PAD, 28.0);
pub const DRAWER_SUMMARY: Rect =
    Rect::new(DRAWER.x + PAD, DRAWER.y + 50.0, DRAWER_W - 2.0 * PAD, 20.0);

#[must_use]
pub fn change(i: usize) -> Rect {
    Rect::new(
        DRAWER.x + PAD,
        DRAWER.y + 84.0 + i as f32 * CHANGE_H,
        DRAWER_W - 2.0 * PAD,
        CHANGE_H,
    )
}

pub const DRAWER_CLI: Rect = Rect::new(DRAWER.x + PAD, HEIGHT - 116.0, DRAWER_W - 2.0 * PAD, 44.0);
pub const DRAWER_CANCEL: Rect = Rect::new(
    WIDTH - PAD - 2.0 * 112.0 - 8.0,
    HEIGHT - 56.0,
    112.0,
    BUTTON_H,
);
pub const DRAWER_APPLY: Rect = Rect::new(WIDTH - PAD - 112.0, HEIGHT - 56.0, 112.0, BUTTON_H);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_fits_the_window() {
        for r in cards() {
            assert!(r.right() <= CONTENT.right() - PAD + 0.01, "{r:?}");
        }
        assert!(recent().bottom() <= HEIGHT, "{:?}", recent());
        assert!(
            change(11).bottom() <= DRAWER_CLI.y - 8.0,
            "{:?}",
            change(11)
        );
        let last = nav_entries().last().map(|(_, _, r)| r);
        assert!(last.is_some_and(|r| r.bottom() <= HEIGHT), "{last:?}");
        let (x, w, _) = COLUMNS[4];
        assert!(x + w <= TABLE.w, "columns overflow the table");
        assert!(PILL.right() < SEARCH.x && SEARCH.right() < ACTIVITY.x);
    }

    #[test]
    fn the_snapshot_row_is_visible() {
        let v = visible_rows(data::SNAPSHOT_SCROLL, data::ROWS);
        assert!(v.contains(&data::SNAPSHOT_SELECTED), "{v:?}");
        assert!(v.len() <= 21, "{v:?}");
        assert_eq!(
            visible_rows(max_scroll(data::ROWS), data::ROWS).end,
            data::ROWS
        );
    }
}
