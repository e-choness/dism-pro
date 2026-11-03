//! The three screens, drawn immediately from a `scene::State` at `scene::layout`'s coordinates.
//!
//! `glass` selects the Windows 11 materials (ADR-0007): the window base stays transparent for DWM's
//! Mica, cards and controls become translucent layers, and the drawer is in-app acrylic over the
//! content beneath it. Otherwise everything is the solid token colors (WinPE, transparency off).

use scene::{
    Screen, State,
    data::{self, Nav, Reversibility, Rows},
    layout::{self, Rect},
    tokens::{Palette, Rgb, Theme, font, radius},
};

use crate::{
    acrylic,
    canvas::Canvas,
    text::{Align, Style, Text},
};

/// Materials for one theme (doc 05 §4); `alpha` 255 in solid mode.
#[derive(Clone, Copy)]
pub struct Look {
    pub glass: bool,
    pub card: (Rgb, u8),
    pub card_border: (Rgb, u8),
    pub control: (Rgb, u8),
    pub selected: (Rgb, u8),
}

impl Look {
    pub fn new(theme: Theme, glass: bool) -> Self {
        let p = theme.palette();
        let white = Rgb(255, 255, 255);
        let pct = |v: u32| (v * 255 / 100) as u8;
        match (glass, theme) {
            (false, _) => Self {
                glass,
                card: (p.bg_surface, 255),
                card_border: (p.border, 255),
                control: (p.bg_surface, 255),
                selected: (p.selected, 255),
            },
            (true, Theme::Light) => Self {
                glass,
                card: (white, pct(62)),
                card_border: (white, pct(78)),
                control: (white, pct(70)),
                selected: (p.accent, pct(13)),
            },
            (true, Theme::Dark) => Self {
                glass,
                card: (white, pct(5)),
                card_border: (white, pct(8)),
                control: (white, pct(6)),
                selected: (p.accent, pct(14)),
            },
        }
    }
}

const fn st(size: f32, c: Rgb) -> Style {
    Style::new(size, c)
}

/// Renders one full frame.
pub fn frame(c: &mut Canvas, t: &mut Text, s: &State, rows: &Rows, glass: bool) {
    let p = s.theme.palette();
    let look = Look::new(s.theme, glass);
    c.set_clip(None);
    c.clear(p.bg_app, if glass { 0 } else { 255 });
    chrome(c, t, s, p, look);
    match s.screen {
        Screen::Shell => overview(c, t, p, look),
        Screen::List | Screen::Drawer => list(c, t, s, p, look, rows),
    }
    if s.screen == Screen::Drawer {
        drawer(c, t, s.theme, p, look);
    }
    t.end_frame();
}

fn chrome(c: &mut Canvas, t: &mut Text, s: &State, p: &Palette, look: Look) {
    t.draw(
        c,
        data::APP_NAME,
        layout::APP_NAME,
        st(font::SUBTITLE, p.text_primary).bold(),
    );
    let pill = layout::PILL;
    c.rrect(pill, [pill.h / 2.0; 4], p.accent, 255);
    t.draw(
        c,
        data::TARGET,
        pill.inset(12.0, 0.0),
        st(font::BODY, p.on_accent).bold(),
    );
    c.bordered(
        layout::SEARCH,
        [radius::CONTROL; 4],
        look.control,
        look.card_border,
    );
    t.draw(
        c,
        data::SEARCH,
        layout::SEARCH.inset(12.0, 0.0),
        st(font::BODY, p.text_secondary),
    );
    c.bordered(
        layout::ACTIVITY,
        [radius::CONTROL; 4],
        look.control,
        look.card_border,
    );
    t.draw(
        c,
        data::ACTIVITY,
        layout::ACTIVITY,
        st(font::BODY, p.text_primary).bold().align(Align::Center),
    );

    for (i, e, r) in layout::nav_entries() {
        match e {
            Nav::Item(label) => {
                if i == s.nav {
                    c.rrect(r, [radius::CONTROL; 4], look.selected.0, look.selected.1);
                }
                t.draw(c, label, r.inset(12.0, 0.0), st(font::BODY, p.text_primary));
            }
            Nav::Group(label) => t.draw(
                c,
                label,
                r.inset(12.0, 0.0),
                st(font::CAPTION, p.text_secondary).bold(),
            ),
            Nav::Separator => c.hline(r.x + 12.0, r.y + r.h / 2.0, r.w - 24.0, p.border, 255),
        }
    }
}

