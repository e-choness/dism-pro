// Copyright 2022 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

use accesskit::{
    ActionHandler, ActivationHandler, Live, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate,
};
use accesskit_consumer::{FilterResult, FullNodeId, NodeRef, Tree, TreeChangeHandler, TreeState};
use hashbrown::{HashMap, HashSet};
use std::fmt::{Debug, Formatter};
use std::sync::{Arc, atomic::Ordering};
use windows::Win32::{
    Foundation::*,
    UI::{Accessibility::*, WindowsAndMessaging::*},
};

use crate::{
    context::{ActionHandlerNoMut, ActionHandlerWrapper, Context},
    filters::filter,
    node::{NodeWrapper, PlatformNode},
    util::{QueuedEvent, StringBuffer},
    window_handle::WindowHandle,
};
// dpro: a table's rows realized on request (PATCHES.md, 6).
use crate::realize::{Realizer, RealizingHandler};
// dpro: notifications (PATCHES.md, 7).
use std::sync::OnceLock;
use windows::{
    Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
    core::{BSTR, HRESULT, Interface, s, w},
};

fn focus_event(context: &Arc<Context>, node_id: FullNodeId) -> QueuedEvent {
    let platform_node = context.get_or_create_platform_node(node_id);
    let element: IRawElementProviderSimple = platform_node.into_interface();
    QueuedEvent::Simple {
        element,
        event_id: UIA_AutomationFocusChangedEventId,
    }
}

struct AdapterChangeHandler<'a> {
    context: &'a Arc<Context>,
    queue: Vec<QueuedEvent>,
    text_changed: HashSet<FullNodeId>,
    selection_changed: HashMap<FullNodeId, SelectionChanges>,
}

impl<'a> AdapterChangeHandler<'a> {
    fn new(context: &'a Arc<Context>) -> Self {
        Self {
            context,
            queue: Vec::new(),
            text_changed: HashSet::new(),
            selection_changed: HashMap::new(),
        }
    }
}

