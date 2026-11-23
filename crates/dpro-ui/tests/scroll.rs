//! TD-0009 A2 slice 5a: scroll viewports, written before the code (A2.9).
//!
//! - A viewport ([`Element::scroll`]) clips its content and offsets it by whole physical pixels.
//! - The wheel scrolls the innermost viewport under the pointer that can still move that way,
//!   else the next one out. A notch is Windows' wheel lines ([`Ui::set_wheel`]) of the viewport's
//!   line height; `u32::MAX` lines is a page, and a finer wheel's smaller deltas scroll their share
//!   at once. The horizontal wheel scrolls its characters, each half the body size.
//! - Page Up, Page Down, Ctrl+Home and Ctrl+End scroll the viewport holding the focus. A viewport
//!   with no tab stop inside is one itself, and its arrows scroll it.
//! - Focus moving to a node out of view scrolls it wholly into view (the focus ring is drawn
//!   inside it). A client can scroll by line, page or offset, and bring a node into view.
//! - The bar: a thin indicator while the content overflows, an expanded bar while the pointer is
//!   on its strip, the full bar with "Automatically hide scroll bars" off. Dragging the thumb
//!   scrolls in proportion; a press on the track moves a page toward it.
//! - A scroll damages the viewport and nothing else, and asks for no frames or wakes after it.
//! - Menus and lists scroll when they're taller than their popup.
//!
//! Then what a review of the slice found, each fixed and kept fixed here:
//! - A menu fills a window shorter than its items, so it scrolls in a real popup; a focus moved
//!   before the layout that places it is scrolled into view after that layout.
//! - A node that stops scrolling is repainted; Page Up and Page Down reach the viewport from a
//!   focused field; a lost pointer ends a drag; a drag holds the hover; a reused slot's bar
//!   lights from the pointer; an offset stays in range across a DPI change; content wider than
//!   the view it scrolls across starts at its start.
//! - Paint: content stays inside a viewport's hairline; high contrast's bar is `COLOR_BTNFACE`
//!   and `COLOR_BTNTEXT`.
//! - Accessibility: `ScrollIntoView` and a lone viewport's `Focus` follow their changes at once;
//!   Enter and Space on a focused viewport go on unused.

#![expect(
    clippy::expect_used,
    reason = "test helpers: a node that isn't there fails the test"
)]

use dpro_ui::{
    Align, Axes, Canvas, Color, Dim, Element, FakeText, Input, Insets, Key, KeyCode, KeyEvent,
    Look, Mods, Motion, NodeId, Point, Radius, Rect, RectPx, ScrollInfo, Size, Style, SysColors,
    TextEdit, Theme, Tone, Ui,
    accesskit::{self, Action, ActionData, ActionRequest, NodeId as AkId, ScrollUnit, TreeId},
    tokens,
};

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Row(u64),
}

const WINDOW: Size = Size { w: 400.0, h: 300.0 };
const ROW: f32 = 32.0;
const ROWS: u64 = 30;

fn rows_style() -> Style {
    Style {
        height: Dim::Fixed(200.0),
        ..Style::default()
    }
}

fn row_style() -> Style {
    Style {
        height: Dim::Fixed(ROW),
        ..Style::default()
    }
}

/// A 200 px viewport holding `ROWS` buttons of `ROW` px: 960 px of content.
fn list(ui: &mut Ui<Msg>) {
    ui.build(|cx| {
        cx.add_with(
            Element::scroll(Key::name("rows"), Axes::Y).style(rows_style()),
            |cx| {
                for r in 0..ROWS {
                    cx.add(Element::button(Key::index(r), "row", Msg::Row(r)).style(row_style()));
                }
            },
        );
    });
    ui.layout(&mut FakeText);
}

fn new_ui(scale: f32) -> Ui<Msg> {
    let mut ui = Ui::new(WINDOW, scale, Motion::default());
    ui.set_active(true);
    ui.set_wheel(3, 3);
    ui
}

fn viewport(ui: &Ui<Msg>) -> NodeId {
    ui.find(&[Key::name("rows")]).expect("the viewport")
}

fn offset_y(ui: &Ui<Msg>) -> f32 {
    scroll(ui).offset.y
}

fn scroll(ui: &Ui<Msg>) -> ScrollInfo {
    ui.info(viewport(ui))
        .and_then(|n| n.scroll)
        .expect("a viewport's scroll state")
}

fn row_rect(ui: &Ui<Msg>, r: u64) -> Rect {
    ui.find(&[Key::name("rows"), Key::index(r)])
        .and_then(|id| ui.info(id))
        .expect("a row")
        .rect
}

fn wheel(ui: &mut Ui<Msg>, at: Point, delta: i32) -> bool {
    let mut out = Vec::new();
    let used = ui.dispatch(
        Input::Wheel {
            at,
            delta,
            horizontal: false,
        },
        &mut out,
    );
    ui.layout(&mut FakeText);
    assert_eq!(out, []);
    used
}

fn key(ui: &mut Ui<Msg>, key: KeyCode, mods: Mods) -> bool {
    let mut out = Vec::new();
    let used = ui.dispatch(
        Input::Key(KeyEvent {
            key,
            down: true,
            mods,
        }),
        &mut out,
    );
    ui.layout(&mut FakeText);
    used
}

const CTRL: Mods = Mods {
    shift: false,
    ctrl: true,
    alt: false,
    win: false,
};

const INSIDE: Point = Point { x: 100.0, y: 100.0 };

#[test]
fn a_notch_scrolls_the_wheel_lines_and_the_content_moves_with_it() {
    let mut ui = new_ui(1.0);
    list(&mut ui);
    let line = ui.info(viewport(&ui)).unwrap().scroll.unwrap().line.h;
    assert!(line > 0.0);
    let before = row_rect(&ui, 0);
    // Toward the user: down the content.
    assert!(wheel(&mut ui, INSIDE, -120));
    assert_eq!(offset_y(&ui), 3.0 * line);
    assert_eq!(row_rect(&ui, 0).y, before.y - 3.0 * line);
    // Away: back up, and never above the top.
    assert!(wheel(&mut ui, INSIDE, 120));
    assert_eq!(offset_y(&ui), 0.0);
    assert!(
        !wheel(&mut ui, INSIDE, 120),
        "nothing above the top to scroll"
    );
    assert_eq!(offset_y(&ui), 0.0);
}

