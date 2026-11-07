//! TD-0009 A1.9's T1 checks of popups, on the shell fixture and its [`Run`], which hosts the
//! popup's `Ui` as the platform does: a row's context menu, Settings' Start page dropdown, and
//! tooltips on the header's drawer button, on Hold to apply and on the menu's disabled item.
//!
//! - **Context requests:** from the keyboard (the focused row; the first item focused, with the
//!   ring) and the pointer (the row under it; nothing focused until a key or the pointer moves).
//!   Only a node marked `context` takes one.
//! - **Menu keys:** Up and Down wrap and skip the separator, Home and End, Enter and Space invoke
//!   and close, Esc closes, a disabled item is focused but never invoked, hovering focuses, a
//!   release invokes the item it's on, and a press the menu closing cancels invokes nothing.
//! - **The dropdown:** Up and Down step it closed; Alt+Down, F4, Enter, Space and a click open the
//!   list on the picked option; Esc, F4 and Alt+Up close it; picking sets the value and closes;
//!   Alt+F4 is the window's.
//! - **Tooltips:** the delay, autopop, staying hidden until the pointer leaves, at once on keyboard
//!   focus, hidden by a press or any key, and the reshow delay.
//! - **Accessibility:** roles, names, values, states and descriptions in both trees.
//! - **Paint damage** of a menu's focus move, **24 × 24 targets**, and **no clipping at 225 %**.
//! - **The anchor going away** closes its popup (A1.8), and so does **a wheel over the window**
//!   (A2.4: it may move the anchor).

#![expect(
    clippy::expect_used,
    reason = "test helpers: a node that isn't there fails the test"
)]

#[path = "../benches/fixtures/shell.rs"]
mod shell;

use dpro_ui::{
    FakeText, Input, Key, KeyCode, KeyEvent, Mods, Motion, NodeId, Rect, Role, Size, TextSystem,
    Ui,
    accesskit::{Action, Node, Role as AkRole, TreeUpdate},
    tokens::popup::{ITEM_HEIGHT, ITEM_PAD, TIP_DELAY_MS, TIP_SHOWS_FOR},
};
use shell::{
    DRAWER_TIP, Msg, PROPERTIES_TIP, ROW_MENU, Run, SETTINGS, START_PAGES, Shell, Step, click, key,
    popup_key, popup_view,
};

/// A data row the first page shows: a desktop program (every fourth is a Store app).
const ROW: u64 = 4;

const NONE: Mods = Mods {
    shift: false,
    ctrl: false,
    alt: false,
    win: false,
};

const ALT: Mods = Mods { alt: true, ..NONE };

/// A key pressed and let go in the main window.
fn press(code: KeyCode) -> [Step; 2] {
    [key(code, true, NONE), key(code, false, NONE)]
}

/// A key pressed and let go in the popup.
fn press_in_popup(code: KeyCode) -> [Step; 2] {
    [popup_key(code, true, NONE), popup_key(code, false, NONE)]
}

fn steps(run: &mut Run, steps: impl IntoIterator<Item = Step>) {
    for s in steps {
        run.step(s);
    }
}

fn id(run: &Run, path: &[Key]) -> NodeId {
    run.ui.find(path).expect("a node at the path")
}

fn popup_id(run: &Run, path: &[Key]) -> NodeId {
    run.popup.find(path).expect("a popup node at the path")
}

/// The first page, the row focused (as Tab and the arrows would leave it), then Shift+F10:
/// its menu opens, from the keyboard.
fn menu_from_keyboard() -> Run {
    let mut run = Run::with(Shell::new());
    let row = id(&run, &Run::row(ROW));
    run.ui.set_focus(Some(row));
    run.step(Step::Input(Input::ContextMenu(None)));
    assert!(run.popup_open, "Shift+F10 on a row opens its menu");
    run
}

/// A right-click on the row: its menu opens, from the pointer.
fn menu_from_pointer() -> Run {
    let mut run = Run::with(Shell::new());
    let at = run.center(&Run::row(ROW));
    run.step(Step::Input(Input::ContextMenu(Some(at))));
    assert!(run.popup_open, "a right-click on a row opens its menu");
    run
}

