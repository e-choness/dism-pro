//! Win32 event objects: the `CancelEvent` handle DISM takes, signalled from a `CancelToken`
//! (TD-0001 §5.3).

use std::{
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};

use windows_sys::Win32::{
    Foundation::WAIT_OBJECT_0,
    System::Threading::{CreateEventW, SetEvent, WaitForSingleObject},
};

use crate::error::{Result, last_error_if_false, last_os_error};

/// A manual-reset event: once set it stays signalled. `Send + Sync` (via `OwnedHandle`), so a
/// console handler or another thread can set it while a DISM call waits on it.
#[derive(Debug)]
pub struct Event(OwnedHandle);

impl Event {
    /// A new, unsignalled, unnamed manual-reset event.
    ///
    /// # Errors
    /// The OS error from `CreateEventW`.
    pub fn manual_reset() -> Result<Self> {
        // SAFETY: default security, manual reset, initially unsignalled, no name.
        let handle = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if handle.is_null() {
            return Err(last_os_error("CreateEventW").into());
        }
        // SAFETY: `handle` is a new event handle that nothing else owns.
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle) }))
    }

    /// Signals the event.
    ///
    /// # Errors
    /// The OS error from `SetEvent`.
    pub fn set(&self) -> Result<()> {
        // SAFETY: the handle is an open event owned by `self`.
        let ok = unsafe { SetEvent(self.0.as_raw_handle()) };
        last_error_if_false(ok, "SetEvent")
    }

    #[must_use]
    pub fn is_set(&self) -> bool {
        // SAFETY: the handle is an open event owned by `self`; a zero timeout only polls.
        unsafe { WaitForSingleObject(self.0.as_raw_handle(), 0) == WAIT_OBJECT_0 }
    }

    /// The raw handle for APIs that take a `CancelEvent`; valid while `self` lives.
    #[must_use]
    pub fn as_raw(&self) -> dpro_sys::types::HANDLE {
        self.0.as_raw_handle()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_is_sticky_and_crosses_threads() {
        let event = Event::manual_reset().unwrap();
        assert!(!event.is_set());
        std::thread::scope(|s| {
            s.spawn(|| event.set().unwrap());
        });
        assert!(event.is_set());
        assert!(event.is_set(), "manual reset: polling doesn't clear it");
    }
}