fn badge(c: &mut Canvas, t: &mut Text, r: Rect, label: &str, fill: Rgb, ink: Rgb) {
    c.rrect(r, [r.h / 2.0; 4], fill, 255);
    t.draw(
        c,
        label,
        r.inset(8.0, 0.0),
        st(11.0, ink).align(Align::Center),
    );
}

fn reversibility(c: &mut Canvas, t: &mut Text, p: &Palette, r: Rect, v: Reversibility) {
    let (fill, ink) = p.reversibility(v);
    badge(c, t, r, v.label(), fill, ink);
}

fn overview(c: &mut Canvas, t: &mut Text, p: &Palette, look: Look) {
    t.draw(
        c,
        data::OVERVIEW_TITLE,
        layout::PAGE_TITLE,
        st(font::TITLE, p.text_primary).bold(),
    );
    for (r, (label, value)) in layout::cards().into_iter().zip(data::CARDS) {
        c.bordered(r, [radius::CARD; 4], look.card, look.card_border);
        t.draw(
            c,
            label,
            Rect::new(r.x + 16.0, r.y + 16.0, r.w - 32.0, 20.0),
            st(font::CAPTION, p.text_secondary),
        );
        t.draw(
            c,
            value,
            Rect::new(r.x + 16.0, r.y + 44.0, r.w - 32.0, 36.0),
            st(font::DISPLAY, p.text_primary).bold(),
        );
    }
    let box_ = layout::recent();
    c.bordered(box_, [radius::CARD; 4], look.card, look.card_border);
    let title = Rect::new(box_.x + 16.0, box_.y + 12.0, box_.w - 32.0, 28.0);
    t.draw(
        c,
        data::RECENT_TITLE,
        title,
        st(font::SUBTITLE, p.text_primary).bold(),
    );
    for (i, (label, rev)) in data::RECENT.into_iter().enumerate() {
        let r = layout::recent_row(i);
        t.draw(
            c,
            label,
            Rect::new(r.x, r.y, r.w - 160.0, r.h),
            st(font::BODY, p.text_primary),
        );
        let b = Rect::new(
            r.right() - 120.0,
            r.y + (r.h - layout::BADGE_H) / 2.0,
            120.0,
            layout::BADGE_H,
        );
        reversibility(c, t, p, b, rev);
    }
}

fn list(c: &mut Canvas, t: &mut Text, s: &State, p: &Palette, look: Look, rows: &Rows) {
    t.draw(
        c,
        data::APPS_TITLE,
        layout::PAGE_TITLE,
        st(font::TITLE, p.text_primary).bold(),
    );
    let table = layout::TABLE;
    c.bordered(table, [radius::CARD; 4], look.card, look.card_border);
    for ((x, w, right), title) in layout::COLUMNS.into_iter().zip(data::COLUMNS) {
        let r = Rect::new(table.x + x, table.y, w, layout::HEADER_H);
        let style = st(font::CAPTION, p.text_secondary).bold();
        t.draw(
            c,
            title,
            r,
            if right {
                style.align(Align::Right)
            } else {
                style
            },
        );
    }
    c.hline(
        table.x + 1.0,
        table.y + layout::HEADER_H - 1.0,
        table.w - 2.0,
        p.border,
        255,
    );

    c.set_clip(Some(layout::VIEWPORT));
    for i in layout::visible_rows(s.scroll, rows.len()) {
        let r = layout::row(i, s.scroll);
        if s.selected == Some(i) {
            c.rect(r, look.selected.0, look.selected.1);
        }
        let [name, publisher, version, size, installed] = layout::COLUMNS;
        let cell = |(x, w, _): (f32, f32, bool)| Rect::new(r.x + x, r.y, w, r.h);
        t.draw(
            c,
            rows.cell(i, 0),
            Rect::new(r.x + name.0, r.y, layout::NAME_CLIP, r.h),
            st(font::BODY, p.text_primary),
        );
        if let Some((fill, ink)) = p.badge(rows.badge[i]) {
            badge(
                c,
                t,
                layout::badge(i, s.scroll),
                rows.badge[i].label(),
                fill,
                ink,
            );
        }
        t.draw(
            c,
            rows.cell(i, 1),
            cell(publisher),
            st(font::BODY, p.text_secondary),
        );
        t.draw(
            c,
            rows.cell(i, 2),
            cell(version),
            st(font::BODY, p.text_secondary),
        );
        t.draw(
            c,
            rows.cell(i, 3),
            cell(size),
            st(font::BODY, p.text_primary).align(Align::Right),
        );
        t.draw(
            c,
            rows.cell(i, 4),
            cell(installed),
            st(font::BODY, p.text_secondary),
        );
    }
    c.set_clip(None);
}