impl AdapterChangeHandler<'_> {
    fn insert_text_change_if_needed_parent(&mut self, node: NodeRef) {
        if !node.supports_text_ranges() {
            return;
        }
        let id = node.id();
        if self.text_changed.contains(&id) {
            return;
        }
        let platform_node = self.context.get_or_create_platform_node(node.id());
        let element: IRawElementProviderSimple = platform_node.into_interface();
        // Text change events must come before selection change
        // events. It doesn't matter if text change events come
        // before other events.
        self.queue.insert(
            0,
            QueuedEvent::Simple {
                element,
                event_id: UIA_Text_TextChangedEventId,
            },
        );
        self.text_changed.insert(id);
    }

    fn insert_text_change_if_needed(&mut self, node: &NodeRef) {
        if node.role() != Role::TextRun {
            return;
        }
        if let Some(node) = node.filtered_parent(&filter) {
            self.insert_text_change_if_needed_parent(node);
        }
    }

    fn element_for_possibly_removed_node(
        &self,
        tree_state: &TreeState,
        id: FullNodeId,
    ) -> IRawElementProviderSimple {
        if tree_state.node_by_id(id).is_some() {
            self.context
                .get_or_create_platform_node(id)
                .into_interface()
        } else {
            PlatformNode::new(self.context, id).into_interface()
        }
    }

    fn handle_selection_state_change(&mut self, node: &NodeRef, is_selected: bool) {
        // If `node` belongs to a selection container, then map the events with the
        // selection container as the key because |FinalizeSelectionEvents| needs to
        // determine whether or not there is only one element selected in order to
        // optimize what platform events are sent.
        let key = if let Some(container) = node.selection_container(&filter) {
            container.id()
        } else {
            node.id()
        };

        let changes = self
            .selection_changed
            .entry(key)
            .or_insert_with(|| SelectionChanges {
                added_items: HashSet::new(),
                removed_items: HashSet::new(),
            });
        if is_selected {
            changes.added_items.insert(node.id());
        } else {
            changes.removed_items.insert(node.id());
        }
    }

    fn enqueue_selection_changes(&mut self, tree: &Tree) {
        let tree_state = tree.state();
        for (id, changes) in self.selection_changed.iter() {
            let Some(node) = tree_state.node_by_id(*id) else {
                continue;
            };
            // Determine if `node` is a selection container with one selected child in
            // order to optimize what platform events are sent.
            let mut container = None;
            let mut only_selected_child = None;
            if node.is_container_with_selectable_children() {
                container = Some(node);
                for child in node.filtered_children(filter) {
                    if let Some(true) = child.is_selected() {
                        if only_selected_child.is_none() {
                            only_selected_child = Some(child);
                        } else {
                            only_selected_child = None;
                            break;
                        }
                    }
                }
            }

            if let Some(only_selected_child) = only_selected_child {
                self.queue.push(QueuedEvent::Simple {
                    element: self
                        .context
                        .get_or_create_platform_node(only_selected_child.id())
                        .into_interface(),
                    event_id: UIA_SelectionItem_ElementSelectedEventId,
                });
                self.queue.push(QueuedEvent::PropertyChanged {
                    element: self
                        .context
                        .get_or_create_platform_node(only_selected_child.id())
                        .into_interface(),
                    property_id: UIA_SelectionItemIsSelectedPropertyId,
                    old_value: false.into(),
                    new_value: true.into(),
                });
                for child_id in changes.removed_items.iter() {
                    let element = self.element_for_possibly_removed_node(tree_state, *child_id);
                    self.queue.push(QueuedEvent::PropertyChanged {
                        element,
                        property_id: UIA_SelectionItemIsSelectedPropertyId,
                        old_value: true.into(),
                        new_value: false.into(),
                    });
                }
            } else {
                // Per UIA documentation, beyond the "invalidate limit" we're supposed to
                // fire a 'SelectionInvalidated' event.  The exact value isn't specified,
                // but System.Windows.Automation.Provider uses a value of 20.
                const INVALIDATE_LIMIT: usize = 20;
                if let Some(container) = container.filter(|_| {
                    changes.added_items.len() + changes.removed_items.len() > INVALIDATE_LIMIT
                }) {
                    let platform_node = self.context.get_or_create_platform_node(container.id());
                    self.queue.push(QueuedEvent::Simple {
                        element: platform_node.into_interface(),
                        event_id: UIA_Selection_InvalidatedEventId,
                    });
                } else {
                    let container_is_multiselectable =
                        container.is_some_and(|c| c.is_multiselectable());
                    for added_id in changes.added_items.iter() {
                        self.queue.push(QueuedEvent::Simple {
                            element: self
                                .context
                                .get_or_create_platform_node(*added_id)
                                .into_interface(),
                            event_id: match container_is_multiselectable {
                                true => UIA_SelectionItem_ElementAddedToSelectionEventId,
                                false => UIA_SelectionItem_ElementSelectedEventId,
                            },
                        });
                        self.queue.push(QueuedEvent::PropertyChanged {
                            element: self
                                .context
                                .get_or_create_platform_node(*added_id)
                                .into_interface(),
                            property_id: UIA_SelectionItemIsSelectedPropertyId,
                            old_value: false.into(),
                            new_value: true.into(),
                        });
                    }
                    for removed_id in changes.removed_items.iter() {
                        let element =
                            self.element_for_possibly_removed_node(tree_state, *removed_id);
                        self.queue.push(QueuedEvent::Simple {
                            element: element.clone(),
                            event_id: UIA_SelectionItem_ElementRemovedFromSelectionEventId,
                        });
                        self.queue.push(QueuedEvent::PropertyChanged {
                            element,
                            property_id: UIA_SelectionItemIsSelectedPropertyId,
                            old_value: true.into(),
                            new_value: false.into(),
                        });
                    }
                }
            }
        }
    }
}

struct SelectionChanges {
    added_items: HashSet<FullNodeId>,
    removed_items: HashSet<FullNodeId>,
}

