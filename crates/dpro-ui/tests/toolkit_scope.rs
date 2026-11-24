//! Scopes (TD-0009 A3.5 addition 10): the Esc the focused control doesn't use goes to the nearest
//! enclosing scope that has an escape, or, with focus outside every scope, to the window's open
//! scope; when a scope goes, focus goes back to the node focused when it appeared, or, if that's
//! gone, to the scope's parent's first control. Focus the user moved out of the scope stays.

#![allow(missing_docs)]
#![expect(
    clippy::expect_used,
    reason = "test helpers: a node that isn't there fails the test"
)]

use dpro_ui::{
    Element, FakeText, Input, Key, KeyCode, KeyEvent, Mods, Motion, NodeId, Size, TextEdit, Ui,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Msg {
    Press(u8),
    Escape(u8),
    Edit,
}

const OUTSIDE: Key = Key::name("outside");
const OTHER: Key = Key::name("other");
const SCOPE: Key = Key::name("scope");
const INNER: Key = Key::name("inner");
const BUTTON: Key = Key::name("button");
const FIELD: Key = Key::name("field");

/// What one build shows.
#[derive(Clone, Copy, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent parts of a test view"
)]
struct View {
    /// The scope is there.
    open: bool,
    /// The inner scope, inside it, with an escape or not.
    inner: Option<bool>,
    /// A field in the scope instead of a button.
    field: bool,
    /// The button outside the scope is there.
    outside: bool,
    /// The button in the scope has a tooltip.
    tip: bool,
}

fn ui() -> Ui<Msg> {
    let mut ui = Ui::new(Size { w: 400.0, h: 300.0 }, 1.0, Motion::STANDARD);
    ui.set_active(true);
    ui
}

fn build(ui: &mut Ui<Msg>, v: View) {
    ui.build(|cx| {
        if v.outside {
            cx.add(Element::button(OUTSIDE, "Outside", Msg::Press(0)));
        }
        cx.add(Element::button(OTHER, "Other", Msg::Press(9)));
        if v.open {
            cx.add_with(
                Element::scope(SCOPE).on_escape(|| Msg::Escape(1)),
                |cx| match v.inner {
                    Some(escapes) => {
                        let inner = Element::scope(INNER);
                        let inner = if escapes {
                            inner.on_escape(|| Msg::Escape(2))
                        } else {
                            inner
                        };
                        cx.add_with(inner, |cx| {
                            cx.add(Element::button(BUTTON, "In", Msg::Press(2)));
                        });
                    }
                    None if v.field => {
                        cx.add(Element::field(FIELD, "", |_: TextEdit| Msg::Edit));
                    }
                    None => {
                        let button = Element::button(BUTTON, "In", Msg::Press(1));
                        cx.add(if v.tip { button.tooltip("Tip") } else { button });
                    }
                },
            );
        }
    });
    ui.layout(&mut FakeText);
}

fn esc(ui: &mut Ui<Msg>) -> (bool, Vec<Msg>) {
    let mut out = Vec::new();
    let used = ui.dispatch(
        Input::Key(KeyEvent {
            key: KeyCode::Escape,
            down: true,
            mods: Mods::default(),
        }),
        &mut out,
    );
    (used, out)
}

fn key(ui: &mut Ui<Msg>, key: KeyCode) {
    ui.dispatch(
        Input::Key(KeyEvent {
            key,
            down: true,
            mods: Mods::default(),
        }),
        &mut Vec::new(),
    );
}

fn find(ui: &Ui<Msg>, path: &[Key]) -> NodeId {
    ui.find(path).expect("the node")
}

#[test]
fn without_a_scope_esc_passes_on() {
    let mut ui = ui();
    build(&mut ui, View::default());
    assert_eq!(esc(&mut ui), (false, vec![]));
}

#[test]
fn esc_goes_to_the_nearest_scope_with_an_escape() {
    for (escapes, want) in [(true, Msg::Escape(2)), (false, Msg::Escape(1))] {
        let mut ui = ui();
        let v = View {
            open: true,
            inner: Some(escapes),
            ..View::default()
        };
        build(&mut ui, v);
        ui.set_focus(Some(find(&ui, &[SCOPE, INNER, BUTTON])));
        assert_eq!(esc(&mut ui), (true, vec![want]), "inner escapes: {escapes}");
    }
}

