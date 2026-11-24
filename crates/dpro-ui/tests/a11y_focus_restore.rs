//! Focus restoration (TD-0009 A4.4, A4.5, A4.9), slice 7c: focus never falls back to the window.
//!
//! - The successor rule: a removed focus goes to the next control among its siblings' subtrees,
//!   else the last before it, else the same one level up. The search stops at the landmark,
//!   whose first control is the last resort; past a landmark with none it goes on. The focus
//!   ring stays as it was.
//! - The parked focus: a control disabled while focused keeps the focus, takes no activation,
//!   and Tab and Shift+Tab leave it. A client reads it as unavailable.
//! - Memos find their node again by its keys after a rebuild; when it went, the nearest surviving
//!   ancestor's first control; with nothing of its path left, the Main landmark's. None past
//!   depth 16.
//! - The return stack holds four: the oldest drops.
//! - A popup whose anchor went closes with the main window's focus on the successor.
//! - The page-change rule: the rail keeps focus; anything else focuses the Main landmark's first
//!   control.
//! - Scopes give focus back by memo: a sheet whose opener was rebuilt still finds it.

#![allow(missing_docs)]
#![expect(
    clippy::expect_used,
    reason = "test helpers: a node that isn't there fails the test"
)]

#[path = "../benches/fixtures/drawer.rs"]
mod drawer;
#[path = "../benches/fixtures/shell.rs"]
mod shell;

use dpro_ui::{
    Axes, Cx, Element, FakeText, Input, Key, KeyCode, KeyEvent, Landmark, Mods, Motion, NodeId,
    Size, Ui,
};

const PANEL: Key = Key::name("panel");
const OTHER: Key = Key::name("other");
const MAIN: Key = Key::name("main");
const NAV: Key = Key::name("nav");
const AFTER: Key = Key::name("after");
const TEXT: Key = Key::name("text");

fn ui() -> Ui<u8> {
    let mut ui = Ui::new(Size { w: 600.0, h: 400.0 }, 1.0, Motion::default());
    ui.set_active(true);
    ui
}

fn build(ui: &mut Ui<u8>, view: impl FnOnce(&mut Cx<'_, u8>)) {
    ui.build(view);
    ui.layout(&mut FakeText);
}

/// Button `k`, keyed by its index, sending `k`.
fn button(k: u8) -> Element<'static, u8> {
    Element::button(Key::index(u64::from(k)), "Button", k)
}

fn id(ui: &Ui<u8>, path: &[Key]) -> NodeId {
    ui.find(path).expect("the node")
}

/// The focused node's key.
fn focused(ui: &Ui<u8>) -> Option<Key> {
    ui.focus().and_then(|f| ui.info(f)).map(|n| n.key)
}

fn index(k: u8) -> Key {
    Key::index(u64::from(k))
}

/// A key pressed (or let go), as the platform delivers it: whether it was used, and what it sent.
fn press(ui: &mut Ui<u8>, key: KeyCode, down: bool, shift: bool) -> (bool, Vec<u8>) {
    let mut out = Vec::new();
    let used = ui.dispatch(
        Input::Key(KeyEvent {
            key,
            down,
            mods: Mods {
                shift,
                ..Mods::default()
            },
        }),
        &mut out,
    );
    (used, out)
}

/// Buttons `0..n` in a row, but `skip`.
fn buttons(cx: &mut Cx<'_, u8>, n: u8, skip: Option<u8>) {
    for k in (0..n).filter(|&k| Some(k) != skip) {
        cx.add(button(k));
    }
}

// ---- the successor rule ----------------------------------------------------------------------

#[test]
fn a_removed_focus_goes_to_the_next_control() {
    let mut ui = ui();
    build(&mut ui, |cx| buttons(cx, 4, None));
    ui.set_focus(Some(id(&ui, &[index(1)])));
    build(&mut ui, |cx| buttons(cx, 4, Some(1)));
    assert_eq!(focused(&ui), Some(index(2)));
}

#[test]
fn with_nothing_after_it_the_previous_control() {
    let mut ui = ui();
    let view = |cx: &mut Cx<'_, u8>, last: bool| {
        buttons(cx, 3, None);
        if last {
            cx.add(button(3));
        }
        cx.add(Element::text(TEXT, "A line"));
    };
    build(&mut ui, |cx| view(cx, true));
    ui.set_focus(Some(id(&ui, &[index(3)])));
    build(&mut ui, |cx| view(cx, false));
    assert_eq!(
        focused(&ui),
        Some(index(2)),
        "the text after it isn't a stop"
    );
}

