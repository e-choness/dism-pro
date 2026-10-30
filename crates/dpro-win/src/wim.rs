//! WIM/ESD files via WIMGAPI (doc 02 §5) over the S-03 bindings. **Unverified** until
//! `cargo xtask ffi-check` passes (CAVEATS.md §5).

use std::path::Path;

use dpro_sys::{
    types::HANDLE,
    wim::{self, WIM_GENERIC_READ, WIM_OPEN_EXISTING},
};

use crate::{
    error::{Result, last_os_error},
    wide::{WideCString, long_path},
};

/// An open WIM file, closed with `WIMCloseHandle`.
#[derive(Debug)]
pub struct WimFile {
    handle: HANDLE,
}

impl WimFile {
    /// Opens an existing WIM/ESD read-only.
    ///
    /// # Errors
    /// The OS error from `WIMCreateFile`.
    pub fn open_read(path: &Path) -> Result<Self> {
        let path = WideCString::new(long_path(path))?;
        let mut creation_result = 0_u32;
        // SAFETY: `path` is NUL-terminated; `creation_result` is a valid out pointer.
        let handle = unsafe {
            wim::WIMCreateFile(
                path.as_ptr(),
                WIM_GENERIC_READ,
                WIM_OPEN_EXISTING,
                0,
                0,
                &raw mut creation_result,
            )
        };
        if handle.is_null() {
            return Err(last_os_error("WIMCreateFile").into());
        }
        Ok(Self { handle })
    }

    /// Number of images in the file.
    #[must_use]
    pub fn image_count(&self) -> u32 {
        // SAFETY: `self.handle` is an open WIM handle.
        unsafe { wim::WIMGetImageCount(self.handle) }
    }
}

impl Drop for WimFile {
    fn drop(&mut self) {
        // SAFETY: `handle` came from WIMCreateFile and is closed exactly once.
        // ignore: nothing useful to do if closing fails during drop.
        let _ = unsafe { wim::WIMCloseHandle(self.handle) };
    }
}