/// Settings, the Start page dropdown focused.
fn on_settings() -> Run {
    let mut run = Run::with(Shell::new());
    run.step(Step::Update(Msg::Page(SETTINGS)));
    let combo = id(&run, &Run::START_COMBO);
    run.ui.set_focus(Some(combo));
    run
}

fn item(run: &Run, i: u64) -> Option<NodeId> {
    run.popup.find(&Run::menu_item(i))
}

#[test]
fn shift_f10_on_a_row_selects_it_and_focuses_the_menu_s_first_item() {
    let run = menu_from_keyboard();
    let row = id(&run, &Run::row(ROW));
    assert_eq!(run.shell.selected(), Some(4), "picked, as a click would");
    assert_eq!(run.popup.focus(), item(&run, 0));
    assert!(
        run.popup.focus_visible(),
        "opened from the keyboard: the ring shows"
    );
    // The main window's focus stays on the anchor, for when the menu closes.
    assert_eq!(run.ui.focus(), Some(row));
    let rect = run.ui.info(row).map(|n| n.rect);
    assert_eq!(run.ui.popup_anchor(), rect, "anchored at the row");
}

#[test]
fn a_right_click_opens_the_menu_at_the_pointer_with_nothing_focused() {
    let mut run = menu_from_pointer();
    let at = run.center(&Run::row(ROW));
    assert_eq!(run.shell.selected(), Some(4));
    assert_eq!(run.ui.popup_anchor(), Some(Rect::new(at.x, at.y, 0.0, 0.0)));
    assert_eq!(
        run.popup.focus(),
        None,
        "nothing until a key or the pointer moves"
    );
    // The first arrow focuses the first item, with the ring.
    steps(&mut run, press_in_popup(KeyCode::Down));
    assert_eq!(run.popup.focus(), item(&run, 0));
    assert!(run.popup.focus_visible());
}

#[test]
fn a_context_request_away_from_a_context_node_does_nothing() {
    let mut run = Run::with(Shell::new());
    let title = run.center(&[Key::name("header"), Key::name("title")]);
    run.step(Step::Input(Input::ContextMenu(Some(title))));
    assert!(!run.popup_open);
    assert_eq!(run.shell.selected(), None);
    // From the keyboard with nothing focused, nor.
    run.step(Step::Input(Input::ContextMenu(None)));
    assert!(!run.popup_open);
}

#[test]
fn menu_keys_wrap_skip_the_separator_and_reach_the_disabled_item() {
    let mut run = menu_from_keyboard();
    let mut seen = Vec::new();
    for _ in 0..ROW_MENU.len() {
        steps(&mut run, press_in_popup(KeyCode::Down));
        seen.push(run.popup.focus());
    }
    let want: Vec<_> = [1, 2, 3, 4, 0].into_iter().map(|i| item(&run, i)).collect();
    assert_eq!(
        seen, want,
        "Down: the separator skipped, the disabled item in, wrapping"
    );
    steps(&mut run, press_in_popup(KeyCode::Up));
    assert_eq!(run.popup.focus(), item(&run, 4), "Up wraps too");
    steps(&mut run, press_in_popup(KeyCode::Home));
    assert_eq!(run.popup.focus(), item(&run, 0));
    steps(&mut run, press_in_popup(KeyCode::End));
    assert_eq!(run.popup.focus(), item(&run, 4));
    // The disabled item: Enter and Space invoke nothing, and the menu stays.
    steps(&mut run, press_in_popup(KeyCode::Enter));
    steps(&mut run, press_in_popup(KeyCode::Space));
    assert!(run.popup_open);
    assert_eq!(run.shell.done(), None);
    assert_eq!(run.popup.focus(), item(&run, 4));
}

