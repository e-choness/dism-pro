//! Registry access for the running OS (doc 02 §5). Offline hives are read from their files by
//! `dpro_engine::regf` (ADR-0008).
//!
//! Mutating calls (`set`, `delete_value`) are only for `Backend::apply` implementations: services
//! never write the registry directly (TD-0001, CLAUDE.md rule 5).

use std::{
    ffi::{OsStr, OsString},
    os::windows::ffi::{OsStrExt, OsStringExt},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA},
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_BINARY, REG_DWORD,
        REG_EXPAND_SZ, REG_MULTI_SZ, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME, REG_QWORD,
        REG_SZ, RegCloseKey, RegDeleteValueW, RegNotifyChangeKeyValue, RegOpenKeyExW,
        RegQueryValueExW, RegSetValueExW,
    },
};

use crate::{
    error::{Error, Result, win32},
    event::Event,
    wide::WideCString,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    ReadWrite,
}

impl Access {
    fn sam(self) -> u32 {
        match self {
            Self::Read => KEY_READ,
            Self::ReadWrite => KEY_READ | KEY_WRITE,
        }
    }
}

/// Registry value data, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegValue {
    Sz(OsString),
    ExpandSz(OsString),
    MultiSz(Vec<OsString>),
    Dword(u32),
    Qword(u64),
    Binary(Vec<u8>),
    /// Any other type, kept raw so it can be restored exactly on undo.
    Other {
        ty: u32,
        data: Vec<u8>,
    },
}

/// An open registry key. Predefined roots (`HKEY_LOCAL_MACHINE`) are never closed.
/// Not `Send`/`Sync` (raw handle): each target actor owns its keys.
#[derive(Debug)]
pub struct RegKey {
    hkey: HKEY,
    owned: bool,
}

impl Drop for RegKey {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: `hkey` came from RegOpenKeyExW and is closed exactly once.
            // ignore: nothing useful to do if closing fails during drop.
            let _ = unsafe { RegCloseKey(self.hkey) };
        }
    }
}

impl RegKey {
    /// `HKEY_LOCAL_MACHINE` of the running OS.
    #[must_use]
    pub fn local_machine() -> Self {
        Self {
            hkey: HKEY_LOCAL_MACHINE,
            owned: false,
        }
    }

    /// `HKEY_CURRENT_USER`: the hive of the user the process runs as.
    #[must_use]
    pub fn current_user() -> Self {
        Self {
            hkey: HKEY_CURRENT_USER,
            owned: false,
        }
    }

    /// Opens `subkey` below this key.
    ///
    /// # Errors
    /// The OS error from `RegOpenKeyExW` (`ERROR_FILE_NOT_FOUND` if it doesn't exist).
    pub fn open(&self, subkey: impl AsRef<OsStr>, access: Access) -> Result<Self> {
        let subkey = WideCString::new(subkey)?;
        let mut hkey: HKEY = ptr::null_mut();
        // SAFETY: `self.hkey` is open; `subkey` is NUL-terminated; `hkey` is a valid out pointer.
        let status =
            unsafe { RegOpenKeyExW(self.hkey, subkey.as_ptr(), 0, access.sam(), &raw mut hkey) };
        win32(status, "RegOpenKeyExW")?;
        Ok(Self { hkey, owned: true })
    }

    /// Reads a value; `Ok(None)` if it doesn't exist. Use `""` for the default value.
    ///
    /// # Errors
    /// The OS error from `RegQueryValueExW`.
    pub fn get(&self, name: impl AsRef<OsStr>) -> Result<Option<RegValue>> {
        let name = WideCString::new(name)?;
        let mut data: Vec<u8> = vec![0; 256];
        loop {
            let mut ty = 0_u32;
            let mut len = u32::try_from(data.len()).unwrap_or(u32::MAX);
            // SAFETY: `name` is NUL-terminated; `data` has `len` writable bytes; out pointers are valid.
            let status = unsafe {
                RegQueryValueExW(
                    self.hkey,
                    name.as_ptr(),
                    ptr::null(),
                    &raw mut ty,
                    data.as_mut_ptr(),
                    &raw mut len,
                )
            };
            match status {
                0 => {
                    data.truncate(usize::try_from(len).unwrap_or(data.len()));
                    return decode(ty, &data).map(Some);
                }
                ERROR_FILE_NOT_FOUND => return Ok(None),
                // The value may grow between calls; retry with the size the OS reported.
                ERROR_MORE_DATA => {
                    let needed = usize::try_from(len).unwrap_or(0);
                    data.resize(needed.max(data.len() * 2), 0);
                }
                _ => return win32(status, "RegQueryValueExW").map(|()| None),
            }
        }
    }