impl TreeChangeHandler for AdapterChangeHandler<'_> {
    fn node_added(&mut self, node: &NodeRef) {
        self.insert_text_change_if_needed(node);
        if filter(node) != FilterResult::Include {
            return;
        }
        let mut buffer = StringBuffer::acquire();
        let mut wrapper = NodeWrapper {
            node,
            string_buffer: &mut buffer,
        };
        if node.is_dialog() {
            let platform_node = self.context.get_or_create_platform_node(node.id());
            let element: IRawElementProviderSimple = platform_node.into_interface();
            self.queue.push(QueuedEvent::Simple {
                element,
                event_id: UIA_Window_WindowOpenedEventId,
            });
        }
        if wrapper.name().is_some() && node.live() != Live::Off {
            let platform_node = self.context.get_or_create_platform_node(node.id());
            let element: IRawElementProviderSimple = platform_node.into_interface();
            self.queue.push(QueuedEvent::Simple {
                element,
                event_id: UIA_LiveRegionChangedEventId,
            });
        }
        if wrapper.is_selection_item_pattern_supported() && wrapper.is_selected() {
            self.handle_selection_state_change(node, true);
        }
    }

    fn node_updated(&mut self, old_node: &NodeRef, new_node: &NodeRef) {
        if old_node.raw_value() != new_node.raw_value() {
            self.insert_text_change_if_needed(new_node);
        }
        let old_node_was_filtered_out = filter(old_node) != FilterResult::Include;
        if filter(new_node) != FilterResult::Include {
            if !old_node_was_filtered_out {
                if old_node.is_dialog() {
                    let platform_node = self.context.get_or_create_platform_node(old_node.id());
                    let element: IRawElementProviderSimple = platform_node.into_interface();
                    self.queue.push(QueuedEvent::Simple {
                        element,
                        event_id: UIA_Window_WindowClosedEventId,
                    });
                }
                let mut old_buffer = StringBuffer::acquire();
                let old_wrapper = NodeWrapper {
                    node: old_node,
                    string_buffer: &mut old_buffer,
                };
                if old_wrapper.is_selection_item_pattern_supported() && old_wrapper.is_selected() {
                    self.handle_selection_state_change(old_node, false);
                }
            }
            return;
        }
        let platform_node = self.context.get_or_create_platform_node(new_node.id());
        let element: IRawElementProviderSimple = platform_node.into_interface();
        let mut old_buffer = StringBuffer::acquire();
        let mut old_wrapper = NodeWrapper {
            node: old_node,
            string_buffer: &mut old_buffer,
        };
        let mut new_buffer = StringBuffer::acquire();
        let mut new_wrapper = NodeWrapper {
            node: new_node,
            string_buffer: &mut new_buffer,
        };
        new_wrapper.enqueue_property_changes(
            &mut self.queue,
            self.context,
            &element,
            &mut old_wrapper,
        );
        let new_name = new_wrapper.name();
        if new_name.is_some()
            && new_node.live() != Live::Off
            && (new_node.live() != old_node.live()
                || old_node_was_filtered_out
                || new_name != old_wrapper.name())
        {
            self.queue.push(QueuedEvent::Simple {
                element,
                event_id: UIA_LiveRegionChangedEventId,
            });
        }
        if old_node_was_filtered_out && new_node.is_dialog() {
            let platform_node = self.context.get_or_create_platform_node(new_node.id());
            let element: IRawElementProviderSimple = platform_node.into_interface();
            self.queue.push(QueuedEvent::Simple {
                element,
                event_id: UIA_Window_WindowOpenedEventId,
            });
        }
        if new_wrapper.is_selection_item_pattern_supported()
            && (new_wrapper.is_selected() != old_wrapper.is_selected()
                || (old_node_was_filtered_out && new_wrapper.is_selected()))
        {
            self.handle_selection_state_change(new_node, new_wrapper.is_selected());
        }
    }

    fn focus_moved(&mut self, _old_node: Option<&NodeRef>, new_node: Option<&NodeRef>) {
        if let Some(new_node) = new_node {
            self.queue.push(focus_event(self.context, new_node.id()));
        }
    }

    fn node_removed(&mut self, node: &NodeRef) {
        self.insert_text_change_if_needed(node);
        self.context.remove_platform_node(node.id());
        if filter(node) != FilterResult::Include {
            return;
        }
        if node.is_dialog() {
            let platform_node = PlatformNode::new(self.context, node.id());
            let element: IRawElementProviderSimple = platform_node.into_interface();
            self.queue.push(QueuedEvent::Simple {
                element,
                event_id: UIA_Window_WindowClosedEventId,
            });
        }
        let mut buffer = StringBuffer::acquire();
        let wrapper = NodeWrapper {
            node,
            string_buffer: &mut buffer,
        };
        if wrapper.is_selection_item_pattern_supported() {
            self.handle_selection_state_change(node, false);
        }
    }

    // TODO: handle other events (#20)
}