#[test]
fn enter_or_space_invokes_the_item_closes_the_menu_and_focus_returns_to_the_row() {
    let mut run = menu_from_keyboard();
    let row = id(&run, &Run::row(ROW));
    steps(&mut run, press_in_popup(KeyCode::Down));
    run.step(popup_key(KeyCode::Enter, true, NONE));
    assert_eq!(run.shell.done(), Some(1));
    assert!(!run.popup_open, "an item's message closes its menu");
    assert_eq!(run.shell.popup(), None);
    assert_eq!(run.ui.focus(), Some(row));
    let said = run
        .ui
        .find(&[
            Key::name("body"),
            Key::name("content"),
            Key::name("toolbar"),
            Key::name("selection"),
        ])
        .and_then(|s| run.ui.info(s))
        .map(|n| n.text.to_owned());
    assert_eq!(said.as_deref(), Some("Opened file location"));
    // Again, with Space: it invokes on the release.
    run.step(Step::Input(Input::ContextMenu(None)));
    assert!(run.popup_open);
    steps(&mut run, press_in_popup(KeyCode::Down));
    steps(&mut run, press_in_popup(KeyCode::Down));
    run.step(popup_key(KeyCode::Space, true, NONE));
    assert!(run.popup_open, "Space invokes on the release");
    run.step(popup_key(KeyCode::Space, false, NONE));
    assert_eq!(run.shell.done(), Some(2));
    assert!(!run.popup_open);
}

#[test]
fn esc_closes_the_menu_and_invokes_nothing() {
    let mut run = menu_from_keyboard();
    let row = id(&run, &Run::row(ROW));
    steps(&mut run, press_in_popup(KeyCode::Down));
    steps(&mut run, press_in_popup(KeyCode::Escape));
    assert!(!run.popup_open);
    assert_eq!(run.shell.done(), None);
    assert_eq!(run.ui.focus(), Some(row));
}

#[test]
fn the_danger_item_opens_the_drawer_and_applies_nothing() {
    let mut run = menu_from_keyboard();
    steps(&mut run, press_in_popup(KeyCode::Enter));
    assert!(run.shell.drawer(), "Uninstall opens the Preview drawer");
    assert_eq!(
        run.shell.applies(),
        0,
        "a menu never applies (A1.10 item 3)"
    );
    assert!(!run.popup_open);
}

#[test]
fn hovering_an_item_focuses_it_and_a_release_invokes_the_item_it_is_on() {
    let mut run = menu_from_pointer();
    let two = run.popup_center(&Run::menu_item(2));
    run.step(Step::Popup(Input::PointerMove(two)));
    assert_eq!(run.popup.focus(), item(&run, 2), "hover focuses");
    assert!(
        !run.popup.focus_visible(),
        "the pointer's focus has no ring"
    );
    // Pressed on one item and released on another: the one it's released on.
    let one = run.popup_center(&Run::menu_item(1));
    let three = run.popup_center(&Run::menu_item(3));
    run.step(Step::Popup(Input::PointerDown(one)));
    run.step(Step::Popup(Input::PointerMove(three)));
    run.step(Step::Popup(Input::PointerUp(three)));
    assert_eq!(run.shell.done(), Some(3));
    assert!(!run.popup_open);
}

/// An item held when Esc closes its menu is never invoked: its release, which may still reach
/// the hidden window, finds no press. Nor is a button held when its window goes inactive.
#[test]
fn a_press_the_popup_closing_or_the_window_going_inactive_cancels_invokes_nothing() {
    let mut run = menu_from_pointer();
    let two = run.popup_center(&Run::menu_item(2));
    run.step(Step::Popup(Input::PointerDown(two)));
    assert!(
        run.popup
            .info(item(&run, 2).expect("item 2"))
            .is_some_and(|n| n.pressed)
    );
    run.step(popup_key(KeyCode::Escape, true, NONE));
    assert!(!run.popup_open);
    let mut out = Vec::new();
    assert!(
        !run.popup.dispatch(Input::PointerUp(two), &mut out),
        "no press left"
    );
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(run.shell.done(), None);

    let drawer = run.center(&Run::DRAWER_BUTTON);
    let before = run.shell.drawer();
    run.step(Step::Input(Input::PointerDown(drawer)));
    run.ui.set_active(false);
    run.ui.set_active(true);
    assert!(!run.ui.dispatch(Input::PointerUp(drawer), &mut out));
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(run.shell.drawer(), before);
    // The next press is a press as ever.
    for s in click(drawer) {
        run.step(s);
    }
    assert_ne!(run.shell.drawer(), before);
}

