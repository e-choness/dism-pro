//! Admin-only folders for DISM Pro's own state (TD-0001 §8). The journal says what to undo
//! and the elevated process replays it, so a standard user must not be able to read, plant or
//! edit one. `ProgramData` lets any user create subfolders (which they then own), so an
//! existing folder is trusted only if SYSTEM or Administrators own it.

use std::{
    fs::OpenOptions,
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::Path,
    ptr,
};

use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, GetLastError, LocalFree},
    Security::{
        Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
            SE_FILE_OBJECT,
        },
        IsWellKnownSid, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        SECURITY_ATTRIBUTES, WinBuiltinAdministratorsSid, WinLocalSystemSid,
    },
    Storage::FileSystem::{
        CreateDirectoryW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, READ_CONTROL,
    },
};

use dpro_core::OsError;

use crate::{
    error::{Error, Result, last_error_if_false, win32},
    wide::{WideCString, long_path},
};

/// Owner Administrators; protected DACL: full control for SYSTEM and Administrators, inherited
/// by everything below. No other access.
const ADMIN_ONLY: &str = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// Makes sure `dir` is an admin-only folder: creates it (parents normally, `dir` itself with
/// the admin-only descriptor), or checks that an existing one is a real folder (not a link)
/// owned by SYSTEM or Administrators. Needs elevation.
///
/// # Errors
/// [`Error::UntrustedDir`] for an existing folder someone else owns, or a link; OS errors
/// otherwise.
pub fn ensure_admin_only_dir(dir: &Path) -> Result<()> {
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_error(&e, "CreateDirectoryW"))?;
    }
    let sd = LocalBox(admin_only_descriptor()?);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let path = WideCString::new(long_path(dir))?;
    // SAFETY: `path` is NUL-terminated; `attributes` points to a valid descriptor that lives
    // in `sd` until after the call.
    if unsafe { CreateDirectoryW(path.as_ptr(), &raw const attributes) } != 0 {
        return Ok(());
    }
    // SAFETY: no preconditions; read right after the failing call.
    let code = unsafe { GetLastError() };
    if code != ERROR_ALREADY_EXISTS {
        return Err(OsError::from_win32(code, "CreateDirectoryW").into());
    }

    // Open the folder itself, not what a planted junction points to, and check that handle.
    let folder = OpenOptions::new()
        .access_mode(READ_CONTROL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(dir)
        .map_err(|e| io_error(&e, "CreateFileW"))?;
    let md = folder
        .metadata()
        .map_err(|e| io_error(&e, "GetFileInformationByHandle"))?;
    if !md.is_dir() || md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(Error::UntrustedDir(dir.to_path_buf()));
    }
    let mut owner: PSID = ptr::null_mut();
    let mut owner_sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: the handle is open with READ_CONTROL; the out pointers are valid; `owner` points
    // into `owner_sd`, which is freed (LocalBox) after its last use.
    let status = unsafe {
        GetSecurityInfo(
            folder.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &raw mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &raw mut owner_sd,
        )
    };
    win32(status, "GetSecurityInfo")?;
    let _owner_sd = LocalBox(owner_sd);
    // No owner (a file system without ACLs) can't be trusted either.
    // SAFETY: a non-null `owner` is a valid SID inside `owner_sd`.
    let trusted = !owner.is_null()
        && unsafe {
            IsWellKnownSid(owner, WinBuiltinAdministratorsSid) != 0
                || IsWellKnownSid(owner, WinLocalSystemSid) != 0
        };
    if trusted {
        Ok(())
    } else {
        Err(Error::UntrustedDir(dir.to_path_buf()))
    }
}

fn admin_only_descriptor() -> Result<PSECURITY_DESCRIPTOR> {
    let sddl = WideCString::new(ADMIN_ONLY)?;
    let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `sddl` is NUL-terminated; `sd` is a valid out pointer; the size is optional.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &raw mut sd,
            ptr::null_mut(),
        )
    };
    last_error_if_false(ok, "ConvertStringSecurityDescriptorToSecurityDescriptorW")?;
    Ok(sd)
}

/// A `LocalAlloc`ed buffer from a security API, freed with `LocalFree`.
struct LocalBox(PSECURITY_DESCRIPTOR);

impl Drop for LocalBox {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: allocated with LocalAlloc by the security API and freed exactly once.
            // ignore: LocalFree only fails for an invalid handle.
            let _ = unsafe { LocalFree(self.0) };
        }
    }
}

fn io_error(e: &std::io::Error, api: &'static str) -> Error {
    let code = e
        .raw_os_error()
        .and_then(|c| u32::try_from(c).ok())
        .unwrap_or(0);
    OsError::from_win32(code, api).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_parses() {
        let sd = LocalBox(admin_only_descriptor().unwrap());
        assert!(!sd.0.is_null());
    }

    #[test]
    fn refuses_links() {
        let base = std::env::temp_dir().join(format!("dpro-sec-{}", std::process::id()));
        let target = base.join("target");
        let link = base.join("link");
        std::fs::create_dir_all(&target).unwrap();
        // Directory symlinks need a privilege or developer mode; skip where unavailable. Wine
        // reports success without listing the link, so check that it's really there.
        let made = std::os::windows::fs::symlink_dir(&target, &link).is_ok();
        if made && std::fs::read_dir(&base).unwrap().count() == 2 {
            assert!(matches!(
                ensure_admin_only_dir(&link),
                Err(Error::UntrustedDir(_))
            ));
        }
        std::fs::remove_dir_all(&base).ok();
    }
}
