// dpro: DISM Pro's tests of the queued events' raising (PATCHES.md, planned item 11; TD-0009
// A4.5), under the crate's licences: the MIT license or the Apache License, Version 2.0, at your
// option. Items 12 and 13 (MenuOpened and MenuClosed, slice 7c) join this file with their code.

//! `QueuedEvents::raise` counts the raises UI Automation refuses instead of panicking: one lost
//! event must never abort the process, the elevated broker's included.

use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use windows::Win32::UI::Accessibility::*;

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

#[test]
#[ignore = "slice 6a: lands with patch 11 (PATCHES.md)"]
fn a_refused_raise_is_counted_not_a_panic() {
    let requests = Requests::default();
    let context = context(&requests, tree());
    let button = element(&context, BUTTON);
    // Event id 0 is no UI Automation event, so the raise is refused (E_INVALIDARG). (verify: on
    // Windows and on the T2 image's Wine, when patch 11 lands.)
    let events = QueuedEvents::of(vec![QueuedEvent::Simple {
        element: button,
        event_id: UIA_EVENT_ID(0),
    }]);
    assert_eq!(events.raise(), 1);
}

#[test]
fn an_empty_queue_raises_nothing_and_counts_nothing() {
    assert_eq!(QueuedEvents::of(Vec::new()).raise(), 0);
}
