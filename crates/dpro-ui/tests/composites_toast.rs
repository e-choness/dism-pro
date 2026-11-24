//! A job's toast (TD-0009 A3.5 (7), A3.9), in its own window at the owner's bottom end: its line
//! is a polite live region, and the job card's line is live only while the toast doesn't show, so
//! two live views of one job never coexist. A progress event damages only the two bars (U17). Esc
//! or Dismiss hides it; an ended toast hides after Windows' message duration, which costs exactly
//! one wake, and the pointer over it holds it. That the toast window never takes activation on
//! show or click, and F6 does, is the host's: the platform's T2 tests check it.

#![expect(
    clippy::expect_used,
    reason = "test helpers: a node that isn't there fails the test"
)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use composites::{FRAME_MS, JOB, JOB_NAME, Model, Msg, PAGE, Run, Step, TOAST, Window};
use dpro_ui::{
    Input, KeyCode, KeyEvent, MESSAGE_MS, Mods, NodeId, RectPx,
    accesskit::{Live, Role as AkRole},
    composite::{JOB_BAR, JOB_LINE, TOAST_BAR, TOAST_DISMISS, TOAST_LINE, ToastTone},
    tokens::toast,
};

fn running() -> Run {
    Run::with(Model::new()).job_running()
}

fn toast_part(run: &Run, key: dpro_ui::Key) -> NodeId {
    run.part_in(Window::Toast, key).expect("the toast's part")
}

fn card_part(run: &Run, key: dpro_ui::Key) -> NodeId {
    run.part(&[PAGE, JOB], key).expect("the job card's part")
}

fn esc() -> Step {
    Step::Toast(Input::Key(KeyEvent {
        key: KeyCode::Escape,
        down: true,
        mods: Mods::default(),
    }))
}

/// `r` holds the rectangle of node `id` of `ui`, at scale 1.
fn covers(r: &RectPx, ui: &dpro_ui::Ui<Msg>, id: NodeId) -> bool {
    let n = ui.info(id).expect("the node").rect;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "pixels inside a 1000 × 700 window"
    )]
    let (x0, y0, x1, y1) = (
        n.x.floor() as i32,
        n.y.floor() as i32,
        (n.x + n.w).ceil() as i32,
        (n.y + n.h).ceil() as i32,
    );
    r.x0 <= x0 && r.y0 <= y0 && r.x1 >= x1 && r.y1 >= y1
}

#[test]
fn a_job_shows_its_toast_and_its_card() {
    let run = running();
    assert!(run.win.toast_open);
    assert!(run.find(&[PAGE, JOB]).is_some(), "the job card");
    let line = toast_part(&run, TOAST_LINE);
    assert_eq!(
        run.ui_of(Window::Toast).info(line).map(|n| n.text),
        Some("Cleanup: 40 %")
    );
}

#[test]
#[ignore = "slice 6c: lands with the code"]
fn the_toast_is_a_named_group_with_its_title_bar_and_dismiss() {
    let mut run = Run::accessible(Model::new()).job_running();
    let tree = run.a11y_tree_of(Window::Toast);
    let toast = run.part_in(Window::Toast, TOAST).expect("the toast");
    let node = run.ak_in(Window::Toast, &tree, toast).expect("listed");
    assert_eq!(node.role(), AkRole::Group);
    assert_eq!(node.label(), Some("Job status"));
    let bar = run
        .ak_in(Window::Toast, &tree, toast_part(&run, TOAST_BAR))
        .expect("the bar");
    assert_eq!(bar.role(), AkRole::ProgressIndicator);
    assert_eq!(bar.label(), Some(JOB_NAME));
    assert_eq!(bar.numeric_value(), Some(40.0));
    let dismiss = run
        .ak_in(Window::Toast, &tree, toast_part(&run, TOAST_DISMISS))
        .expect("Dismiss");
    assert_eq!(dismiss.role(), AkRole::Button);
    assert_eq!(dismiss.label(), Some("Dismiss"));
    // The window is the toast's width.
    assert!((run.ui_of(Window::Toast).viewport().w - toast::WIDTH).abs() < 0.5);
}

#[test]
#[ignore = "slice 6c: lands with the code"]
fn its_line_is_live_and_two_live_views_of_a_job_never_coexist() {
    let mut run = Run::accessible(Model::new()).job_running();
    let toast_tree = run.a11y_tree_of(Window::Toast);
    let line = run
        .ak_in(Window::Toast, &toast_tree, toast_part(&run, TOAST_LINE))
        .expect("the toast's line");
    assert_eq!(line.live(), Some(Live::Polite));
    let page_tree = run.a11y_tree_of(Window::Page);
    let card_line = run
        .ak(&page_tree, card_part(&run, JOB_LINE))
        .expect("the card's line");
    assert_eq!(card_line.live(), None, "the toast's is the live one");
    // Dismissed, the job goes on: its card's line is now the live one.
    run.step(Step::Update(Msg::ToastDismiss));
    assert!(!run.win.toast_open);
    let page_tree = run.a11y_tree_of(Window::Page);
    let card_line = run
        .ak(&page_tree, card_part(&run, JOB_LINE))
        .expect("the card's line");
    assert_eq!(card_line.live(), Some(Live::Polite));
}