#[test]
fn with_no_sibling_left_the_search_goes_up_a_level() {
    // Three groups: the first holds button 0, the second the focused button 1, the third a line
    // and, deeper, button 2. The second's button goes: the next group's subtree has the
    // successor. Without the third group's button, the first group's.
    fn view(cx: &mut Cx<'_, u8>, focused: bool, third: bool) {
        cx.add_with(Element::stack(Key::index(10)), |cx| {
            cx.add(button(0));
        });
        cx.add_with(Element::stack(Key::index(11)), |cx| {
            if focused {
                cx.add(button(1));
            }
            cx.add(Element::text(TEXT, "Nothing to press"));
        });
        cx.add_with(Element::stack(Key::index(12)), |cx| {
            cx.add(Element::text(TEXT, "A line"));
            cx.add_with(Element::stack(PANEL), |cx| {
                if third {
                    cx.add(button(2));
                }
            });
        });
    }
    for third in [true, false] {
        let mut ui = ui();
        build(&mut ui, |cx| view(cx, true, third));
        ui.set_focus(Some(id(&ui, &[Key::index(11), index(1)])));
        build(&mut ui, |cx| view(cx, false, third));
        let want = if third {
            [Key::index(12), PANEL, index(2)].to_vec()
        } else {
            [Key::index(10), index(0)].to_vec()
        };
        assert_eq!(ui.focus(), ui.find(&want), "third group's button: {third}");
    }
}

#[test]
fn the_search_stops_at_the_landmark_whose_first_control_is_the_last_resort() {
    // The Main landmark is a scroll viewport: once its only button goes, it's a stop itself (a
    // viewport with none inside, A2.4), and it takes focus rather than the rail's button before
    // it or the one after it.
    fn view(cx: &mut Cx<'_, u8>, focused: bool) {
        cx.add_with(
            Element::stack(NAV)
                .name("Pages")
                .landmark(Landmark::Navigation),
            |cx| {
                cx.add(button(0));
            },
        );
        cx.add_with(
            Element::scroll(MAIN, Axes::Y)
                .name("Clean")
                .landmark(Landmark::Main),
            |cx| {
                cx.add(Element::text(TEXT, "A line"));
                if focused {
                    cx.add(button(1));
                }
            },
        );
        cx.add(Element::button(AFTER, "After", 2));
    }
    let mut ui = ui();
    build(&mut ui, |cx| view(cx, true));
    ui.set_focus(Some(id(&ui, &[MAIN, index(1)])));
    build(&mut ui, |cx| view(cx, false));
    assert_eq!(ui.focus(), ui.find(&[MAIN]));
}

#[test]
fn past_a_landmark_with_nothing_to_focus_the_search_goes_on() {
    // A region with no control left: focus doesn't fall to the window (A4.1), it goes on to the
    // next control after the region (decision #41).
    fn view(cx: &mut Cx<'_, u8>, focused: bool) {
        cx.add(button(0));
        cx.add_with(
            Element::stack(PANEL)
                .name("Changes")
                .landmark(Landmark::Region { kind: "region" }),
            |cx| {
                cx.add(Element::text(TEXT, "A line"));
                if focused {
                    cx.add(button(1));
                }
            },
        );
        cx.add(button(2));
    }
    let mut ui = ui();
    build(&mut ui, |cx| view(cx, true));
    ui.set_focus(Some(id(&ui, &[PANEL, index(1)])));
    build(&mut ui, |cx| view(cx, false));
    assert_eq!(focused(&ui), Some(index(2)));
}

#[test]
fn the_successor_keeps_the_ring_as_it_was() {
    for keyboard in [true, false] {
        let mut ui = ui();
        build(&mut ui, |cx| buttons(cx, 3, None));
        if keyboard {
            press(&mut ui, KeyCode::Tab, true, false);
            press(&mut ui, KeyCode::Tab, true, false);
        } else {
            ui.set_focus(Some(id(&ui, &[index(1)])));
        }
        assert_eq!(focused(&ui), Some(index(1)));
        build(&mut ui, |cx| buttons(cx, 3, Some(1)));
        assert_eq!(focused(&ui), Some(index(2)));
        assert_eq!(ui.focus_visible(), keyboard, "keyboard: {keyboard}");
    }
}

#[test]
fn a_removed_card_gives_focus_to_the_next_cards_review() {
    // The shell's summary page, as `interact::event.remove_focused` measures it.
    let mut run = shell::Run::with(shell::Shell::new()).card_focused(3);
    run.step(shell::Step::Update(shell::Msg::Cleaned(3)));
    assert_eq!(run.ui.focus(), run.ui.find(&shell::Run::card_review(4)));
}