fn drawer(c: &mut Canvas, t: &mut Text, theme: Theme, p: &Palette, look: Look) {
    let d = layout::DRAWER;
    let corners = [radius::DRAWER, 0.0, 0.0, radius::DRAWER];
    if look.glass {
        // In-app acrylic (ADR-0007): the content beneath, blurred and tinted. Square-cornered in
        // the spike; the product clips it to the drawer's shape.
        let (tint, alpha) = match theme {
            Theme::Light => (Rgb::hex(0xF9FAFD), 189),
            Theme::Dark => (Rgb::hex(0x202228), 189),
        };
        acrylic::apply(c, d, tint, alpha);
        c.vline(d.x, d.y, d.h, look.card_border.0, look.card_border.1);
    } else {
        c.bordered(d, corners, (p.bg_surface, 255), (p.border, 255));
    }
    t.draw(
        c,
        data::DRAWER_TITLE,
        layout::DRAWER_TITLE,
        st(font::TITLE, p.text_primary).bold(),
    );
    t.draw(
        c,
        data::DRAWER_SUMMARY,
        layout::DRAWER_SUMMARY,
        st(font::BODY, p.text_secondary),
    );
    for (i, (label, detail, rev)) in data::CHANGES.into_iter().enumerate() {
        let r = layout::change(i);
        t.draw(
            c,
            label,
            Rect::new(r.x, r.y, r.w - 220.0, r.h),
            st(font::BODY, p.text_primary),
        );
        let dr = Rect::new(r.right() - 212.0, r.y, 76.0, r.h);
        t.draw(
            c,
            detail,
            dr,
            st(font::BODY, p.text_secondary).align(Align::Right),
        );
        let b = Rect::new(
            r.right() - 124.0,
            r.y + (r.h - layout::BADGE_H) / 2.0,
            124.0,
            layout::BADGE_H,
        );
        reversibility(c, t, p, b, rev);
    }
    c.rrect(layout::DRAWER_CLI, [radius::CONTROL; 4], p.bg_subtle, 255);
    t.draw(
        c,
        data::DRAWER_CLI,
        layout::DRAWER_CLI.inset(12.0, 0.0),
        st(font::CAPTION, p.text_primary).mono(),
    );
    c.bordered(
        layout::DRAWER_CANCEL,
        [radius::CONTROL; 4],
        (p.bg_surface, 255),
        (p.border, 255),
    );
    let center = |c: Rgb| st(font::BODY, c).bold().align(Align::Center);
    t.draw(
        c,
        data::DRAWER_CANCEL,
        layout::DRAWER_CANCEL,
        center(p.text_primary),
    );
    c.rrect(layout::DRAWER_APPLY, [radius::CONTROL; 4], p.accent, 255);
    t.draw(
        c,
        data::DRAWER_APPLY,
        layout::DRAWER_APPLY,
        center(p.on_accent),
    );
}
