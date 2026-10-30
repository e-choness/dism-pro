//! Token privileges (ADR-0004): the elevated process enables what it needs, e.g.
//! `SeBackupPrivilege`/`SeRestorePrivilege` for hive and protected-file access.

use std::{
    os::windows::io::{FromRawHandle, OwnedHandle},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{ERROR_NOT_ALL_ASSIGNED, GetLastError, HANDLE, LUID},
    Security::{
        AdjustTokenPrivileges, GetTokenInformation, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW,
        SE_PRIVILEGE_ENABLED, TOKEN_ACCESS_MASK, TOKEN_ADJUST_PRIVILEGES, TOKEN_ELEVATION,
        TOKEN_PRIVILEGES, TOKEN_QUERY, TokenElevation,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

use crate::{
    error::{Error, Result, last_error_if_false},
    wide::WideCString,
};

pub const SE_BACKUP: &str = "SeBackupPrivilege";
pub const SE_RESTORE: &str = "SeRestorePrivilege";
pub const SE_TAKE_OWNERSHIP: &str = "SeTakeOwnershipPrivilege";

/// Enables `privilege` on the current process token. Idempotent.
///
/// # Errors
/// [`Error::PrivilegeNotHeld`] if the token doesn't have it (not elevated); OS errors otherwise.
pub fn enable(privilege: &'static str) -> Result<()> {
    let token = process_token(TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY)?;

    let name = WideCString::new(privilege)?;
    let mut luid = LUID {
        LowPart: 0,
        HighPart: 0,
    };
    // SAFETY: `name` is NUL-terminated; null system name means the local system; `luid` is valid.
    let ok = unsafe { LookupPrivilegeValueW(ptr::null(), name.as_ptr(), &raw mut luid) };
    last_error_if_false(ok, "LookupPrivilegeValueW")?;

    let state = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    let handle: HANDLE = std::os::windows::io::AsRawHandle::as_raw_handle(&token);
    // SAFETY: `handle` is an open token with TOKEN_ADJUST_PRIVILEGES; `state` is a valid
    // one-entry TOKEN_PRIVILEGES; no previous state is requested.
    let ok = unsafe {
        AdjustTokenPrivileges(
            handle,
            0,
            &raw const state,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    last_error_if_false(ok, "AdjustTokenPrivileges")?;
    // AdjustTokenPrivileges "succeeds" without assigning a privilege the token lacks.
    // SAFETY: no preconditions; read immediately after the call.
    if unsafe { GetLastError() } == ERROR_NOT_ALL_ASSIGNED {
        return Err(Error::PrivilegeNotHeld(privilege));
    }
    Ok(())
}

/// Whether the process runs elevated (UAC "Run as administrator", or as SYSTEM).
///
/// # Errors
/// OS errors from the token query.
pub fn is_elevated() -> Result<bool> {
    let token = process_token(TOKEN_QUERY)?;
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut len = 0_u32;
    let size = u32::try_from(size_of::<TOKEN_ELEVATION>()).unwrap_or(4);
    let handle: HANDLE = std::os::windows::io::AsRawHandle::as_raw_handle(&token);
    // SAFETY: `handle` is an open token with TOKEN_QUERY; the buffer is a TOKEN_ELEVATION of `size` bytes.
    let ok = unsafe {
        GetTokenInformation(
            handle,
            TokenElevation,
            (&raw mut elevation).cast(),
            size,
            &raw mut len,
        )
    };
    last_error_if_false(ok, "GetTokenInformation")?;
    Ok(elevation.TokenIsElevated != 0)
}

fn process_token(access: TOKEN_ACCESS_MASK) -> Result<OwnedHandle> {
    let mut raw: HANDLE = ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle; `raw` is a valid out pointer.
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), access, &raw mut raw) };
    last_error_if_false(ok, "OpenProcessToken")?;
    // SAFETY: OpenProcessToken succeeded, so `raw` is a token handle we now own.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}
