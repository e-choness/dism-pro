// dpro: DISM Pro's tests of the queued events' raising (PATCHES.md, 11; TD-0009 A4.5), and of
// the opened and closed events of menus (12) and tooltips (13), under the crate's licences: the
// MIT license or the Apache License, Version 2.0, at your option.

//! `QueuedEvents::raise` counts the raises UI Automation refuses instead of panicking: one lost
//! event must never abort the process, the elevated broker's included.
//!
//! A menu raises MenuOpened when a client starts seeing it (added, shown, or in the whole tree
//! that ends a placeholder) and MenuClosed when it stops, on the platform node a client may hold
//! for it; a tooltip raises ToolTipOpened and ToolTipClosed the same way. The window tests run on
//! the window's own thread with no client, so Wine runs them too (T2): the adapter is activated
//! through `handle_wm_getobject` without handing its answer on, as `notify.rs` explains.

use accesskit::{ActivationHandler, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use windows::{
    Win32::{
        Foundation::{E_FAIL, E_INVALIDARG, LPARAM, WPARAM},
        System::Variant::VARIANT,
        UI::{Accessibility::*, WindowsAndMessaging::*},
    },
    core::{HRESULT, Interface, Result},
};

use super::{
    MUTEX, create_window, get_window_state,
    providers::{Requests, context, control_type, element, name},
    simple::NullActionHandler,
};
use crate::{adapter::QueuedEvents, util::QueuedEvent};

const WINDOW: NodeId = NodeId(0);
const BUTTON: NodeId = NodeId(1);
/// A popup's first node: a menu, whose item is the next id, or a tooltip.
const POPUP: NodeId = NodeId(2);
/// The same popup open again: the toolkit builds a kept popup's content anew, with new ids.
const POPUP_AGAIN: NodeId = NodeId(5);
const TIP: &str = "Every change before it applies";

fn tree() -> TreeUpdate {
    let mut window = Node::new(Role::Window);
    window.set_children(vec![BUTTON]);
    let mut button = Node::new(Role::Button);
    button.set_label("Apply");
    TreeUpdate {
        nodes: vec![(WINDOW, window), (BUTTON, button)],
        tree: Some(TreeInfo::new(WINDOW)),
        tree_id: TreeId::ROOT,
        focus: BUTTON,
    }
}

/// Event id 0 is no UI Automation event.
fn no_event(element: IRawElementProviderSimple) -> QueuedEvent {
    QueuedEvent::Simple {
        element,
        event_id: UIA_EVENT_ID(0),
    }
}

#[test]
fn a_refused_raise_is_counted_not_a_panic() {
    let requests = Requests::default();
    let context = context(&requests, tree());
    let button = element(&context, BUTTON);
    let events = QueuedEvents::of(vec![
        no_event(button.clone()),
        QueuedEvent::Simple {
            element: button.clone(),
            event_id: UIA_AutomationFocusChangedEventId,
        },
        QueuedEvent::PropertyChanged {
            element: button,
            property_id: UIA_NamePropertyId,
            old_value: VARIANT::default(),
            new_value: VARIANT::default(),
        },
    ]);
    // A raiser that refuses the unknown id, as UI Automation may (E_INVALIDARG). The T2 image's
    // Wine accepts any id, so the real raise can't refuse one there.
    let mut raised = Vec::new();
    let refused = events.raise_with(
        |_, id| -> Result<()> {
            raised.push(id.0);
            if id.0 == 0 {
                Err(E_INVALIDARG.into())
            } else {
                Ok(())
            }
        },
        |_, _, _, _| Ok(()),
    );
    assert_eq!(refused, 1);
    // The events after the refused one are still raised.
    assert_eq!(raised, [0, UIA_AutomationFocusChangedEventId.0]);
}

#[test]
fn ui_automation_s_answer_to_an_unknown_event_never_panics() {
    let requests = Requests::default();
    let context = context(&requests, tree());
    let button = element(&context, BUTTON);
    // Measured in T2: the image's Wine accepts it (0). Windows may refuse it (1) (verify: T3, on
    // the Windows runner).
    let refused = QueuedEvents::of(vec![no_event(button)]).raise();
    assert!(refused <= 1, "{refused}");
}

#[test]
fn an_empty_queue_raises_nothing_and_counts_nothing() {
    assert_eq!(QueuedEvents::of(Vec::new()).raise(), 0);
}

// ---- menus and tooltips (PATCHES.md, 12 and 13) ------------------------------------------------

/// What a client's first ask gets: the tree held, or a placeholder once it's taken (or if none
/// was given).
struct Initial(Option<TreeUpdate>);

impl ActivationHandler for Initial {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0.take()
    }
}