const PLACEHOLDER_ROOT_ID: NodeId = NodeId(0);

enum State {
    Inactive {
        hwnd: WindowHandle,
        is_window_focused: bool,
        action_handler: Arc<dyn ActionHandlerNoMut + Send + Sync>,
    },
    Placeholder(Arc<Context>),
    Active(Arc<Context>),
}

impl Debug for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            State::Inactive {
                hwnd,
                is_window_focused,
                action_handler: _,
            } => f
                .debug_struct("Inactive")
                .field("hwnd", hwnd)
                .field("is_window_focused", is_window_focused)
                .field("action_handler", &"ActionHandler")
                .finish(),
            State::Placeholder(context) => f.debug_tuple("Placeholder").field(context).finish(),
            State::Active(context) => f.debug_tuple("Active").field(context).finish(),
        }
    }
}

#[derive(Debug)]
pub struct Adapter {
    state: State,
}

impl Adapter {
    /// Creates a new Windows platform adapter.
    ///
    /// The action handler may or may not be called on the thread that owns
    /// the window.
    ///
    /// This must not be called while handling the `WM_GETOBJECT` message,
    /// because this function must initialize UI Automation before
    /// that message is handled. This is necessary to prevent a race condition
    /// that leads to nested `WM_GETOBJECT` messages and, in some cases,
    /// assistive technologies not realizing that the window natively implements.
    /// UIA. See [AccessKit issue #37](https://github.com/AccessKit/accesskit/issues/37)
    /// for more details.
    pub fn new(
        hwnd: HWND,
        is_window_focused: bool,
        action_handler: impl 'static + ActionHandler + Send,
    ) -> Self {
        Self::with_wrapped_action_handler(
            hwnd,
            is_window_focused,
            Arc::new(ActionHandlerWrapper::new(action_handler)),
        )
    }

    // dpro: an adapter whose virtualized tables realize their rows on request (PATCHES.md, 6).
    /// Creates a new Windows platform adapter, as [`new`](Self::new) does, whose tables realize
    /// the rows a client asks for through `realizer`.
    pub fn with_realizer(
        hwnd: HWND,
        is_window_focused: bool,
        action_handler: impl 'static + ActionHandler + Send,
        realizer: impl 'static + Realizer,
    ) -> Self {
        Self::with_wrapped_action_handler(
            hwnd,
            is_window_focused,
            Arc::new(RealizingHandler::new(action_handler, realizer)),
        )
    }

    // Currently required by the test infrastructure
    pub(crate) fn with_wrapped_action_handler(
        hwnd: HWND,
        is_window_focused: bool,
        action_handler: Arc<dyn ActionHandlerNoMut + Send + Sync>,
    ) -> Self {
        init_uia();

        let state = State::Inactive {
            hwnd: hwnd.into(),
            is_window_focused,
            action_handler,
        };
        Self { state }
    }

    /// If and only if the tree has been initialized, call the provided function
    /// and apply the resulting update. Note: If the caller's implementation of
    /// [`ActivationHandler::request_initial_tree`] initially returned `None`,
    /// the [`TreeUpdate`] returned by the provided function must contain
    /// a full tree.
    ///
    /// If a [`QueuedEvents`] instance is returned, the caller must call
    /// [`QueuedEvents::raise`] on it.
    ///
    /// This method may be safely called on any thread, but refer to
    /// [`QueuedEvents::raise`] for restrictions on the context in which
    /// it should be called.
    pub fn update_if_active(
        &mut self,
        update_factory: impl FnOnce() -> TreeUpdate,
    ) -> Option<QueuedEvents> {
        match &self.state {
            State::Inactive { .. } => None,
            State::Placeholder(context) => {
                let is_window_focused = context.read_tree().state().is_host_focused();
                let tree = Tree::new(update_factory(), is_window_focused);
                *context.tree.write().unwrap() = tree;
                context.is_placeholder.store(false, Ordering::SeqCst);
                let result = context
                    .read_tree()
                    .state()
                    .focus()
                    .map(|node| QueuedEvents(vec![focus_event(context, node.id())]));
                self.state = State::Active(Arc::clone(context));
                result
            }
            State::Active(context) => {
                let mut handler = AdapterChangeHandler::new(context);
                let mut tree = context.tree.write().unwrap();
                tree.update_and_process_changes(update_factory(), &mut handler);
                handler.enqueue_selection_changes(&tree);
                Some(QueuedEvents(handler.queue))
            }
        }
    }

