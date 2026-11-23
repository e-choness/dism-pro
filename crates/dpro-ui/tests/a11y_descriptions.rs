//! Descriptions and shortcuts (TD-0009 A3.5 addition 4, A4.4, A4.5, A4.9), slice 7a: a description
//! wins over a tooltip, a tooltip alone is the description (A1's rule, kept), a hold button says how
//! to hold, and a command button carries its accelerator.

#![allow(missing_docs)]

#[path = "../benches/fixtures/shell.rs"]
mod shell;

use dpro_ui::{
    Element, FakeText, Key, Motion, Size, Ui,
    accesskit::{Node, TreeUpdate},
    tokens,
};
use shell::{DRAWER_KEYS, DRAWER_TIP, HOLD_DESCRIPTION, Msg, Run, Shell, Step};

/// The node at `path` as `tree` has it.
fn node<'t, M>(ui: &Ui<M>, tree: &'t TreeUpdate, path: &[Key]) -> Option<&'t Node> {
    let id = ui.find(path)?;
    tree.nodes
        .iter()
        .find(|(ak, _)| ui.a11y_node(*ak) == Some(id))
        .map(|(_, n)| n)
}

fn small() -> Ui<u8> {
    Ui::new(
        Size { w: 400.0, h: 300.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    )
}

#[test]
fn a_tooltip_alone_is_the_description() {
    let mut run = Run::accessible(Shell::new());
    let tree = run.ui.a11y_tree();
    let button = node(&run.ui, &tree, &Run::DRAWER_BUTTON).expect("the drawer button");
    assert_eq!(button.description(), Some(DRAWER_TIP));
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn a_description_wins_over_a_tooltip_and_a_hold_says_how_to_hold() {
    let mut run = Run::accessible(Shell::new());
    run.step(Step::Update(Msg::Drawer));
    let tree = run.ui.a11y_tree();
    let apply = node(&run.ui, &tree, &Run::APPLY).expect("Hold to apply");
    assert_eq!(apply.description(), Some(HOLD_DESCRIPTION));
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn a_description_without_a_tooltip_shows_no_tooltip() {
    let mut ui = small();
    ui.build(|cx| {
        cx.add(Element::button(Key::name("b"), "Clean", 1).describe("Frees 5.6 GB"));
    });
    ui.layout(&mut FakeText);
    let tree = ui.a11y_tree();
    let b = node(&ui, &tree, &[Key::name("b")]).expect("the button");
    assert_eq!(b.description(), Some("Frees 5.6 GB"));
    // Focus by keyboard shows a tooltip at once for a node that has one (A1); a description
    // isn't one.
    ui.set_focus(ui.find(&[Key::name("b")]));
    assert_eq!(ui.tooltip(), None);
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn a_command_button_carries_its_accelerator() {
    let mut run = Run::accessible(Shell::new());
    let tree = run.ui.a11y_tree();
    let button = node(&run.ui, &tree, &Run::DRAWER_BUTTON).expect("the drawer button");
    assert_eq!(button.keyboard_shortcut(), Some(DRAWER_KEYS));
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn a_description_change_is_in_the_next_update() {
    let mut ui = small();
    let build = |ui: &mut Ui<u8>, d: &str| {
        ui.build(|cx| {
            cx.add(Element::button(Key::name("b"), "Clean", 1).describe(d));
        });
        ui.layout(&mut FakeText);
    };
    build(&mut ui, "Frees 5.6 GB");
    drop(ui.a11y_tree());
    build(&mut ui, "Frees 5.7 GB");
    let update = ui.a11y_update().expect("an update");
    let b = node(&ui, &update, &[Key::name("b")]).expect("the button is sent");
    assert_eq!(b.description(), Some("Frees 5.7 GB"));
    build(&mut ui, "Frees 5.7 GB");
    assert!(ui.a11y_update().is_none());
}
