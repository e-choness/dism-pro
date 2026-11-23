//! What a client sees of slice 6a's new groups (TD-0009 A3.5 additions 7 and 8): a toolbar is a
//! `ToolBar`; suggestions are a list box whose options are list box options, each placed among the
//! options alone (`PositionInSet` and `SizeOfSet` skip a header).

#![allow(missing_docs)]

use dpro_ui::{
    Element, FakeText, Key, Motion, Size, Ui,
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

const BAR: Key = Key::name("bar");
const LIST: Key = Key::name("results");

fn built() -> Ui<u8> {
    let mut ui = Ui::new(
        Size { w: 600.0, h: 400.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    );
    ui.build(|cx| {
        cx.add_with(Element::toolbar(BAR).name("Selection"), |cx| {
            cx.add(Element::button(Key::index(0), "Clean", 1));
            cx.add(Element::button(Key::index(1), "Review", 2));
        });
        cx.add_with(Element::suggestions(LIST).name("Results"), |cx| {
            cx.add(Element::text(Key::name("header"), "Commands"));
            for (i, label) in (0..).zip(["Clean", "Repair", "Packages"]) {
                cx.add(Element::option(Key::index(i), label, i == 0, 10));
            }
        });
    });
    ui.layout(&mut FakeText);
    ui
}

#[test]
fn a_toolbar_is_a_toolbar() {
    let mut ui = built();
    let tree = ui.a11y_tree();
    let bar = node(&ui, &tree, &[BAR]).expect("the toolbar");
    assert_eq!(bar.role(), Role::Toolbar);
    assert_eq!(bar.label(), Some("Selection"));
}

#[test]
fn suggestions_are_a_list_box() {
    let mut ui = built();
    let tree = ui.a11y_tree();
    let list = node(&ui, &tree, &[LIST]).expect("the list");
    assert_eq!(list.role(), Role::ListBox);
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn each_suggestion_is_placed_among_the_options_alone() {
    let mut ui = built();
    let tree = ui.a11y_tree();
    for i in 0..3_u64 {
        let option = node(&ui, &tree, &[LIST, Key::index(i)]).expect("an option");
        assert_eq!(option.role(), Role::ListBoxOption);
        assert_eq!(
            option.position_in_set(),
            Some(usize::try_from(i).expect("small") + 1),
            "option {i}"
        );
        assert_eq!(option.size_of_set(), Some(3), "option {i}");
    }
    let selected: Vec<_> = (0..3_u64)
        .filter(|i| {
            node(&ui, &tree, &[LIST, Key::index(*i)]).and_then(Node::is_selected) == Some(true)
        })
        .collect();
    assert_eq!(selected, [0], "exactly one selected");
}
