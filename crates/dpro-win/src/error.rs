//! Errors for `dpro-win` (coding standards §3).

use dpro_core::OsError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Os(#[from] OsError),
    /// A DISM API call failed; `message` is `DismGetLastErrorMessage` when available.
    #[error("{source}: {message}")]
    Dism { source: OsError, message: String },
    #[error("string contains an interior NUL")]
    InteriorNul,
    #[error("{0} is not held by this process token")]
    PrivilegeNotHeld(&'static str),
    #[error("registry value is {actual} bytes, expected {expected}")]
    BadValueSize { expected: usize, actual: usize },
    #[error("registry value of {0} bytes is too large to write")]
    ValueTooLarge(usize),
    #[error("string from the OS is longer than {0} UTF-16 units")]
    StringTooLong(usize),
    /// DISM Pro state must live in a folder only SYSTEM and Administrators control (TD-0001 §8).
    #[error("{} isn't a folder owned by SYSTEM or Administrators; inspect and delete it, then retry", .0.display())]
    UntrustedDir(std::path::PathBuf),
}

pub type Result<T> = std::result::Result<T, Error>;

/// `Err` with the Win32 code `status` unless it is `ERROR_SUCCESS`.
pub(crate) fn win32(status: u32, api: &'static str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(OsError::from_win32(status, api).into())
    }
}

/// `Err` with `GetLastError()` when a `BOOL`-returning API returned FALSE.
pub(crate) fn last_error_if_false(ok: i32, api: &'static str) -> Result<()> {
    if ok != 0 {
        return Ok(());
    }
    Err(last_os_error(api).into())
}

/// `GetLastError()` as an [`OsError`]; call right after the API that failed.
pub(crate) fn last_os_error(api: &'static str) -> OsError {
    // SAFETY: GetLastError has no preconditions.
    let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    OsError::from_win32(code, api)
}