#[test]
fn finer_deltas_scroll_their_share_and_a_page_setting_scrolls_pages() {
    let mut ui = new_ui(1.0);
    list(&mut ui);
    let line = ui.info(viewport(&ui)).unwrap().scroll.unwrap().line.h;
    for _ in 0..4 {
        wheel(&mut ui, INSIDE, -30);
    }
    assert_eq!(
        offset_y(&ui),
        3.0 * line,
        "four quarter notches are one notch"
    );
    ui.set_wheel(u32::MAX, 3);
    wheel(&mut ui, INSIDE, 120);
    wheel(&mut ui, INSIDE, -120);
    // A page is the viewport's height less a line, as a page key moves.
    assert_eq!(offset_y(&ui), 200.0 - line);
}

#[test]
fn the_offset_stops_at_the_end_and_whole_pixels_hold_at_any_scale() {
    for scale in [1.0, 1.25, 1.5] {
        let mut ui = new_ui(scale);
        list(&mut ui);
        for _ in 0..100 {
            wheel(&mut ui, INSIDE, -120);
        }
        let s = ui.info(viewport(&ui)).unwrap().scroll.unwrap();
        assert_eq!(s.offset.y, s.content.h - 200.0, "at {scale}");
        assert_eq!(
            f64::from(i32::try_from(s.offset_px[1]).unwrap()),
            f64::from((s.offset.y * scale).round()),
            "at {scale}"
        );
        // The last row ends at the viewport's bottom, on a whole pixel.
        let last = row_rect(&ui, ROWS - 1);
        let bottom = ui.info(viewport(&ui)).unwrap().rect.bottom();
        assert!(((last.bottom() - bottom) * scale).abs() < 0.5, "at {scale}");
    }
}

#[test]
fn the_wheel_goes_to_the_innermost_viewport_that_can_move_then_outward() {
    let mut ui = new_ui(1.0);
    ui.build(|cx| {
        cx.add_with(
            Element::scroll(Key::name("page"), Axes::Y).style(Style {
                height: Dim::Fixed(300.0),
                ..Style::default()
            }),
            |cx| {
                cx.add_with(
                    Element::scroll(Key::name("rows"), Axes::Y).style(rows_style()),
                    |cx| {
                        for r in 0..ROWS {
                            cx.add(
                                Element::button(Key::index(r), "row", Msg::Row(r))
                                    .style(row_style()),
                            );
                        }
                    },
                );
                cx.add(Element::stack(Key::name("tail")).style(Style {
                    height: Dim::Fixed(400.0),
                    ..Style::default()
                }));
            },
        );
    });
    ui.layout(&mut FakeText);
    let page = ui.find(&[Key::name("page")]).unwrap();
    let rows = ui.find(&[Key::name("page"), Key::name("rows")]).unwrap();
    let off = |ui: &Ui<Msg>, id| ui.info(id).unwrap().scroll.unwrap().offset.y;
    // Up at the top: neither can move.
    assert!(!wheel(&mut ui, INSIDE, 120));
    // Down: the inner list moves until its end, then the page does.
    for _ in 0..100 {
        wheel(&mut ui, INSIDE, -120);
        if off(&ui, page) > 0.0 {
            break;
        }
    }
    let s = ui.info(rows).unwrap().scroll.unwrap();
    assert_eq!(
        s.offset.y,
        s.content.h - 200.0,
        "the inner list reached its end first"
    );
    assert!(off(&ui, page) > 0.0);
}

#[test]
fn the_horizontal_wheel_scrolls_characters() {
    let mut ui = new_ui(1.0);
    ui.build(|cx| {
        cx.add_with(
            Element::scroll(Key::name("wide"), Axes::X).style(Style {
                width: Dim::Fixed(200.0),
                height: Dim::Fixed(100.0),
                axis: dpro_ui::Axis::Row,
                ..Style::default()
            }),
            |cx| {
                cx.add(Element::stack(Key::name("long")).style(Style {
                    width: Dim::Fixed(1000.0),
                    ..Style::default()
                }));
            },
        );
    });
    ui.layout(&mut FakeText);
    let id = ui.find(&[Key::name("wide")]).unwrap();
    // A character is half the body size (13 px), in whole pixels.
    let char_w = ui.info(id).unwrap().scroll.unwrap().line.w;
    assert_eq!(char_w, (tokens::font::BODY / 2.0).round());
    let mut out = Vec::new();
    // Positive is to the right.
    ui.dispatch(
        Input::Wheel {
            at: Point { x: 50.0, y: 50.0 },
            delta: 120,
            horizontal: true,
        },
        &mut out,
    );
    ui.layout(&mut FakeText);
    assert_eq!(ui.info(id).unwrap().scroll.unwrap().offset.x, 3.0 * char_w);
    // The vertical wheel finds no vertical scrolling here, and isn't used.
    assert!(!ui.dispatch(
        Input::Wheel {
            at: Point { x: 50.0, y: 50.0 },
            delta: -120,
            horizontal: false,
        },
        &mut out,
    ));
}

#[test]
fn page_keys_and_ctrl_home_end_scroll_the_viewport_holding_the_focus() {
    let mut ui = new_ui(1.0);
    list(&mut ui);
    // Focus row 0 (Tab), so the keys have a viewport.
    key(&mut ui, KeyCode::Tab, Mods::default());
    let line = ui.info(viewport(&ui)).unwrap().scroll.unwrap().line.h;
    assert!(key(&mut ui, KeyCode::PageDown, Mods::default()));
    assert_eq!(offset_y(&ui), 200.0 - line);
    assert!(key(&mut ui, KeyCode::End, CTRL));
    let s = ui.info(viewport(&ui)).unwrap().scroll.unwrap();
    assert_eq!(s.offset.y, s.content.h - 200.0);
    assert!(key(&mut ui, KeyCode::PageUp, Mods::default()));
    assert!(key(&mut ui, KeyCode::Home, CTRL));
    assert_eq!(offset_y(&ui), 0.0);
}