#[test]
fn a_click_on_the_disabled_item_does_nothing() {
    let mut run = menu_from_pointer();
    let four = run.popup_center(&Run::menu_item(4));
    for s in click(four) {
        if let Step::Input(input) = s {
            run.step(Step::Popup(input));
        }
    }
    assert!(run.popup_open);
    assert_eq!(run.shell.done(), None);
}

#[test]
fn the_menu_closes_when_its_row_goes_away() {
    let mut run = menu_from_pointer();
    // The Store apps tab doesn't show the row: its node goes, and with it the anchor.
    run.step(Step::Update(Msg::Tab(1)));
    assert_eq!(run.ui.find(&Run::row(ROW)), None);
    assert_eq!(run.ui.popup_anchor(), None);
    assert!(
        !run.popup_open,
        "a popup whose anchor went away closes (A1.8)"
    );
    assert_eq!(run.shell.popup(), None);
}

#[test]
fn a_wheel_over_the_window_closes_the_menu_and_scrolls() {
    let mut run = menu_from_keyboard();
    let table = run.ui.find(&Run::TABLE).expect("the table");
    let before = run
        .ui
        .info(table)
        .and_then(|n| n.scroll)
        .expect("it scrolls")
        .offset;
    let step = run.table_wheel(-120);
    run.step(step);
    assert!(!run.popup_open, "the wheel closed the menu");
    assert_eq!(run.shell.popup(), None);
    assert_eq!(run.shell.done(), None, "and invoked nothing");
    let after = run
        .ui
        .info(table)
        .and_then(|n| n.scroll)
        .expect("it scrolls")
        .offset;
    assert!(after.y > before.y, "the wheel scrolled the table too");
}

#[test]
fn up_and_down_step_the_closed_dropdown() {
    let mut run = on_settings();
    let combo = id(&run, &Run::START_COMBO);
    let mut seen = Vec::new();
    for code in [KeyCode::Down; 5].into_iter().chain([KeyCode::Up]) {
        steps(&mut run, press(code));
        seen.push(run.shell.start_page());
    }
    assert_eq!(seen, [1, 2, 3, 3, 3, 2], "stepping stops at the ends");
    assert!(!run.popup_open, "stepping never opens the list");
    let value = run.ui.info(combo).map(|n| n.text.to_owned());
    assert_eq!(value.as_deref(), Some(START_PAGES[2]));
}

#[test]
fn the_dropdown_opens_on_its_keys_at_the_picked_option_and_closes_on_its_keys() {
    let openers: [(&str, Vec<Step>); 4] = [
        ("Alt+Down", vec![key(KeyCode::Down, true, ALT)]),
        ("F4", vec![key(KeyCode::F4, true, NONE)]),
        ("Enter", vec![key(KeyCode::Enter, true, NONE)]),
        ("Space", press(KeyCode::Space).to_vec()),
    ];
    let closers = [
        ("Esc", popup_key(KeyCode::Escape, true, NONE)),
        ("F4", popup_key(KeyCode::F4, true, NONE)),
        ("Alt+Up", popup_key(KeyCode::Up, true, ALT)),
        ("Esc", popup_key(KeyCode::Escape, true, NONE)),
    ];
    let mut run = on_settings();
    steps(&mut run, press(KeyCode::Down));
    let combo = id(&run, &Run::START_COMBO);
    for ((open, keys), (close, closer)) in openers.into_iter().zip(closers) {
        steps(&mut run, keys);
        assert!(run.popup_open, "{open} opens the list");
        assert_eq!(
            run.popup.focus(),
            run.popup.find(&Run::start_option(1)),
            "{open}: the picked option focused"
        );
        assert_eq!(run.ui.info(combo).and_then(|n| n.expanded), Some(true));
        run.step(closer);
        assert!(!run.popup_open, "{close} closes the list");
        assert_eq!(run.shell.start_page(), 1, "{close} keeps the value");
        assert_eq!(run.ui.info(combo).and_then(|n| n.expanded), Some(false));
        assert_eq!(
            run.ui.focus(),
            Some(combo),
            "{close}: focus back on the dropdown"
        );
    }
}

