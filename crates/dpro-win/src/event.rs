//! Win32 event objects: the `CancelEvent` handle DISM takes, signalled from a `CancelToken`
//! (TD-0001 §5.3).

use std::{
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};

use dpro_core::OsError;
use windows_sys::Win32::{
    Foundation::{ERROR_INVALID_PARAMETER, WAIT_OBJECT_0},
    System::Threading::{
        CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects, WaitForSingleObject,
    },
};

use crate::error::{Result, last_error_if_false, last_os_error};

/// `WinNT.h`'s `MAXIMUM_WAIT_OBJECTS`: the most handles one wait takes. windows-sys files it
/// under `Win32_System_SystemServices`, a large feature for one constant.
const MAXIMUM_WAIT_OBJECTS: u32 = 64;

/// An event object: manual-reset (once set it stays signalled) or auto-reset (a wait that sees
/// it set clears it). `Send + Sync` (via `OwnedHandle`), so a console handler or another thread
/// can set it while a DISM call waits on it.
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

    /// A new, unsignalled, unnamed auto-reset event: the wait that sees it signalled clears it.
    ///
    /// # Errors
    /// The OS error from `CreateEventW`.
    pub fn auto_reset() -> Result<Self> {
        // SAFETY: default security, auto reset, initially unsignalled, no name.
        let handle = unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) };
        if handle.is_null() {
            return Err(last_os_error("CreateEventW").into());
        }
        // SAFETY: `handle` is a new event handle that nothing else owns.
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle) }))
    }

    /// Waits, with no time limit, until one of `events` is signalled, and returns its index (the
    /// lowest, if several are). An auto-reset event it returns is cleared.
    ///
    /// # Errors
    /// `ERROR_INVALID_PARAMETER` for no events or more than 64 (`MAXIMUM_WAIT_OBJECTS`), or the
    /// OS error from `WaitForMultipleObjects`.
    pub fn wait_any(events: &[&Self]) -> Result<usize> {
        let mut all = [ptr::null_mut(); MAXIMUM_WAIT_OBJECTS as usize];
        let handles = match all.get_mut(..events.len()) {
            Some(h) if !h.is_empty() => h,
            _ => {
                return Err(
                    OsError::from_win32(ERROR_INVALID_PARAMETER, "WaitForMultipleObjects").into(),
                );
            }
        };
        for (h, e) in handles.iter_mut().zip(events) {
            *h = e.0.as_raw_handle();
        }
        let count = u32::try_from(handles.len()).unwrap_or(MAXIMUM_WAIT_OBJECTS);
        // SAFETY: `handles` holds `count` open events, borrowed from `events` for the call.
        let r = unsafe { WaitForMultipleObjects(count, handles.as_ptr(), 0, INFINITE) };
        match r.checked_sub(WAIT_OBJECT_0) {
            Some(i) if i < count => Ok(usize::try_from(i).unwrap_or(usize::MAX)),
            _ => Err(last_os_error("WaitForMultipleObjects").into()),
        }
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

    #[test]
    fn an_auto_reset_event_clears_when_a_wait_sees_it() {
        let event = Event::auto_reset().unwrap();
        assert!(!event.is_set());
        event.set().unwrap();
        assert!(event.is_set(), "the poll saw it and cleared it");
        assert!(!event.is_set());
    }

    #[test]
    fn wait_any_returns_the_signalled_event() {
        let (a, b) = (Event::manual_reset().unwrap(), Event::auto_reset().unwrap());
        std::thread::scope(|s| {
            s.spawn(|| b.set().unwrap());
        });
        assert_eq!(Event::wait_any(&[&a, &b]).unwrap(), 1);
        assert!(!b.is_set(), "the wait cleared the auto-reset event");
        a.set().unwrap();
        b.set().unwrap();
        assert_eq!(Event::wait_any(&[&a, &b]).unwrap(), 0, "the lowest of two");
        assert!(Event::wait_any(&[]).is_err());
        let many: Vec<&Event> = std::iter::repeat_n(&a, 65).collect();
        assert!(Event::wait_any(&many).is_err());
    }
}