#[test]
fn focus_moving_out_of_view_scrolls_it_wholly_into_view() {
    let mut ui = new_ui(1.0);
    list(&mut ui);
    let view = ui.info(viewport(&ui)).unwrap().rect;
    // Tab down through the rows: each one focused is wholly in view (its ring is inside it).
    for r in 0..12 {
        key(&mut ui, KeyCode::Tab, Mods::default());
        let rect = row_rect(&ui, r);
        assert!(rect.y >= view.y - 0.01, "row {r} at {rect:?}");
        assert!(rect.bottom() <= view.bottom() + 0.01, "row {r} at {rect:?}");
    }
    assert!(offset_y(&ui) > 0.0);
    // Back up with Shift+Tab: the top row comes back whole.
    for _ in 0..11 {
        key(
            &mut ui,
            KeyCode::Tab,
            Mods {
                shift: true,
                ..Mods::default()
            },
        );
    }
    assert!(row_rect(&ui, 0).y >= view.y - 0.01);
}

#[test]
fn a_viewport_without_tab_stops_is_one_and_its_arrows_scroll_it() {
    let mut ui = new_ui(1.0);
    ui.build(|cx| {
        cx.add_with(
            Element::scroll(Key::name("rows"), Axes::Y)
                .style(rows_style())
                .name("Log"),
            |cx| {
                for r in 0..ROWS {
                    cx.add(Element::text(Key::index(r), "line").style(row_style()));
                }
            },
        );
    });
    ui.layout(&mut FakeText);
    key(&mut ui, KeyCode::Tab, Mods::default());
    assert_eq!(ui.focus(), Some(viewport(&ui)));
    let line = ui.info(viewport(&ui)).unwrap().scroll.unwrap().line.h;
    assert!(key(&mut ui, KeyCode::Down, Mods::default()));
    assert_eq!(offset_y(&ui), line);
    assert!(key(&mut ui, KeyCode::Up, Mods::default()));
    assert_eq!(offset_y(&ui), 0.0);
    // Its accessibility node can be focused and says where it is.
    let tree = ui.a11y_tree();
    let (_, node) = tree
        .nodes
        .iter()
        .find(|(_, n)| n.label() == Some("Log"))
        .unwrap();
    assert!(node.supports_action(Action::Focus));
    assert_eq!(node.scroll_y(), Some(0.0));
    assert_eq!(node.scroll_y_max(), Some(f64::from(30.0 * ROW - 200.0)));
}

/// A client's request for `node` in the root tree.
fn request(node: AkId, action: Action, data: Option<ActionData>) -> ActionRequest {
    ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: node,
        data,
    }
}

#[test]
fn a_client_scrolls_by_line_page_and_offset_and_brings_an_item_into_view() {
    let mut ui = new_ui(1.0);
    list(&mut ui);
    let tree = ui.a11y_tree();
    let ak = |ui: &Ui<Msg>, id: NodeId| {
        tree.nodes
            .iter()
            .map(|(k, _)| *k)
            .find(|k| ui.a11y_node(*k) == Some(id))
            .unwrap()
    };
    let rows = ak(&ui, viewport(&ui));
    let (_, node) = tree.nodes.iter().find(|(k, _)| *k == rows).unwrap();
    for a in [
        Action::ScrollDown,
        Action::ScrollUp,
        Action::SetScrollOffset,
    ] {
        assert!(node.supports_action(a), "{a:?}");
    }
    assert!(
        !node.supports_action(Action::ScrollLeft),
        "it scrolls on Y only"
    );
    let line = ui.info(viewport(&ui)).unwrap().scroll.unwrap().line.h;
    let mut out = Vec::new();
    let mut act = |ui: &mut Ui<Msg>, r: ActionRequest| {
        let applied = ui.a11y_action(&r, &mut out);
        ui.layout(&mut FakeText);
        applied
    };
    let item = Some(ActionData::ScrollUnit(ScrollUnit::Item));
    let page = Some(ActionData::ScrollUnit(ScrollUnit::Page));
    assert!(act(
        &mut ui,
        request(rows, Action::ScrollDown, item.clone())
    ));
    assert_eq!(offset_y(&ui), line);
    assert!(act(&mut ui, request(rows, Action::ScrollDown, page)));
    assert_eq!(offset_y(&ui), line + 200.0 - line);
    // No unit is a page (the view less a line), as UIA's ScrollPattern's large increment.
    assert!(act(&mut ui, request(rows, Action::ScrollUp, None)));
    assert_eq!(offset_y(&ui), line);
    assert!(act(&mut ui, request(rows, Action::ScrollUp, item.clone())));
    assert_eq!(offset_y(&ui), 0.0);
    assert!(
        !act(&mut ui, request(rows, Action::ScrollUp, item)),
        "already at the top"
    );
    let to = dpro_ui::accesskit::Point { x: 0.0, y: 100.0 };
    assert!(act(
        &mut ui,
        request(
            rows,
            Action::SetScrollOffset,
            Some(ActionData::SetScrollOffset(to))
        )
    ));
    assert_eq!(offset_y(&ui), 100.0);
    // ScrollIntoView on row 25 brings it wholly into view, and moves no focus.
    let row = ui.find(&[Key::name("rows"), Key::index(25)]).unwrap();
    let focus = ui.focus();
    let target = ak(&ui, row);
    assert!(act(&mut ui, request(target, Action::ScrollIntoView, None)));
    let view = ui.info(viewport(&ui)).unwrap().rect;
    let rect = row_rect(&ui, 25);
    assert!(
        rect.y >= view.y - 0.01 && rect.bottom() <= view.bottom() + 0.01,
        "{rect:?}"
    );
    assert_eq!(ui.focus(), focus);
    assert_eq!(out, []);
    // The update says where it went.
    let update = ui.a11y_update().unwrap();
    let (_, node) = update.nodes.iter().find(|(k, _)| *k == rows).unwrap();
    assert_eq!(node.scroll_y(), Some(f64::from(offset_y(&ui))));
}

