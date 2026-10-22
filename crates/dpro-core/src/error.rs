//! Errors for `dpro-core` (coding standards §3). `Display` text is for logs; user-facing
//! messages come from the localized diagnosis table.

/// A failed OS call: the `HRESULT`/Win32 code and the API that returned it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{api} failed with {code:#010x}")]
pub struct OsError {
    pub code: i32,
    pub api: &'static str,
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
    fn os_error_shows_hresult_as_hex() {
        // E_ACCESSDENIED
        let e = OsError { code: i32::from_ne_bytes(0x8007_0005_u32.to_ne_bytes()), api: "DismOpenSession" };
        assert_eq!(e.to_string(), "DismOpenSession failed with 0x80070005");
    }
}
