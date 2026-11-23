//! A field's step (TD-0009 A3.5 addition 6): a field given `.step()` sends it on Up (`true`, more)
//! and Down (`false`, less) instead of passing them on; Page Up and Page Down still pass on; a field
//! without one is unchanged, and a combo's step works as before (Down is `true`, the next value).

#![allow(missing_docs)]

use dpro_ui::{
    Element, FakeText, Input, Key, KeyCode, KeyEvent, Mods, Motion, Size, TextEdit, Ui, tokens,
};

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Edit(TextEdit),
    Step(bool),
    Combo,
}

const FIELD: Key = Key::name("days");

fn built(step: bool) -> Ui<Msg> {
    let mut ui = Ui::new(
        Size { w: 400.0, h: 200.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    );
    ui.set_active(true);
    ui.build(|cx| {
        let mut f = Element::field(FIELD, "30", Msg::Edit).name("Days");
        if step {
            f = f.step(Msg::Step);
        }
        cx.add(f);
        cx.add(
            Element::combo(Key::name("combo"), "Keep", "30 days", false, Msg::Combo)
                .step(Msg::Step),
        );
    });
    ui.layout(&mut FakeText);
    ui.set_focus(ui.find(&[FIELD]));
    ui
}

fn press(ui: &mut Ui<Msg>, key: KeyCode) -> Vec<Msg> {
    let mut out = Vec::new();
    ui.dispatch(
        Input::Key(KeyEvent {
            key,
            down: true,
            mods: Mods::default(),
        }),
        &mut out,
    );
    out
}

#[test]
fn up_and_down_send_a_fields_step() {
    let mut ui = built(true);
    assert_eq!(press(&mut ui, KeyCode::Up), [Msg::Step(true)]);
    assert_eq!(press(&mut ui, KeyCode::Down), [Msg::Step(false)]);
    assert_eq!(ui.focus(), ui.find(&[FIELD]), "focus stays");
}

#[test]
fn page_up_and_page_down_dont_step() {
    let mut ui = built(true);
    for key in [KeyCode::PageUp, KeyCode::PageDown] {
        let sent = press(&mut ui, key);
        assert!(
            !sent.iter().any(|m| matches!(m, Msg::Step(_))),
            "{key:?} sent {sent:?}"
        );
    }
}

#[test]
fn a_field_without_a_step_sends_none() {
    let mut ui = built(false);
    for key in [KeyCode::Up, KeyCode::Down] {
        let sent = press(&mut ui, key);
        assert!(
            !sent.iter().any(|m| matches!(m, Msg::Step(_))),
            "{key:?} sent {sent:?}"
        );
    }
}

#[test]
fn a_combos_step_still_works() {
    let mut ui = built(true);
    ui.set_focus(ui.find(&[Key::name("combo")]));
    // A combo steps to the next value on Down: `true` is forward, as Up is on a field.
    assert_eq!(press(&mut ui, KeyCode::Down), [Msg::Step(true)]);
    assert_eq!(press(&mut ui, KeyCode::Up), [Msg::Step(false)]);
}

#[test]
fn a_step_is_used_and_ctrl_passes_up_and_down_on() {
    let mut ui = built(true);
    let mut out = Vec::new();
    let up = |mods| {
        Input::Key(KeyEvent {
            key: KeyCode::Up,
            down: true,
            mods,
        })
    };
    assert!(ui.dispatch(up(Mods::default()), &mut out), "a step is used");
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    assert!(!ui.dispatch(up(ctrl), &mut out), "Ctrl+Up goes on");
    assert_eq!(out, [Msg::Step(true)]);
    assert_eq!(ui.info(ui.find(&[FIELD]).unwrap()).unwrap().text, "30");
}

/// Rebuilds the field with or without a step and a tooltip: the step shares the tooltip's side
/// slot, so each change must keep or drop the right one.
fn rebuild(ui: &mut Ui<Msg>, step: bool, tip: &str) {
    ui.build(|cx| {
        let mut f = Element::field(FIELD, "30", Msg::Edit)
            .name("Days")
            .tooltip(tip);
        if step {
            f = f.step(Msg::Step);
        }
        cx.add(f);
    });
    ui.layout(&mut FakeText);
}

#[test]
fn the_step_follows_the_view_as_a_tooltip_comes_and_goes() {
    let mut ui = built(false);
    let field = ui.find(&[FIELD]);
    for (step, tip) in [
        (true, "1 to 365"),
        (true, ""),
        (false, "1 to 365"),
        (true, "1 to 365"),
        (false, ""),
        (true, ""),
        (false, ""),
    ] {
        rebuild(&mut ui, step, tip);
        assert_eq!(ui.find(&[FIELD]), field, "the same node");
        assert_eq!(ui.focus(), field);
        let want: &[Msg] = if step { &[Msg::Step(true)] } else { &[] };
        assert_eq!(
            press(&mut ui, KeyCode::Up),
            want,
            "step {step}, tip {tip:?}"
        );
    }
}

#[test]
fn a_stepped_field_gives_its_slot_back_when_it_goes() {
    // Its side slot has no popup state to find it by: freeing the node must still release it (a
    // debug assertion checks every freed node's slot).
    let mut ui = built(true);
    for _ in 0..3 {
        ui.build(|_| {});
        ui.layout(&mut FakeText);
        rebuild(&mut ui, true, "");
        ui.set_focus(ui.find(&[FIELD]));
        assert_eq!(press(&mut ui, KeyCode::Down), [Msg::Step(false)]);
    }
}