#[test]
fn the_bar_is_an_indicator_until_the_pointer_is_on_its_strip() {
    let mut ui = new_ui(1.0);
    list(&mut ui);
    let bar = ui
        .info(viewport(&ui))
        .unwrap()
        .scroll
        .unwrap()
        .bar_y
        .unwrap();
    assert!(!bar.expanded);
    assert!(bar.thumb.w <= tokens::scroll::INDICATOR + 0.01);
    let view = ui.info(viewport(&ui)).unwrap().rect;
    let on_strip = Point {
        x: view.right() - tokens::scroll::STRIP / 2.0,
        y: view.y + 50.0,
    };
    let mut out = Vec::new();
    ui.dispatch(Input::PointerMove(on_strip), &mut out);
    ui.layout(&mut FakeText);
    let bar = ui
        .info(viewport(&ui))
        .unwrap()
        .scroll
        .unwrap()
        .bar_y
        .unwrap();
    assert!(bar.expanded);
    assert!((bar.thumb.w - tokens::scroll::BAR).abs() < 0.01);
    // Off the strip, it's an indicator again.
    ui.dispatch(Input::PointerMove(INSIDE), &mut out);
    ui.layout(&mut FakeText);
    assert!(
        !ui.info(viewport(&ui))
            .unwrap()
            .scroll
            .unwrap()
            .bar_y
            .unwrap()
            .expanded
    );
    // With "Automatically hide scroll bars" off, it's the full bar wherever the pointer is.
    ui.set_auto_hide_scroll_bars(false);
    ui.layout(&mut FakeText);
    assert!(
        ui.info(viewport(&ui))
            .unwrap()
            .scroll
            .unwrap()
            .bar_y
            .unwrap()
            .expanded
    );
    // Content that fits has no bar.
    ui.build(|cx| {
        cx.add_with(
            Element::scroll(Key::name("rows"), Axes::Y).style(rows_style()),
            |cx| {
                cx.add(Element::button(Key::index(0), "row", Msg::Row(0)).style(row_style()));
            },
        );
    });
    ui.layout(&mut FakeText);
    assert_eq!(ui.info(viewport(&ui)).unwrap().scroll.unwrap().bar_y, None);
}

#[test]
fn dragging_the_thumb_scrolls_in_proportion_and_the_track_pages() {
    let mut ui = new_ui(1.0);
    list(&mut ui);
    ui.set_auto_hide_scroll_bars(false);
    ui.layout(&mut FakeText);
    let s = ui.info(viewport(&ui)).unwrap().scroll.unwrap();
    let bar = s.bar_y.unwrap();
    let grab = bar.thumb.center();
    let mut out = Vec::new();
    ui.dispatch(Input::PointerDown(grab), &mut out);
    ui.dispatch(
        Input::PointerMove(Point {
            y: grab.y + 40.0,
            ..grab
        }),
        &mut out,
    );
    ui.layout(&mut FakeText);
    let scrolled = offset_y(&ui);
    let expect = 40.0 * (s.content.h - 200.0) / (bar.track.h - bar.thumb.h);
    assert!((scrolled - expect).abs() <= 1.0, "{scrolled} vs {expect}");
    ui.dispatch(
        Input::PointerUp(Point {
            y: grab.y + 40.0,
            ..grab
        }),
        &mut out,
    );
    assert_eq!(out, [], "the drag invoked nothing under it");
    // A press on the track below the thumb moves a page down.
    let s = ui.info(viewport(&ui)).unwrap().scroll.unwrap();
    let bar = s.bar_y.unwrap();
    let below = Point {
        x: bar.track.center().x,
        y: bar.track.bottom() - 2.0,
    };
    let before = s.offset.y;
    ui.dispatch(Input::PointerDown(below), &mut out);
    ui.dispatch(Input::PointerUp(below), &mut out);
    ui.layout(&mut FakeText);
    assert_eq!(
        offset_y(&ui),
        (before + 200.0 - s.line.h).min(s.content.h - 200.0)
    );
    assert_eq!(out, []);
}

#[test]
fn a_scroll_damages_the_viewport_only_and_asks_for_nothing_after() {
    let mut ui = new_ui(1.0);
    list(&mut ui);
    ui.clear_damage();
    wheel(&mut ui, INSIDE, -120);
    let view = ui.info(viewport(&ui)).unwrap().rect;
    let view_px = view.to_px(1.0);
    for r in ui.damage() {
        assert!(view_px.contains(*r), "{r:?} outside the viewport {view:?}");
    }
    assert_ne!(ui.damage(), []);
    assert!(!ui.wants_frame());
    assert_eq!(ui.tooltip_timer(0), None);
}

#[test]
fn content_that_shrinks_pulls_the_offset_back() {
    let mut ui = new_ui(1.0);
    list(&mut ui);
    for _ in 0..100 {
        wheel(&mut ui, INSIDE, -120);
    }
    ui.build(|cx| {
        cx.add_with(
            Element::scroll(Key::name("rows"), Axes::Y).style(rows_style()),
            |cx| {
                for r in 0..8 {
                    cx.add(Element::button(Key::index(r), "row", Msg::Row(r)).style(row_style()));
                }
            },
        );
    });
    ui.layout(&mut FakeText);
    assert_eq!(offset_y(&ui), 8.0 * ROW - 200.0);
}

#[test]
fn a_menu_taller_than_its_popup_scrolls_and_keeps_the_focused_item_in_view() {
    let mut ui = new_ui(1.0);
    ui.set_viewport(Size { w: 200.0, h: 300.0 });
    ui.build(|cx| {
        cx.add_with(
            Element::options(Key::name("list")).style(Style {
                height: Dim::Fixed(300.0),
                padding: Insets::all(tokens::popup::PADDING),
                ..Style::default()
            }),
            |cx| {
                for r in 0..40 {
                    cx.add(Element::option(
                        Key::index(r),
                        "option",
                        r == 0,
                        Msg::Row(r),
                    ));
                }
            },
        );
    });
    ui.layout(&mut FakeText);
    let options = ui.find(&[Key::name("list")]).unwrap();
    let s = ui.info(options).unwrap().scroll.unwrap();
    assert!(s.content.h > 300.0);
    let view = ui.info(options).unwrap().rect;
    // Nothing is focused yet, so the first key focuses the first option (A1.4); End then goes to
    // the last, out of view.
    key(&mut ui, KeyCode::Down, Mods::default());
    key(&mut ui, KeyCode::End, Mods::default());
    let last = ui.find(&[Key::name("list"), Key::index(39)]).unwrap();
    assert_eq!(ui.focus(), Some(last));
    let rect = ui.info(last).unwrap().rect;
    assert!(
        rect.bottom() <= view.bottom() + 0.01,
        "{rect:?} in {view:?}"
    );
    // The wheel scrolls it too.
    assert!(wheel(&mut ui, Point { x: 50.0, y: 100.0 }, 120));
}

