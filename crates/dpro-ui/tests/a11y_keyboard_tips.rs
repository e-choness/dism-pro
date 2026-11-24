//! TD-0009 A4.4 and A4.10 item 5 (slice 7c): a tooltip the keyboard showed stays, as WCAG 2.2's
//! 1.4.13 Content on Hover or Focus asks, on the shell fixture and its [`Run`].
//!
//! - It has no autopop, and asks for no wake while it shows (U6).
//! - It goes with a key (Esc among them), the window going inactive, or the focus leaving its
//!   node with no key (a client's `SetFocus`, the app).
//! - The pointer resting on its node keeps it as it is; the pointer reaching another node with a
//!   tip shows that one, which autopops as a pointer's tip does.
//! - A menu item's tip shown by the keyboard stays too.
//!
//! A1's tooltip rules (the delay, autopop for the pointer, reshow, hiding on a press) are
//! `popups.rs`'s.

#![expect(
    clippy::expect_used,
    reason = "test helpers: a node that isn't there fails the test"
)]

#[path = "../benches/fixtures/shell.rs"]
mod shell;

use dpro_ui::{
    Input, Key, KeyCode, Mods, NodeId,
    tokens::popup::{TIP_DELAY_MS, TIP_SHOWS_FOR},
};
use shell::{DRAWER_TIP, PROPERTIES_TIP, Run, Shell, Step, key, popup_key};

const NONE: Mods = Mods {
    shift: false,
    ctrl: false,
    alt: false,
    win: false,
};

/// Autopop: how long a pointer's tip shows.
fn shows_for() -> u64 {
    u64::from(TIP_DELAY_MS) * u64::from(TIP_SHOWS_FOR)
}

fn id(run: &Run, path: &[Key]) -> NodeId {
    run.ui.find(path).expect("a node at the path")
}

/// The tooltip showing in the main window: its node and text.
fn tip(run: &Run) -> Option<(NodeId, String)> {
    run.ui.tooltip().map(|(id, text)| (id, text.to_owned()))
}

/// The shell, Tab pressed and let go: the drawer button, its first stop, is focused by the
/// keyboard, and its tip shows at once.
fn tabbed(run: Run) -> Run {
    let mut run = run;
    run.step(key(KeyCode::Tab, true, NONE));
    run.step(key(KeyCode::Tab, false, NONE));
    let drawer = id(&run, &Run::DRAWER_BUTTON);
    assert_eq!(run.ui.focus(), Some(drawer));
    assert_eq!(tip(&run), Some((drawer, DRAWER_TIP.to_owned())));
    run
}

#[test]
fn a_keyboard_tip_stays_past_autopop_with_no_wake_until_a_key() {
    let mut run = tabbed(Run::with(Shell::new()));
    assert_eq!(run.wake(), None, "a keyboard tip asks for no wake");
    run.step(Step::Tick(shows_for()));
    run.step(Step::Tick(shows_for()));
    assert!(tip(&run).is_some(), "it stays past autopop");
    run.step(key(KeyCode::Other(0x10), true, NONE));
    assert_eq!(tip(&run), None, "a key hides it");
    assert_eq!(run.wake(), None);
}

#[test]
fn esc_hides_a_keyboard_tip() {
    let mut run = tabbed(Run::with(Shell::new()));
    run.step(Step::Tick(shows_for()));
    assert!(tip(&run).is_some(), "past autopop");
    run.step(key(KeyCode::Escape, true, NONE));
    assert_eq!(tip(&run), None);
}

#[test]
fn a_keyboard_tip_goes_when_the_focus_leaves_its_node_with_no_key() {
    let mut run = tabbed(Run::with(Shell::new()));
    // A client's SetFocus on a row: no key, so only the focus says the tip's node lost it.
    let row = id(&run, &Run::row(4));
    run.ui.set_focus(Some(row));
    assert_eq!(tip(&run), None, "gone at once for whoever asks");
    run.step(Step::Tick(1));
    assert_eq!(tip(&run), None, "and forgotten at the next timer");
    assert_eq!(run.wake(), None);
    // Back on the button, with no key: no tip, as for focus a client moves.
    let drawer = id(&run, &Run::DRAWER_BUTTON);
    run.ui.set_focus(Some(drawer));
    run.step(Step::Tick(1));
    assert_eq!(tip(&run), None);
}

#[test]
fn the_window_going_inactive_hides_a_keyboard_tip() {
    let mut run = tabbed(Run::with(Shell::new()));
    run.ui.set_active(false);
    assert_eq!(tip(&run), None);
}

#[test]
fn the_pointer_on_a_keyboard_tip_s_node_keeps_it_and_another_tip_replaces_it() {
    let mut run = tabbed(Run::with(Shell::new()).drawer_open());
    let drawer = id(&run, &Run::DRAWER_BUTTON);
    // Resting on the button: the keyboard's tip, as it was, past autopop.
    let on_drawer = run.center(&Run::DRAWER_BUTTON);
    run.step(Step::Input(Input::PointerMove(on_drawer)));
    run.step(Step::Tick(shows_for()));
    assert_eq!(tip(&run), Some((drawer, DRAWER_TIP.to_owned())));
    assert_eq!(run.wake(), None);
    // Off it, onto nothing with a tip: still the keyboard's.
    let title = run.center(&[Key::name("header"), Key::name("title")]);
    run.step(Step::Input(Input::PointerMove(title)));
    run.step(Step::Tick(shows_for()));
    assert_eq!(tip(&run).map(|(id, _)| id), Some(drawer));
    // Onto Hold to apply: its tip replaces it, at once as a neighbor's does once a tip just hid
    // (Windows' reshow), and that one autopops.
    let apply = id(&run, &Run::APPLY);
    run.step(Step::Input(Input::PointerMove(run.center(&Run::APPLY))));
    assert_eq!(tip(&run).map(|(id, _)| id), Some(apply));
    assert_eq!(run.wake(), Some(shows_for()), "a pointer's tip autopops");
    run.step(Step::Tick(shows_for()));
    assert_eq!(tip(&run), None);
}

#[test]
fn a_menu_item_s_keyboard_tip_stays_too() {
    let mut run = Run::with(Shell::new());
    let row = id(&run, &Run::row(4));
    run.ui.set_focus(Some(row));
    run.step(Step::Input(Input::ContextMenu(None)));
    assert!(run.popup_open, "Shift+F10 on a row opens its menu");
    run.step(popup_key(KeyCode::End, true, NONE));
    let properties = run.popup.find(&Run::menu_item(4)).expect("Properties");
    let shown = || run.popup.tooltip().map(|(id, text)| (id, text.to_owned()));
    assert_eq!(shown(), Some((properties, PROPERTIES_TIP.to_owned())));
    run.step(Step::Tick(shows_for()));
    run.step(Step::Tick(shows_for()));
    let shown = run.popup.tooltip().map(|(id, _)| id);
    assert_eq!(shown, Some(properties), "it stays past autopop");
    assert_eq!(run.wake(), None);
}
