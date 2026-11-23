//! System DLLs loaded at run time: for exports that aren't in every Windows (base `WinPE` has no
//! Direct3D), or that only one path needs.
//!
//! Only System32 is searched, never the current or the application folder: the app runs elevated,
//! often from removable media, where a planted DLL next to the executable would run as admin.

use std::{ffi::CStr, ptr};

use windows_sys::Win32::{
    Foundation::HMODULE,
    System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
};

use crate::{
    error::{Result, last_os_error},
    wide::WideCString,
};

/// An export, as `GetProcAddress` gives it; the caller transmutes it to its real signature.
pub(crate) type Proc = unsafe extern "system" fn() -> isize;

/// `name` from System32; it stays loaded for the life of the process.
pub(crate) fn system(name: &str) -> Result<HMODULE> {
    let wide = WideCString::new(name)?;
    // SAFETY: `wide` is NUL-terminated; a null file handle is what the call expects.
    let module =
        unsafe { LoadLibraryExW(wide.as_ptr(), ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if module.is_null() {
        return Err(last_os_error("LoadLibraryExW").into());
    }
    Ok(module)
}

/// The export `name` of a module [`system`] loaded.
pub(crate) fn export(module: HMODULE, name: &CStr) -> Result<Proc> {
    // SAFETY: `module` is loaded and stays loaded; `name` is NUL-terminated.
    unsafe { GetProcAddress(module, name.as_ptr().cast()) }
        .ok_or_else(|| last_os_error("GetProcAddress").into())
}
