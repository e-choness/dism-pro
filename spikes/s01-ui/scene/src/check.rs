//! The pixel snapshot gate (ADR-0002: "software-raster path renders all three screens"), judged
//! from the pixels alone. Both prototypes draw at [`crate::layout`]'s coordinates, so these checks
//! apply to either: fills sampled at interior points (exact token colors), and "ink" (text or
//! icons) counted inside the rectangles that must carry some. Row badges tie each visible row to
//! its data, which proves the virtualized list shows the right rows at the right offsets.

use crate::{
    Screen, State,
    data::{self, Nav},
    layout::{self, Rect},
    tokens::Rgb,
};

/// A rendered frame: straight-alpha RGBA8, `w × h` physical pixels at `scale`.
#[derive(Clone, Copy, Debug)]
pub struct Image<'a> {
    pub w: u32,
    pub h: u32,
    pub scale: f32,
    pub rgba: &'a [u8],
}

impl Image<'_> {
    /// The pixel under a logical point.
    #[must_use]
    pub fn at(&self, x: f32, y: f32) -> Option<Rgb> {
        let (px, py) = ((x * self.scale) as u32, (y * self.scale) as u32);
        if px >= self.w || py >= self.h {
            return None;
        }
        let i = (py * self.w + px) as usize * 4;
        self.rgba.get(i..i + 3).map(|p| Rgb(p[0], p[1], p[2]))
    }

    /// Share of the pixels in a logical rectangle that differ clearly from `bg`.
    #[must_use]
    pub fn ink(&self, r: Rect, bg: Rgb) -> f32 {
        let r = r.scaled(self.scale);
        let (x0, y0) = (r.x.max(0.0) as u32, r.y.max(0.0) as u32);
        let (x1, y1) = (
            (r.right() as u32).min(self.w),
            (r.bottom() as u32).min(self.h),
        );
        let (mut n, mut hit) = (0_u32, 0_u32);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * self.w + x) as usize * 4;
                n += 1;
                if Rgb(self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]).distance(bg) > 40 {
                    hit += 1;
                }
            }
        }
        if n == 0 { 0.0 } else { hit as f32 / n as f32 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

struct Checks<'a> {
    img: Image<'a>,
    out: Vec<Outcome>,
}

/// Interior fills are exact; this only absorbs rounding in the renderers' blending.
const TOLERANCE: u8 = 3;
/// Text in a rectangle covers at least this share of it.
const MIN_INK: f32 = 0.01;

impl Checks<'_> {
    fn fill(&mut self, name: &str, x: f32, y: f32, want: Rgb) {
        let got = self.img.at(x, y);
        let ok = got.is_some_and(|g| g.distance(want) <= TOLERANCE);
        self.out.push(Outcome {
            name: format!("fill {name}"),
            ok,
            detail: format!("at ({x}, {y}) want {want:?} got {got:?}"),
        });
    }

    fn ink(&mut self, name: &str, r: Rect, bg: Rgb) {
        let share = self.img.ink(r, bg);
        self.out.push(Outcome {
            name: format!("ink {name}"),
            ok: share >= MIN_INK,
            detail: format!("{:.1} % of {r:?}", share * 100.0),
        });
    }

    fn blank(&mut self, name: &str, r: Rect, bg: Rgb) {
        let share = self.img.ink(r, bg);
        self.out.push(Outcome {
            name: format!("blank {name}"),
            ok: share < MIN_INK / 4.0,
            detail: format!("{:.2} % of {r:?}", share * 100.0),
        });
    }
}

