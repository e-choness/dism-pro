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

#![expect(
    clippy::expect_used,
    reason = "test helpers: a node that isn't there fails the test"
)]

use dpro_ui::{
    Axes, Dim, Element, FakeText, Input, Insets, Key, KeyCode, KeyEvent, Mods, Motion, NodeId,
    Point, Rect, ScrollInfo, Size, Style, Ui,
    accesskit::{Action, ActionData, ActionRequest, NodeId as AkId, ScrollUnit, TreeId},
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
