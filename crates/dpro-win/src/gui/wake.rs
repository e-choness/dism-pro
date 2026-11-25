//! Waking the UI thread from another (TD-0009 §5.3): a [`Waker`] posts one message when work
//! finished off the thread (the picker's listing, slice 6e; A5's engine events), and the window
//! sends [`Event::Wake`](super::Event::Wake), on which the handler polls what came.
//!
//! One message at most is on its way: a wake posts only when it flips the flag from clear to
//! set, and the UI thread clears it before the handler hears the event, so a wake meanwhile
//! posts a new one rather than wait for one that already came (the same handshake as UI
//! Automation's requests, `a11y.rs`). A destroyed window drops wakes.

use std::{
    ffi::c_void,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

/// Posted by a [`Waker`]: the window clears the flag and sends `Event::Wake`.
pub(super) const WM_APP_WAKE: u32 = WM_APP + 4;

/// Wakes a window's UI thread from any thread: `Send`, `Sync` and cheap to clone. From
/// [`WindowCx::waker`](super::WindowCx::waker).
#[derive(Clone, Debug)]
pub struct Waker(pub(super) Arc<Wakes>);

impl Waker {
    /// Has the window send `Event::Wake` soon, on its thread, unless a wake is on its way
    /// already. Never waits; after the window is destroyed, does nothing.
    pub fn wake(&self) {
        let wakes = &self.0;
        if wakes.woken.swap(true, Ordering::SeqCst) {
            return;
        }
        let hwnd = wakes.hwnd.load(Ordering::Acquire);
        let posted = hwnd != 0 && {
            let hwnd: HWND = std::ptr::with_exposed_provenance_mut::<c_void>(hwnd);
            // SAFETY: posting to a window needs no access to it from this thread; a destroyed
            // window fails the call.
            unsafe { PostMessageW(hwnd, WM_APP_WAKE, 0, 0) != 0 }
        };
        if !posted {
            // No wake is on its way, so the next one must try again.
            wakes.woken.store(false, Ordering::SeqCst);
        }
    }
}

/// A window's side of its [`Waker`]s.
#[derive(Debug, Default)]
pub(super) struct Wakes {
    /// A wake message is posted and the UI thread hasn't cleared it since.
    woken: AtomicBool,
    /// The window to wake, as its handle's address; 0 before it exists and once it's destroyed.
    hwnd: AtomicUsize,
}

impl Wakes {
    pub(super) fn attach(&self, hwnd: HWND) {
        self.hwnd.store(hwnd.expose_provenance(), Ordering::Release);
    }

    /// The window is being destroyed: wakes from now on are dropped.
    pub(super) fn detach(&self) {
        self.hwnd.store(0, Ordering::Release);
    }

    /// On the UI thread, before the handler hears the wake: a wake from now on posts again.
    pub(super) fn clear(&self) {
        self.woken.store(false, Ordering::SeqCst);
    }
}
