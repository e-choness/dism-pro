//! Errors for `dpro-core` (coding standards §3). `Display` text is for logs; user-facing
//! messages come from the localized diagnosis table.

/// A failed OS call: the `HRESULT`/Win32 code and the API that returned it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{api} failed with {code:#010x}")]
pub struct OsError {
    pub code: i32,
    pub api: &'static str,
}

impl OsError {
    /// From a Win32 error code (`GetLastError`, `LSTATUS`), converted like
    /// `HRESULT_FROM_WIN32` so every OS error is reported as an `HRESULT`.
    #[must_use]
    pub fn from_win32(code: u32, api: &'static str) -> Self {
        let hresult = if code == 0 {
            0
        } else {
            (code & 0xFFFF) | (7 << 16) | 0x8000_0000
        };
        Self {
            code: i32::from_ne_bytes(hresult.to_ne_bytes()),
            api,
        }
    }

    /// The code as an unsigned `HRESULT` bit pattern.
    #[must_use]
    pub fn hresult_bits(self) -> u32 {
        u32::from_ne_bytes(self.code.to_ne_bytes())
    }

    /// DISM reported a package removal as done, but the package is still listed (TD-0002 §7,
    /// decision #31). DISM Pro's own `HRESULT`: the customer bit (`0x2000_0000`) keeps it apart
    /// from every Windows code. The low byte is the `DismPackageFeatureState` the package was
    /// left in (`0xFF` for a value that doesn't fit), so the journal records which state.
    /// Journaled: the value never changes meaning.
    pub const PACKAGE_NOT_REMOVED: u32 = 0xA0D1_0100;

    /// A removal that left the package listed in `state`.
    #[must_use]
    pub fn package_not_removed(state: i32) -> Self {
        let state = u32::try_from(state)
            .ok()
            .filter(|&s| s < 0xFF)
            .unwrap_or(0xFF);
        Self {
            code: i32::from_ne_bytes((Self::PACKAGE_NOT_REMOVED | state).to_ne_bytes()),
            api: "DismRemovePackage",
        }
    }

    /// The state a [`Self::package_not_removed`] code carries; `None` for any other code.
    #[must_use]
    pub fn not_removed_state(code: i32) -> Option<i32> {
        let bits = u32::from_ne_bytes(code.to_ne_bytes());
        (bits & !0xFF == Self::PACKAGE_NOT_REMOVED).then(|| i32::from(bits.to_le_bytes()[0]))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Os(#[from] OsError),
    #[error("invalid {what}: {value:?}")]
    Parse { what: &'static str, value: String },
    /// A change whose kind is always irreversible was given another reversibility.
    #[error("change #{index} ({kind}) must be marked irreversible")]
    MustBeIrreversible { index: usize, kind: &'static str },
    #[error("plan string arena is full")]
    ArenaFull,
    #[error("unknown string atom {0}")]
    UnknownAtom(u32),
    #[error("operation cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win32_codes_become_hresults() {
        assert_eq!(
            OsError::from_win32(5, "RegOpenKeyExW").hresult_bits(),
            0x8007_0005
        );
        assert_eq!(OsError::from_win32(0, "x").code, 0);
    }

    #[test]
    fn os_error_shows_hresult_as_hex() {
        // E_ACCESSDENIED
        let e = OsError {
            code: i32::from_ne_bytes(0x8007_0005_u32.to_ne_bytes()),
            api: "DismOpenSession",
        };
        assert_eq!(e.to_string(), "DismOpenSession failed with 0x80070005");
    }

    #[test]
    fn a_removal_that_changed_nothing_keeps_the_state_in_its_code() {
        // Installed (4): the case DISM reported done in runs 41728952696 and 41736115561.
        let e = OsError::package_not_removed(4);
        assert_eq!(e.hresult_bits(), 0xA0D1_0104);
        assert_eq!(OsError::not_removed_state(e.code), Some(4));
        // A state that doesn't fit the byte is kept as "unknown", not wrapped into another one.
        for odd in [-1, 0xFF, 0x1_0004] {
            let code = OsError::package_not_removed(odd).code;
            assert_eq!(OsError::not_removed_state(code), Some(0xFF), "{odd}");
        }
        // Windows codes never carry the customer bit, so none reads as ours.
        for windows in [0x8007_0005_u32, 0x800F_0805, 0x8000_4005, 0] {
            let code = i32::from_ne_bytes(windows.to_ne_bytes());
            assert_eq!(OsError::not_removed_state(code), None, "{windows:#x}");
        }
        assert_eq!(
            OsError::not_removed_state(i32::from_ne_bytes(0xA0D1_0200_u32.to_ne_bytes())),
            None
        );
    }
}
