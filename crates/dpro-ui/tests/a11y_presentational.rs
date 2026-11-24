//! Presentational children (TD-0009 A3.5 addition 9), its accessibility rule: the children of a
//! check box, and of a button given an explicit name, are left out of the tree, so the control is
//! read once. A button that isn't named keeps its children, and a glyph alone is hidden unless
//! named.

#![allow(missing_docs)]

use dpro_ui::{
    Element, FakeText, Icon, Key, Motion, Size, Tri, Ui,
    accesskit::{Node, Role, TreeUpdate},
    tokens,
};

fn node<'t>(ui: &Ui<u8>, tree: &'t TreeUpdate, path: &[Key]) -> Option<&'t Node> {
    let id = ui.find(path)?;
    tree.nodes
        .iter()
        .find(|(ak, _)| ui.a11y_node(*ak) == Some(id))
        .map(|(_, n)| n)
}

fn built() -> Ui<u8> {
    let mut ui = Ui::new(
        Size { w: 600.0, h: 400.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    );
    ui.build(|cx| {
        cx.add_with(
            Element::toggle(Key::name("check"), "Temporary files", Tri::On, 1),
            |cx| {
                cx.add(Element::text(Key::name("name"), "Temporary files"));
                cx.add(Element::text(Key::name("what"), "Files apps left behind"));
            },
        );
        cx.add_with(
            Element::button(Key::name("named"), "", 2).name("Dismiss"),
            |cx| {
                cx.add(Element::glyph(Key::name("glyph"), Icon::Cancel));
            },
        );
        cx.add(Element::glyph(Key::name("decor"), Icon::Info));
        cx.add(Element::glyph(Key::name("image"), Icon::Warning).name("Warning"));
    });
    ui.layout(&mut FakeText);
    ui
}

#[test]
fn a_check_boxs_children_are_left_out_of_the_tree() {
    let mut ui = built();
    let tree = ui.a11y_tree();
    let check = node(&ui, &tree, &[Key::name("check")]).expect("the check box");
    assert_eq!(check.role(), Role::CheckBox);
    assert_eq!(check.label(), Some("Temporary files"));
    assert_eq!(check.children(), &[]);
    assert!(node(&ui, &tree, &[Key::name("check"), Key::name("what")]).is_none());
}

#[test]
fn a_named_buttons_children_are_left_out_of_the_tree() {
    let mut ui = built();
    let tree = ui.a11y_tree();
    let button = node(&ui, &tree, &[Key::name("named")]).expect("the button");
    assert_eq!(button.label(), Some("Dismiss"));
    assert_eq!(button.children(), &[]);
}

#[test]
fn an_unnamed_glyph_is_hidden() {
    let mut ui = built();
    let tree = ui.a11y_tree();
    let decor = node(&ui, &tree, &[Key::name("decor")]);
    assert!(
        decor.is_none_or(Node::is_hidden),
        "a decoration a client can see: {decor:?}"
    );
}

#[test]
fn a_named_glyph_is_an_image() {
    let mut ui = built();
    let tree = ui.a11y_tree();
    let image = node(&ui, &tree, &[Key::name("image")]).expect("the image");
    assert_eq!(image.role(), Role::Image);
    assert_eq!(image.label(), Some("Warning"));
    assert!(!image.is_hidden());
}
