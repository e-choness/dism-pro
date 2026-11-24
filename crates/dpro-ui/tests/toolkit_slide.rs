//! Slides (TD-0009 A3.5 addition 11): `Cx::slide_in` moves a node's paint in from a window edge
//! over `motion.drawer` on the animation clock. Layout runs once, at the final place; a frame
//! damages where the node was and where it is, and nothing else; after the last frame nothing is
//! scheduled (U6). A node slides once, when it appears; with animations off, it doesn't.

#![allow(missing_docs)]

use dpro_ui::{
    Axis, Dim, Edge, Element, FakeText, Key, Motion, NodeId, RectPx, Size, Style, Ui,
    tokens::{motion::DRAWER_MS, slide::FRAME_MS},
};

const ROW: Key = Key::name("row");
const PAGE: Key = Key::name("page");
const PANE: Key = Key::name("pane");

/// A 400 × 300 window: a row of the page and, if `open`, a 100 px pane at `edge`, which slides in
/// from it.
fn build(ui: &mut Ui<u8>, open: bool, edge: Edge) -> Option<NodeId> {
    let mut pane = None;
    ui.build(|cx| {
        cx.add_with(
            Element::stack(ROW).style(Style {
                axis: Axis::Row,
                grow: 1.0,
                ..Style::default()
            }),
            |cx| {
                let add_pane = |cx: &mut dpro_ui::Cx<'_, u8>| {
                    let id = cx.add(Element::stack(PANE).style(Style {
                        width: Dim::Fixed(100.0),
                        ..Style::default()
                    }));
                    cx.slide_in(id, edge);
                    id
                };
                if open && edge == Edge::Start {
                    pane = Some(add_pane(cx));
                }
                cx.add(Element::stack(PAGE).style(Style {
                    grow: 1.0,
                    ..Style::default()
                }));
                if open && edge == Edge::End {
                    pane = Some(add_pane(cx));
                }
            },
        );
    });
    ui.layout(&mut FakeText);
    pane
}

fn ui(motion: Motion) -> Ui<u8> {
    let mut ui = Ui::new(Size { w: 400.0, h: 300.0 }, 1.0, motion);
    build(&mut ui, false, Edge::End);
    ui.clear_damage();
    ui
}

/// Ticks frames while the UI asks for them; returns the frames and each one's damage.
fn slide(ui: &mut Ui<u8>, edge: Edge) -> Vec<Vec<RectPx>> {
    let mut frames = Vec::new();
    let mut now = 1_000;
    while ui.wants_frame() {
        assert!(frames.len() < 20, "a slide that never ends");
        now += u64::from(FRAME_MS);
        ui.tick(now, &mut Vec::new());
        // The view builds again each frame, as the platform's does, and asks again.
        build(ui, true, edge);
        assert!(
            !ui.needs_layout(),
            "nothing to lay out after a frame's build"
        );
        frames.push(ui.damage().to_vec());
        ui.clear_damage();
    }
    frames
}

#[test]
fn motion_gives_the_drawers_duration_or_none() {
    assert_eq!(Motion::STANDARD.drawer_ms(), DRAWER_MS);
    assert_eq!(Motion::REDUCED.drawer_ms(), 0);
    assert_eq!(Motion::system(true).drawer_ms(), 180);
}

#[test]
fn a_node_slides_in_from_the_end_over_its_frames_and_then_asks_for_nothing() {
    let mut ui = ui(Motion::STANDARD);
    build(&mut ui, true, Edge::End);
    ui.clear_damage();
    assert!(ui.wants_frame(), "the slide asks for frames");
    let frames = slide(&mut ui, Edge::End);
    assert_eq!(
        u64::try_from(frames.len()).unwrap_or(0),
        u64::from(DRAWER_MS).div_ceil(u64::from(FRAME_MS))
    );
    // Every frame's damage is in the pane's band, from where layout put it to the window's end.
    let band = RectPx {
        x0: 300,
        y0: 0,
        x1: 400,
        y1: 300,
    };
    for (i, damage) in frames.iter().enumerate() {
        for d in damage {
            assert!(band.contains(*d), "frame {i}: {d:?}");
        }
    }
    let lefts: Vec<i32> = frames
        .iter()
        .filter_map(|d| d.iter().map(|r| r.x0).min())
        .collect();
    assert!(lefts.windows(2).all(|w| w[1] < w[0]), "{lefts:?}");
    assert_eq!(lefts.last(), Some(&300));
    assert!(!ui.wants_frame());
}

#[test]
fn a_node_slides_in_from_the_start() {
    let mut ui = ui(Motion::STANDARD);
    build(&mut ui, true, Edge::Start);
    ui.clear_damage();
    let frames = slide(&mut ui, Edge::Start);
    let band = RectPx {
        x0: 0,
        y0: 0,
        x1: 100,
        y1: 300,
    };
    for damage in &frames {
        for d in damage {
            assert!(band.contains(*d), "{d:?}");
        }
    }
    let rights: Vec<i32> = frames
        .iter()
        .filter_map(|d| d.iter().map(|r| r.x1).max())
        .collect();
    assert!(rights.windows(2).all(|w| w[1] > w[0]), "{rights:?}");
    assert_eq!(rights.last(), Some(&100));
}

#[test]
fn with_animations_off_nothing_slides() {
    let mut ui = ui(Motion::REDUCED);
    build(&mut ui, true, Edge::End);
    assert!(!ui.wants_frame());
}

#[test]
fn a_node_slides_once_however_often_the_view_asks() {
    let mut ui = ui(Motion::STANDARD);
    build(&mut ui, true, Edge::End);
    slide(&mut ui, Edge::End);
    for _ in 0..3 {
        build(&mut ui, true, Edge::End);
        assert!(!ui.wants_frame(), "laid out, it doesn't slide again");
    }
    // Gone and back, it's a new node, and it slides again.
    build(&mut ui, false, Edge::End);
    build(&mut ui, true, Edge::End);
    assert!(ui.wants_frame());
}

#[test]
fn animations_turned_off_mid_slide_end_it_at_the_next_frame() {
    let mut ui = ui(Motion::STANDARD);
    build(&mut ui, true, Edge::End);
    ui.tick(1_000, &mut Vec::new());
    ui.tick(1_016, &mut Vec::new());
    assert!(ui.wants_frame());
    ui.set_motion(Motion::REDUCED);
    ui.tick(1_032, &mut Vec::new());
    assert!(!ui.wants_frame());
}

#[test]
fn a_node_gone_mid_slide_asks_for_no_more_frames() {
    let mut ui = ui(Motion::STANDARD);
    build(&mut ui, true, Edge::End);
    ui.tick(1_000, &mut Vec::new());
    assert!(ui.wants_frame());
    build(&mut ui, false, Edge::End);
    assert!(!ui.wants_frame());
}