#[test]
fn with_focus_outside_esc_goes_to_the_open_scope() {
    let mut ui = ui();
    build(
        &mut ui,
        View {
            open: true,
            ..View::default()
        },
    );
    ui.set_focus(Some(find(&ui, &[OTHER])));
    assert_eq!(esc(&mut ui), (true, vec![Msg::Escape(1)]));
    // And with nothing focused.
    ui.set_focus(None);
    assert_eq!(esc(&mut ui), (true, vec![Msg::Escape(1)]));
}

#[test]
fn an_esc_that_hides_a_tooltip_goes_no_further() {
    let mut ui = ui();
    build(
        &mut ui,
        View {
            open: true,
            tip: true,
            ..View::default()
        },
    );
    // Tab to the scope's button: focus the keyboard moved shows its tip at once.
    ui.set_focus(Some(find(&ui, &[OTHER])));
    key(&mut ui, KeyCode::Tab);
    assert_eq!(ui.focus(), Some(find(&ui, &[SCOPE, BUTTON])));
    ui.tooltip_timer(0);
    assert!(ui.tooltip().is_some(), "its tip shows");
    // The topmost popup first (§4.3): the tip goes, the scope stays.
    assert_eq!(esc(&mut ui), (false, vec![]));
    assert!(ui.tooltip().is_none());
    assert_eq!(esc(&mut ui), (true, vec![Msg::Escape(1)]));
}

#[test]
fn the_esc_a_field_doesnt_use_reaches_its_scope() {
    let mut ui = ui();
    build(
        &mut ui,
        View {
            open: true,
            field: true,
            ..View::default()
        },
    );
    ui.set_focus(Some(find(&ui, &[SCOPE, FIELD])));
    assert_eq!(esc(&mut ui), (true, vec![Msg::Escape(1)]));
}

#[test]
fn a_scope_that_goes_gives_focus_back() {
    let mut ui = ui();
    let closed = View {
        outside: true,
        ..View::default()
    };
    build(&mut ui, closed);
    let outside = find(&ui, &[OUTSIDE]);
    ui.set_focus(Some(outside));
    build(
        &mut ui,
        View {
            open: true,
            ..closed
        },
    );
    ui.set_focus(Some(find(&ui, &[SCOPE, BUTTON])));
    build(&mut ui, closed);
    assert_eq!(
        ui.focus(),
        Some(outside),
        "back where it was when the scope appeared"
    );
}

#[test]
fn when_that_node_is_gone_focus_goes_to_the_scopes_parent() {
    let mut ui = ui();
    build(
        &mut ui,
        View {
            outside: true,
            ..View::default()
        },
    );
    ui.set_focus(Some(find(&ui, &[OUTSIDE])));
    build(
        &mut ui,
        View {
            open: true,
            outside: true,
            ..View::default()
        },
    );
    ui.set_focus(Some(find(&ui, &[SCOPE, BUTTON])));
    // The node it would go back to goes in the same build.
    build(&mut ui, View::default());
    assert_eq!(
        ui.focus(),
        Some(find(&ui, &[OTHER])),
        "the parent's (the window's) first control"
    );
}

#[test]
fn focus_moved_out_of_a_scope_stays_where_it_went() {
    let mut ui = ui();
    let closed = View {
        outside: true,
        ..View::default()
    };
    build(&mut ui, closed);
    ui.set_focus(Some(find(&ui, &[OUTSIDE])));
    build(
        &mut ui,
        View {
            open: true,
            ..closed
        },
    );
    let other = find(&ui, &[OTHER]);
    ui.set_focus(Some(other));
    build(&mut ui, closed);
    assert_eq!(ui.focus(), Some(other));
}

#[test]
fn an_inner_scope_that_goes_alone_gives_focus_back_inside_the_outer() {
    let mut ui = ui();
    // The outer scope, its button focused; then the inner one appears and takes focus.
    build(
        &mut ui,
        View {
            open: true,
            ..View::default()
        },
    );
    let button = find(&ui, &[SCOPE, BUTTON]);
    ui.set_focus(Some(button));
    build(
        &mut ui,
        View {
            open: true,
            inner: Some(true),
            ..View::default()
        },
    );
    ui.set_focus(Some(find(&ui, &[SCOPE, INNER, BUTTON])));
    // The inner goes; the outer's button is a new node (another key path), so the outer's first
    // control takes focus.
    build(
        &mut ui,
        View {
            open: true,
            ..View::default()
        },
    );
    let focus = ui.focus().expect("focus came back");
    assert_eq!(focus, find(&ui, &[SCOPE, BUTTON]));
}