/// Checks a snapshot of `state` (solid mode). Rows come from [`data::Rows::generate`].
#[must_use]
pub fn verify(state: &State, rows: &data::Rows, img: Image<'_>) -> Vec<Outcome> {
    let p = state.theme.palette();
    let mut c = Checks {
        img,
        out: Vec::new(),
    };
    let expected = (
        (layout::WIDTH * img.scale).round() as u32,
        (layout::HEIGHT * img.scale).round() as u32,
    );
    c.out.push(Outcome {
        name: "size".into(),
        ok: (img.w, img.h) == expected,
        detail: format!("{}x{} want {}x{}", img.w, img.h, expected.0, expected.1),
    });
    if (img.w, img.h) != expected || img.rgba.len() != (img.w * img.h * 4) as usize {
        return c.out;
    }

    // Chrome: title bar, target pill, search, nav rail.
    c.fill("title bar", layout::PILL.right() + 24.0, 4.0, p.bg_app);
    c.ink("app name", layout::APP_NAME, p.bg_app);
    c.fill(
        "target pill",
        layout::PILL.x + 14.0,
        layout::PILL.y + 3.0,
        p.accent,
    );
    c.ink("target pill", layout::PILL.inset(12.0, 6.0), p.accent);
    c.fill(
        "search box",
        layout::SEARCH.right() - 8.0,
        layout::SEARCH.y + 16.0,
        p.bg_surface,
    );
    c.ink("search box", layout::SEARCH.inset(8.0, 6.0), p.bg_surface);
    for (i, e, r) in layout::nav_entries() {
        let bg = if i == state.nav { p.selected } else { p.bg_app };
        match e {
            Nav::Item(label) => {
                c.fill(&format!("nav {label}"), r.x + 3.0, r.y + r.h / 2.0, bg);
                c.ink(&format!("nav {label}"), r.inset(8.0, 6.0), bg);
            }
            Nav::Group(label) => c.ink(&format!("nav group {label}"), r.inset(8.0, 4.0), p.bg_app),
            Nav::Separator => {}
        }
    }

    match state.screen {
        Screen::Shell => {
            c.ink("page title", layout::PAGE_TITLE, p.bg_app);
            for (i, r) in layout::cards().into_iter().enumerate() {
                c.fill(
                    &format!("card {i}"),
                    r.right() - 12.0,
                    r.bottom() - 12.0,
                    p.bg_surface,
                );
                c.ink(&format!("card {i}"), r.inset(16.0, 16.0), p.bg_surface);
            }
            c.fill(
                "recent card",
                layout::recent().right() - 12.0,
                layout::recent().y + 12.0,
                p.bg_surface,
            );
            for i in 0..data::RECENT.len() {
                c.ink(&format!("recent {i}"), layout::recent_row(i), p.bg_surface);
            }
        }
        Screen::List | Screen::Drawer => {
            c.ink("page title", layout::PAGE_TITLE, p.bg_app);
            c.ink(
                "table header",
                layout::HEADER.inset(16.0, 8.0),
                p.bg_surface,
            );
            let covered = |r: Rect| state.screen == Screen::Drawer && r.right() > layout::DRAWER.x;
            for i in layout::visible_rows(state.scroll, rows.len()) {
                let r = layout::row(i, state.scroll);
                // Rows the viewport cuts are covered by the clip checks below.
                if r.y < layout::VIEWPORT.y || r.bottom() > layout::VIEWPORT.bottom() {
                    continue;
                }
                let bg = if state.selected == Some(i) {
                    p.selected
                } else {
                    p.bg_surface
                };
                c.fill(&format!("row {i}"), r.x + 4.0, r.y + r.h / 2.0, bg);
                let name = Rect::new(
                    r.x + layout::COLUMNS[0].0,
                    r.y + 6.0,
                    layout::NAME_CLIP,
                    r.h - 12.0,
                );
                c.ink(&format!("row {i} name"), name, bg);
                let b = layout::badge(i, state.scroll);
                if !covered(b) {
                    let want = p.badge(rows.badge[i]).map_or(bg, |(fill, _)| fill);
                    c.fill(
                        &format!("row {i} badge {:?}", rows.badge[i]),
                        b.x + 4.0,
                        b.y + b.h / 2.0,
                        want,
                    );
                }
            }
            // Nothing draws outside the viewport (the virtualized list is clipped).
            let below = Rect::new(
                layout::TABLE.x + 1.0,
                layout::TABLE.bottom() + 2.0,
                400.0,
                layout::PAD - 4.0,
            );
            c.blank("below the table", below, p.bg_app);
        }
    }

    if state.screen == Screen::Drawer {
        let d = layout::DRAWER;
        c.fill("drawer", d.right() - 8.0, d.y + 76.0, p.bg_surface);
        c.ink("drawer title", layout::DRAWER_TITLE, p.bg_surface);
        c.ink("drawer summary", layout::DRAWER_SUMMARY, p.bg_surface);
        for i in 0..data::CHANGES.len() {
            c.ink(&format!("change {i}"), layout::change(i), p.bg_surface);
        }
        c.ink(
            "drawer command",
            layout::DRAWER_CLI.inset(8.0, 8.0),
            p.bg_subtle,
        );
        c.fill(
            "apply button",
            layout::DRAWER_APPLY.x + 6.0,
            layout::DRAWER_APPLY.y + 16.0,
            p.accent,
        );
        c.ink(
            "apply button",
            layout::DRAWER_APPLY.inset(12.0, 6.0),
            p.accent,
        );
        c.ink(
            "cancel button",
            layout::DRAWER_CANCEL.inset(12.0, 6.0),
            p.bg_surface,
        );
    }
    c.out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::Theme;

    #[test]
    fn a_blank_frame_fails_and_reports_why() {
        let rows = data::Rows::generate(data::ROWS);
        let (w, h) = (layout::WIDTH as u32, layout::HEIGHT as u32);
        let px = [0_u8, 0, 0, 255].repeat((w * h) as usize);
        let img = Image {
            w,
            h,
            scale: 1.0,
            rgba: &px,
        };
        let out = verify(&State::snapshot(Screen::Drawer, Theme::Light), &rows, img);
        assert!(out.iter().any(|o| o.ok), "the size check passes");
        assert!(out.iter().filter(|o| !o.ok).count() > 50, "{out:?}");
        let wrong = Image {
            w: 10,
            h: 10,
            scale: 1.0,
            rgba: &px[..400],
        };
        let out = verify(&State::snapshot(Screen::List, Theme::Dark), &rows, wrong);
        assert_eq!(out.len(), 1);
        assert!(!out[0].ok);
    }
}
