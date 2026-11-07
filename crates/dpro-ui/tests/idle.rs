//! TD-0009 §7 U6, the static part: once nothing changes, the UI asks for nothing.
//!
//! The platform's loop sleeps in `GetMessage` unless the UI wants a frame or its caret timer, so
//! idle costs no CPU when four things hold:
//! - every animation ends, within its duration, after whatever input started it;
//! - a hold on Hold to apply asks for frames only while it runs: until it's let go, or until
//!   `motion.hold` has passed and it has applied, even if it's still pressed (§11 step 4);
//! - a focused field's caret stops blinking, on, within [`Caret::BLINK_FOR_MS`] of the last
//!   keystroke or caret move, and its timer then stops too (§11 step 3);
//! - a frame nobody asked for (a repaint after the window was uncovered, say) finds nothing to do:
//!   no damage, no accessibility update, and no new request for frames or wakes.
//!
//! All of them are checked after every step of the scripted session, with an assistive technology
//! listening. G1's `--ui-probe idle` measures the rest on Windows: no timers, no wakes and no CPU
//! time over 5 s.
//!
//! Popups (TD-0009 A1.7): a pending tooltip is one wake and no frames, a showing one the wake that
//! hides it; an open menu asks for nothing.
//!
//! Scrolling (A2, slice 5a): a scroll is done when its event is, with no smooth scrolling and no
//! fading bar, so a wheel notch, a thumb drag and its release each settle like any other input.
//!
//! The table (A2, slice 5b1): an idle frame doesn't ask for another build, and moving the current
//! row, checking it, sorting and realizing a far row each settle at once.

#[path = "../benches/fixtures/shell.rs"]
mod shell;

use dpro_ui::{
    Caret, ColumnId, FakeText, Input, KeyCode, Mods, Point, TableEvent,
    tokens::{
        motion::{HOLD_MS, HOVER_MS},
        popup::{TIP_DELAY_MS, TIP_SHOWS_FOR},
    },
};
use shell::{FRAME_MS, Msg, Run, Shell, Step, key, popup_view, view};

/// Frames until the animations stop: the first tick starts them, and they end once a whole
/// duration has passed.
fn max_ticks() -> u64 {
    u64::from(HOVER_MS).div_ceil(FRAME_MS) + 1
}

/// Frames until a hold held from its first tick applies: the first tick starts its clock, and it
/// ends once `motion.hold` has passed.
fn hold_ticks() -> u64 {
    u64::from(HOLD_MS).div_ceil(FRAME_MS) + 1
}

/// Ticks frames until the UI stops asking for them, and fails if that takes more than `most`.
/// Returns the frames it took.
fn settle(run: &mut Run, when: &str, most: u64) -> u64 {
    let mut ticks = 0;
    while run.ui.wants_frame() {
        assert!(ticks < most, "{when}: still asks for frames after {ticks}");
        run.step(Step::Tick(FRAME_MS));
        ticks += 1;
    }
    ticks
}

/// Runs the caret timer as the platform's one-shot timer would, until it stops, and fails if
/// that takes longer than the blink allows. Returns the wakes it took.
fn settle_caret(run: &mut Run, when: &str) -> u64 {
    let caret = Caret::WINDOWS;
    let most = u64::from(Caret::BLINK_FOR_MS) / u64::from(caret.blink_ms) + 2;
    let mut wakes = 0;
    while let Some(ms) = run.caret_wake() {
        assert!(
            wakes < most,
            "{when}: the caret still blinks after {wakes} wakes"
        );
        assert!(ms > 0, "{when}: a caret wake 0 ms away");
        run.step(Step::Tick(ms));
        wakes += 1;
    }
    wakes
}

/// A frame nobody asked for finds nothing to do.
fn assert_idle(run: &mut Run, when: &str) {
    assert!(!run.ui.wants_frame(), "{when}: still asks for frames");
    assert!(!run.ui.wants_build(), "{when}: the table asks for a build");
    assert_eq!(run.caret_wake(), None, "{when}: still asks for caret wakes");
    run.ui.build(|cx| view(&run.shell, cx));
    run.ui.layout(&mut FakeText);
    assert_eq!(
        run.ui.damage().len(),
        0,
        "{when}: an idle frame damaged {:?}",
        run.ui.damage()
    );
    assert!(
        run.a11y_update().is_none(),
        "{when}: an idle frame changed the accessibility tree"
    );
    assert!(
        !run.ui.wants_frame(),
        "{when}: an idle frame asked for another"
    );
    assert!(
        !run.ui.wants_build(),
        "{when}: an idle frame asked for another build"
    );
    assert_eq!(
        run.caret_wake(),
        None,
        "{when}: an idle frame armed the caret timer"
    );
}

