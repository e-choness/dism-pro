//! UTF-16 ⇄ OS string conversions at the FFI boundary (coding standards §4). Windows strings
//! may hold unpaired surrogates, so they become `OsString` (WTF-8 inside), never `String`.

use std::{
    ffi::{OsStr, OsString},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

use crate::error::{Error, Result};

/// Upper bound when reading NUL-terminated strings the OS hands us without a length
/// (`UNICODE_STRING` maximum is 32 767 characters; paths with `\\?\` too).
pub const MAX_OS_STRING: usize = 32_767;

/// A NUL-terminated UTF-16 string for passing to `PCWSTR` parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WideCString(Vec<u16>);

impl WideCString {
    /// # Errors
    /// [`Error::InteriorNul`] if `s` contains a NUL.
    pub fn new(s: impl AsRef<OsStr>) -> Result<Self> {
        let mut units: Vec<u16> = s.as_ref().encode_wide().collect();
        if units.contains(&0) {
            return Err(Error::InteriorNul);
        }
        units.push(0);
        Ok(Self(units))
    }

    /// Valid while `self` lives.
    #[must_use]
    pub fn as_ptr(&self) -> *const u16 {
        self.0.as_ptr()
    }
}

/// `OsString` from a buffer, stopping at the first NUL (or the end of the buffer).
#[must_use]
pub fn from_wide_buf(buf: &[u16]) -> OsString {
    let len = buf.iter().position(|&u| u == 0).unwrap_or(buf.len());
    OsString::from_wide(&buf[..len])
}

/// Reads a NUL-terminated string the OS returned without a length.
///
/// # Safety
/// `ptr` must be null or point to readable UTF-16 that is NUL-terminated within
/// [`MAX_OS_STRING`] units.
///
/// # Errors
/// [`Error::StringTooLong`] if no NUL was found within the bound.
pub unsafe fn from_wide_ptr(ptr: *const u16) -> Result<OsString> {
    if ptr.is_null() {
        return Ok(OsString::new());
    }
    let mut len = 0;
    // SAFETY: the caller guarantees NUL termination within MAX_OS_STRING, and we never read past it.
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
        if len > MAX_OS_STRING {
            return Err(Error::StringTooLong(MAX_OS_STRING));
        }
    }
    // SAFETY: `len` units starting at `ptr` were just read above.
    let units = unsafe { std::slice::from_raw_parts(ptr, len) };
    Ok(OsString::from_wide(units))
}

/// Adds the `\\?\` prefix so paths beyond `MAX_PATH` work: `C:\x` → `\\?\C:\x`,
/// `\\server\share\x` → `\\?\UNC\server\share\x`. Relative and already-prefixed paths are
/// returned unchanged (the prefix disables normalization, so it needs an absolute path).
#[must_use]
pub fn long_path(path: &Path) -> PathBuf {
    let units: Vec<u16> = path.as_os_str().encode_wide().collect();
    let starts = |p: &str| {
        let prefix: Vec<u16> = p.encode_utf16().collect();
        units.starts_with(&prefix)
    };
    if starts(r"\\?\") || starts(r"\\.\") {
        return path.to_path_buf();
    }
    let is_drive = units.len() >= 3
        && u8::try_from(units[0]).is_ok_and(|c| c.is_ascii_alphabetic())
        && units[1] == u16::from(b':')
        && (units[2] == u16::from(b'\\') || units[2] == u16::from(b'/'));
    let mut out: Vec<u16> = if is_drive {
        r"\\?\"
            .encode_utf16()
            .chain(units.iter().copied())
            .collect()
    } else if starts(r"\\") {
        r"\\?\UNC\"
            .encode_utf16()
            .chain(units[2..].iter().copied())
            .collect()
    } else {
        return path.to_path_buf();
    };
    // `\\?\` paths must use backslashes.
    for u in &mut out {
        if *u == u16::from(b'/') {
            *u = u16::from(b'\\');
        }
    }
    PathBuf::from(OsString::from_wide(&out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_c_string_rejects_interior_nul() {
        assert!(WideCString::new("a\0b").is_err());
        let w = WideCString::new("ab").unwrap();
        assert_eq!(w.0, [u16::from(b'a'), u16::from(b'b'), 0]);
    }

    #[test]
    fn unpaired_surrogates_survive() {
        let units = [0xD800_u16, u16::from(b'x'), 0];
        let s = from_wide_buf(&units);
        assert_eq!(
            s.encode_wide().collect::<Vec<_>>(),
            [0xD800, u16::from(b'x')]
        );
        // SAFETY: `units` is NUL-terminated.
        let p = unsafe { from_wide_ptr(units.as_ptr()) }.unwrap();
        assert_eq!(p, s);
    }

    #[test]
    fn long_paths_get_the_right_prefix() {
        assert_eq!(
            long_path(Path::new(r"C:\Windows")),
            Path::new(r"\\?\C:\Windows")
        );
        assert_eq!(long_path(Path::new("c:/a/b")), Path::new(r"\\?\c:\a\b"));
        assert_eq!(
            long_path(Path::new(r"\\srv\share\x")),
            Path::new(r"\\?\UNC\srv\share\x")
        );
        assert_eq!(long_path(Path::new(r"\\?\D:\x")), Path::new(r"\\?\D:\x"));
        assert_eq!(long_path(Path::new(r"rel\x")), Path::new(r"rel\x"));
    }
}
