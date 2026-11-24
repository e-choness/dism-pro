//! Live regions (TD-0009 A3.5 addition 5, A4.4, A4.9), slice 6a's part: a label's or a named
//! container's politeness reaches a client, `Live::Off` sets nothing, other roles ignore it, and a
//! live node's text change is in the next update. Notices (A4.5) are slice 7b's and join this file
//! with it.

#![allow(missing_docs)]

use dpro_ui::{
    Element, FakeText, Key, Live, Motion, Size, Ui,
    accesskit::{Live as AkLive, Node, TreeUpdate},
    tokens,
};

const STATUS: Key = Key::name("status");
const BOX: Key = Key::name("box");
const BUTTON: Key = Key::name("button");

fn ui() -> Ui<u8> {
    Ui::new(
        Size { w: 400.0, h: 300.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    )
}

/// A status line live as `live`, saying `text`; a named container live as `live`; and a button
/// asked to be live, which it can't be.
fn build(ui: &mut Ui<u8>, text: &str, live: Live) {
    ui.build(|cx| {
        cx.add(Element::text(STATUS, text).live(live));
        cx.add_with(Element::stack(BOX).name("Results").live(live), |cx| {
            cx.add(Element::text(Key::name("inner"), "inside"));
        });
        cx.add(Element::button(BUTTON, "Run", 1).live(live));
    });
    ui.layout(&mut FakeText);
}

/// Node `key` (a root child) as `tree` has it.
fn node<'t>(ui: &Ui<u8>, tree: &'t TreeUpdate, key: Key) -> Option<&'t Node> {
    let id = ui.find(&[key])?;
    tree.nodes
        .iter()
        .find(|(ak, _)| ui.a11y_node(*ak) == Some(id))
        .map(|(_, n)| n)
}

#[test]
fn live_off_sets_nothing() {
    let mut ui = ui();
    build(&mut ui, "Ready", Live::Off);
    let tree = ui.a11y_tree();
    assert!(tree.nodes.iter().all(|(_, n)| n.live().is_none()));
}

#[test]
fn a_label_and_a_named_container_carry_their_politeness_and_a_button_ignores_it() {
    for (live, want) in [
        (Live::Polite, AkLive::Polite),
        (Live::Assertive, AkLive::Assertive),
    ] {
        let mut ui = ui();
        build(&mut ui, "Scanning", live);
        let tree = ui.a11y_tree();
        assert_eq!(node(&ui, &tree, STATUS).and_then(Node::live), Some(want));
        assert_eq!(node(&ui, &tree, BOX).and_then(Node::live), Some(want));
        assert_eq!(node(&ui, &tree, BUTTON).and_then(Node::live), None);
    }
}

#[test]
fn a_live_nodes_text_change_is_in_the_next_update_and_its_politeness_change_too() {
    let mut ui = ui();
    build(&mut ui, "Scanning", Live::Polite);
    drop(ui.a11y_tree());
    build(&mut ui, "Scanned 7 categories", Live::Polite);
    let update = ui.a11y_update().expect("an update");
    let status = node(&ui, &update, STATUS).expect("the status line is sent");
    assert_eq!(
        status.value().or(status.label()),
        Some("Scanned 7 categories")
    );
    assert_eq!(status.live(), Some(AkLive::Polite));
    build(&mut ui, "Scanned 7 categories", Live::Assertive);
    let update = ui.a11y_update().expect("a politeness change is sent");
    assert_eq!(
        node(&ui, &update, STATUS).and_then(Node::live),
        Some(AkLive::Assertive)
    );
    build(&mut ui, "Scanned 7 categories", Live::Assertive);
    assert!(ui.a11y_update().is_none(), "nothing changed, nothing sent");
}