// ---- the review of slice 5a ---------------------------------------------------------------------

fn offset_of(ui: &Ui<Msg>, id: NodeId) -> Point {
    ui.info(id)
        .and_then(|n| n.scroll)
        .expect("a viewport's scroll state")
        .offset
}

/// Whether `damage` covers every pixel of `r`.
fn covers(damage: &[RectPx], r: RectPx) -> bool {
    (r.y0..r.y1).all(|y| {
        (r.x0..r.x1).all(|x| {
            damage
                .iter()
                .any(|d| d.x0 <= x && x < d.x1 && d.y0 <= y && y < d.y1)
        })
    })
}

/// The whole window painted from scratch, and its width.
fn painted(ui: &Ui<Msg>, theme: &Theme) -> (Vec<u32>, usize) {
    let (w, h) = ui.pixel_size();
    let len = usize::try_from(u64::from(w) * u64::from(h)).expect("the window's pixels");
    let mut px = vec![0; len];
    {
        let mut canvas = Canvas::new(&mut px, w, h).expect("a canvas");
        let all = canvas.bounds();
        ui.paint_region(&mut canvas, &mut FakeText, theme, all);
    }
    (px, usize::try_from(w).expect("a width"))
}

/// The pixel `color` is on a canvas.
fn pixel(color: Color) -> u32 {
    let mut px = [0; 1];
    {
        let mut canvas = Canvas::new(&mut px, 1, 1).expect("a canvas");
        let all = canvas.bounds();
        canvas.fill(all, color);
    }
    px[0]
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a point inside the window, at scale 1"
)]
fn index(p: Point, width: usize) -> usize {
    p.y.floor() as usize * width + p.x.floor() as usize
}

/// The platform sizes a popup's window to its content, at most `popup::MAX_HEIGHT` of the work
/// area, and lays the menu out in it: the menu grows to the window, so one taller scrolls.
#[test]
fn a_menu_in_a_window_shorter_than_its_items_fills_it_and_scrolls() {
    let mut ui = new_ui(1.0);
    ui.build(|cx| {
        cx.add_with(Element::menu(Key::name("menu")), |cx| {
            for r in 0..40 {
                cx.add(Element::menu_item(Key::index(r), "item", Msg::Row(r)));
            }
        });
    });
    let natural = ui.natural_size(&mut FakeText, 200.0);
    assert!(natural.h > 300.0, "{natural:?}");
    // As `fit` does: the window is the items' width, its height capped.
    ui.set_viewport(Size {
        w: natural.w,
        h: 300.0,
    });
    ui.layout(&mut FakeText);
    let menu = ui.find(&[Key::name("menu")]).expect("the menu");
    let info = ui.info(menu).expect("the menu");
    assert!((info.rect.h - 300.0).abs() < 0.01, "{:?}", info.rect);
    let s = info.scroll.expect("a viewport");
    assert!(s.content.h > 300.0);
    assert!(s.bar_y.is_some(), "its bar shows that it scrolls");
    assert!(wheel(&mut ui, Point { x: 50.0, y: 100.0 }, -120));
    assert!(offset_of(&ui, menu).y > 0.0);
    // The keyboard reaches the last item, in view.
    key(&mut ui, KeyCode::Down, Mods::default());
    key(&mut ui, KeyCode::End, Mods::default());
    let last = ui
        .find(&[Key::name("menu"), Key::index(39)])
        .expect("the last item");
    assert_eq!(ui.focus(), Some(last));
    let rect = ui.info(last).expect("the last item").rect;
    assert!(rect.bottom() <= 300.0 + 0.01, "{rect:?}");
}

/// A popup focuses its first item, or a list its picked option, right after it's built: before
/// the layout that places them. The focus is scrolled into view once it's placed.
#[test]
fn a_list_focused_before_its_first_layout_shows_the_picked_option() {
    let mut ui = new_ui(1.0);
    ui.set_viewport(Size { w: 200.0, h: 300.0 });
    ui.build(|cx| {
        cx.add_with(Element::options(Key::name("list")), |cx| {
            for r in 0..40 {
                cx.add(Element::option(
                    Key::index(r),
                    "option",
                    r == 30,
                    Msg::Row(r),
                ));
            }
        });
    });
    ui.focus_first(true);
    let picked = ui
        .find(&[Key::name("list"), Key::index(30)])
        .expect("the picked option");
    assert_eq!(ui.focus(), Some(picked));
    ui.layout(&mut FakeText);
    let rect = ui.info(picked).expect("the picked option").rect;
    assert!(rect.y >= -0.01 && rect.bottom() <= 300.0 + 0.01, "{rect:?}");
}