    /// Update the tree state based on whether the window is focused.
    ///
    /// If a [`QueuedEvents`] instance is returned, the caller must call
    /// [`QueuedEvents::raise`] on it.
    ///
    /// This method may be safely called on any thread, but refer to
    /// [`QueuedEvents::raise`] for restrictions on the context in which
    /// it should be called.
    pub fn update_window_focus_state(&mut self, is_focused: bool) -> Option<QueuedEvents> {
        match &mut self.state {
            State::Inactive {
                is_window_focused, ..
            } => {
                *is_window_focused = is_focused;
                None
            }
            State::Placeholder(context) => {
                let mut handler = AdapterChangeHandler::new(context);
                let mut tree = context.tree.write().unwrap();
                tree.update_host_focus_state_and_process_changes(is_focused, &mut handler);
                Some(QueuedEvents(handler.queue))
            }
            State::Active(context) => {
                let mut handler = AdapterChangeHandler::new(context);
                let mut tree = context.tree.write().unwrap();
                tree.update_host_focus_state_and_process_changes(is_focused, &mut handler);
                Some(QueuedEvents(handler.queue))
            }
        }
    }

    // dpro: notifications (PATCHES.md, 7; TD-0009 A4.5).
    /// A UI Automation notification on the tree's root: `text`, spoken without a focus move, of
    /// `kind`, and superseding earlier ones of `activity` as `processing` says.
    ///
    /// Returns `None` while no client has asked for the tree, and where Windows has no
    /// `UiaRaiseNotificationEvent` (before Windows 10 1709). The function is looked up once in
    /// the loaded `uiautomationcore.dll`, never imported, so the exe still starts there.
    ///
    /// If a [`QueuedEvents`] instance is returned, the caller must call
    /// [`QueuedEvents::raise`] on it.
    pub fn notify(
        &mut self,
        kind: NotificationKind,
        processing: NotificationProcessing,
        text: &str,
        activity: &str,
    ) -> Option<QueuedEvents> {
        self.notify_if(
            raise_notification_fn().is_some(),
            kind,
            processing,
            text,
            activity,
        )
    }

    // dpro: `notify`, told whether the function exists, so a test can take the path where it
    // doesn't (PATCHES.md, 7).
    pub(crate) fn notify_if(
        &mut self,
        available: bool,
        kind: NotificationKind,
        processing: NotificationProcessing,
        text: &str,
        activity: &str,
    ) -> Option<QueuedEvents> {
        let State::Active(context) = &self.state else {
            return None;
        };
        if !available {
            return None;
        }
        let root = context.read_tree().state().root_id();
        let element: IRawElementProviderSimple =
            context.get_or_create_platform_node(root).into_interface();
        Some(QueuedEvents(vec![QueuedEvent::Notification {
            element,
            kind,
            processing,
            text: BSTR::from(text),
            activity: BSTR::from(activity),
        }]))
    }

