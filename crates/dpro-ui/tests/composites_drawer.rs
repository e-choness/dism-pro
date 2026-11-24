//! The drawer frame (TD-0009 A3.5 (6), A3.9, A3.7's U16), on `fixtures/drawer.rs`'s page: Esc
//! closes it and focus goes back to what opened it; opening focuses the body's first control, or
//! Close; a pane beside the page or a sheet over it (the page then out of the tree); exactly
//! ⌈`drawer_ms` / frame⌉ frames and one layout per open, one frame with animations off, and
//! nothing asked for after; the footer stays put while the body scrolls; Invoke on the hold sends
//! the typed confirmation; what a client sees; every target at least 24 px.

#![allow(missing_docs)]
#![expect(
    clippy::expect_used,
    reason = "test helpers: a part that isn't there fails the test"
)]

#[path = "../benches/fixtures/drawer.rs"]
mod drawer;

use dpro_ui::{
    Input, KeyCode, Mods, NodeId, Point, Role, Theme, Tone,
    accesskit::{Action, ActionRequest, Role as AkRole, TreeId},
    composite::{
        CALLOUT_TITLE, CODE_COPY, CODE_TEXT, DRAWER_BODY, DRAWER_CANCEL, DRAWER_CLOSE,
        DRAWER_FOOTER, DRAWER_PRIMARY, DRAWER_TITLE as TITLE_KEY, SECTION_TITLE,
    },
    tokens::{drawer as tokens, motion::DRAWER_MS},
};
use drawer::{
    CALLOUT, CHANGES, CODE, DRAWER_TITLE, FRAME_MS, Model, Msg, NARROW, PAGE, ROW, Run, SCAN,
    START, Step, key,
};

const NONE: Mods = Mods {
    shift: false,
    ctrl: false,
    alt: false,
    win: false,
};

/// A slide's frames (U16).
fn slide_frames() -> u64 {
    u64::from(DRAWER_MS).div_ceil(FRAME_MS)
}

/// Opens the drawer as a keyboard user does: focus on "Review changes", then Enter, which acts on
/// its press.
fn open(run: &mut Run) {
    let opener = run.opener().expect("Review changes");
    run.ui.set_focus(Some(opener));
    run.step(key(KeyCode::Enter, NONE));
    assert!(run.model.open, "Enter on Review changes opens the drawer");
    assert!(run.drawer().is_some());
}

/// The first change's check box, in the body.
fn first_change(run: &Run) -> NodeId {
    let changes = run.part(CHANGES).expect("the Changes section");
    run.ui
        .children(changes)
        .find(|&c| run.ui.info(c).is_some_and(|n| n.role == Role::CheckBox))
        .expect("a check box")
}

#[test]
#[ignore = "slice 6d: lands with the code"]
fn esc_closes_the_drawer_and_focus_goes_back_to_its_opener() {
    let mut run = Run::with(Model::new());
    open(&mut run);
    run.settle();
    let drawer = run.drawer().expect("open");
    let focus = run.ui.focus().expect("focus moved into the drawer");
    assert!(run.is_under(focus, drawer), "focus is in the drawer");
    run.step(key(KeyCode::Escape, NONE));
    assert_eq!(run.model.last, Some(Msg::Close), "Esc sends close");
    assert!(run.drawer().is_none());
    assert_eq!(
        run.ui.focus(),
        run.opener(),
        "focus is back on Review changes"
    );
}

#[test]
#[ignore = "slice 6d: lands with the code"]
fn close_and_cancel_close_it_and_focus_goes_back_too() {
    for (part, msg) in [(DRAWER_CLOSE, Msg::Close), (DRAWER_CANCEL, Msg::Cancel)] {
        let mut run = Run::with(Model::new());
        open(&mut run);
        let button = run.part(part).expect("the button");
        run.ui.set_focus(Some(button));
        run.step(key(KeyCode::Enter, NONE));
        assert_eq!(run.model.last, Some(msg));
        assert!(run.drawer().is_none());
        assert_eq!(run.ui.focus(), run.opener(), "{msg:?}");
    }
}

