//! The clipboard's text (TD-0009 §11 step 3), as a field copies, cuts and pastes it:
//! `CF_UNICODETEXT`, which Windows converts to and from the other text formats.
//!
//! Another window can hold the clipboard open for a moment, so opening it is tried a few times,
//! a few milliseconds apart, before the operation gives up. A read takes at most the code units its
//! caller can use, so a huge clipboard costs no more than a field can hold.

use std::{ffi::c_void, thread, time::Duration};

use windows_sys::Win32::{
    Foundation::{GlobalFree, HGLOBAL, HWND},
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
            OpenClipboard, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
    },
};

/// `WinUser.h`'s value; windows-sys files it under `Win32_System_Ole`, a large feature for one
/// constant.
const CF_UNICODETEXT: u32 = 13;
/// How often opening the clipboard is tried, and how long apart.
const OPEN_TRIES: u32 = 4;
const OPEN_GAP: Duration = Duration::from_millis(5);

/// The clipboard, open on this thread; closed when dropped.
struct Open;

impl Open {
    fn new(hwnd: HWND) -> Option<Self> {
        for k in 0..OPEN_TRIES {
            if k > 0 {
                thread::sleep(OPEN_GAP);
            }
            // SAFETY: hwnd is a live window of this thread, which then owns what it puts there.
            if unsafe { OpenClipboard(hwnd) } != 0 {
                return Some(Self);
            }
        }
        None
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: this thread opened the clipboard, and closes it once.
        unsafe { CloseClipboard() };
    }
}

/// A global memory block, locked; unlocked when dropped.
struct Locked {
    mem: HGLOBAL,
    ptr: *mut c_void,
}

impl Locked {
    fn new(mem: HGLOBAL) -> Option<Self> {
        // SAFETY: mem is a live moveable block (the clipboard's, or one just allocated).
        let ptr = unsafe { GlobalLock(mem) };
        (!ptr.is_null()).then_some(Self { mem, ptr })
    }
}

impl Drop for Locked {
    fn drop(&mut self) {
        // SAFETY: locked once by `new`, unlocked once here.
        unsafe { GlobalUnlock(self.mem) };
    }
}

/// The clipboard's text, its first `max_units` UTF-16 code units at most, up to a NUL; unpaired
/// surrogates are dropped. `None` if it holds no text, or couldn't be opened.
pub(super) fn read(hwnd: HWND, max_units: usize) -> Option<String> {
    // SAFETY: no preconditions.
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) } == 0 {
        return None;
    }
    let _open = Open::new(hwnd)?;
    // SAFETY: the clipboard is open; the handle is the clipboard's, valid until it closes.
    let mem = unsafe { GetClipboardData(CF_UNICODETEXT) };
    if mem.is_null() {
        return None;
    }
    // SAFETY: mem is the clipboard's live block.
    let bytes = unsafe { GlobalSize(mem) };
    let lock = Locked::new(mem)?;
    let units = (bytes / 2).min(max_units);
    // SAFETY: the block is locked and `bytes` long, so `units` code units fit in it; GlobalLock's
    // memory is 8-byte aligned. The slice ends before the lock does.
    let wide = unsafe { std::slice::from_raw_parts(lock.ptr.cast::<u16>().cast_const(), units) };
    let end = wide.iter().position(|&u| u == 0).unwrap_or(wide.len());
    Some(
        char::decode_utf16(wide[..end].iter().copied())
            .filter_map(Result::ok)
            .collect(),
    )
}

/// Puts `text` on the clipboard as this window's. Returns whether it's there.
pub(super) fn write(hwnd: HWND, text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let Some(_open) = Open::new(hwnd) else {
        return false;
    };
    // SAFETY: the clipboard is open, by `hwnd`, which emptying it makes the owner.
    if unsafe { EmptyClipboard() } == 0 {
        return false;
    }
    // SAFETY: no preconditions; the block is freed below unless the clipboard takes it.
    let mem = unsafe { GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) };
    if mem.is_null() {
        return false;
    }
    let copied = Locked::new(mem).map(|lock| {
        // SAFETY: the block is locked and `wide.len()` code units long; `wide` doesn't overlap it.
        unsafe { std::ptr::copy_nonoverlapping(wide.as_ptr(), lock.ptr.cast::<u16>(), wide.len()) };
    });
    // SAFETY: the clipboard is open and empty, and mem unlocked; on success the system owns it.
    if copied.is_none() || unsafe { SetClipboardData(CF_UNICODETEXT, mem) }.is_null() {
        // SAFETY: still ours, unlocked, freed once.
        unsafe { GlobalFree(mem) };
        return false;
    }
    true
}
