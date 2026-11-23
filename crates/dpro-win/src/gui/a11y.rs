//! UI Automation's side of the window (TD-0009 §5.3, §5.5): the queue that carries clients'
//! requests from UI Automation's threads to the UI thread, the realizer that builds a table's row
//! a client asks for while it waits (A2.5), and the glue that asks the handler for its tree on a
//! client's first request.

use std::{
    cell::RefMut,
    ffi::c_void,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use accesskit_windows::{Realize, Realized, Realizer};
use windows_sys::Win32::{
    Foundation::HWND,
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{
        GetWindowThreadProcessId, PostMessageW, SMTO_ABORTIFHUNG, SMTO_ERRORONEXIT,
        SendMessageTimeoutW, WM_APP,
    },
};

use super::{Handler, Surface};

/// Posted to the window when requests are waiting; one per drain, however many arrive.
pub(super) const WM_APP_A11Y: u32 = WM_APP + 1;
/// Sent to the window while a client waits for a row (TD-0009 A2.5). Its `WPARAM` is the
/// realize's number, never a pointer: a send that times out is still delivered, later.
pub(super) const WM_APP_REALIZE: u32 = WM_APP + 3;
/// Requests waiting at most. More are dropped: a client gone wild must not grow memory, and no
/// person issues 64 commands between two turns of the message loop.
const QUEUE_CAP: usize = 64;
/// How long a client waits for the UI thread to build a row (TD-0009 A2.5). A hung thread
/// fails the send at once.
pub(super) const REALIZE_TIMEOUT_MS: u32 = 500;

/// The realize a client waits on, between its thread and the UI thread.
#[derive(Debug, Default)]
struct Pending {
    /// Its number, 0 when none waits; a late message with another number does nothing.
    id: usize,
    request: Option<Realize>,
    answer: Option<Realized>,
}

/// Clients' requests on their way to the UI thread.
#[derive(Debug, Default)]
pub(super) struct Requests {
    queue: Mutex<Vec<ActionRequest>>,
    /// A wake message is posted and the UI thread hasn't drained since.
    woken: AtomicBool,
    /// The window to wake, as its handle's address; 0 before it exists and once it's destroyed.
    hwnd: AtomicUsize,
    /// The realize in flight.
    pending: Mutex<Pending>,
    /// Held while a client waits on a realize: one at a time. The UI thread never takes it.
    realizing: Mutex<()>,
    /// The last realize's number.
    numbered: AtomicUsize,
}

/// `m` locked, its poison ignored: every holder leaves its data whole.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
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

impl Requests {
    /// Has the UI thread build `request`'s row, on a client's thread, and waits for its answer:
    /// at most [`REALIZE_TIMEOUT_MS`]. [`Realized::Unavailable`] without a window, on the UI
    /// thread itself (an in-process client must not re-enter it), or if it didn't answer.
    pub(super) fn realize(&self, request: &Realize) -> Realized {
        let hwnd = self.hwnd.load(Ordering::Acquire);
        if hwnd == 0 {
            return Realized::Unavailable;
        }
        let hwnd: HWND = std::ptr::with_exposed_provenance_mut::<c_void>(hwnd);
        // SAFETY: reading a window's thread needs no access to it; a destroyed window reads 0.
        let thread = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
        // SAFETY: no preconditions.
        if thread == 0 || thread == unsafe { GetCurrentThreadId() } {
            return Realized::Unavailable;
        }
        let _one = lock(&self.realizing);
        let id = self
            .numbered
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1)
            .max(1);
        *lock(&self.pending) = Pending {
            id,
            request: Some(request.clone()),
            answer: None,
        };
        let mut result = 0;
        // SAFETY: a window destroyed meanwhile fails the call. The message carries a number, and
        // `result` outlives the call, which writes it only before it returns.
        let sent = unsafe {
            SendMessageTimeoutW(
                hwnd,
                WM_APP_REALIZE,
                id,
                0,
                SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                REALIZE_TIMEOUT_MS,
                &raw mut result,
            )
        };
        // Whatever happened, nothing waits now: a late message finds no realize to run.
        let answer = std::mem::take(&mut *lock(&self.pending)).answer;
        if sent == 0 {
            return Realized::Unavailable;
        }
        answer.unwrap_or(Realized::Unavailable)
    }

    /// The realize numbered `id`, on the UI thread, if its client still waits.
    pub(super) fn realize_request(&self, id: usize) -> Option<Realize> {
        let pending = lock(&self.pending);
        if pending.id == id && id != 0 {
            pending.request.clone()
        } else {
            None
        }
    }

    /// The UI thread's answer to the realize numbered `id`, its tree update already served. Its
    /// client may have stopped waiting; then it's dropped.
    pub(super) fn realize_answer(&self, id: usize, answer: Realized) {
        let mut pending = lock(&self.pending);
        if pending.id == id && id != 0 {
            pending.answer = Some(answer);
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

/// The realizer `accesskit_windows` calls on UI Automation's threads, with no lock of its own
/// held (TD-0009 A2.5).
pub(super) struct Realizing(pub(super) Arc<Requests>);

impl Realizer for Realizing {
    fn realize(&self, request: &Realize) -> Realized {
        self.0.realize(request)
    }
}

/// A client's first request in a window (the main one or a popup): asks the handler for that
/// surface's tree if it's free, and remembers whether the adapter got one.
pub(super) struct Initial<'a> {
    pub(super) handler: Option<RefMut<'a, Box<dyn Handler>>>,
    pub(super) surface: Surface,
    /// The adapter asked and got nothing: it serves a placeholder until it gets a whole tree.
    pub(super) placeholder: bool,
}

impl ActivationHandler for Initial<'_> {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        let surface = self.surface;
        let tree = self.handler.as_mut().and_then(|h| h.a11y_tree(surface));
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