// ---- the parked focus ------------------------------------------------------------------------

/// Buttons 0 to 2, the middle one enabled or not, and a hold button (3) enabled or not.
fn parking(cx: &mut Cx<'_, u8>, middle: bool, hold: bool) {
    cx.add(button(0));
    cx.add(button(1).enabled(middle));
    cx.add(button(2));
    cx.add(button(3).hold(|| 30).enabled(hold));
}

#[test]
fn a_disabled_focus_stays_parked_and_takes_no_activation() {
    let mut ui = ui();
    build(&mut ui, |cx| parking(cx, true, true));
    ui.set_focus(Some(id(&ui, &[index(1)])));
    build(&mut ui, |cx| parking(cx, false, true));
    assert_eq!(focused(&ui), Some(index(1)), "parked");
    // Enter and Space are used, so nothing behind the page takes them, and do nothing.
    assert_eq!(press(&mut ui, KeyCode::Enter, true, false), (true, vec![]));
    assert_eq!(press(&mut ui, KeyCode::Space, true, false), (true, vec![]));
    let info = ui.info(id(&ui, &[index(1)])).expect("the button");
    assert!(!info.pressed, "Space doesn't press it");
    assert_eq!(press(&mut ui, KeyCode::Space, false, false).1, vec![]);
    // A parked hold button doesn't hold.
    ui.set_focus(Some(id(&ui, &[index(3)])));
    build(&mut ui, |cx| parking(cx, false, false));
    assert_eq!(focused(&ui), Some(index(3)), "parked");
    press(&mut ui, KeyCode::Enter, true, false);
    let mut out = Vec::new();
    ui.tick(0, &mut out);
    ui.tick(10_000, &mut out);
    assert_eq!((ui.hold_progress(), out), (None, vec![]));
}

#[test]
fn tab_and_shift_tab_leave_a_parked_focus() {
    for (shift, want) in [(false, 2), (true, 0)] {
        let mut ui = ui();
        build(&mut ui, |cx| parking(cx, true, true));
        ui.set_focus(Some(id(&ui, &[index(1)])));
        build(&mut ui, |cx| parking(cx, false, true));
        press(&mut ui, KeyCode::Tab, true, shift);
        assert_eq!(focused(&ui), Some(index(want)), "shift: {shift}");
    }
}

#[test]
fn a_client_reads_a_parked_focus_as_unavailable() {
    let mut ui = ui();
    build(&mut ui, |cx| parking(cx, true, true));
    let parked = id(&ui, &[index(1)]);
    ui.set_focus(Some(parked));
    build(&mut ui, |cx| parking(cx, false, true));
    let tree = ui.a11y_tree();
    assert_eq!(ui.a11y_node(tree.focus), Some(parked));
    let (_, node) = tree
        .nodes
        .iter()
        .find(|(ak, _)| *ak == tree.focus)
        .expect("the focus is in the tree");
    assert!(node.is_disabled());
}

// ---- memos -----------------------------------------------------------------------------------

/// Another button, then a panel holding `inside`'s buttons.
fn paneled(cx: &mut Cx<'_, u8>, inside: &[u8]) {
    cx.add(Element::button(OTHER, "Other", 9));
    cx.add_with(Element::stack(PANEL), |cx| {
        for &k in inside {
            cx.add(button(k));
        }
    });
}

#[test]
fn a_memo_finds_its_node_again_by_its_keys() {
    let mut ui = ui();
    build(&mut ui, |cx| paneled(cx, &[0, 1]));
    ui.set_focus(Some(id(&ui, &[PANEL, index(1)])));
    let memo = ui.focus_memo().expect("a memo");
    assert_eq!(memo.keys(), [PANEL, index(1)]);
    // The button goes and comes back: a new node with the same keys.
    let old = ui.focus().expect("focus");
    build(&mut ui, |cx| paneled(cx, &[0]));
    build(&mut ui, |cx| paneled(cx, &[0, 1]));
    let new = id(&ui, &[PANEL, index(1)]);
    assert_ne!(old, new);
    assert_eq!(ui.restore_focus(&memo), Some(new));
    assert_eq!(ui.focus(), Some(new));
}

#[test]
fn a_memo_whose_node_went_falls_back_to_its_nearest_ancestors_first_control() {
    let mut ui = ui();
    build(&mut ui, |cx| paneled(cx, &[1]));
    ui.set_focus(Some(id(&ui, &[PANEL, index(1)])));
    let memo = ui.focus_memo().expect("a memo");
    build(&mut ui, |cx| paneled(cx, &[2, 3]));
    let want = id(&ui, &[PANEL, index(2)]);
    assert_eq!(
        ui.restore_focus(&memo),
        Some(want),
        "not the other button before the panel"
    );
}

