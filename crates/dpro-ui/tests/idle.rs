//! TD-0009 §7 U6, the static part: once nothing changes, the UI asks for nothing.
//!
//! The platform's loop sleeps in `GetMessage` unless the UI wants a frame, so idle costs no CPU
//! when two things hold:
//! - every animation ends, within its duration, after whatever input started it;
//! - a frame nobody asked for (a repaint after the window was uncovered, say) finds nothing to do:
//!   no damage, no accessibility update, and no new request for frames.
//!
//! Both are checked after every step of the scripted session, with an assistive technology
//! listening. G1's `--ui-probe idle` measures the rest on Windows: no timers, no wakes and no CPU
//! time over 5 s.

#[path = "../benches/fixtures/shell.rs"]
mod shell;

use dpro_ui::{FakeText, tokens::motion::HOVER_MS};
use shell::{Run, Shell, Step, view};

/// One frame at 60 Hz, rounded down to whole milliseconds.
const FRAME_MS: u64 = 16;

/// Frames until the animations stop: the first tick starts them, and they end once a whole
/// duration has passed.
fn max_ticks() -> u64 {
    u64::from(HOVER_MS).div_ceil(FRAME_MS) + 1
}

/// Ticks frames until the UI stops asking for them, and fails if that takes longer than the
/// hover duration allows. Returns the frames it took.
fn settle(run: &mut Run, when: &str) -> u64 {
    let mut ticks = 0;
    while run.ui.wants_frame() {
        assert!(
            ticks < max_ticks(),
            "{when}: still animating after {ticks} frames"
        );
        run.step(Step::Tick(FRAME_MS));
        ticks += 1;
    }
    ticks
}

/// A frame nobody asked for finds nothing to do.
fn assert_idle(run: &mut Run, when: &str) {
    assert!(!run.ui.wants_frame(), "{when}: still asks for frames");
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
}

#[test]
fn every_step_settles_within_the_hover_duration_and_stays_idle() {
    let mut run = Run::accessible(Shell::new());
    assert_idle(&mut run, "the first frame");
    let mut longest = 0;
    for (n, step) in run.session().into_iter().enumerate() {
        run.step(step);
        let when = format!("step {n} ({step:?})");
        longest = longest.max(settle(&mut run, &when));
        assert_idle(&mut run, &when);
    }
    // A full fade takes exactly the bound, so the bound is tight and the hovers did animate.
    assert_eq!(longest, max_ticks());
}

#[test]
fn the_session_as_scripted_settles_and_stays_idle() {
    // No frames between steps: hovers interrupt each other mid-fade, as a fast pointer does.
    let mut run = Run::accessible(Shell::new());
    for step in run.session() {
        run.step(step);
    }
    settle(&mut run, "after the session");
    assert_idle(&mut run, "after the session");
    // Idle frames stay idle.
    for k in 0..3 {
        assert_idle(&mut run, &format!("idle frame {k}"));
    }
}
