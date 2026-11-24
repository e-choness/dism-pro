//! The job card (TD-0009 A3.5 (8), A3.9), as Activity's Running section shows a job: a group named
//! by its title, holding its progress bar (named by the title), its elapsed time, Cancel (a Tab
//! stop, disabled and reading `ui-cancelling` once asked) and its line, live only when `live`.

#![expect(
    clippy::expect_used,
    reason = "test helpers: a node that isn't there fails the test"
)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use composites::{JOB, JOB_NAME, Model, Msg, PAGE, Run, Step};
use dpro_ui::{
    Cx, FakeText, Key, KeyCode, KeyEvent, Mods, Motion, Size, Ui,
    accesskit::{Live, Role as AkRole},
    composite::{JOB_BAR, JOB_CANCEL, JOB_ELAPSED, JOB_LINE, JobCard, ToastTone},
};

fn card(run: &Run, key: Key) -> dpro_ui::NodeId {
    run.part(&[PAGE, JOB], key).expect("the job card's part")
}

#[test]
fn it_shows_while_its_job_runs_and_leaves_when_it_ends() {
    let mut run = Run::with(Model::new());
    assert!(run.find(&[PAGE, JOB]).is_none());
    let mut run2 = Run::with(Model::new()).job_running();
    assert!(run2.find(&[PAGE, JOB]).is_some());
    run2.step(Step::Update(Msg::JobEnd(ToastTone::Done)));
    assert!(run2.find(&[PAGE, JOB]).is_none(), "the history has it");
    run.step(Step::Update(Msg::JobStart));
    assert!(run.find(&[PAGE, JOB]).is_some());
}

#[test]
fn a_client_sees_a_group_named_by_its_title_with_a_named_bar() {
    let mut run = Run::accessible(Model::new()).job_running();
    let tree = run.ui.a11y_tree();
    let card_id = run.find(&[PAGE, JOB]).expect("the card");
    let node = run.ak(&tree, card_id).expect("listed");
    assert_eq!(node.role(), AkRole::Group);
    assert_eq!(node.label(), Some(JOB_NAME));
    let bar = run.ak(&tree, card(&run, JOB_BAR)).expect("the bar");
    assert_eq!(bar.role(), AkRole::ProgressIndicator);
    assert_eq!(bar.label(), Some(JOB_NAME));
    assert_eq!(bar.numeric_value(), Some(40.0));
    let elapsed = card(&run, JOB_ELAPSED);
    assert_eq!(run.ui.info(elapsed).map(|n| n.text), Some("0:00"));
}

#[test]
fn cancel_is_a_tab_stop_and_reads_cancelling_once_asked() {
    let mut run = Run::with(Model::new()).job_running();
    let cancel = card(&run, JOB_CANCEL);
    run.ui.set_focus(Some(cancel));
    run.step(Step::Input(dpro_ui::Input::Key(KeyEvent {
        key: KeyCode::Enter,
        down: true,
        mods: Mods::default(),
    })));
    assert_eq!(run.model.last, Some(Msg::JobCancel));
    let cancel = card(&run, JOB_CANCEL);
    let info = run.ui.info(cancel).expect("Cancel");
    assert!(!info.enabled, "disabled while it cancels");
    assert_eq!(info.text, "Cancelling\u{2026}");
}

/// A card alone, `live` as given.
fn alone(live: bool) -> Ui<u8> {
    let mut ui = Ui::new(Size { w: 600.0, h: 300.0 }, 1.0, Motion::default());
    let view = |cx: &mut Cx<'_, u8>| {
        cx.job_card(
            Key::name("card"),
            &JobCard {
                title: "Cleanup",
                elapsed: "0:12",
                progress: 0.5,
                line: "Cleanup: 50 %",
                live,
                cancel: Some(("Cancel", 1)),
                cancelling: false,
            },
        );
    };
    ui.build(view);
    ui.layout(&mut FakeText);
    ui
}

#[test]
fn its_line_is_live_only_when_live() {
    for live in [true, false] {
        let mut ui = alone(live);
        let tree = ui.a11y_tree();
        let line = ui.find(&[Key::name("card")]).and_then(|card| {
            let mut queue = std::collections::VecDeque::from([card]);
            while let Some(id) = queue.pop_front() {
                for c in ui.children(id) {
                    if ui.info(c).is_some_and(|n| n.key == JOB_LINE) {
                        return Some(c);
                    }
                    queue.push_back(c);
                }
            }
            None
        });
        let line = line.expect("the line");
        let node = tree
            .nodes
            .iter()
            .find(|(ak, _)| ui.a11y_node(*ak) == Some(line))
            .map(|(_, n)| n)
            .expect("listed");
        assert_eq!(node.live(), live.then_some(Live::Polite), "live: {live}");
    }
}