    /// Handle the `WM_GETOBJECT` window message. The accessibility tree
    /// is lazily initialized if necessary using the provided
    /// [`ActivationHandler`] implementation.
    ///
    /// This returns an `Option` so the caller can pass the message
    /// to `DefWindowProc` if AccessKit decides not to handle it.
    /// The optional value is an `Into<LRESULT>` rather than simply an `LRESULT`
    /// so the necessary call to UIA, which may lead to a nested `WM_GETOBJECT`
    /// message, can be done outside of any lock that the caller might hold
    /// on the `Adapter` or window state, while still abstracting away
    /// the details of that call to UIA.
    pub fn handle_wm_getobject<H: ActivationHandler + ?Sized>(
        &mut self,
        wparam: WPARAM,
        lparam: LPARAM,
        activation_handler: &mut H,
    ) -> Option<impl Into<LRESULT> + use<H>> {
        // Don't bother with MSAA object IDs that are asking for something other
        // than the client area of the window. DefWindowProc can handle those.
        // First, cast the lparam to i32, to handle inconsistent conversion
        // behavior in senders.
        let objid = normalize_objid(lparam);
        if objid < 0 && objid != UiaRootObjectId && objid != OBJID_CLIENT.0 {
            return None;
        }

        let (hwnd, platform_node) = match &self.state {
            State::Inactive {
                hwnd,
                is_window_focused,
                action_handler,
            } => match activation_handler.request_initial_tree() {
                Some(initial_state) => {
                    let hwnd = *hwnd;
                    let tree = Tree::new(initial_state, *is_window_focused);
                    let context = Context::new(hwnd, tree, Arc::clone(action_handler), false);
                    let node_id = context.read_tree().state().root_id();
                    let platform_node = context.get_or_create_platform_node(node_id);
                    self.state = State::Active(context);
                    (hwnd, platform_node)
                }
                None => {
                    let hwnd = *hwnd;
                    let placeholder_update = TreeUpdate {
                        nodes: vec![(PLACEHOLDER_ROOT_ID, Node::new(Role::Window))],
                        tree: Some(TreeInfo::new(PLACEHOLDER_ROOT_ID)),
                        tree_id: TreeId::ROOT,
                        focus: PLACEHOLDER_ROOT_ID,
                    };
                    let placeholder_tree = Tree::new(placeholder_update, *is_window_focused);
                    let context =
                        Context::new(hwnd, placeholder_tree, Arc::clone(action_handler), true);
                    let platform_node = PlatformNode::unspecified_root(&context);
                    self.state = State::Placeholder(context);
                    (hwnd, platform_node)
                }
            },
            State::Placeholder(context) => (context.hwnd, PlatformNode::unspecified_root(context)),
            State::Active(context) => {
                let node_id = context.read_tree().state().root_id();
                (context.hwnd, context.get_or_create_platform_node(node_id))
            }
        };
        let el: IRawElementProviderSimple = platform_node.into_interface();
        Some(WmGetObjectResult {
            hwnd,
            wparam,
            lparam,
            el,
        })
    }
}