/// A node that stops being a viewport, or stops scrolling along an axis, drops its offset: its
/// content shows somewhere else although no rectangle changed.
#[test]
fn content_that_stops_scrolling_is_repainted_where_it_shows_now() {
    let rows = |ui: &mut Ui<Msg>, el: Element<'static, Msg>| {
        ui.build(|cx| {
            cx.add_with(el.style(rows_style()), |cx| {
                for r in 0..ROWS {
                    cx.add(Element::button(Key::index(r), "row", Msg::Row(r)).style(row_style()));
                }
            });
        });
        ui.layout(&mut FakeText);
    };
    for then in [
        Element::stack(Key::name("rows")),
        Element::scroll(Key::name("rows"), Axes::X),
    ] {
        let mut ui = new_ui(1.0);
        list(&mut ui);
        for _ in 0..3 {
            wheel(&mut ui, INSIDE, -120);
        }
        assert!(offset_y(&ui) > 0.0);
        ui.clear_damage();
        rows(&mut ui, then);
        let view = ui.info(viewport(&ui)).expect("the rows").rect.to_px(1.0);
        assert!(
            covers(ui.damage(), view),
            "{:?} doesn't cover {view:?}",
            ui.damage()
        );
        // The pointer finds what shows: row 0 at the top.
        let mut out = Vec::new();
        let top = Point { x: 100.0, y: 10.0 };
        ui.dispatch(Input::PointerDown(top), &mut out);
        ui.dispatch(Input::PointerUp(top), &mut out);
        assert_eq!(out, [Msg::Row(0)]);
    }
}

fn edit(_: TextEdit) -> Msg {
    Msg::Row(u64::MAX)
}

/// A field has no use for Page Up and Page Down: they scroll the viewport holding it.
#[test]
fn page_keys_scroll_the_viewport_from_a_focused_field() {
    let mut ui = new_ui(1.0);
    ui.build(|cx| {
        cx.add_with(
            Element::scroll(Key::name("rows"), Axes::Y).style(rows_style()),
            |cx| {
                cx.add(Element::field(Key::name("filter"), "", edit).style(row_style()));
                for r in 0..ROWS {
                    cx.add(Element::button(Key::index(r), "row", Msg::Row(r)).style(row_style()));
                }
            },
        );
    });
    ui.layout(&mut FakeText);
    key(&mut ui, KeyCode::Tab, Mods::default());
    assert_eq!(
        ui.focus(),
        ui.find(&[Key::name("rows"), Key::name("filter")])
    );
    let line = scroll(&ui).line.h;
    assert!(key(&mut ui, KeyCode::PageDown, Mods::default()));
    assert_eq!(offset_y(&ui), 200.0 - line);
    assert!(key(&mut ui, KeyCode::PageUp, Mods::default()));
    assert_eq!(offset_y(&ui), 0.0);
    // The keys it uses stay its own: Ctrl+End moves its caret and scrolls nothing.
    assert!(key(&mut ui, KeyCode::End, CTRL));
    assert_eq!(offset_y(&ui), 0.0);
}

/// Windows sends no leave while a press holds the capture: a leave mid-drag means another window
/// took the pointer. The drag ends, so the bare pointer moving scrolls nothing, and the next
/// click is a click. `cancel_press` (the window going inactive or hiding) ends one too.
#[test]
fn a_drag_the_window_lost_ends_and_the_next_click_works() {
    fn leave(ui: &mut Ui<Msg>) {
        ui.dispatch(Input::PointerLeave, &mut Vec::new());
    }
    fn cancel(ui: &mut Ui<Msg>) {
        ui.cancel_press();
    }
    let mut ui = new_ui(1.0);
    list(&mut ui);
    ui.set_auto_hide_scroll_bars(false);
    ui.layout(&mut FakeText);
    let mut out = Vec::new();
    for lose in [leave as fn(&mut Ui<Msg>), cancel] {
        let grab = scroll(&ui).bar_y.expect("a bar").thumb.center();
        ui.dispatch(Input::PointerDown(grab), &mut out);
        ui.dispatch(
            Input::PointerMove(Point {
                y: grab.y + 20.0,
                ..grab
            }),
            &mut out,
        );
        ui.layout(&mut FakeText);
        let held = offset_y(&ui);
        lose(&mut ui);
        ui.dispatch(
            Input::PointerMove(Point {
                y: grab.y + 80.0,
                ..grab
            }),
            &mut out,
        );
        ui.layout(&mut FakeText);
        assert_eq!(offset_y(&ui), held, "the bare pointer drags nothing");
        // A click on a row in view invokes it, and leaves it unpressed.
        let row = (0..ROWS)
            .find(|&r| {
                let rect = row_rect(&ui, r);
                rect.y >= 0.0 && rect.bottom() <= 200.0
            })
            .expect("a row in view");
        let at = row_rect(&ui, row).center();
        ui.dispatch(Input::PointerMove(at), &mut out);
        ui.dispatch(Input::PointerDown(at), &mut out);
        ui.dispatch(Input::PointerUp(at), &mut out);
        assert_eq!(out, [Msg::Row(row)]);
        out.clear();
        let id = ui
            .find(&[Key::name("rows"), Key::index(row)])
            .expect("the row");
        assert!(!ui.info(id).expect("the row").pressed);
    }
}

/// While the thumb is dragged, the content moving past the pointer lights no row.
#[test]
fn a_drag_holds_the_hover_where_it_was() {
    let mut ui = new_ui(1.0);
    list(&mut ui);
    ui.set_auto_hide_scroll_bars(false);
    ui.layout(&mut FakeText);
    let grab = scroll(&ui).bar_y.expect("a bar").thumb.center();
    let mut out = Vec::new();
    ui.dispatch(Input::PointerMove(grab), &mut out);
    ui.dispatch(Input::PointerDown(grab), &mut out);
    // Off the strip, over the rows, as a hand drifts.
    let drifted = Point {
        x: INSIDE.x,
        y: grab.y + 60.0,
    };
    ui.dispatch(Input::PointerMove(drifted), &mut out);
    ui.layout(&mut FakeText);
    assert!(offset_y(&ui) > 0.0);
    let hovered = |ui: &Ui<Msg>| {
        (0..ROWS).find(|&r| {
            ui.find(&[Key::name("rows"), Key::index(r)])
                .and_then(|id| ui.info(id))
                .is_some_and(|n| n.hovered)
        })
    };
    assert_eq!(hovered(&ui), None);
    // Let go: the row under the pointer is hovered.
    ui.dispatch(Input::PointerUp(drifted), &mut out);
    ui.layout(&mut FakeText);
    assert!(hovered(&ui).is_some());
    assert_eq!(out, []);
}

