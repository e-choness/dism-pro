//! Win32 base types used by the bindings, spelled as in the SDK headers. Kept local
//! instead of pulling `windows-sys` in for a handful of aliases.

#![allow(clippy::upper_case_acronyms)]

use core::ffi::c_void;

pub type HRESULT = i32;
pub type BOOL = i32;
pub type UINT = u32;
pub type DWORD = u32;
pub type HANDLE = *mut c_void;
pub type PVOID = *mut c_void;
pub type PCWSTR = *const u16;
pub type WPARAM = usize;
pub type LPARAM = isize;

pub const S_OK: HRESULT = 0;
pub const INVALID_HANDLE_VALUE: HANDLE = usize::MAX as HANDLE;

/// `SYSTEMTIME` (`minwinbase.h`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(non_snake_case)]
pub struct SYSTEMTIME {
    pub wYear: u16,
    pub wMonth: u16,
    pub wDayOfWeek: u16,
    pub wDay: u16,
    pub wHour: u16,
    pub wMinute: u16,
    pub wSecond: u16,
    pub wMilliseconds: u16,
}
