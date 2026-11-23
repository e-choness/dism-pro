//! The inline error panel (TD-0009 A3.5 (9), A3.9): Details toggles, the excerpt is capped at 8
//! lines and sanitized, the actions come in order, and a client sees a group named by the title,
//! whose title is live when the panel came unbidden.

#![allow(missing_docs)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use composites::{ERROR, Model, Msg, PAGE, Run, Step, key, key_up};
use dpro_ui::{
    Element, FakeText, Key, KeyCode, Live, Mods, Motion, NodeId, Role, Size, Ui,
    accesskit::{Live as AkLive, Role as AkRole},
    composite::{
        ERROR_ACTIONS, ERROR_CODE, ERROR_COPY, ERROR_DETAILS, ERROR_LOG, ERROR_TITLE, ErrorPanel,
        ErrorTone, LogExcerpt,
    },
    tokens::{self, error},
};

#[test]
fn details_toggles_the_code_the_call_and_the_log() {
    let mut run = Run::with(Model::new());
    assert!(run.part(&[PAGE, ERROR], ERROR_CODE).is_none(), "collapsed");
    let details = run.part(&[PAGE, ERROR], ERROR_DETAILS).expect("Details");
    assert_eq!(run.ui.info(details).and_then(|n| n.expanded), Some(false));
    run.ui.set_focus(Some(details));
    run.step(key(KeyCode::Enter, Mods::default()));
    assert_eq!(run.model.last, Some(Msg::Expand));
    assert!(run.part(&[PAGE, ERROR], ERROR_CODE).is_some(), "expanded");
    assert!(
        run.part(&[PAGE, ERROR], ERROR_LOG).is_some(),
        "the log shows"
    );
    let details = run.part(&[PAGE, ERROR], ERROR_DETAILS).expect("Details");
    assert_eq!(run.ui.info(details).and_then(|n| n.expanded), Some(true));
    run.step(key(KeyCode::Space, Mods::default()));
    run.step(key_up(KeyCode::Space));
    assert!(
        run.part(&[PAGE, ERROR], ERROR_CODE).is_none(),
        "collapsed again"
    );
}

#[test]
fn the_excerpt_is_the_logs_last_8_lines() {
    let mut run = Run::with(Model::new());
    run.step(Step::Update(Msg::Expand));
    let log = run.part(&[PAGE, ERROR], ERROR_LOG).expect("the excerpt");
    let text = run.ui.info(log).map(|n| n.text.to_owned()).expect("text");
    assert_eq!(text.lines().count(), error::LOG_LINES, "{text}");
    assert!(text.ends_with("Job 42 ended: partially applied"), "{text}");
    assert!(composites::LOG.ends_with(&text), "the last lines, whole");
}

/// A panel alone, expanded, over `log`.
fn alone(log: &str) -> Ui<u8> {
    let mut ui = Ui::new(
        Size { w: 800.0, h: 600.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    );
    ui.build(|cx| {
        cx.error_panel(
            Key::name("e"),
            &ErrorPanel {
                tone: ErrorTone::Error,
                title: "Couldn't clean",
                code: "Code 0x80070005",
                api: "DeleteFileW",
                next: "Try again as administrator.",
                log: Some(LogExcerpt {
                    source: "job.log",
                    text: log,
                }),
                expanded: true,
                details: ("Details", 0),
                actions: [Some(("Try again", 1)), None],
                copy: ("Copy details", 2),
                live: Live::Off,
            },
        );
        cx.add(Element::text(Key::name("after"), ""));
    });
    ui.layout(&mut FakeText);
    ui
}

/// The first node keyed `key` anywhere in `ui`.
fn part<M>(ui: &Ui<M>, key: Key) -> Option<NodeId> {
    let mut stack = vec![ui.root()];
    while let Some(id) = stack.pop() {
        for c in ui.children(id) {
            if ui.info(c).is_some_and(|n| n.key == key) {
                return Some(c);
            }
            stack.push(c);
        }
    }
    None
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn the_excerpt_is_sanitized() {
    // Control characters and bidirectional overrides from a log never reach the screen or a
    // client: they'd reorder or hide text (TD-0009 §8's field rules).
    let ui = alone("ok\u{1b}[31m red\u{7}\n\u{202e}evil\u{202c} line\r\nlast\u{0}");
    let log = part(&ui, ERROR_LOG)
        .and_then(|id| ui.info(id))
        .map(|n| n.text.to_owned())
        .expect("the excerpt");
    assert!(
        log.chars()
            .all(|c| c == '\n' || !(c.is_control() || ('\u{202a}'..='\u{202e}').contains(&c))),
        "{log:?}"
    );
    assert!(log.contains("red") && log.contains("last"), "{log:?}");
}

#[test]
fn the_actions_come_in_order_then_copy_details() {
    let run = Run::with(Model::new());
    let row = run
        .part(&[PAGE, ERROR], ERROR_ACTIONS)
        .expect("the actions");
    let labels: Vec<String> = run
        .ui
        .children(row)
        .filter_map(|c| run.ui.info(c))
        .filter(|n| n.role == Role::Button)
        .map(|n| n.text.to_owned())
        .collect();
    assert_eq!(labels, ["Try again", "Open log", "Copy details"]);
    let copy = run.part(&[PAGE, ERROR], ERROR_COPY).expect("Copy details");
    assert_eq!(run.ui.info(copy).map(|n| n.text), Some("Copy details"));
}

#[test]
fn a_client_sees_a_group_named_by_the_title() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let panel = run.find(&[PAGE, ERROR]).expect("the panel");
    let node = run.ak(&tree, panel).expect("in the tree");
    assert_eq!(node.role(), AkRole::Group);
    assert_eq!(node.label(), Some("3 of 7 categories couldn't be cleaned"));
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn the_title_is_assertive_when_the_panel_came_unbidden_and_silent_otherwise() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let title = run.part(&[PAGE, ERROR], ERROR_TITLE).expect("the title");
    let node = run.ak(&tree, title).expect("in the tree");
    assert_eq!(node.live(), Some(AkLive::Assertive));
    let mut quiet = alone("one line");
    let tree = quiet.a11y_tree();
    assert!(
        tree.nodes.iter().all(|(_, n)| n.live().is_none()),
        "Live::Off sets nothing"
    );
}