    /// Writes a value (creates or replaces it).
    ///
    /// # Errors
    /// The OS error from `RegSetValueExW`.
    pub fn set(&self, name: impl AsRef<OsStr>, value: &RegValue) -> Result<()> {
        let name = WideCString::new(name)?;
        let (ty, data) = encode(value);
        let len = u32::try_from(data.len()).map_err(|_| Error::ValueTooLarge(data.len()))?;
        // SAFETY: `name` is NUL-terminated; `data` holds `len` readable bytes.
        let status = unsafe { RegSetValueExW(self.hkey, name.as_ptr(), 0, ty, data.as_ptr(), len) };
        win32(status, "RegSetValueExW")
    }

    /// Deletes a value. Returns `false` if it didn't exist.
    ///
    /// # Errors
    /// The OS error from `RegDeleteValueW`.
    pub fn delete_value(&self, name: impl AsRef<OsStr>) -> Result<bool> {
        let name = WideCString::new(name)?;
        // SAFETY: `self.hkey` is open; `name` is NUL-terminated.
        let status = unsafe { RegDeleteValueW(self.hkey, name.as_ptr()) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(false);
        }
        win32(status, "RegDeleteValueW").map(|()| true)
    }

    /// Asks Windows to signal `event` at the next change of `what` in this key (not its
    /// subkeys'). One signal per call: call again to hear of the next change. Closing the key, or
    /// the calling thread ending, also signals it. The key must be open for [`Access::Read`],
    /// which includes `KEY_NOTIFY`.
    ///
    /// # Errors
    /// The OS error from `RegNotifyChangeKeyValue`.
    pub fn notify(&self, what: Notify, event: &Event) -> Result<()> {
        let filter = match what {
            Notify::Values => REG_NOTIFY_CHANGE_LAST_SET,
            Notify::Subkeys => REG_NOTIFY_CHANGE_NAME,
        };
        // SAFETY: `self.hkey` is open and `event` is an open event for the call. Asynchronous:
        // the call returns at once, and Windows holds its own references until the signal.
        let status = unsafe { RegNotifyChangeKeyValue(self.hkey, 0, filter, event.as_raw(), 1) };
        win32(status, "RegNotifyChangeKeyValue")
    }
}

/// What [`RegKey::notify`] watches for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notify {
    /// A value set, added or deleted.
    Values,
    /// A subkey added or deleted.
    Subkeys,
}

/// Decodes `RegQueryValueExW` output. String data may lack its terminating NUL (the
/// registry doesn't enforce it), so it's read by length.
///
/// # Errors
/// [`Error::BadValueSize`] for a DWORD/QWORD of the wrong length.
pub fn decode(ty: u32, data: &[u8]) -> Result<RegValue> {
    let units = || -> Vec<u16> {
        data.as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect()
    };
    let string = |u: &[u16]| {
        let end = u.iter().position(|&c| c == 0).unwrap_or(u.len());
        OsString::from_wide(&u[..end])
    };
    Ok(match ty {
        REG_SZ => RegValue::Sz(string(&units())),
        REG_EXPAND_SZ => RegValue::ExpandSz(string(&units())),
        REG_MULTI_SZ => {
            let u = units();
            let mut out: Vec<OsString> = u.split(|&c| c == 0).map(OsString::from_wide).collect();
            // A well-formed value ends with "\0\0": drop the empty tail entries.
            while out.last().is_some_and(|s| s.is_empty()) {
                out.pop();
            }
            RegValue::MultiSz(out)
        }
        REG_DWORD => RegValue::Dword(u32::from_le_bytes(fixed(data)?)),
        REG_QWORD => RegValue::Qword(u64::from_le_bytes(fixed(data)?)),
        REG_BINARY => RegValue::Binary(data.to_vec()),
        ty => RegValue::Other {
            ty,
            data: data.to_vec(),
        },
    })
}