#[test]
fn every_step_settles_within_its_bound_and_stays_idle() {
    let mut run = Run::accessible(Shell::new());
    assert_idle(&mut run, "the first frame");
    let (mut longest, mut held, mut blinked) = (0, 0, 0);
    for (n, step) in run.session().into_iter().enumerate() {
        let when = format!("step {n} ({step:?})");
        run.step(step);
        // The session's press on Hold to apply, settled here: held until it applies.
        if run.ui.hold_progress().is_some() {
            held = held.max(settle(&mut run, &when, hold_ticks()));
        } else {
            longest = longest.max(settle(&mut run, &when, max_ticks()));
        }
        blinked = blinked.max(settle_caret(&mut run, &when));
        assert_idle(&mut run, &when);
    }
    // A full fade takes exactly the bound, so the bound is tight and the hovers did animate.
    assert_eq!(longest, max_ticks());
    // So does the hold, which then applied, once.
    assert_eq!(held, hold_ticks());
    assert_eq!(run.shell.applies(), 1);
    // The field had focus, and its caret blinked for the whole time it may.
    let caret = Caret::WINDOWS;
    assert_eq!(
        blinked,
        u64::from(Caret::BLINK_FOR_MS).div_ceil(u64::from(caret.blink_ms))
    );
}

#[test]
fn the_session_as_scripted_settles_and_stays_idle() {
    // No frames between steps: hovers interrupt each other mid-fade, as a fast pointer does, and
    // the hold is let go after three frames.
    let mut run = Run::accessible(Shell::new());
    for step in run.session() {
        run.step(step);
    }
    settle(&mut run, "after the session", max_ticks());
    settle_caret(&mut run, "after the session");
    assert_idle(&mut run, "after the session");
    assert_eq!(run.shell.applies(), 0, "a hold let go early applied");
    // Idle frames stay idle.
    for k in 0..3 {
        assert_idle(&mut run, &format!("idle frame {k}"));
    }
}

/// TD-0009 A1.7: a pending tooltip costs one wake and no frames, a showing one the wake that hides
/// it, and then nothing.
#[test]
fn a_tooltip_is_one_wake_at_a_time_and_no_frames() {
    let mut run = Run::accessible(Shell::new());
    let at = run.center(&Run::DRAWER_BUTTON);
    run.step(Step::Input(Input::PointerMove(at)));
    settle(&mut run, "hovered", max_ticks());
    let delay = u64::from(TIP_DELAY_MS);
    let due = run.wake().expect("a pending tip's wake");
    assert!(due > 0 && due <= delay, "due in {due} ms");
    assert!(!run.ui.wants_frame(), "a pending tip asks for no frames");
    run.step(Step::Tick(due));
    assert!(run.ui.tooltip().is_some(), "due, it shows");
    assert!(!run.ui.wants_frame(), "a showing tip asks for no frames");
    let autopop = delay * u64::from(TIP_SHOWS_FOR);
    assert_eq!(run.wake(), Some(autopop), "one wake, to hide it");
    run.step(Step::Tick(autopop));
    assert!(run.ui.tooltip().is_none());
    assert_eq!(run.wake(), None, "hidden, it asks for nothing");
    assert_idle(&mut run, "the tip came and went");
}

/// TD-0009 A1.7: an open menu asks for no frames and no wakes, in either window.
#[test]
fn an_open_menu_asks_for_nothing() {
    let mut run = Run::accessible(Shell::new());
    let row = run.ui.find(&Run::row(4)).expect("row 4");
    run.ui.set_focus(Some(row));
    run.step(Step::Input(Input::ContextMenu(None)));
    assert!(run.popup_open, "Shift+F10 opens the row's menu");
    settle(&mut run, "the menu opened", max_ticks());
    assert!(!run.popup.wants_frame(), "the menu asks for frames");
    assert_eq!(run.wake(), None, "the menu asks for a wake");
    assert_idle(&mut run, "the menu open");
    // A frame of the menu nobody asked for finds nothing to do either.
    run.popup.build(|cx| popup_view(&run.shell, cx));
    run.popup.layout(&mut FakeText);
    assert_eq!(run.popup.damage().len(), 0, "{:?}", run.popup.damage());
    assert!(!run.popup.wants_frame());
}