/// The bar the pointer was on goes with its viewport; the next one given its slot starts at rest
/// and lights from the pointer.
#[test]
fn a_viewport_built_again_lights_its_bar_from_the_pointer() {
    let mut ui = new_ui(1.0);
    list(&mut ui);
    let view = ui.info(viewport(&ui)).expect("the rows").rect;
    let strip = |y: f32| Point {
        x: view.right() - tokens::scroll::STRIP / 2.0,
        y,
    };
    let mut out = Vec::new();
    ui.dispatch(Input::PointerMove(strip(50.0)), &mut out);
    ui.layout(&mut FakeText);
    assert!(scroll(&ui).bar_y.expect("a bar").expanded);
    // Another page, and back.
    ui.build(|_| {});
    ui.layout(&mut FakeText);
    list(&mut ui);
    assert!(!scroll(&ui).bar_y.expect("a bar").expanded);
    ui.dispatch(Input::PointerMove(strip(60.0)), &mut out);
    ui.layout(&mut FakeText);
    assert!(scroll(&ui).bar_y.expect("a bar").expanded);
}

/// A DPI change rounds the view and the content to other pixels: an offset at the end stays at
/// most the new end, though no logical size changed and layout clamps nothing.
#[test]
fn an_offset_at_the_end_stays_in_range_across_a_dpi_change() {
    for row in [32.015, 31.99, 33.333] {
        for (from, to) in [(1.25, 1.0), (1.5, 1.25), (1.75, 1.0), (1.0, 1.5)] {
            let mut ui = new_ui(from);
            ui.build(|cx| {
                cx.add_with(
                    Element::scroll(Key::name("rows"), Axes::Y).style(rows_style()),
                    |cx| {
                        for r in 0..ROWS {
                            cx.add(Element::button(Key::index(r), "row", Msg::Row(r)).style(
                                Style {
                                    height: Dim::Fixed(row),
                                    ..Style::default()
                                },
                            ));
                        }
                    },
                );
            });
            ui.layout(&mut FakeText);
            wheel(&mut ui, INSIDE, -120 * 100);
            ui.set_scale(to);
            ui.layout(&mut FakeText);
            // A client reads the offset and the end, both in the toolkit's pixels.
            let tree = ui.a11y_tree();
            let node = ak_of(&tree.nodes, &ui, viewport(&ui)).expect("the rows");
            let (at, end) = (
                node.scroll_y().expect("an offset"),
                node.scroll_y_max().expect("an end"),
            );
            assert!(at <= end, "rows {row}, {from} to {to}: {at} past {end}");
            // And it kept its place: at the end, within the rounding.
            assert!(end - at <= 1.0, "rows {row}, {from} to {to}: {at} of {end}");
        }
    }
}

/// Across an axis its viewport scrolls, a child wider than the view starts at the view's start,
/// centered or at the end: no offset could reach content before it.
#[test]
fn content_wider_than_a_view_it_scrolls_across_starts_at_its_start() {
    for align in [Align::Center, Align::End] {
        let mut ui = new_ui(1.0);
        ui.build(|cx| {
            cx.add_with(
                Element::scroll(Key::name("wide"), Axes::X).style(Style {
                    height: Dim::Fixed(100.0),
                    align,
                    ..Style::default()
                }),
                |cx| {
                    cx.add(Element::stack(Key::name("child")).style(Style {
                        width: Dim::Fixed(2.0 * WINDOW.w),
                        height: Dim::Fixed(50.0),
                        ..Style::default()
                    }));
                },
            );
        });
        ui.layout(&mut FakeText);
        let view = ui
            .find(&[Key::name("wide")])
            .and_then(|id| ui.info(id))
            .expect("the viewport")
            .rect;
        let child = |ui: &Ui<Msg>| {
            ui.find(&[Key::name("wide"), Key::name("child")])
                .and_then(|id| ui.info(id))
                .expect("the child")
                .rect
        };
        assert!(
            (child(&ui).x - view.x).abs() < 0.01,
            "{align:?}: {:?}",
            child(&ui)
        );
        // Scrolled to the end, its end shows at the view's.
        let mut out = Vec::new();
        ui.dispatch(
            Input::Wheel {
                at: Point { x: 100.0, y: 25.0 },
                delta: 120 * 100,
                horizontal: true,
            },
            &mut out,
        );
        ui.layout(&mut FakeText);
        assert!(
            (child(&ui).right() - view.right()).abs() < 0.01,
            "{align:?}: {:?}",
            child(&ui)
        );
    }
}

/// Content scrolls under a viewport's hairline, not over it: the hairline's pixels are the ones
/// an empty viewport has, at every offset.
#[test]
fn scrolled_content_stays_inside_its_viewport_s_hairline() {
    let theme = Theme::LIGHT;
    let framed = Look {
        fill: Some(Tone::Surface),
        border: Some(Tone::Border),
        radius: Radius::None,
        ink: Tone::Text,
    };
    let build = |ui: &mut Ui<Msg>, rows: u64| {
        ui.build(|cx| {
            cx.add_with(
                Element::scroll(Key::name("rows"), Axes::Y)
                    .style(rows_style())
                    .look(framed),
                |cx| {
                    // Filled edge to edge, so any that strays onto the hairline shows.
                    for r in 0..rows {
                        cx.add(Element::stack(Key::index(r)).style(row_style()).look(Look {
                            fill: Some(Tone::Accent),
                            ..Look::PLAIN
                        }));
                    }
                },
            );
        });
        ui.layout(&mut FakeText);
    };
    let mut ui = new_ui(1.0);
    build(&mut ui, 0);
    let (empty, width) = painted(&ui, &theme);
    build(&mut ui, ROWS);
    let v = ui.info(viewport(&ui)).expect("the rows").rect.to_px(1.0);
    let at = |x: i32, y: i32| {
        usize::try_from(y).expect("a row") * width + usize::try_from(x).expect("a column")
    };
    let mut ring: Vec<usize> = (v.x0..v.x1)
        .flat_map(|x| [at(x, v.y0), at(x, v.y1 - 1)])
        .collect();
    ring.extend((v.y0..v.y1).flat_map(|y| [at(v.x0, y), at(v.x1 - 1, y)]));
    for step in 0..4 {
        if step > 0 {
            // Partway down, at offsets that aren't a row's multiple.
            wheel(&mut ui, INSIDE, -100);
        }
        let (px, _) = painted(&ui, &theme);
        for &i in &ring {
            assert_eq!(
                px[i],
                empty[i],
                "step {step}: pixel ({}, {})",
                i % width,
                i / width
            );
        }
    }
}