/// Encodes for `RegSetValueExW`: strings get their terminating NUL(s).
#[must_use]
pub fn encode(value: &RegValue) -> (u32, Vec<u8>) {
    let wide = |s: &OsStr| -> Vec<u8> {
        s.encode_wide()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    };
    match value {
        RegValue::Sz(s) => (REG_SZ, wide(s)),
        RegValue::ExpandSz(s) => (REG_EXPAND_SZ, wide(s)),
        RegValue::MultiSz(items) => {
            let mut data: Vec<u8> = items.iter().flat_map(|s| wide(s)).collect();
            data.extend_from_slice(&[0, 0]);
            (REG_MULTI_SZ, data)
        }
        RegValue::Dword(v) => (REG_DWORD, v.to_le_bytes().to_vec()),
        RegValue::Qword(v) => (REG_QWORD, v.to_le_bytes().to_vec()),
        RegValue::Binary(b) => (REG_BINARY, b.clone()),
        RegValue::Other { ty, data } => (*ty, data.clone()),
    }
}

fn fixed<const N: usize>(data: &[u8]) -> Result<[u8; N]> {
    data.try_into().map_err(|_| Error::BadValueSize {
        expected: N,
        actual: data.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(v: &RegValue) {
        let (ty, data) = encode(v);
        assert_eq!(&decode(ty, &data).unwrap(), v);
    }

    #[test]
    fn values_round_trip() {
        round_trip(&RegValue::Sz("C:\\Program Files\\X".into()));
        round_trip(&RegValue::ExpandSz("%SystemRoot%\\x".into()));
        round_trip(&RegValue::MultiSz(vec!["a".into(), "bc".into()]));
        round_trip(&RegValue::MultiSz(vec![]));
        round_trip(&RegValue::Dword(0xDEAD_BEEF));
        round_trip(&RegValue::Qword(u64::MAX));
        round_trip(&RegValue::Binary(vec![1, 2, 3]));
        round_trip(&RegValue::Other {
            ty: 8,
            data: vec![9],
        });
    }

    #[test]
    fn strings_without_nul_and_bad_sizes() {
        let data: Vec<u8> = "ab".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode(REG_SZ, &data).unwrap(), RegValue::Sz("ab".into()));
        assert!(matches!(
            decode(REG_DWORD, &[1, 2]),
            Err(Error::BadValueSize {
                expected: 4,
                actual: 2
            })
        ));
    }

    /// Polls `event` every 10 ms for up to `ms`.
    fn signalled(event: &Event, ms: u64) -> bool {
        (0..ms / 10).any(|_| {
            let set = event.is_set();
            if !set {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            set
        })
    }

    #[test]
    fn notify_signals_one_value_change_per_call() {
        // A value of the test's own in a key every hive has, removed again.
        const NAME: &str = "dpro-win notify test";
        let key = RegKey::current_user()
            .open("Software", Access::ReadWrite)
            .unwrap();
        let event = Event::auto_reset().unwrap();
        key.notify(Notify::Values, &event).unwrap();
        assert!(!event.is_set(), "nothing changed yet");
        key.set(NAME, &RegValue::Dword(1)).unwrap();
        assert!(signalled(&event, 2000), "setting a value signals");
        key.set(NAME, &RegValue::Dword(2)).unwrap();
        assert!(!signalled(&event, 100), "one signal per call");
        key.notify(Notify::Values, &event).unwrap();
        assert!(key.delete_value(NAME).unwrap());
        assert!(signalled(&event, 2000), "deleting a value signals");
    }
}
