// dpro: DISM Pro's tests of the queued events' raising (PATCHES.md, 11; TD-0009 A4.5), under
// the crate's licences: the MIT license or the Apache License, Version 2.0, at your option. Items
// 12 and 13 (MenuOpened and MenuClosed, slice 7c) join this file with their code.

//! `QueuedEvents::raise` counts the raises UI Automation refuses instead of panicking: one lost
//! event must never abort the process, the elevated broker's included.

use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use windows::{
    Win32::{
        Foundation::E_INVALIDARG,
        System::Variant::VARIANT,
        UI::Accessibility::*,
    },
    core::Result,
};

use super::providers::{Requests, context, element};
use crate::{adapter::QueuedEvents, util::QueuedEvent};

const WINDOW: NodeId = NodeId(0);
const BUTTON: NodeId = NodeId(1);

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
