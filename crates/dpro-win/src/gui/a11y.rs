//! UI Automation's side of the window (TD-0009 §5.3, §5.5): the queue that carries clients'
//! requests from UI Automation's threads to the UI thread, and the glue that asks the handler for
//! its tree on a client's first request.

use std::{
    cell::RefMut,
    ffi::c_void,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

use super::Handler;

/// Posted to the window when requests are waiting; one per drain, however many arrive.
pub(super) const WM_APP_A11Y: u32 = WM_APP + 1;
/// Requests waiting at most. More are dropped: a client gone wild must not grow memory, and no
/// person issues 64 commands between two turns of the message loop.
const QUEUE_CAP: usize = 64;

/// Clients' requests on their way to the UI thread.
#[derive(Debug, Default)]
pub(super) struct Requests {
    queue: Mutex<Vec<ActionRequest>>,
    /// A wake message is posted and the UI thread hasn't drained since.
    woken: AtomicBool,
    /// The window to wake, as its handle's address; 0 before it exists and once it's destroyed.
    hwnd: AtomicUsize,
}

impl Requests {
    pub(super) fn attach(&self, hwnd: HWND) {
        self.hwnd.store(hwnd.expose_provenance(), Ordering::Release);
    }

    /// The window is being destroyed: requests from now on are dropped.
    pub(super) fn detach(&self) {
        self.hwnd.store(0, Ordering::Release);
    }

    /// Takes every waiting request, on the UI thread. The wake flag is cleared first, so a request
    /// pushed meanwhile posts a new wake rather than wait for one that already came.
    pub(super) fn drain(&self) -> Vec<ActionRequest> {
        self.woken.store(false, Ordering::SeqCst);
        self.queue
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }

    /// Queues `request`, on any thread, and wakes the window if it isn't already.
    fn push(&self, request: ActionRequest) {
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };
        if queue.len() >= QUEUE_CAP {
            return;
        }
        queue.push(request);
        drop(queue);
        if self.woken.swap(true, Ordering::SeqCst) {
            return;
        }
        let hwnd = self.hwnd.load(Ordering::Acquire);
        let posted = hwnd != 0 && {
            let hwnd: HWND = std::ptr::with_exposed_provenance_mut::<c_void>(hwnd);
            // SAFETY: posting to a window needs no access to it from this thread; a destroyed
            // window fails the call.
            unsafe { PostMessageW(hwnd, WM_APP_A11Y, 0, 0) != 0 }
        };
        if !posted {
            // No wake is on its way, so the next request must try again.
            self.woken.store(false, Ordering::SeqCst);
        }
    }
}

/// The action handler `accesskit_windows` calls on UI Automation's threads.
pub(super) struct ToUiThread(pub(super) Arc<Requests>);

impl ActionHandler for ToUiThread {
    fn do_action(&mut self, request: ActionRequest) {
        self.0.push(request);
    }
}

/// A client's first request: asks the handler for its tree if it's free, and remembers whether
/// the adapter got one.
pub(super) struct Initial<'a> {
    pub(super) handler: Option<RefMut<'a, Box<dyn Handler>>>,
    /// The adapter asked and got nothing: it serves a placeholder until it gets a whole tree.
    pub(super) placeholder: bool,
}

impl ActivationHandler for Initial<'_> {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        let tree = self.handler.as_mut().and_then(|h| h.a11y_tree());
        self.placeholder = tree.is_none();
        tree
    }
}

#[cfg(test)]
mod tests {
    use accesskit::{Action, NodeId, TreeId};

    use super::*;

    fn click(node: u64) -> ActionRequest {
        ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: NodeId(node),
            data: None,
        }
    }

    #[test]
    fn requests_wait_in_order_and_are_capped() {
        let requests = Requests::default();
        let mut handler = ToUiThread(Arc::new(requests));
        for n in 0..100 {
            handler.do_action(click(n));
        }
        let drained = handler.0.drain();
        assert_eq!(drained.len(), QUEUE_CAP);
        assert_eq!(drained.first(), Some(&click(0)));
        assert_eq!(drained.last(), Some(&click(63)));
        assert_eq!(handler.0.drain().len(), 0);
    }

    #[test]
    fn without_a_window_no_wake_is_pending() {
        let requests = Arc::new(Requests::default());
        let mut handler = ToUiThread(Arc::clone(&requests));
        handler.do_action(click(1));
        // Nothing could be posted, so the flag is down for the next request to try again.
        assert!(!requests.woken.load(Ordering::SeqCst));
        assert_eq!(requests.drain(), vec![click(1)]);
    }
}