/// The window's nodes: its button, and beside it `popup` if given, a menu "Row" holding one item
/// "Uninstall" (the id after the menu's), or a tooltip. The focus is on the menu's item, else on
/// the button. `hidden` hides the popup. As an update, it sends every node, changed or not.
fn window(popup: Option<(Role, NodeId)>, hidden: bool) -> TreeUpdate {
    let mut button = Node::new(Role::Button);
    button.set_label("Review changes");
    let mut nodes = vec![(BUTTON, button)];
    let mut children = vec![BUTTON];
    let mut focus = BUTTON;
    if let Some((role, id)) = popup {
        children.push(id);
        let mut node = Node::new(role);
        if hidden {
            node.set_hidden();
        }
        if role == Role::Menu {
            node.set_label("Row");
            let item_id = NodeId(id.0 + 1);
            node.set_children(vec![item_id]);
            let mut item = Node::new(Role::MenuItem);
            item.set_label("Uninstall");
            nodes.push((item_id, item));
            if !hidden {
                focus = item_id;
            }
        } else {
            node.set_label(TIP);
        }
        nodes.push((id, node));
    }
    let mut root = Node::new(Role::Window);
    root.set_children(children);
    nodes.insert(0, (WINDOW, root));
    TreeUpdate {
        nodes,
        tree: Some(TreeInfo::new(WINDOW)),
        tree_id: TreeId::ROOT,
        focus,
    }
}

/// `window`'s nodes as an update, which leaves the tree's info as it is.
fn update(popup: Option<(Role, NodeId)>, hidden: bool) -> TreeUpdate {
    TreeUpdate {
        tree: None,
        ..window(popup, hidden)
    }
}

/// Runs `f` with the adapter of a harness window on this thread, focused, after a client's first
/// ask has activated it: answered with `initial`, or with a placeholder if it's `None`. The
/// answer isn't handed to anyone, as 7b's `notify.rs` does.
fn with_adapter(initial: Option<TreeUpdate>, f: impl FnOnce(&mut crate::Adapter)) {
    let _lock = MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let window = create_window("Menu events test", Initial(None), NullActionHandler).unwrap();
    {
        // SAFETY: the harness's window procedure set this window's state at WM_NCCREATE, and it
        // lives until WM_DESTROY, below.
        let state = unsafe { &*get_window_state(window) };
        let mut adapter = state.adapter.borrow_mut();
        // As if the window had the keyboard focus, which a hidden one never gets: the adapter
        // raises focus events only then.
        assert!(adapter.update_window_focus_state(true).is_none());
        let root = LPARAM(UiaRootObjectId as isize);
        let answer = adapter.handle_wm_getobject(WPARAM(0), root, &mut Initial(initial));
        assert!(answer.is_some(), "the adapter answers for its root");
        f(&mut adapter);
    }
    // SAFETY: the window is this thread's, and nothing of it is borrowed any more.
    unsafe { DestroyWindow(window) }.unwrap();
    // Take the WM_QUIT that destroying it posted.
    let mut message = MSG::default();
    // SAFETY: `message` is a live local the call fills.
    while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.into() {}
}

type Raised = Vec<(UIA_EVENT_ID, IRawElementProviderSimple)>;

/// The events `events` raises, in order, each with its element; property changes left out.
fn raised(events: Option<QueuedEvents>) -> Raised {
    let mut out = Vec::new();
    if let Some(events) = events {
        let refused = events.raise_with(
            |element, id| {
                out.push((id, element.clone()));
                Ok(())
            },
            |_, _, _, _| Ok(()),
        );
        assert_eq!(refused, 0);
    }
    out
}

const OPEN_CLOSE: [UIA_EVENT_ID; 4] = [
    UIA_MenuOpenedEventId,
    UIA_MenuClosedEventId,
    UIA_ToolTipOpenedEventId,
    UIA_ToolTipClosedEventId,
];

/// The opened and closed events among `events`, in order.
fn open_close(events: &Raised) -> Raised {
    events
        .iter()
        .filter(|(id, _)| OPEN_CLOSE.contains(id))
        .cloned()
        .collect()
}