#[test]
fn with_nothing_of_its_path_left_the_main_landmarks_first_control() {
    let mut ui = ui();
    build(&mut ui, |cx| paneled(cx, &[1]));
    ui.set_focus(Some(id(&ui, &[PANEL, index(1)])));
    let memo = ui.focus_memo().expect("a memo");
    build(&mut ui, |cx| {
        cx.add_with(
            Element::stack(NAV)
                .name("Pages")
                .landmark(Landmark::Navigation),
            |cx| {
                cx.add(button(0));
            },
        );
        cx.add_with(
            Element::stack(MAIN).name("Clean").landmark(Landmark::Main),
            |cx| {
                cx.add(Element::text(TEXT, "Clean"));
                cx.add(button(5));
            },
        );
    });
    assert_eq!(ui.restore_focus(&memo), ui.find(&[MAIN, index(5)]));
}

#[test]
fn a_memo_whose_node_is_disabled_parks_focus_on_it() {
    let mut ui = ui();
    build(&mut ui, |cx| parking(cx, true, true));
    ui.set_focus(Some(id(&ui, &[index(1)])));
    let memo = ui.focus_memo().expect("a memo");
    ui.set_focus(Some(id(&ui, &[index(2)])));
    build(&mut ui, |cx| parking(cx, false, true));
    assert_eq!(ui.restore_focus(&memo), ui.find(&[index(1)]));
}

#[test]
fn a_node_deeper_than_sixteen_has_no_memo() {
    fn nest(cx: &mut Cx<'_, u8>, depth: u8) {
        if depth == 1 {
            cx.add(button(0));
        } else {
            cx.add_with(Element::stack(PANEL), |cx| nest(cx, depth - 1));
        }
    }
    for (depth, memo) in [(16, true), (17, false)] {
        let mut ui = ui();
        build(&mut ui, |cx| nest(cx, depth));
        let mut path = vec![PANEL; usize::from(depth) - 1];
        path.push(index(0));
        ui.set_focus(Some(id(&ui, &path)));
        assert_eq!(ui.focus_memo().is_some(), memo, "depth {depth}");
        if let Some(m) = ui.focus_memo() {
            assert_eq!(m.keys(), path);
        }
    }
}

// ---- the return stack ------------------------------------------------------------------------

#[test]
fn the_return_stack_keeps_four_and_drops_the_oldest() {
    let mut ui = ui();
    build(&mut ui, |cx| buttons(cx, 6, None));
    for k in 0..5 {
        ui.set_focus(Some(id(&ui, &[index(k)])));
        ui.push_focus_return();
    }
    ui.set_focus(Some(id(&ui, &[index(5)])));
    for k in (1..5).rev() {
        assert_eq!(ui.pop_focus_return(), ui.find(&[index(k)]));
        assert_eq!(focused(&ui), Some(index(k)));
    }
    assert_eq!(ui.pop_focus_return(), None, "the first push dropped");
    assert_eq!(focused(&ui), Some(index(1)), "focus stays");
}

#[test]
fn a_dialog_whose_opener_was_rebuilt_gives_focus_back_to_it() {
    // A dialog keeps the main window's focus where it was (A1); the page rebuilt its opener
    // meanwhile, so the pop finds it by keys.
    let mut ui = ui();
    build(&mut ui, |cx| paneled(cx, &[0, 1]));
    ui.set_focus(Some(id(&ui, &[PANEL, index(1)])));
    ui.push_focus_return();
    build(&mut ui, |cx| paneled(cx, &[0]));
    build(&mut ui, |cx| paneled(cx, &[0, 1]));
    assert_eq!(ui.pop_focus_return(), ui.find(&[PANEL, index(1)]));
}

// ---- popups ----------------------------------------------------------------------------------

#[test]
fn a_popup_whose_anchor_went_leaves_focus_on_the_successor() {
    fn view(cx: &mut Cx<'_, u8>, anchor: bool) {
        cx.add(button(0));
        if anchor {
            cx.add(Element::combo(index(1), "Start page", "Overview", false, 1));
        }
        cx.add(button(2));
    }
    let mut ui = ui();
    build(&mut ui, |cx| view(cx, true));
    ui.set_focus(Some(id(&ui, &[index(1)])));
    // F4 opens its list: the dropdown is the popup's anchor.
    assert_eq!(press(&mut ui, KeyCode::F4, true, false), (true, vec![1]));
    assert!(ui.popup_anchor().is_some());
    build(&mut ui, |cx| view(cx, false));
    // The host closes the popup whose anchor went (A1.8); the main window's focus is already on
    // the successor.
    assert!(ui.popup_anchor().is_none());
    assert_eq!(focused(&ui), Some(index(2)));
}

