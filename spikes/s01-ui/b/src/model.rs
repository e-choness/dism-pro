//! Binds `scene` to the Slint component: geometry, strings, the 100k-row model, the palette.

use std::rc::Rc;

use scene::{
    State,
    data::{self, Badge, Nav, Rows},
    layout::{self, Rect},
    tokens::Rgb,
};
use slint::{ComponentHandle, Model, ModelRc, ModelTracker, SharedString, VecModel};

use crate::{App, Area, Column, Item, NavEntry, Palette, Row};

fn area(r: Rect) -> Area {
    Area {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
    }
}

fn color(c: Rgb) -> slint::Color {
    slint::Color::from_rgb_u8(c.0, c.1, c.2)
}

/// The list model: rows are made on demand, only for the items the `ListView` instantiates.
struct RowsModel(Rc<Rows>);

impl Model for RowsModel {
    type Data = Row;

    fn row_count(&self) -> usize {
        self.0.len()
    }

    fn row_data(&self, i: usize) -> Option<Row> {
        let rows = &self.0;
        (i < rows.len()).then(|| Row {
            name: rows.cell(i, 0).into(),
            publisher: rows.cell(i, 1).into(),
            version: rows.cell(i, 2).into(),
            size: rows.cell(i, 3).into(),
            installed: rows.cell(i, 4).into(),
            badge: match rows.badge[i] {
                Badge::None => 0,
                Badge::New => 1,
                Badge::Changed => 2,
                Badge::Orphan => 3,
            },
            badge_label: rows.badge[i].label().into(),
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &()
    }
}

fn items(v: impl Iterator<Item = Item>) -> ModelRc<Item> {
    ModelRc::new(VecModel::from(v.collect::<Vec<_>>()))
}

/// Everything that doesn't change with the state.
pub fn fill(app: &App, rows: Rc<Rows>) {
    let nav = layout::nav_entries().map(|(_, e, r)| {
        let (kind, label) = match e {
            Nav::Item(l) => (0, l),
            Nav::Group(l) => (1, l),
            Nav::Separator => (2, ""),
        };
        NavEntry {
            kind,
            label: label.into(),
            r: area(r),
        }
    });
    app.set_nav(ModelRc::new(VecModel::from(nav.collect::<Vec<_>>())));
    app.set_rows(ModelRc::new(RowsModel(rows)));
    app.set_cards(items(layout::cards().into_iter().zip(data::CARDS).map(
        |(r, (label, value))| Item {
            label: label.into(),
            detail: value.into(),
            badge: SharedString::new(),
            permanent: false,
            r: area(r),
        },
    )));
    app.set_recent(items(data::RECENT.into_iter().enumerate().map(
        |(i, (label, rev))| Item {
            label: label.into(),
            detail: SharedString::new(),
            badge: rev.label().into(),
            permanent: rev == data::Reversibility::Permanent,
            r: area(layout::recent_row(i)),
        },
    )));
    app.set_changes(items(data::CHANGES.into_iter().enumerate().map(
        |(i, (label, detail, rev))| Item {
            label: label.into(),
            detail: detail.into(),
            badge: rev.label().into(),
            permanent: rev == data::Reversibility::Permanent,
            r: area(layout::change(i)),
        },
    )));
    let columns = layout::COLUMNS
        .into_iter()
        .zip(data::COLUMNS)
        .map(|((x, w, right), title)| Column {
            title: title.into(),
            x,
            w,
            right,
        });
    app.set_columns(ModelRc::new(VecModel::from(columns.collect::<Vec<_>>())));
    app.set_app_name(data::APP_NAME.into());
    app.set_target(data::TARGET.into());
    app.set_search(data::SEARCH.into());
    app.set_activity(data::ACTIVITY.into());
    app.set_overview_title(data::OVERVIEW_TITLE.into());
    app.set_apps_title(data::APPS_TITLE.into());
    app.set_recent_title(data::RECENT_TITLE.into());
    app.set_drawer_title(data::DRAWER_TITLE.into());
    app.set_drawer_summary(data::DRAWER_SUMMARY.into());
    app.set_drawer_cli(data::DRAWER_CLI.into());
    app.set_drawer_cancel(data::DRAWER_CANCEL.into());
    app.set_drawer_apply(data::DRAWER_APPLY.into());
    app.set_pill(area(layout::PILL));
    app.set_search_box(area(layout::SEARCH));
    app.set_activity_box(area(layout::ACTIVITY));
    app.set_page_title(area(layout::PAGE_TITLE));
    app.set_table(area(layout::TABLE));
    app.set_recent_box(area(layout::recent()));
    app.set_drawer(area(layout::DRAWER));
    app.set_cli_box(area(layout::DRAWER_CLI));
    app.set_cancel_box(area(layout::DRAWER_CANCEL));
    app.set_apply_box(area(layout::DRAWER_APPLY));
    app.set_header_h(layout::HEADER_H);
    app.set_row_h(layout::ROW_H);
    app.set_name_clip(layout::NAME_CLIP);
    app.set_badge_x(layout::badge(0, 0.0).x - layout::VIEWPORT.x);
}

/// The state-dependent properties. `glass` leaves the window base transparent for Mica.
pub fn apply(app: &App, s: &State, glass: bool) {
    let p = s.theme.palette();
    let g = app.global::<Palette>();
    g.set_bg_app(color(p.bg_app));
    g.set_bg_surface(color(p.bg_surface));
    g.set_bg_subtle(color(p.bg_subtle));
    g.set_border(color(p.border));
    g.set_text_primary(color(p.text_primary));
    g.set_text_secondary(color(p.text_secondary));
    g.set_accent(color(p.accent));
    g.set_on_accent(color(p.on_accent));
    g.set_selected(color(p.selected));
    for (b, fill, ink) in [Badge::New, Badge::Changed, Badge::Orphan].map(|b| {
        let (f, i) = p.badge(b).unwrap_or((p.bg_surface, p.text_primary));
        (b, color(f), color(i))
    }) {
        match b {
            Badge::New => (g.set_new_fill(fill), g.set_new_ink(ink)),
            Badge::Changed => (g.set_changed_fill(fill), g.set_changed_ink(ink)),
            Badge::Orphan | Badge::None => (g.set_orphan_fill(fill), g.set_orphan_ink(ink)),
        };
    }
    let (f, i) = p.reversibility(data::Reversibility::Undoable);
    g.set_undo_fill(color(f));
    g.set_undo_ink(color(i));
    let (f, i) = p.reversibility(data::Reversibility::Permanent);
    g.set_permanent_fill(color(f));
    g.set_permanent_ink(color(i));
    g.set_window_base(if glass {
        slint::Color::from_argb_u8(0, 0, 0, 0)
    } else {
        color(p.bg_app)
    });

    app.set_screen(match s.screen {
        scene::Screen::Shell => 0,
        scene::Screen::List => 1,
        scene::Screen::Drawer => 2,
    });
    app.set_nav_selected(s.nav as i32);
    app.set_selected_row(s.selected.map_or(-1, |i| i as i32));
    set_scroll(app, s.scroll);
}

pub fn set_scroll(app: &App, scroll: f32) {
    app.set_list_viewport_y(-scroll);
}