fn ids(events: &Raised) -> Vec<i32> {
    events.iter().map(|(id, _)| id.0).collect()
}

/// A popup of `role` added raises `opened` on it, and removed raises `closed` on the same
/// platform node, which a client that saw it open may hold; a change to it that a client still
/// sees raises neither.
fn added_then_removed(role: Role, opened: UIA_EVENT_ID, closed: UIA_EVENT_ID, label: &str) {
    with_adapter(Some(window(None, false)), |adapter| {
        let events = raised(adapter.update_if_active(|| update(Some((role, POPUP)), false)));
        let open = open_close(&events);
        assert_eq!(ids(&open), [opened.0], "{role:?} added");
        let element = open[0].1.clone();
        let wanted = if role == Role::Menu {
            UIA_MenuControlTypeId
        } else {
            UIA_ToolTipControlTypeId
        };
        assert_eq!(control_type(&element), wanted);
        assert_eq!(name(&element), label);
        if role == Role::Menu {
            // Opened before the focus moves into it, as Windows' own menus raise them.
            assert_eq!(
                ids(&events),
                [opened.0, UIA_AutomationFocusChangedEventId.0]
            );
        }

        // A change a client still sees: no event of either kind.
        let mut renamed = update(Some((role, POPUP)), false);
        for (id, node) in &mut renamed.nodes {
            if *id == POPUP {
                node.set_label("Renamed");
            }
        }
        let events = raised(adapter.update_if_active(|| renamed));
        assert!(open_close(&events).is_empty(), "{role:?} renamed");

        let events = raised(adapter.update_if_active(|| update(None, false)));
        let shut = open_close(&events);
        assert_eq!(ids(&shut), [closed.0], "{role:?} removed");
        assert_eq!(
            shut[0].1.as_raw(),
            element.as_raw(),
            "closed on the platform node it opened on"
        );
        // The node is gone: a client reads nothing more of it than its runtime id.
        // SAFETY: a provider call in process, as a client's would be.
        let gone = unsafe { shut[0].1.GetPropertyValue(UIA_NamePropertyId) };
        assert_eq!(
            gone.map(|_| ()).map_err(|e| e.code()),
            Err(HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32))
        );
    });
}

#[test]
#[ignore = "slice 7c: lands with the code"]
fn a_menu_added_opens_and_removed_closes_on_its_platform_node() {
    added_then_removed(
        Role::Menu,
        UIA_MenuOpenedEventId,
        UIA_MenuClosedEventId,
        "Row",
    );
}

#[test]
#[ignore = "slice 7c: lands with the code"]
fn a_tooltip_added_opens_and_removed_closes_on_its_platform_node() {
    added_then_removed(
        Role::Tooltip,
        UIA_ToolTipOpenedEventId,
        UIA_ToolTipClosedEventId,
        TIP,
    );
}

#[test]
#[ignore = "slice 7c: lands with the code"]
fn a_popup_kept_for_its_next_open_raises_the_pair_at_every_open() {
    // The toolkit keeps a popup's window, and builds its content anew at each open (TD-0009
    // A1.5), so each open is a remove and an add.
    with_adapter(Some(window(None, false)), |adapter| {
        let menu = Some((Role::Menu, POPUP));
        let again = Some((Role::Menu, POPUP_AGAIN));
        let mut seen = Vec::new();
        for popup in [menu, None, again, None] {
            let events = raised(adapter.update_if_active(|| update(popup, false)));
            seen.extend(ids(&open_close(&events)));
        }
        let (opened, closed) = (UIA_MenuOpenedEventId.0, UIA_MenuClosedEventId.0);
        assert_eq!(seen, [opened, closed, opened, closed]);
    });
}