#[test]
#[ignore = "slice 6d: lands with the code"]
fn esc_with_focus_on_the_page_closes_the_drawer_and_leaves_focus_there() {
    let mut run = Run::with(Model::new());
    open(&mut run);
    // The user went back to the page beside the pane.
    let opener = run.opener().expect("the page shows beside a pane");
    run.ui.set_focus(Some(opener));
    run.step(key(KeyCode::Escape, NONE));
    assert!(
        run.drawer().is_none(),
        "the window's open scope takes the Esc"
    );
    assert_eq!(run.ui.focus(), Some(opener));
}

#[test]
fn focus_moved_out_of_the_drawer_stays_where_it_went_when_it_closes() {
    let mut run = Run::with(Model::new());
    open(&mut run);
    let scan = run.find(&[ROW, PAGE, SCAN]).expect("Scan again");
    run.ui.set_focus(Some(scan));
    run.step(Step::Update(Msg::Close));
    assert_eq!(run.ui.focus(), Some(scan), "not back to Review changes");
}

#[test]
#[ignore = "slice 6d: lands with the code"]
fn opening_focuses_the_bodys_first_control_or_close() {
    let mut run = Run::with(Model::new());
    open(&mut run);
    assert_eq!(run.ui.focus(), Some(first_change(&run)));
    // A body with nothing to focus: Close.
    let mut model = Model::new();
    model.empty = true;
    let mut run = Run::with(model);
    open(&mut run);
    assert_eq!(run.ui.focus(), run.part(DRAWER_CLOSE));
    // Built again, open, it leaves focus where it is.
    let footer_cancel = run.part(DRAWER_CANCEL).expect("Cancel");
    run.ui.set_focus(Some(footer_cancel));
    run.step(Step::Update(Msg::Check(0)));
    assert_eq!(run.ui.focus(), Some(footer_cancel));
}

#[test]
#[ignore = "slice 6d: lands with the code"]
fn the_slide_takes_its_frames_lays_out_once_and_then_asks_for_nothing() {
    let mut run = Run::with(Model::new()).warmed();
    let layouts = run.layouts;
    run.step(Step::Update(Msg::Open));
    assert_eq!(run.layouts, layouts + 1, "opening lays out once");
    let at = run
        .drawer()
        .and_then(|d| run.ui.info(d))
        .expect("open")
        .rect;
    assert_eq!((at.x, at.w), (START.w - tokens::WIDTH, tokens::WIDTH));
    let band = dpro_ui::RectPx {
        x0: 540,
        y0: 0,
        x1: 1000,
        y1: 700,
    };
    let mut frames = 0;
    let mut moved = 0;
    let mut left = i32::MAX;
    while run.ui.wants_frame() {
        assert!(
            frames < slide_frames(),
            "still sliding after {frames} frames"
        );
        run.step(Step::Tick(FRAME_MS));
        frames += 1;
        for d in &run.damage {
            assert!(
                band.contains(*d),
                "frame {frames} damages {d:?} outside the drawer"
            );
        }
        // The drawer comes in from the end: each frame that moves it shows more of it. Easing
        // out, the last moves are under a pixel, and a frame that moves nothing damages nothing.
        if let Some(x0) = run.damage.iter().map(|d| d.x0).min() {
            assert!(x0 < left, "frame {frames}: {x0} isn't left of {left}");
            left = x0;
            moved += 1;
        }
    }
    assert_eq!(frames, slide_frames(), "⌈drawer_ms / frame⌉ frames");
    assert!(moved + 2 >= frames, "{moved} of {frames} frames moved it");
    assert_eq!(left, band.x0, "the last move puts it where layout put it");
    assert_eq!(run.layouts, layouts + 1, "no frame of the slide lays out");
    assert_eq!(run.wake(), None, "nothing scheduled after the last frame");
    // Closing: one frame and one layout, then nothing.
    run.step(Step::Update(Msg::Close));
    assert_eq!(run.layouts, layouts + 2);
    assert!(!run.ui.wants_frame());
    assert_eq!(run.wake(), None);
}