/// Alt+F4 is the window's: a focused dropdown doesn't take it (the system closes the window),
/// and an open list leaves it to its window, which hands it to the main window.
#[test]
fn alt_f4_is_never_the_dropdown_s() {
    let alt_f4 = KeyEvent {
        key: KeyCode::F4,
        down: true,
        mods: ALT,
    };
    let mut run = on_settings();
    let combo = id(&run, &Run::START_COMBO);
    assert_eq!(run.ui.focus(), Some(combo));
    let mut out = Vec::new();
    assert!(
        !run.ui.dispatch(Input::Key(alt_f4), &mut out),
        "left to the system"
    );
    assert!(out.is_empty(), "{out:?}");
    run.step(Step::Input(Input::Key(alt_f4)));
    assert!(!run.popup_open);
    run.step(key(KeyCode::F4, true, NONE));
    assert!(run.popup_open);
    run.step(Step::Popup(Input::Key(alt_f4)));
    assert!(run.popup_open, "not the list's to close");
    run.step(popup_key(KeyCode::Escape, true, NONE));
    assert!(!run.popup_open);
}

#[test]
fn picking_an_option_sets_the_value_and_closes_the_list() {
    let mut run = on_settings();
    run.step(key(KeyCode::F4, true, NONE));
    steps(&mut run, press_in_popup(KeyCode::Down));
    assert_eq!(run.popup.focus(), run.popup.find(&Run::start_option(1)));
    assert_eq!(run.shell.start_page(), 0, "moving doesn't pick");
    run.step(popup_key(KeyCode::Enter, true, NONE));
    assert_eq!(run.shell.start_page(), 1);
    assert!(!run.popup_open);
    // A list doesn't wrap: End, then Down stays on the last.
    run.step(key(KeyCode::F4, true, NONE));
    steps(&mut run, press_in_popup(KeyCode::End));
    steps(&mut run, press_in_popup(KeyCode::Down));
    assert_eq!(run.popup.focus(), run.popup.find(&Run::start_option(3)));
    run.step(popup_key(KeyCode::Enter, true, NONE));
    assert_eq!(run.shell.start_page(), 3);
    // A click opens it with the picked option focused, without the ring; a click picks.
    for s in click(run.center(&Run::START_COMBO)) {
        run.step(s);
    }
    assert!(run.popup_open);
    assert_eq!(run.popup.focus(), run.popup.find(&Run::start_option(3)));
    assert!(!run.popup.focus_visible());
    let two = run.popup_center(&Run::start_option(2));
    for s in click(two) {
        if let Step::Input(input) = s {
            run.step(Step::Popup(input));
        }
    }
    assert_eq!(run.shell.start_page(), 2);
    assert!(!run.popup_open);
}

/// The tooltip showing in `ui`, its node and text.
fn tip(ui: &Ui<Msg>) -> Option<(NodeId, String)> {
    ui.tooltip().map(|(id, text)| (id, text.to_owned()))
}

#[test]
fn a_tooltip_shows_after_the_delay_and_hides_after_ten() {
    let delay = u64::from(TIP_DELAY_MS);
    let mut run = Run::with(Shell::new());
    let drawer = id(&run, &Run::DRAWER_BUTTON);
    let at = run.center(&Run::DRAWER_BUTTON);
    run.step(Step::Input(Input::PointerMove(at)));
    assert_eq!(run.wake(), Some(delay), "one wake, when it's due");
    run.step(Step::Tick(delay - 1));
    assert_eq!(tip(&run.ui), None);
    run.step(Step::Tick(1));
    assert_eq!(tip(&run.ui), Some((drawer, DRAWER_TIP.to_owned())));
    let shows_for = delay * u64::from(TIP_SHOWS_FOR);
    assert_eq!(run.wake(), Some(shows_for), "the next wake is autopop's");
    run.step(Step::Tick(shows_for));
    assert_eq!(tip(&run.ui), None, "autopop");
    assert_eq!(run.wake(), None);
    // Resting on the button, it stays hidden.
    run.step(Step::Input(Input::PointerMove(dpro_ui::Point {
        x: at.x + 1.0,
        ..at
    })));
    assert_eq!(run.wake(), None);
    // Leaving and coming back at once: it just hid, so it shows at once (the reshow delay).
    let title = run.center(&[Key::name("header"), Key::name("title")]);
    run.step(Step::Input(Input::PointerMove(title)));
    run.step(Step::Input(Input::PointerMove(at)));
    assert_eq!(tip(&run.ui), Some((drawer, DRAWER_TIP.to_owned())));
    // Leaving for longer: the whole delay again.
    run.step(Step::Input(Input::PointerMove(title)));
    run.step(Step::Tick(delay));
    run.step(Step::Input(Input::PointerMove(at)));
    assert_eq!(tip(&run.ui), None);
    assert_eq!(run.wake(), Some(delay));
}