#[test]
#[ignore = "slice 6c: lands with the code"]
fn a_progress_event_damages_only_the_two_bars() {
    let mut run = running();
    run.step(Step::Update(Msg::Progress(41)));
    let card_bar = card_part(&run, JOB_BAR);
    let toast_bar = toast_part(&run, TOAST_BAR);
    assert_eq!(run.damage.len(), 1, "page: {:?}", run.damage);
    assert!(covers(&run.damage[0], &run.ui, card_bar));
    let bar = run.ui.info(card_bar).expect("the card's bar").rect;
    #[allow(clippy::cast_precision_loss, reason = "small pixel counts")]
    let area = |r: &RectPx| ((r.x1 - r.x0) * (r.y1 - r.y0)) as f32;
    assert!(
        area(&run.damage[0]) <= (bar.w + 2.0) * (bar.h + 2.0),
        "the bar alone: {:?} for {bar:?}",
        run.damage
    );
    assert_eq!(run.win.toast_damage.len(), 1, "{:?}", run.win.toast_damage);
    assert!(covers(
        &run.win.toast_damage[0],
        run.ui_of(Window::Toast),
        toast_bar
    ));
    // The line changes only when the caller brings it up to date (at most every 2 s).
    let toast_line = toast_part(&run, TOAST_LINE);
    assert_eq!(
        run.ui_of(Window::Toast).info(toast_line).map(|n| n.text),
        Some("Cleanup: 40 %")
    );
    run.step(Step::Update(Msg::JobLine));
    assert_eq!(
        run.ui_of(Window::Toast).info(toast_line).map(|n| n.text),
        Some("Cleanup: 41 %")
    );
}

#[test]
#[ignore = "slice 6c: lands with the code"]
fn esc_or_dismiss_hides_it_and_the_job_goes_on() {
    let mut run = running();
    run.step(esc());
    assert_eq!(run.model.last, Some(Msg::ToastDismiss));
    assert!(!run.win.toast_open);
    assert!(run.model.over.job.is_some(), "the job goes on");
    let mut run = running();
    let at = run.center_in(Window::Toast, toast_part(&run, TOAST_DISMISS));
    run.step(Step::Toast(Input::PointerDown(at)));
    run.step(Step::Toast(Input::PointerUp(at)));
    assert_eq!(run.model.last, Some(Msg::ToastDismiss));
    assert!(!run.win.toast_open);
}

#[test]
fn a_running_toast_never_hides_by_itself() {
    let mut run = running();
    run.settle();
    assert_eq!(run.wake(), None, "no hide wake while it runs");
    run.step(Step::Tick(u64::from(MESSAGE_MS) * 3));
    assert!(run.win.toast_open);
}

#[test]
#[ignore = "slice 6c: lands with the code"]
fn an_ended_toast_hides_after_the_message_duration_in_one_wake() {
    let mut run = running();
    run.step(Step::Update(Msg::JobEnd(ToastTone::Done)));
    run.settle();
    assert_eq!(run.wake(), Some(u64::from(MESSAGE_MS)), "one wake");
    run.step(Step::Tick(u64::from(MESSAGE_MS) - FRAME_MS));
    assert!(run.win.toast_open, "not yet");
    run.step(Step::Tick(FRAME_MS));
    assert_eq!(run.model.last, Some(Msg::ToastDismiss));
    assert!(!run.win.toast_open, "hidden");
    run.settle();
    assert_eq!(run.wake(), None, "then nothing");
    assert!(!run.ui.wants_frame());
}

#[test]
#[ignore = "slice 6c: lands with the code"]
fn the_pointer_over_an_ended_toast_holds_it() {
    let mut run = running();
    run.step(Step::Update(Msg::JobEnd(ToastTone::Failed)));
    run.settle();
    let at = run.center_in(
        Window::Toast,
        run.part_in(Window::Toast, TOAST).expect("the toast"),
    );
    run.step(Step::Toast(Input::PointerMove(at)));
    assert_eq!(run.wake(), None, "held while the pointer is over it");
    run.step(Step::Tick(u64::from(MESSAGE_MS) * 2));
    assert!(run.win.toast_open);
    run.step(Step::Toast(Input::PointerLeave));
    run.settle();
    assert_eq!(
        run.wake(),
        Some(u64::from(MESSAGE_MS)),
        "its wait starts again"
    );
}

#[test]
#[ignore = "slice 6c: lands with the code"]
fn windows_message_duration_sets_the_wait() {
    let mut run = running();
    run.win.toast.set_message_duration(9_000);
    run.step(Step::Update(Msg::JobEnd(ToastTone::Warning)));
    run.settle();
    assert_eq!(run.wake(), Some(9_000));
}

#[test]
#[ignore = "slice 6c: lands with the code"]
fn an_ended_toast_shows_its_tone_and_actions() {
    let mut run = running();
    run.step(Step::Update(Msg::JobEnd(ToastTone::Done)));
    let line = toast_part(&run, TOAST_LINE);
    assert_eq!(
        run.ui_of(Window::Toast).info(line).map(|n| n.text),
        Some("Cleanup finished.")
    );
    assert!(
        run.part_in(Window::Toast, TOAST_BAR).is_none(),
        "no bar once it ended"
    );
    assert!(run.find(&[PAGE, JOB]).is_none(), "the card left");
    let actions = run
        .part_in(Window::Toast, dpro_ui::composite::TOAST_ACTIONS)
        .expect("View and Undo");
    assert_eq!(run.ui_of(Window::Toast).children(actions).count(), 2);
}