#[test]
#[ignore = "slice 6d: lands with the code"]
fn closing_mid_slide_stops_its_frames() {
    let mut run = Run::with(Model::new()).warmed();
    run.step(Step::Update(Msg::Open));
    for _ in 0..3 {
        run.step(Step::Tick(FRAME_MS));
    }
    assert!(run.ui.wants_frame(), "still sliding");
    run.step(Step::Update(Msg::Close));
    assert!(!run.ui.wants_frame(), "gone, it asks for no more frames");
}

#[test]
fn with_animations_off_the_drawer_opens_and_closes_in_one_frame() {
    let mut run = Run::reduced(Model::new()).warmed();
    let (frames, layouts) = (run.frames, run.layouts);
    run.step(Step::Update(Msg::Open));
    assert!(run.drawer().is_some());
    assert!(!run.ui.wants_frame(), "no slide");
    run.step(Step::Update(Msg::Close));
    assert!(!run.ui.wants_frame());
    assert_eq!(
        (run.frames - frames, run.layouts - layouts),
        (2, 2),
        "one frame and one layout each"
    );
}

#[test]
#[ignore = "slice 6d: lands with the code"]
fn the_slide_paints_the_drawer_moved_toward_the_end() {
    let mut run = Run::painted(Model::new(), Theme::LIGHT).warmed();
    let argb = |tone: Tone| {
        let c = Theme::LIGHT.color(tone);
        0xFF00_0000 | u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
    };
    run.step(Step::Update(Msg::Open));
    run.step(Step::Tick(FRAME_MS));
    // Just inside where layout put the drawer, below its header: the window, not the drawer.
    let (x, y) = (560, 600);
    assert_eq!(
        run.pixel(x, y),
        argb(Tone::App),
        "the drawer hasn't got here yet"
    );
    run.settle();
    assert_eq!(run.pixel(x, y), argb(Tone::Surface), "now it has");
    // Incremental painting gave the pixels a full repaint gives.
    let incremental = (0..700)
        .step_by(7)
        .flat_map(|y| (0..1000).step_by(7).map(move |x| (x, y)))
        .map(|(x, y)| run.pixel(x, y))
        .collect::<Vec<_>>();
    run.repaint();
    let full = (0..700)
        .step_by(7)
        .flat_map(|y| (0..1000).step_by(7).map(move |x| (x, y)))
        .map(|(x, y)| run.pixel(x, y))
        .collect::<Vec<_>>();
    assert!(incremental == full, "the slide's damage left stale pixels");
}

#[test]
fn a_pane_beside_the_page_or_a_sheet_over_it() {
    let mut run = Run::with(Model::new());
    run.step(Step::Update(Msg::Open));
    let rect = |run: &Run, id: Option<NodeId>| id.and_then(|i| run.ui.info(i)).map(|n| n.rect);
    let drawer = rect(&run, run.drawer()).expect("open");
    let page = rect(&run, run.find(&[ROW, PAGE])).expect("the page beside it");
    assert_eq!((drawer.x, drawer.w), (540.0, tokens::WIDTH));
    assert_eq!(
        (page.x, page.w),
        (0.0, 540.0),
        "the page takes its new width"
    );
    // Too narrow for the page beside it: a sheet over it, and the page goes.
    run.step(Step::Update(Msg::Close));
    run.step(Step::Resize(NARROW));
    run.step(Step::Update(Msg::Open));
    let sheet = rect(&run, run.drawer()).expect("open");
    assert_eq!((sheet.x, sheet.w), (0.0, NARROW.w));
    assert!(
        run.find(&[ROW, PAGE]).is_none(),
        "the page leaves the window"
    );
    run.step(Step::Update(Msg::Close));
    assert!(run.find(&[ROW, PAGE]).is_some(), "and comes back");
    assert!(dpro_ui::composite::drawer_is_sheet(939.0));
    assert!(!dpro_ui::composite::drawer_is_sheet(940.0));
}