#[test]
fn keyboard_focus_shows_the_tip_at_once_and_any_key_hides_it() {
    let mut run = Run::with(Shell::new());
    let drawer = id(&run, &Run::DRAWER_BUTTON);
    // The drawer button is the first Tab stop.
    run.step(key(KeyCode::Tab, true, NONE));
    assert_eq!(run.ui.focus(), Some(drawer));
    assert_eq!(tip(&run.ui), Some((drawer, DRAWER_TIP.to_owned())));
    run.step(key(KeyCode::Tab, false, NONE));
    assert!(tip(&run.ui).is_some(), "a key's release hides nothing");
    run.step(key(KeyCode::Other(0x10), true, NONE));
    assert_eq!(tip(&run.ui), None, "any key down hides it");
}

#[test]
fn a_press_hides_the_tip_until_the_pointer_leaves() {
    let delay = u64::from(TIP_DELAY_MS);
    let mut run = Run::with(Shell::new());
    let at = run.center(&Run::DRAWER_BUTTON);
    run.step(Step::Input(Input::PointerMove(at)));
    run.step(Step::Tick(delay));
    assert!(tip(&run.ui).is_some());
    run.step(Step::Input(Input::PointerDown(at)));
    assert_eq!(tip(&run.ui), None, "a press hides it");
    run.step(Step::Input(Input::PointerUp(at)));
    run.step(Step::Tick(delay));
    assert_eq!(
        tip(&run.ui),
        None,
        "and it stays hidden while the pointer rests there"
    );
}

#[test]
fn moving_to_a_neighbor_soon_after_shows_its_tip_at_once() {
    let delay = u64::from(TIP_DELAY_MS);
    let mut run = Run::with(Shell::new()).drawer_open();
    let apply = id(&run, &Run::APPLY);
    let drawer = run.center(&Run::DRAWER_BUTTON);
    let title = run.center(&[Key::name("header"), Key::name("title")]);
    run.step(Step::Input(Input::PointerMove(drawer)));
    run.step(Step::Tick(delay));
    assert!(tip(&run.ui).is_some());
    // Off it, and onto Hold to apply within a fifth of the delay: at once.
    run.step(Step::Input(Input::PointerMove(title)));
    assert_eq!(tip(&run.ui), None);
    run.step(Step::Tick(delay / 10));
    run.step(Step::Input(Input::PointerMove(run.center(&Run::APPLY))));
    assert_eq!(tip(&run.ui).map(|(id, _)| id), Some(apply), "reshow");
    // Off, and back after more than a fifth: the whole delay again.
    run.step(Step::Input(Input::PointerMove(title)));
    run.step(Step::Tick(delay / 4));
    run.step(Step::Input(Input::PointerMove(drawer)));
    assert_eq!(tip(&run.ui), None);
    assert_eq!(run.wake(), Some(delay));
}

#[test]
fn the_disabled_item_s_tip_says_why_in_the_menu() {
    let mut run = menu_from_keyboard();
    steps(&mut run, press_in_popup(KeyCode::End));
    let four = item(&run, 4).expect("Properties");
    assert_eq!(tip(&run.popup), Some((four, PROPERTIES_TIP.to_owned())));
    assert_eq!(tip(&run.ui), None, "only the active window shows a tip");
}

/// `tree`'s node for `id` in `ui`.
fn node<'a>(tree: &'a TreeUpdate, ui: &Ui<Msg>, id: NodeId) -> &'a Node {
    &tree
        .nodes
        .iter()
        .find(|(ak, _)| ui.a11y_node(*ak) == Some(id))
        .expect("the node in the tree")
        .1
}