#[test]
fn a_hold_asks_for_frames_only_while_it_runs() {
    let mut run = Run::accessible(Shell::new()).drawer_open();
    let at = run.center(&Run::APPLY);
    // Let go halfway: nothing applies, and the frames stop with it.
    run.step(Step::Input(Input::PointerDown(at)));
    for _ in 0..hold_ticks() / 2 {
        run.step(Step::Tick(FRAME_MS));
        assert!(
            run.ui.wants_frame(),
            "a running hold stopped asking for frames"
        );
    }
    run.step(Step::Input(Input::PointerUp(at)));
    settle(&mut run, "let go", max_ticks());
    assert_idle(&mut run, "let go");
    assert_eq!(run.shell.applies(), 0, "a hold let go early applied");

    // Held to the end: it applies once, and asks for nothing more while still pressed, nor when
    // it's let go.
    run.step(Step::Input(Input::PointerDown(at)));
    settle(&mut run, "held", hold_ticks());
    assert_eq!(run.shell.applies(), 1);
    assert_idle(&mut run, "held, still pressed");
    run.step(Step::Input(Input::PointerUp(at)));
    settle(&mut run, "held, let go", max_ticks());
    assert_idle(&mut run, "held, let go");
    assert_eq!(run.shell.applies(), 1, "letting go applied again");
}

/// A2, slice 5a: a wheel notch scrolls at once, and a thumb drag asks for nothing while it's held
/// still, nor after it's let go.
#[test]
fn a_scroll_and_a_thumb_drag_ask_for_nothing_after() {
    let mut run = Run::accessible(Shell::new()).over_table();
    let step = run.table_wheel(-120);
    run.step(step);
    // Rows moved under the resting pointer, so a hover may fade: no more than that.
    settle(&mut run, "a notch down", max_ticks());
    assert_idle(&mut run, "a notch down");
    assert!(run.wake().is_none(), "a notch armed a wake");

    let mut run = run.thumb_grabbed();
    assert_idle(&mut run, "the thumb pressed");
    let step = run.thumb_moved(40.0);
    run.step(step);
    settle(&mut run, "the thumb dragged", max_ticks());
    assert_idle(&mut run, "the thumb dragged, held still");
    let at = run.table_bar().expect("the table overflows").thumb.center();
    run.step(Step::Input(Input::PointerUp(at)));
    settle(&mut run, "the thumb let go", max_ticks());
    assert_idle(&mut run, "the thumb let go");
    assert!(run.wake().is_none(), "the drag armed a wake");
}

/// A2, slice 5b1: the table's keys, a sort and a realized row each settle at once.
#[test]
fn the_table_settles_after_each_change() {
    let mut run = Run::accessible(Shell::new()).bottom_row_current();
    assert_idle(&mut run, "the bottom row current");
    let plain = Mods::default();
    for (step, when) in [
        (key(KeyCode::Down, true, plain), "Down past the view's end"),
        (key(KeyCode::Down, false, plain), "Down let go"),
        (key(KeyCode::Space, true, plain), "Space checking the row"),
        (key(KeyCode::Space, false, plain), "Space let go"),
        (
            Step::Update(Msg::Table(TableEvent::Sort(ColumnId(0)))),
            "a sort",
        ),
        (Step::Realize(700), "a far row realized"),
        (key(KeyCode::End, true, plain), "End"),
    ] {
        run.step(step);
        settle(&mut run, when, max_ticks());
        assert_idle(&mut run, when);
        assert!(run.wake().is_none(), "{when} armed a wake");
    }
}

/// A2, slice 5b2: a divider's drag, a fit, a column hidden and the column menu each settle at
/// once.
#[test]
fn the_columns_settle_after_each_change() {
    let mut run = Run::accessible(Shell::new()).divider_grabbed(1);
    assert_idle(&mut run, "a divider pressed");
    let at = run.divider(1);
    for (step, when) in [
        (run.divider_moved(1, 30.0), "a divider dragged"),
        (
            Step::Input(Input::PointerUp(Point { x: at.x + 30.0, ..at })),
            "a divider let go",
        ),
        (
            Step::Update(Msg::Table(TableEvent::Fit(Some(ColumnId(2))))),
            "a column fitted",
        ),
        (
            Step::Update(Msg::Table(TableEvent::Fit(None))),
            "every column fitted",
        ),
        (
            Step::Update(Msg::Table(TableEvent::Show(ColumnId(3), false))),
            "a column hidden",
        ),
        (
            Step::Update(Msg::Table(TableEvent::ResetColumns)),
            "the columns reset",
        ),
    ] {
        run.step(step);
        settle(&mut run, when, max_ticks());
        assert_idle(&mut run, when);
        assert!(run.wake().is_none(), "{when} armed a wake");
    }
    let mut run = run.column_menu_open();
    assert!(run.popup_open, "the column menu opened");
    assert_idle(&mut run, "the column menu open");
}
