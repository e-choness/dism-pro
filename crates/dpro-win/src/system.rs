//! Facts about the running system used by target discovery (roadmap 2.4).

use std::{
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
};

use windows_sys::Win32::{
    Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, GetFileAttributesW, GetLogicalDrives, INVALID_FILE_ATTRIBUTES},
    System::SystemInformation::GetSystemWindowsDirectoryW,
};

use crate::{
    error::{Result, last_error_if_false},
    wide::{WideCString, long_path},
};

/// The running OS's Windows directory (e.g. `C:\Windows`), also correct under Terminal
/// Services where `GetWindowsDirectoryW` is per-user.
///
/// # Errors
/// The OS error from `GetSystemWindowsDirectoryW`.
pub fn windows_directory() -> Result<PathBuf> {
    let mut buf = vec![0_u16; 260];
    loop {
        let size = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        // SAFETY: `buf` has `size` writable units.
        let len = unsafe { GetSystemWindowsDirectoryW(buf.as_mut_ptr(), size) };
        last_error_if_false(i32::from(len != 0), "GetSystemWindowsDirectoryW")?;
        let len = usize::try_from(len).unwrap_or(usize::MAX);
        // On a too-small buffer the return value is the required size including the NUL.
        if len < buf.len() {
            return Ok(PathBuf::from(OsString::from_wide(&buf[..len])));
        }
        buf.resize(len, 0);
    }
}

/// Root paths (`C:\`, `D:\`, …) of every drive letter currently assigned.
#[must_use]
pub fn drive_roots() -> Vec<PathBuf> {
    // SAFETY: no preconditions.
    let mask = unsafe { GetLogicalDrives() };
    (0..26_u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| PathBuf::from(format!("{}:\\", char::from(b'A' + i))))
        .collect()
}

/// What exists at `path`, without following it anywhere: `None` if nothing (or no access).
#[must_use]
pub fn probe(path: &Path) -> Option<PathKind> {
    let wide = WideCString::new(long_path(path)).ok()?;
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    let attrs = unsafe { GetFileAttributesW(wide.as_ptr()) };
    if attrs == INVALID_FILE_ATTRIBUTES {
        None
    } else if attrs & FILE_ATTRIBUTE_DIRECTORY != 0 {
        Some(PathKind::Dir)
    } else {
        Some(PathKind::File)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    File,
    Dir,
}