#[test]
#[ignore = "slice 7c: lands with the code"]
fn a_popup_replaced_in_one_update_closes_before_the_new_one_opens() {
    // A tip moving straight to another node: the toolkit builds it as a new node in the same
    // update, which the tree reports as an addition before the removal.
    with_adapter(Some(window(None, false)), |adapter| {
        let first = Some((Role::Tooltip, POPUP));
        let opened = open_close(&raised(adapter.update_if_active(|| update(first, false))));
        let second = Some((Role::Tooltip, POPUP_AGAIN));
        let events = open_close(&raised(adapter.update_if_active(|| update(second, false))));
        assert_eq!(
            ids(&events),
            [UIA_ToolTipClosedEventId.0, UIA_ToolTipOpenedEventId.0]
        );
        assert_eq!(events[0].1.as_raw(), opened[0].1.as_raw());
        assert_eq!(name(&events[1].1), TIP);
    });
    // A menu replaced: closed, opened, then the focus moves into the new one.
    with_adapter(Some(window(Some((Role::Menu, POPUP)), false)), |adapter| {
        let again = Some((Role::Menu, POPUP_AGAIN));
        let events = raised(adapter.update_if_active(|| update(again, false)));
        assert_eq!(
            ids(&events),
            [
                UIA_MenuClosedEventId.0,
                UIA_MenuOpenedEventId.0,
                UIA_AutomationFocusChangedEventId.0
            ]
        );
    });
}

#[test]
#[ignore = "slice 7c: lands with the code"]
fn a_menu_hidden_closes_and_shown_again_opens() {
    with_adapter(Some(window(Some((Role::Menu, POPUP)), false)), |adapter| {
        let menu = Some((Role::Menu, POPUP));
        let hidden = open_close(&raised(adapter.update_if_active(|| update(menu, true))));
        assert_eq!(ids(&hidden), [UIA_MenuClosedEventId.0]);
        let shown = open_close(&raised(adapter.update_if_active(|| update(menu, false))));
        assert_eq!(ids(&shown), [UIA_MenuOpenedEventId.0]);
        assert_eq!(hidden[0].1.as_raw(), shown[0].1.as_raw());
    });
}

#[test]
#[ignore = "slice 7c: lands with the code"]
fn the_whole_tree_after_a_placeholder_opens_the_popups_it_holds() {
    // The first ask found the handler busy: a placeholder, then the whole tree as an update.
    with_adapter(None, |adapter| {
        let menu = Some((Role::Menu, POPUP));
        let events = raised(adapter.update_if_active(|| window(menu, false)));
        assert_eq!(
            ids(&events),
            [UIA_MenuOpenedEventId.0, UIA_AutomationFocusChangedEventId.0]
        );
        assert_eq!(name(&events[0].1), "Row");
    });
    with_adapter(None, |adapter| {
        let tip = Some((Role::Tooltip, POPUP));
        let events = raised(adapter.update_if_active(|| window(tip, false)));
        assert_eq!(ids(&open_close(&events)), [UIA_ToolTipOpenedEventId.0]);
    });
    // A hidden popup isn't one a client sees open.
    with_adapter(None, |adapter| {
        let menu = Some((Role::Menu, POPUP));
        let events = raised(adapter.update_if_active(|| window(menu, true)));
        assert!(open_close(&events).is_empty());
    });
}

#[test]
#[ignore = "slice 7c: lands with the code"]
fn a_refused_menu_opened_is_counted_and_the_focus_after_it_still_raised() {
    with_adapter(Some(window(None, false)), |adapter| {
        let events = adapter
            .update_if_active(|| update(Some((Role::Menu, POPUP)), false))
            .unwrap();
        let mut raised = Vec::new();
        let refused = events.raise_with(
            |_, id| -> Result<()> {
                raised.push(id.0);
                if id == UIA_MenuOpenedEventId {
                    Err(E_FAIL.into())
                } else {
                    Ok(())
                }
            },
            |_, _, _, _| Ok(()),
        );
        assert_eq!(refused, 1);
        assert_eq!(
            raised,
            [UIA_MenuOpenedEventId.0, UIA_AutomationFocusChangedEventId.0]
        );
    });
}

#[test]
fn the_pair_goes_through_ui_automation_without_a_panic() {
    with_adapter(Some(window(None, false)), |adapter| {
        let mut refused = 0;
        let menu = Some((Role::Menu, POPUP));
        let tip = Some((Role::Tooltip, POPUP_AGAIN));
        for popup in [menu, None, tip, None] {
            if let Some(events) = adapter.update_if_active(|| update(popup, false)) {
                refused += events.raise();
            }
        }
        // No client listens, so UI Automation has no one to refuse them for; a refusal would
        // still be counted, not a panic. Measured in T2: see the line this prints.
        eprintln!("menu and tooltip events refused: {refused}");
        assert!(refused <= 8, "{refused}");
    });
}