#[test]
fn a_sheet_takes_the_page_out_of_the_tree() {
    let mut run = Run::accessible(Model::new());
    run.step(Step::Resize(NARROW));
    run.step(Step::Update(Msg::Open));
    let tree = run.ui.a11y_tree();
    let mains = Run::nodes(&tree)
        .filter(|(_, n)| n.role() == AkRole::Main)
        .count();
    assert_eq!(mains, 0, "the page isn't in the tree");
    let regions: Vec<_> = Run::nodes(&tree)
        .filter(|(_, n)| n.role() == AkRole::Region)
        .map(|(_, n)| n.label().map(str::to_owned))
        .collect();
    assert_eq!(regions, [Some(DRAWER_TITLE.to_owned())]);
}

#[test]
fn the_footer_stays_put_while_the_body_scrolls() {
    let mut run = Run::with(Model::new());
    open(&mut run);
    run.settle();
    let footer = rect(&run, DRAWER_FOOTER);
    let body = run
        .part(DRAWER_BODY)
        .and_then(|id| run.ui.info(id))
        .expect("the body");
    assert!(
        body.scroll.is_some_and(|s| s.content.h > body.rect.h),
        "20 changes overflow the body"
    );
    let at = body.rect.center();
    let callout = rect(&run, CALLOUT);
    run.step(Step::Input(Input::Wheel {
        at,
        delta: -120,
        horizontal: false,
    }));
    let scrolled = run
        .part(DRAWER_BODY)
        .and_then(|id| run.ui.info(id))
        .and_then(|b| b.scroll)
        .expect("a viewport");
    assert!(scrolled.offset.y > 0.0, "the body scrolled");
    assert_eq!(rect(&run, DRAWER_FOOTER), footer);
    assert!(
        rect(&run, CALLOUT).y < callout.y,
        "the body's content moved up"
    );
    assert!(footer.bottom() <= START.h, "the footer is in the window");
}

/// Where the drawer's part `key` shows, in the window.
fn rect(run: &Run, key: dpro_ui::Key) -> dpro_ui::Rect {
    run.part(key)
        .and_then(|id| run.ui.info(id))
        .expect("the part")
        .rect
}

#[test]
fn invoke_on_the_hold_sends_the_typed_confirmation_never_the_delete() {
    let mut run = Run::accessible(Model::new());
    run.step(Step::Update(Msg::Open));
    let tree = run.ui.a11y_tree();
    let hold = run.part(DRAWER_PRIMARY).expect("the hold");
    let target = Run::nodes(&tree)
        .find(|(ak, _)| run.ui.a11y_node(*ak) == Some(hold))
        .map(|(ak, _)| ak)
        .expect("in the tree");
    let mut out = Vec::new();
    assert!(run.ui.a11y_action(
        &ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: target,
            data: None,
        },
        &mut out,
    ));
    assert_eq!(out, [Msg::Confirm]);
}