fn init_uia() {
    // `UiaLookupId` is a cheap way of forcing UIA to initialize itself.
    unsafe {
        UiaLookupId(
            AutomationIdentifierType_Property,
            &ControlType_Property_GUID,
        )
    };
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn normalize_objid(lparam: LPARAM) -> i32 {
    (lparam.0 & 0xFFFFFFFF) as _
}
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn normalize_objid(lparam: LPARAM) -> i32 {
    lparam.0 as _
}

struct WmGetObjectResult {
    hwnd: WindowHandle,
    wparam: WPARAM,
    lparam: LPARAM,
    el: IRawElementProviderSimple,
}

impl From<WmGetObjectResult> for LRESULT {
    fn from(this: WmGetObjectResult) -> Self {
        unsafe { UiaReturnRawElementProvider(this.hwnd.0, this.wparam, this.lparam, &this.el) }
    }
}

/// Events generated by a tree update.
#[must_use = "events must be explicitly raised"]
pub struct QueuedEvents(Vec<QueuedEvent>);

impl QueuedEvents {
    /// Raise all queued events synchronously.
    ///
    /// The window may receive `WM_GETOBJECT` messages during this call.
    /// This means that any locks required by the `WM_GETOBJECT` handler
    /// must not be held when this method is called.
    ///
    /// This method should be called on the thread that owns the window.
    /// It's not clear whether this is a strict requirement of UIA itself,
    /// but based on the known behavior of UIA, MSAA, and some ATs,
    /// it's strongly recommended.
    // dpro: returns how many raises UI Automation refused, instead of panicking on the first
    // (PATCHES.md, 11; TD-0009 A4.5). One lost event must never abort the process, which the
    // release profile's `panic = "abort"` would; the events after it are still raised.
    pub fn raise(self) -> usize {
        self.raise_with(
            |element, event_id| {
                // SAFETY: `element` is a live provider the queue owns; the call only reads it
                // and the event id.
                unsafe { UiaRaiseAutomationEvent(element, event_id) }
            },
            |element, property_id, old_value, new_value| {
                // SAFETY: the provider and both variants are owned by the queue and live for
                // the call, which only reads them.
                unsafe {
                    UiaRaiseAutomationPropertyChangedEvent(
                        element,
                        property_id,
                        old_value,
                        new_value,
                    )
                }
            },
        )
    }

    // dpro: `raise` through the given raisers, so a test can refuse a raise: Wine accepts any
    // event id (PATCHES.md, 11).
    pub(crate) fn raise_with(
        self,
        mut simple: impl FnMut(&IRawElementProviderSimple, UIA_EVENT_ID) -> windows::core::Result<()>,
        mut changed: impl FnMut(
            &IRawElementProviderSimple,
            UIA_PROPERTY_ID,
            &windows::Win32::System::Variant::VARIANT,
            &windows::Win32::System::Variant::VARIANT,
        ) -> windows::core::Result<()>,
    ) -> usize {
        let mut failed = 0;
        for event in self.0 {
            let raised = match event {
                QueuedEvent::Simple { element, event_id } => simple(&element, event_id),
                QueuedEvent::PropertyChanged {
                    element,
                    property_id,
                    old_value,
                    new_value,
                } => changed(&element, property_id, &old_value, &new_value),
                // dpro: notifications (PATCHES.md, 7).
                QueuedEvent::Notification {
                    element,
                    kind,
                    processing,
                    text,
                    activity,
                } => raise_notification(&element, kind, processing, text, activity),
            };
            failed += usize::from(raised.is_err());
        }
        failed
    }
}

// dpro: notifications (PATCHES.md, 7; TD-0009 A4.5).
/// `UiaRaiseNotificationEvent`'s signature (uiautomationcoreapi.h, Windows 10 1709): the
/// provider, the kind, the processing, and the display string and activity id as BSTRs.
pub(crate) type RaiseNotification = unsafe extern "system" fn(
    *mut core::ffi::c_void,
    NotificationKind,
    NotificationProcessing,
    *const u16,
    *const u16,
) -> HRESULT;

/// `UiaRaiseNotificationEvent`, looked up once in the loaded `uiautomationcore.dll`; `None` on a
/// Windows that hasn't it, such as a WinPE older than 1709. Never a static import, which would
/// stop the exe from starting there.
pub(crate) fn raise_notification_fn() -> Option<RaiseNotification> {
    static RAISE: OnceLock<Option<RaiseNotification>> = OnceLock::new();
    *RAISE.get_or_init(|| {
        // SAFETY: a lookup by a static, NUL-terminated name. The adapter's own imports from the
        // DLL keep it loaded for the life of the process, so the handle stays valid.
        let module = unsafe { GetModuleHandleW(w!("uiautomationcore.dll")) }.ok()?;
        // SAFETY: `module` is the handle just returned; the name is static and NUL-terminated.
        let raise = unsafe { GetProcAddress(module, s!("UiaRaiseNotificationEvent")) }?;
        // SAFETY: the export has the documented signature that `RaiseNotification` spells out,
        // and both are `extern "system"` function pointers, of one size.
        Some(unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, RaiseNotification>(raise)
        })
    })
}

/// Raises one notification. The BSTRs are freed after the call.
fn raise_notification(
    element: &IRawElementProviderSimple,
    kind: NotificationKind,
    processing: NotificationProcessing,
    text: BSTR,
    activity: BSTR,
) -> windows::core::Result<()> {
    let Some(raise) = raise_notification_fn() else {
        return Err(E_NOTIMPL.into());
    };
    let (text, activity) = (text.into_raw(), activity.into_raw());
    // SAFETY: the function is `UiaRaiseNotificationEvent` (above). The provider is live for the
    // call, which takes its own reference if it keeps it, and the two BSTRs are valid (or null,
    // an empty string) and owned here until they're freed below.
    let raised = unsafe { raise(element.as_raw(), kind, processing, text, activity) };
    // SAFETY: each pointer came from `BSTR::into_raw` above and is freed once.
    drop(unsafe { (BSTR::from_raw(text), BSTR::from_raw(activity)) });
    raised.ok()
}

// dpro: the raise count's tests build a queue of their own (PATCHES.md, 11).
#[cfg(test)]
impl QueuedEvents {
    pub(crate) fn of(events: Vec<QueuedEvent>) -> Self {
        Self(events)
    }
}

// We explicitly want to allow the queued events to be sent to the UI thread,
// so implement Send even though windows-rs doesn't implement it for all
// contained types. This is safe because we're not using COM threading.
unsafe impl Send for QueuedEvents {}