/// High contrast paints the bar in `COLOR_BTNFACE` and `COLOR_BTNTEXT` (A2.5).
#[test]
fn in_high_contrast_the_bar_is_button_face_and_button_text() {
    let sys = SysColors {
        window: Color::hex(0x00_0000),
        window_text: Color::hex(0xFF_FFFF),
        highlight: Color::hex(0x1A_EBFF),
        highlight_text: Color::hex(0x00_0000),
        hot_light: Color::hex(0xFF_FF00),
        gray_text: Color::hex(0x3F_F23F),
        btn_face: Color::hex(0x20_2040),
        btn_text: Color::hex(0xFF_E000),
    };
    let theme = Theme::high_contrast(sys);
    let mut ui = new_ui(1.0);
    list(&mut ui);
    // The host turns auto-hide off in high contrast.
    ui.set_auto_hide_scroll_bars(false);
    ui.layout(&mut FakeText);
    let bar = scroll(&ui).bar_y.expect("a bar");
    assert!(bar.expanded);
    let (px, width) = painted(&ui, &theme);
    // The track beside the thumb, and the thumb's middle.
    let track = Point {
        x: bar.track.x,
        y: bar.thumb.center().y,
    };
    assert!(track.x < bar.thumb.x);
    assert_eq!(px[index(track, width)], pixel(sys.btn_face));
    assert_eq!(px[index(bar.thumb.center(), width)], pixel(sys.btn_text));
}

fn ak_of(nodes: &[(AkId, accesskit::Node)], ui: &Ui<Msg>, id: NodeId) -> Option<accesskit::Node> {
    nodes
        .iter()
        .find(|(k, _)| ui.a11y_node(*k) == Some(id))
        .map(|(_, n)| n.clone())
}

/// A node gains `ScrollIntoView` when a viewport comes to hold it, and loses it when none does,
/// though neither moves it.
#[test]
fn nodes_a_viewport_comes_to_hold_can_be_scrolled_into_view_at_once() {
    let rows = |ui: &mut Ui<Msg>, scrolls: bool| {
        ui.build(|cx| {
            let el = if scrolls {
                Element::scroll(Key::name("rows"), Axes::Y)
            } else {
                Element::stack(Key::name("rows"))
            };
            cx.add_with(el.style(rows_style()), |cx| {
                for r in 0..ROWS {
                    cx.add(Element::button(Key::index(r), "row", Msg::Row(r)).style(row_style()));
                }
            });
        });
        ui.layout(&mut FakeText);
    };
    let mut ui = new_ui(1.0);
    rows(&mut ui, false);
    drop(ui.a11y_tree());
    for scrolls in [true, false] {
        rows(&mut ui, scrolls);
        let update = ui.a11y_update().expect("an update");
        let last = ui
            .find(&[Key::name("rows"), Key::index(ROWS - 1)])
            .expect("the last row");
        let node = ak_of(&update.nodes, &ui, last).expect("the last row is sent again");
        assert_eq!(node.supports_action(Action::ScrollIntoView), scrolls);
    }
}

/// A viewport with no tab stop inside is one (A2.4), and a client sees it at once when its last
/// stop deeper down is disabled or goes, or comes back, though its content's size stays.
#[test]
fn a_viewport_whose_stops_come_and_go_tells_a_client_whether_it_takes_focus() {
    let log = |ui: &mut Ui<Msg>, copy: Option<bool>| {
        ui.build(|cx| {
            cx.add_with(
                Element::scroll(Key::name("rows"), Axes::Y)
                    .style(rows_style())
                    .name("Log"),
                |cx| {
                    cx.add_with(
                        Element::stack(Key::name("tools")).style(row_style()),
                        |cx| {
                            if let Some(enabled) = copy {
                                cx.add(
                                    Element::button(Key::name("copy"), "Copy", Msg::Row(0))
                                        .enabled(enabled),
                                );
                            }
                        },
                    );
                    for r in 0..ROWS {
                        cx.add(Element::text(Key::index(r), "line").style(row_style()));
                    }
                },
            );
        });
        ui.layout(&mut FakeText);
    };
    let focusable = |nodes: &[(AkId, accesskit::Node)], ui: &Ui<Msg>| {
        ak_of(nodes, ui, viewport(ui)).map(|n| n.supports_action(Action::Focus))
    };
    let mut ui = new_ui(1.0);
    log(&mut ui, Some(true));
    let tree = ui.a11y_tree();
    assert_eq!(focusable(&tree.nodes, &ui), Some(false));
    for (copy, takes_focus) in [(Some(false), true), (Some(true), false), (None, true)] {
        log(&mut ui, copy);
        let update = ui.a11y_update().expect("an update");
        assert_eq!(
            focusable(&update.nodes, &ui),
            Some(takes_focus),
            "the button {copy:?}"
        );
    }
}

/// A focused viewport has nothing to press: Enter and Space go on unused.
#[test]
fn enter_and_space_on_a_focused_viewport_go_on_unused() {
    let mut ui = new_ui(1.0);
    ui.build(|cx| {
        cx.add_with(
            Element::scroll(Key::name("rows"), Axes::Y)
                .style(rows_style())
                .name("Log"),
            |cx| {
                for r in 0..ROWS {
                    cx.add(Element::text(Key::index(r), "line").style(row_style()));
                }
            },
        );
    });
    ui.layout(&mut FakeText);
    key(&mut ui, KeyCode::Tab, Mods::default());
    assert_eq!(ui.focus(), Some(viewport(&ui)));
    assert!(!key(&mut ui, KeyCode::Enter, Mods::default()));
    assert!(!key(&mut ui, KeyCode::Space, Mods::default()));
    assert!(!ui.info(viewport(&ui)).expect("the rows").pressed);
}