#[test]
fn a_client_sees_a_region_named_by_its_title_and_its_parts() {
    let mut run = Run::accessible(Model::new());
    run.step(Step::Update(Msg::Open));
    let tree = run.ui.a11y_tree();
    let ak = |id: Option<NodeId>| id.and_then(|id| run.ak(&tree, id));
    let drawer = ak(run.drawer()).expect("the drawer");
    assert_eq!(drawer.role(), AkRole::Region);
    assert_eq!(drawer.label(), Some(DRAWER_TITLE));
    assert_eq!(drawer.role_description(), Some(drawer::REGION));
    let title = ak(run.part(TITLE_KEY)).expect("the title");
    // AccessKit's levels are aria-level less one.
    assert_eq!(title.level(), Some(1), "a level-2 heading");
    let close = ak(run.part(DRAWER_CLOSE)).expect("Close");
    assert_eq!(
        (close.role(), close.label()),
        (AkRole::Button, Some("Close"))
    );
    assert!(close.children().is_empty(), "its glyph is presentational");
    let section = run.part(CHANGES).expect("Changes");
    let heading = ak(run.part_of(section, SECTION_TITLE)).expect("its title");
    assert_eq!(
        heading.level(),
        Some(2),
        "level 3, under the drawer's title"
    );
    let callout = ak(run.part(CALLOUT)).expect("the callout");
    assert_eq!(
        (callout.role(), callout.label()),
        (AkRole::Group, Some("Permanent"))
    );
    assert!(ak(run.part_of(run.part(CALLOUT).expect("callout"), CALLOUT_TITLE)).is_some());
    let code = run.part(CODE).expect("the code block");
    let text = ak(run.part_of(code, CODE_TEXT)).expect("its text");
    assert_eq!(text.role(), AkRole::Label);
    let copy = ak(run.part_of(code, CODE_COPY)).expect("Copy");
    assert_eq!(
        (copy.role(), copy.label()),
        (AkRole::Button, Some(drawer::COPY))
    );
    for (part, name) in [
        (DRAWER_CANCEL, drawer::CANCEL),
        (DRAWER_PRIMARY, drawer::HOLD),
    ] {
        let b = ak(run.part(part)).expect("a footer button");
        assert_eq!((b.role(), b.label()), (AkRole::Button, Some(name)));
    }
}

#[test]
fn copy_sends_its_message() {
    let mut run = Run::with(Model::new());
    open(&mut run);
    let code = run.part(CODE).expect("the code block");
    let copy = run.part_of(code, CODE_COPY).expect("Copy");
    run.ui.set_focus(Some(copy));
    run.step(key(KeyCode::Enter, NONE));
    assert_eq!((run.model.last, run.model.copies), (Some(Msg::Copy), 1));
}

/// Every enabled target in the drawer, as a pane and as a sheet: at least 24 × 24 (doc 05 §6).
#[test]
fn every_target_in_the_drawer_is_at_least_24_px() {
    let mut small = Vec::new();
    for size in [START, NARROW] {
        let mut run = Run::with(Model::new());
        run.step(Step::Resize(size));
        run.step(Step::Update(Msg::Open));
        let mut stack = vec![run.drawer().expect("open")];
        let mut targets = 0;
        while let Some(id) = stack.pop() {
            stack.extend(run.ui.children(id));
            let Some(n) = run.ui.info(id) else { continue };
            if n.enabled && matches!(n.role, Role::Button | Role::CheckBox) {
                targets += 1;
                if n.rect.w < 24.0 || n.rect.h < 24.0 {
                    small.push(format!(
                        "{size:?}: {:?} {:?} is {:?}",
                        n.role, n.text, n.rect
                    ));
                }
            }
        }
        assert!(targets > 20, "Close, 20 changes, Copy, Cancel and the hold");
    }
    assert!(small.is_empty(), "{small:#?}");
}

/// The pointer on the page beside a pane still reaches it.
#[test]
fn the_page_beside_a_pane_takes_the_pointer() {
    let mut run = Run::with(Model::new());
    open(&mut run);
    run.settle();
    let scan = run.find(&[ROW, PAGE, SCAN]).expect("the page shows");
    let at: Point = run.center(scan);
    run.step(Step::Input(Input::PointerDown(at)));
    run.step(Step::Input(Input::PointerUp(at)));
    assert_eq!(run.model.last, Some(Msg::Scan));
    assert!(run.drawer().is_some(), "the pane stays open");
}