#[test]
fn the_trees_have_roles_names_values_states_and_descriptions() {
    // The main window: the dropdown, and a tooltip as a description.
    let mut run = on_settings();
    let combo = id(&run, &Run::START_COMBO);
    let tree = run.ui.a11y_tree();
    let c = node(&tree, &run.ui, combo);
    assert_eq!(c.role(), AkRole::ComboBox);
    assert_eq!(
        (c.label(), c.value()),
        (Some("Start page"), Some(START_PAGES[0]))
    );
    assert_eq!(c.is_expanded(), Some(false));
    assert!(c.supports_action(Action::Expand) && c.supports_action(Action::Focus));
    assert!(!c.supports_action(Action::Collapse) && !c.supports_action(Action::Click));
    let drawer = id(&run, &Run::DRAWER_BUTTON);
    assert_eq!(node(&tree, &run.ui, drawer).description(), Some(DRAWER_TIP));
    // Its list: a list box of options, the picked one selected.
    run.step(key(KeyCode::F4, true, NONE));
    let tree = run.ui.a11y_tree();
    let c = node(&tree, &run.ui, combo);
    assert_eq!(c.is_expanded(), Some(true));
    assert!(c.supports_action(Action::Collapse));
    let list = run.popup.a11y_tree();
    let options = popup_id(&run, &[Key::name("start-pages")]);
    let l = node(&list, &run.popup, options);
    assert_eq!((l.role(), l.label()), (AkRole::ListBox, Some("Start page")));
    let first = node(&list, &run.popup, popup_id(&run, &Run::start_option(0)));
    assert_eq!(first.role(), AkRole::ListBoxOption);
    assert_eq!(
        (first.label(), first.is_selected()),
        (Some(START_PAGES[0]), Some(true))
    );
    let second = node(&list, &run.popup, popup_id(&run, &Run::start_option(1)));
    assert_eq!(second.is_selected(), Some(false));
    // UI Automation's focus is in the list, on the picked option.
    assert_eq!(
        run.popup.a11y_node(list.focus),
        run.popup.find(&Run::start_option(0))
    );

    // A menu: items, a separator, and the disabled item, focusable, saying why.
    let run = menu_from_keyboard();
    let mut popup = run.popup;
    let menu = popup.a11y_tree();
    let row_menu = popup.find(&[Key::name("row-menu")]).expect("the menu");
    let group = node(&menu, &popup, row_menu);
    assert_eq!((group.role(), group.label()), (AkRole::Menu, Some("Row")));
    let path = Run::menu_item(0);
    let first = node(&menu, &popup, popup.find(&path).expect("Uninstall"));
    assert_eq!(
        (first.role(), first.label()),
        (AkRole::MenuItem, Some(ROW_MENU[0].0))
    );
    assert!(first.supports_action(Action::Click));
    let sep = popup
        .find(&[Key::name("row-menu"), Key::name("sep")])
        .expect("the separator");
    assert_eq!(node(&menu, &popup, sep).role(), AkRole::Splitter);
    let properties = node(
        &menu,
        &popup,
        popup.find(&Run::menu_item(4)).expect("Properties"),
    );
    assert!(properties.is_disabled());
    assert!(
        properties.supports_action(Action::Focus) && !properties.supports_action(Action::Click)
    );
    assert_eq!(properties.description(), Some(PROPERTIES_TIP));

    // A tooltip window's text.
    let mut tip = Ui::<Msg>::new(Size { w: 0.0, h: 0.0 }, 1.0, Motion::STANDARD);
    tip.build(|cx| {
        cx.add(dpro_ui::Element::tooltip_text(Key::name("tip"), DRAWER_TIP));
    });
    let size = tip.natural_size(&mut FakeText, 1000.0);
    tip.set_viewport(size);
    tip.layout(&mut FakeText);
    let tree = tip.a11y_tree();
    let text = tip.find(&[Key::name("tip")]).expect("the tip");
    let shown = &tree
        .nodes
        .iter()
        .find(|(ak, _)| tip.a11y_node(*ak) == Some(text))
        .expect("in the tree")
        .1;
    assert_eq!(
        (shown.role(), shown.label()),
        (AkRole::Tooltip, Some(DRAWER_TIP))
    );
}

