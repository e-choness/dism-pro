//! WIMGAPI (`wimgapi.h`, `wimgapi.dll`). First cut for S-03, **unverified** until
//! `cargo xtask ffi-check` passes against the real header.
//!
//! Handles returned here are closed with `WIMCloseHandle`, not `CloseHandle`.

#![allow(non_snake_case, non_camel_case_types)]

#[cfg(windows)]
use crate::types::{BOOL, HANDLE, PCWSTR};
use crate::types::{DWORD, LPARAM, PVOID, WPARAM};

pub const WIM_GENERIC_READ: DWORD = 0x8000_0000;
pub const WIM_GENERIC_WRITE: DWORD = 0x4000_0000;
pub const WIM_GENERIC_MOUNT: DWORD = 0x2000_0000;

pub const WIM_CREATE_NEW: DWORD = 1;
pub const WIM_CREATE_ALWAYS: DWORD = 2;
pub const WIM_OPEN_EXISTING: DWORD = 3;
pub const WIM_OPEN_ALWAYS: DWORD = 4;

pub const WIM_COMPRESS_NONE: DWORD = 0;
pub const WIM_COMPRESS_XPRESS: DWORD = 1;
pub const WIM_COMPRESS_LZX: DWORD = 2;
pub const WIM_COMPRESS_LZMS: DWORD = 3;

/// `WIM_MSG` (`WM_APP + 0x1476`); the `WIM_MSG_*` ids are offsets from it.
pub const WIM_MSG: DWORD = 0x8000 + 0x1476;
pub const WIM_MSG_TEXT: DWORD = WIM_MSG + 1;
/// `wParam` = percent done, `lParam` = estimated milliseconds left.
pub const WIM_MSG_PROGRESS: DWORD = WIM_MSG + 2;
pub const WIM_MSG_PROCESS: DWORD = WIM_MSG + 3;
/// Callback return values.
pub const WIM_MSG_SUCCESS: DWORD = 0;
pub const WIM_MSG_DONE: DWORD = 0xFFFF_FFF0;
pub const WIM_MSG_SKIP_ERROR: DWORD = 0xFFFF_FFFE;
pub const WIM_MSG_ABORT_IMAGE: DWORD = 0xFFFF_FFFF;

/// Message callback registered with `WIMRegisterMessageCallback`.
pub type WIM_MESSAGE_CALLBACK = Option<
    unsafe extern "system" fn(
        dwMessageId: DWORD,
        wParam: WPARAM,
        lParam: LPARAM,
        pvUserData: PVOID,
    ) -> DWORD,
>;

#[cfg(windows)]
#[link(name = "wimgapi", kind = "raw-dylib")]
unsafe extern "system" {
    pub fn WIMCreateFile(
        pszWimPath: PCWSTR,
        dwDesiredAccess: DWORD,
        dwCreationDisposition: DWORD,
        dwFlagsAndAttributes: DWORD,
        dwCompressionType: DWORD,
        pdwCreationResult: *mut DWORD,
    ) -> HANDLE;
    pub fn WIMCloseHandle(hObject: HANDLE) -> BOOL;
    pub fn WIMSetTemporaryPath(hWim: HANDLE, pszPath: PCWSTR) -> BOOL;
    pub fn WIMGetImageCount(hWim: HANDLE) -> DWORD;
    pub fn WIMLoadImage(hWim: HANDLE, dwImageIndex: DWORD) -> HANDLE;
    pub fn WIMApplyImage(hImage: HANDLE, pszPath: PCWSTR, dwApplyFlags: DWORD) -> BOOL;
    pub fn WIMCaptureImage(hWim: HANDLE, pszPath: PCWSTR, dwCaptureFlags: DWORD) -> HANDLE;
    pub fn WIMGetImageInformation(
        hImage: HANDLE,
        ppvImageInfo: *mut PVOID,
        pcbImageInfo: *mut DWORD,
    ) -> BOOL;
    // The header declares the callback parameter as FARPROC; the ABI is the same pointer.
    pub fn WIMRegisterMessageCallback(
        hWim: HANDLE,
        fpMessageProc: WIM_MESSAGE_CALLBACK,
        pvUserData: PVOID,
    ) -> DWORD;
    pub fn WIMUnregisterMessageCallback(hWim: HANDLE, fpMessageProc: WIM_MESSAGE_CALLBACK) -> BOOL;
}

/// C prototypes of the functions above, for the ffi-check signature test. Keep in the same
/// order and in sync with the `extern` block.
pub const C_PROTOTYPES: &[(&str, &str)] = &[
    (
        "WIMCreateFile",
        "HANDLE (WINAPI *)(PCWSTR, DWORD, DWORD, DWORD, DWORD, PDWORD)",
    ),
    ("WIMCloseHandle", "BOOL (WINAPI *)(HANDLE)"),
    ("WIMSetTemporaryPath", "BOOL (WINAPI *)(HANDLE, PCWSTR)"),
    ("WIMGetImageCount", "DWORD (WINAPI *)(HANDLE)"),
    ("WIMLoadImage", "HANDLE (WINAPI *)(HANDLE, DWORD)"),
    ("WIMApplyImage", "BOOL (WINAPI *)(HANDLE, PCWSTR, DWORD)"),
    (
        "WIMCaptureImage",
        "HANDLE (WINAPI *)(HANDLE, PCWSTR, DWORD)",
    ),
    (
        "WIMGetImageInformation",
        "BOOL (WINAPI *)(HANDLE, PVOID *, PDWORD)",
    ),
    (
        "WIMRegisterMessageCallback",
        "DWORD (WINAPI *)(HANDLE, FARPROC, PVOID)",
    ),
    (
        "WIMUnregisterMessageCallback",
        "BOOL (WINAPI *)(HANDLE, FARPROC)",
    ),
];
