//! Slice 6a's new groups' input (TD-0009 A3.5 additions 7 and 8).
//!
//! A toolbar is one Tab stop; Left and Right move through its items without wrapping, Home and End
//! go to its ends. Suggestions never take focus: the field keeps it while the pointer hovers an
//! option, a click sends the option's message, and Tab skips the list.

#![allow(missing_docs)]

use dpro_ui::{
    Element, FakeText, Input, Key, KeyCode, KeyEvent, Mods, Motion, Size, TextEdit, Ui, tokens,
};

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Button(u8),
    Edit(TextEdit),
    Pick(u8),
}

const BEFORE: Key = Key::name("before");
const BAR: Key = Key::name("bar");
const AFTER: Key = Key::name("after");
const FIELD: Key = Key::name("field");
const LIST: Key = Key::name("results");

fn built() -> Ui<Msg> {
    let mut ui = Ui::new(
        Size { w: 600.0, h: 400.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    );
    ui.set_active(true);
    ui.build(|cx| {
        cx.add(Element::button(BEFORE, "Before", Msg::Button(9)));
        cx.add_with(Element::toolbar(BAR).name("Selection"), |cx| {
            for i in 0..3 {
                cx.add(Element::button(
                    Key::index(u64::from(i)),
                    "Action",
                    Msg::Button(i),
                ));
            }
        });
        cx.add(Element::field(FIELD, "cle", Msg::Edit).name("Search"));
        cx.add_with(Element::suggestions(LIST).name("Results"), |cx| {
            for i in 0..3 {
                cx.add(Element::option(
                    Key::index(u64::from(i)),
                    "Clean",
                    i == 0,
                    Msg::Pick(i),
                ));
            }
        });
        cx.add(Element::button(AFTER, "After", Msg::Button(8)));
    });
    ui.layout(&mut FakeText);
    ui
}

fn press(ui: &mut Ui<Msg>, key: KeyCode, mods: Mods) -> Vec<Msg> {
    let mut out = Vec::new();
    ui.dispatch(
        Input::Key(KeyEvent {
            key,
            down: true,
            mods,
        }),
        &mut out,
    );
    out
}

fn item(i: u64) -> [Key; 2] {
    [BAR, Key::index(i)]
}

#[test]
fn a_toolbar_is_one_tab_stop() {
    let mut ui = built();
    ui.set_focus(ui.find(&[BEFORE]));
    press(&mut ui, KeyCode::Tab, Mods::default());
    assert_eq!(
        ui.focus(),
        ui.find(&item(0)),
        "Tab enters on the first item"
    );
    press(&mut ui, KeyCode::Tab, Mods::default());
    assert_eq!(ui.focus(), ui.find(&[FIELD]), "one Tab leaves the toolbar");
}

#[test]
fn left_and_right_move_without_wrapping_and_home_and_end_go_to_the_ends() {
    let mut ui = built();
    ui.set_focus(ui.find(&item(0)));
    press(&mut ui, KeyCode::Left, Mods::default());
    assert_eq!(ui.focus(), ui.find(&item(0)), "no wrap at the start");
    press(&mut ui, KeyCode::Right, Mods::default());
    assert_eq!(ui.focus(), ui.find(&item(1)));
    press(&mut ui, KeyCode::End, Mods::default());
    assert_eq!(ui.focus(), ui.find(&item(2)));
    press(&mut ui, KeyCode::Right, Mods::default());
    assert_eq!(ui.focus(), ui.find(&item(2)), "no wrap at the end");
    press(&mut ui, KeyCode::Home, Mods::default());
    assert_eq!(ui.focus(), ui.find(&item(0)));
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn tab_skips_the_suggestions() {
    let mut ui = built();
    ui.set_focus(ui.find(&[FIELD]));
    press(&mut ui, KeyCode::Tab, Mods::default());
    assert_eq!(ui.focus(), ui.find(&[AFTER]), "the list is no Tab stop");
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn hovering_and_clicking_a_suggestion_leave_focus_in_the_field() {
    let mut ui = built();
    let field = ui.find(&[FIELD]);
    ui.set_focus(field);
    let option = ui
        .find(&[LIST, Key::index(1)])
        .and_then(|id| ui.info(id))
        .map(|n| n.rect.center())
        .expect("an option");
    let mut out = Vec::new();
    ui.dispatch(Input::PointerMove(option), &mut out);
    assert_eq!(ui.focus(), field, "hover only paints");
    ui.dispatch(Input::PointerDown(option), &mut out);
    ui.dispatch(Input::PointerUp(option), &mut out);
    assert_eq!(out, [Msg::Pick(1)]);
    assert_eq!(ui.focus(), field, "a click keeps focus in the field");
}