// ---- the page-change rule --------------------------------------------------------------------

#[test]
fn navigation_from_the_rail_keeps_focus_on_its_item() {
    let mut run = shell::Run::with(shell::Shell::new());
    let item = run.ui.find(&shell::Run::page(2));
    run.ui.set_focus(item);
    run.step(shell::key(KeyCode::Enter, true, Mods::default()));
    assert!(
        run.ui.find(&shell::Run::card_review(0)).is_some(),
        "the summary page"
    );
    assert_eq!(run.ui.focus(), item);
}

#[test]
fn navigation_from_elsewhere_focuses_the_main_landmarks_first_control() {
    let mut run = shell::Run::with(shell::Shell::new());
    for s in run.focus_filter() {
        run.step(s);
    }
    // As the palette or Alt+Left would: the app's page change, then its PageStart.
    run.step(shell::Step::Update(shell::Msg::Page(2)));
    let first = run.ui.find(&shell::Run::card_review(0));
    assert!(first.is_some(), "the summary page");
    assert_eq!(run.ui.focus_page_start(), first);
    assert_eq!(run.ui.focus(), first);
}

#[test]
fn focus_first_in_a_container_takes_its_first_control() {
    let mut ui = ui();
    build(&mut ui, |cx| paneled(cx, &[3, 4]));
    let panel = id(&ui, &[PANEL]);
    assert_eq!(ui.focus_first_in(panel), ui.find(&[PANEL, index(3)]));
    // A container with none: focus stays.
    build(&mut ui, |cx| paneled(cx, &[]));
    let panel = id(&ui, &[PANEL]);
    let before = ui.focus();
    assert_eq!(ui.focus_first_in(panel), None);
    assert_eq!(ui.focus(), before);
}

// ---- scopes ----------------------------------------------------------------------------------

#[test]
fn the_shells_drawer_gives_focus_back_to_review_changes() {
    // As `interact::event.drawer_close` measures it: Enter on Cancel.
    let mut run = shell::Run::with(shell::Shell::new()).drawer_from_keyboard();
    assert_eq!(run.ui.focus(), run.ui.find(&shell::Run::CANCEL));
    run.step(shell::key(KeyCode::Enter, true, Mods::default()));
    assert!(!run.shell.drawer());
    assert_eq!(run.ui.focus(), run.ui.find(&shell::Run::DRAWER_BUTTON));
    assert!(run.ui.focus_visible());
}

#[test]
fn a_sheet_whose_opener_was_rebuilt_finds_it_by_its_keys() {
    // The sheet takes the page out of the tree, and closing it builds the page anew. Opened with
    // "Scan again" focused (an accelerator's open), focus goes back to it, not to the page's first
    // control.
    let mut run = drawer::Run::with(drawer::Model::new());
    run.step(drawer::Step::Resize(drawer::NARROW));
    let scan = run.find(&[drawer::ROW, drawer::PAGE, drawer::SCAN]);
    run.ui.set_focus(scan);
    run.step(drawer::Step::Update(drawer::Msg::Open));
    assert!(run.opener().is_none(), "the sheet took the page away");
    run.step(drawer::key(KeyCode::Escape, Mods::default()));
    assert!(run.drawer().is_none());
    let scan = run.find(&[drawer::ROW, drawer::PAGE, drawer::SCAN]);
    assert!(scan.is_some());
    assert_eq!(run.ui.focus(), scan);
}

/// The drawer's change `i`, while it's listed.
fn change(run: &drawer::Run, i: u64) -> Option<NodeId> {
    let changes = run.part(drawer::CHANGES)?;
    run.ui
        .children(changes)
        .find(|&c| run.ui.info(c).is_some_and(|n| n.key == Key::index(i)))
}

#[test]
fn a_dropped_change_gives_focus_to_the_next() {
    // G1's "Accessibility depth: focus" on the drawer probe: Delete drops the first change, which
    // has focus as the drawer opens.
    let mut run = drawer::Run::with(drawer::Model::new());
    run.step(drawer::Step::Update(drawer::Msg::Open));
    let first = change(&run, 0);
    assert!(first.is_some());
    assert_eq!(run.ui.focus(), first);
    run.step(drawer::Step::Update(drawer::Msg::Drop));
    assert!(change(&run, 0).is_none());
    assert_eq!(run.ui.focus(), change(&run, 1));
}