/// A menu's `Ui`, as the platform opens it from the keyboard, laid out at `text_scale`, with
/// nothing animating; and its shell.
fn menu_ui(text_scale: f32) -> (Ui<Msg>, Shell) {
    let mut shell = Shell::new();
    shell.update(Msg::RowMenu(0));
    let mut ui = Ui::new(Size { w: 0.0, h: 0.0 }, 1.0, Motion::REDUCED);
    ui.set_text_scale(text_scale);
    ui.build(|cx| popup_view(&shell, cx));
    ui.focus_first(true);
    let size = ui.natural_size(&mut FakeText, 1000.0);
    ui.set_viewport(size);
    ui.layout(&mut FakeText);
    (ui, shell)
}

#[test]
fn a_focus_move_in_the_menu_damages_the_two_items_only() {
    let (mut ui, shell) = menu_ui(1.0);
    ui.clear_damage();
    let mut out = Vec::new();
    ui.dispatch(
        Input::Key(KeyEvent {
            key: KeyCode::Down,
            down: true,
            mods: NONE,
        }),
        &mut out,
    );
    ui.build(|cx| popup_view(&shell, cx));
    ui.layout(&mut FakeText);
    let rect = |i: u64| {
        ui.find(&Run::menu_item(i))
            .and_then(|id| ui.info(id))
            .map(|n| n.rect.to_px(1.0))
            .expect("an item")
    };
    let both = rect(0).union(rect(1));
    let damage = ui.damage();
    assert!(!damage.is_empty(), "the focus moved: something repaints");
    for r in damage {
        assert!(both.contains(*r), "{r:?} outside the two items {both:?}");
    }
    let (w, h) = ui.pixel_size();
    let damaged: i64 = damage.iter().map(dpro_ui::RectPx::area).sum();
    assert!(
        damaged * 2 < i64::from(w) * i64::from(h),
        "{damaged} px of a {w} × {h} menu"
    );
}

/// Every menu item, option and dropdown under `id` that is smaller than 24 × 24, or whose text
/// doesn't fit it (one line high, its width plus its padding), described.
fn misfits(ui: &Ui<Msg>, id: NodeId, out: &mut Vec<String>) -> usize {
    let mut seen = 0;
    for c in ui.children(id) {
        let Some(n) = ui.info(c) else { continue };
        if matches!(n.role, Role::MenuItem | Role::ComboBox) {
            seen += 1;
            let m = FakeText.measure(n.text, &n.text_style, f32::INFINITY);
            if n.rect.w < 24.0 || n.rect.h < ITEM_HEIGHT {
                out.push(format!(
                    "{:?} {:?} is {} × {}",
                    n.role, n.text, n.rect.w, n.rect.h
                ));
            }
            if n.rect.h + 0.01 < m.height || n.rect.w + 0.01 < m.width + 2.0 * ITEM_PAD {
                out.push(format!(
                    "{:?} {:?}: {} × {} holds {} × {} text",
                    n.role, n.text, n.rect.w, n.rect.h, m.width, m.height
                ));
            }
        }
        seen += misfits(ui, c, out);
    }
    seen
}

#[test]
fn items_are_at_least_an_item_high_and_fit_their_text_at_100_and_225_percent() {
    for scale in [1.0, 2.25] {
        let (ui, _) = menu_ui(scale);
        let mut bad = Vec::new();
        let seen = misfits(&ui, ui.root(), &mut bad);
        assert_eq!(seen, ROW_MENU.len(), "every item at {scale}");
        assert_eq!(bad, [] as [String; 0], "the menu at {scale}");
        // The list, and the dropdown it opens from.
        let mut run = Run::text_scaled(Shell::new(), scale);
        run.step(Step::Update(Msg::Page(SETTINGS)));
        let combo = id(&run, &Run::START_COMBO);
        run.ui.set_focus(Some(combo));
        run.step(key(KeyCode::F4, true, NONE));
        assert!(run.popup_open);
        let mut bad = Vec::new();
        assert_eq!(
            misfits(&run.popup, run.popup.root(), &mut bad),
            START_PAGES.len()
        );
        assert_eq!(misfits(&run.ui, run.ui.root(), &mut bad), 1, "the dropdown");
        assert_eq!(bad, [] as [String; 0], "the list at {scale}");
    }
}
