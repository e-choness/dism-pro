// dpro: DISM Pro's tests of notifications (PATCHES.md, 7; TD-0009 A4.5), under the crate's
// licences: the MIT license or the Apache License, Version 2.0, at your option.

//! `Adapter::notify` queues a UI Automation notification on the tree's root once a client has
//! asked for the tree, and nothing before; with `UiaRaiseNotificationEvent` missing (a Windows
//! older than 10 1709), it queues nothing and nothing fails. They run on the window's own thread,
//! with no client, so Wine runs them too (T2).

use accesskit::{ActivationHandler, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use windows::Win32::{
    Foundation::*,
    UI::{Accessibility::*, WindowsAndMessaging::*},
};

use super::{MUTEX, create_window, get_window_state, simple::NullActionHandler};
use crate::adapter::raise_notification_fn;

const WINDOW: NodeId = NodeId(0);
const BUTTON: NodeId = NodeId(1);

struct Tree;

impl ActivationHandler for Tree {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        let mut window = Node::new(Role::Window);
        window.set_children(vec![BUTTON]);
        let mut button = Node::new(Role::Button);
        button.set_label("Apply");
        Some(TreeUpdate {
            nodes: vec![(WINDOW, window), (BUTTON, button)],
            tree: Some(TreeInfo::new(WINDOW)),
            tree_id: TreeId::ROOT,
            focus: BUTTON,
        })
    }
}

/// Runs `f` with a window of the harness on this thread, `asked` whether a client has asked for
/// its tree, and its adapter.
///
/// The ask is `handle_wm_getobject`, as the harness's `WM_GETOBJECT` calls it, but its answer
/// isn't handed to UI Automation (`UiaReturnRawElementProvider`): with no client there's no one to
/// hand it to, and the T2 image's Wine faults in that call from the harness's window (measured:
/// a read of 0xFFFFFFFFFFFFFFEF), which is also why T2 skips upstream's `tests::simple`.
fn with_adapter(asked: bool, f: impl FnOnce(&mut crate::Adapter)) {
    let _lock = MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let window = create_window("Notify test", Tree, NullActionHandler).unwrap();
    {
        // SAFETY: the harness's window procedure set this window's state at WM_NCCREATE, and it
        // lives until WM_DESTROY, below.
        let state = unsafe { &*get_window_state(window) };
        let mut adapter = state.adapter.borrow_mut();
        if asked {
            let root = LPARAM(UiaRootObjectId as isize);
            let answer = adapter.handle_wm_getobject(WPARAM(0), root, &mut Tree);
            assert!(answer.is_some(), "the adapter answers for its root");
        }
        f(&mut adapter);
    }
    // SAFETY: the window is this thread's, and nothing of it is borrowed any more.
    unsafe { DestroyWindow(window) }.unwrap();
    // Take the WM_QUIT that destroying it posted.
    let mut message = MSG::default();
    // SAFETY: `message` is a live local the call fills.
    while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.into() {}
}

fn progress(adapter: &mut crate::Adapter) -> Option<crate::QueuedEvents> {
    adapter.notify(
        NotificationKind_Other,
        NotificationProcessing_MostRecent,
        "Apply 20 changes: 40 percent",
        "DismPro.Progress.0",
    )
}

#[test]
fn nothing_is_queued_before_a_client_asks() {
    with_adapter(false, |adapter| {
        assert!(progress(adapter).is_none());
    });
}

#[test]
fn a_notification_is_queued_on_the_root_and_raised_without_a_panic() {
    with_adapter(true, |adapter| {
        let events = progress(adapter);
        // Windows 10 1709 and later has the function; the T2 image's Wine may not.
        assert_eq!(events.is_some(), raise_notification_fn().is_some());
        eprintln!(
            "UiaRaiseNotificationEvent: {}",
            if raise_notification_fn().is_some() {
                "found"
            } else {
                "missing"
            }
        );
        if let Some(events) = events {
            // No client listens, so UI Automation has no one to refuse it for; a refusal would
            // still be counted, not a panic.
            let refused = events.raise();
            assert!(refused <= 1, "{refused}");
            eprintln!("refused: {refused}");
        }
    });
}

#[test]
fn with_the_function_missing_nothing_is_queued() {
    with_adapter(true, |adapter| {
        let events = adapter.notify_if(
            false,
            NotificationKind_ActionAborted,
            NotificationProcessing_ImportantMostRecent,
            "Released early. Nothing was changed.",
            "DismPro.Hold",
        );
        assert!(events.is_none());
    });
}
