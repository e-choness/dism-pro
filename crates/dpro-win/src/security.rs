//! Files and folders only administrators control.
//!
//! - DISM Pro's state folder (TD-0001 §8). The journal says what to undo and the elevated process
//!   replays it, so a standard user must not be able to read, plant or edit one.
//! - The agent policy inside it (TD-0013 §15), which tells the elevated broker what agents may do.
//!
//! `ProgramData` lets any user create subfolders (which they then own), so an existing folder or
//! file is trusted only if SYSTEM or Administrators own it.

use std::{
    ffi::c_void,
    fs::{File, OpenOptions},
    io::{self, Read},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::Path,
    ptr,
};

use windows_sys::Win32::{
    Foundation::{
        ERROR_ALREADY_EXISTS, GENERIC_ALL, GENERIC_READ, GENERIC_WRITE, GetLastError, LocalFree,
    },
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL,
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT,
        },
        DACL_SECURITY_INFORMATION, GetAce, IsWellKnownSid, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, WinBuiltinAdministratorsSid,
        WinLocalSystemSid,
    },
    Storage::FileSystem::{
        CreateDirectoryW, DELETE, FILE_APPEND_DATA, FILE_ATTRIBUTE_HIDDEN,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_WRITE_DATA, FILE_WRITE_EA, GetFileAttributesW, INVALID_FILE_ATTRIBUTES, READ_CONTROL,
        SetFileAttributesW, WRITE_DAC, WRITE_OWNER,
    },
};

use dpro_core::OsError;

use crate::{
    error::{Error, Result, last_error_if_false, last_os_error, win32},
    wide::{WideCString, from_wide_ptr, long_path},
};

/// Owner Administrators; protected DACL: full control for SYSTEM and Administrators, inherited
/// by everything below. No other access.
const ADMIN_ONLY: &str = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// Rights that let someone change a file: its data, its extended attributes, the file itself
/// (delete), its permissions or its owner, and the generic rights that include them.
const CHANGE_RIGHTS: u32 = FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_WRITE_EA
    | DELETE
    | WRITE_DAC
    | WRITE_OWNER
    | GENERIC_WRITE
    | GENERIC_ALL;

// ACE types and flags (winnt.h); the DACL walk needs only these.
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
/// `ACCESS_DENIED`, `ACCESS_DENIED_OBJECT`, `ACCESS_DENIED_CALLBACK`,
/// `ACCESS_DENIED_CALLBACK_OBJECT`: they only take rights away.
const DENY_ACE_TYPES: [u8; 4] = [1, 6, 10, 12];
const INHERIT_ONLY_ACE: u8 = 0x08;

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
    let sd = descriptor(ADMIN_ONLY)?;
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
    let folder = open_no_follow(dir, READ_CONTROL).map_err(|e| io_error(&e, "CreateFileW"))?;
    check_admin_folder(&folder, dir)
}

/// [`ensure_admin_only_dir`], plus the hidden attribute: the quarantine root (TD-0001 §5.2) sits
/// at the top of a volume, where Explorer would otherwise show it.
///
/// # Errors
/// As [`ensure_admin_only_dir`]; OS errors reading or setting the attributes.
pub fn ensure_hidden_admin_only_dir(dir: &Path) -> Result<()> {
    ensure_admin_only_dir(dir)?;
    let path = WideCString::new(long_path(dir))?;
    // SAFETY: `path` is NUL-terminated and outlives the call.
    let attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return Err(last_os_error("GetFileAttributesW").into());
    }
    if attributes & FILE_ATTRIBUTE_HIDDEN != 0 {
        return Ok(());
    }
    // SAFETY: as above.
    let ok = unsafe { SetFileAttributesW(path.as_ptr(), attributes | FILE_ATTRIBUTE_HIDDEN) };
    last_error_if_false(ok, "SetFileAttributesW")
}

/// Reads a small file that only administrators can change, such as the agent policy (TD-0013
/// §15), through the handle whose security it checked: no one can swap the file in between.
///
/// `Ok(None)`: the file or its folder doesn't exist. Doesn't need elevation, but an unelevated
/// process can't open the admin-only folder (`Error::Os` with access denied).
///
/// # Errors
/// - [`Error::UntrustedDir`]: the folder is a link or isn't owned by SYSTEM or Administrators.
/// - [`Error::UntrustedFile`]: the file is a link or a folder, isn't owned by them, someone else
///   may change it, or it's larger than `max_len`.
/// - OS errors otherwise.
pub fn read_admin_only_file(path: &Path, max_len: u64) -> Result<Option<Vec<u8>>> {
    let untrusted = |why: String| Error::UntrustedFile {
        path: path.to_path_buf(),
        why,
    };
    let dir = path
        .parent()
        .ok_or_else(|| untrusted("has no folder".to_owned()))?;
    let folder = match open_no_follow(dir, READ_CONTROL) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(&e, "CreateFileW")),
    };
    check_admin_folder(&folder, dir)?;

    // GENERIC_READ includes READ_CONTROL, which GetSecurityInfo needs.
    let mut file = match open_no_follow(path, GENERIC_READ) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(&e, "CreateFileW")),
    };
    let md = file
        .metadata()
        .map_err(|e| io_error(&e, "GetFileInformationByHandle"))?;
    if md.is_dir() || md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(untrusted("is a folder or a link".to_owned()));
    }
    let security = Security::of(&file)?;
    if !security.owned_by_admins() {
        return Err(untrusted(
            "isn't owned by SYSTEM or Administrators".to_owned(),
        ));
    }
    if let Some(who) = security.other_changer()? {
        return Err(untrusted(format!("can be changed by {who}")));
    }
    if md.len() > max_len {
        return Err(untrusted(format!(
            "is {} bytes; the limit is {max_len}",
            md.len()
        )));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(max_len.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| io_error(&e, "ReadFile"))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_len {
        return Err(untrusted(format!("grew past {max_len} bytes while read")));
    }
    Ok(Some(bytes))
}

/// Opens `path` itself, not what a planted link points to; works for folders too.
fn open_no_follow(path: &Path, access: u32) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(access)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

/// A real folder (not a link) owned by SYSTEM or Administrators.
fn check_admin_folder(folder: &File, dir: &Path) -> Result<()> {
    let md = folder
        .metadata()
        .map_err(|e| io_error(&e, "GetFileInformationByHandle"))?;
    if !md.is_dir() || md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(Error::UntrustedDir(dir.to_path_buf()));
    }
    if Security::of(folder)?.owned_by_admins() {
        Ok(())
    } else {
        Err(Error::UntrustedDir(dir.to_path_buf()))
    }
}

/// The owner and DACL of an open file, from one `GetSecurityInfo` call.
struct Security {
    owner: PSID,
    dacl: *const ACL,
    /// Owns the descriptor `owner` and `dacl` point into.
    _sd: LocalBox,
}

impl Security {
    fn of(file: &File) -> Result<Self> {
        let mut owner: PSID = ptr::null_mut();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: the handle is open with READ_CONTROL; the out pointers are valid; `owner` and
        // `dacl` point into `sd`, which `_sd` frees after their last use.
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &raw mut owner,
                ptr::null_mut(),
                &raw mut dacl,
                ptr::null_mut(),
                &raw mut sd,
            )
        };
        win32(status, "GetSecurityInfo")?;
        Ok(Self {
            owner,
            dacl,
            _sd: LocalBox(sd),
        })
    }

    /// No owner (a file system without ACLs) can't be trusted either.
    fn owned_by_admins(&self) -> bool {
        // SAFETY: a non-null owner is a valid SID inside the descriptor this struct owns.
        !self.owner.is_null() && unsafe { is_admin_sid(self.owner) }
    }

    /// The first SID other than SYSTEM or Administrators that the DACL lets change the file, as
    /// text. A missing DACL lets everyone; an allow entry of a kind not inspected here counts.
    fn other_changer(&self) -> Result<Option<String>> {
        if self.dacl.is_null() {
            return Ok(Some("everyone (the file has no access list)".to_owned()));
        }
        // SAFETY: a non-null DACL from GetSecurityInfo is a valid ACL in the owned descriptor.
        let count = unsafe { (*self.dacl).AceCount };
        for index in 0..u32::from(count) {
            let mut ace: *mut c_void = ptr::null_mut();
            // SAFETY: `index` < AceCount of a valid ACL; `ace` is a valid out pointer.
            let ok = unsafe { GetAce(self.dacl, index, &raw mut ace) };
            last_error_if_false(ok, "GetAce")?;
            // SAFETY: every ACE starts with an ACE_HEADER.
            let header = unsafe { *ace.cast::<ACE_HEADER>() };
            if header.AceFlags & INHERIT_ONLY_ACE != 0 || DENY_ACE_TYPES.contains(&header.AceType) {
                continue;
            }
            if header.AceType != ACCESS_ALLOWED_ACE_TYPE {
                return Ok(Some(format!("an access entry of type {}", header.AceType)));
            }
            let allowed = ace.cast::<ACCESS_ALLOWED_ACE>();
            // SAFETY: an ACCESS_ALLOWED_ACE: a mask, then the SID starting at SidStart.
            let (mask, sid) = unsafe { ((*allowed).Mask, (&raw mut (*allowed).SidStart).cast()) };
            // SAFETY: `sid` points to the ACE's SID, inside the owned descriptor.
            if mask & CHANGE_RIGHTS != 0 && !unsafe { is_admin_sid(sid) } {
                // SAFETY: as above.
                return Ok(Some(unsafe { sid_string(sid) }?));
            }
        }
        Ok(None)
    }
}

/// # Safety
/// `sid` must point to a valid SID.
unsafe fn is_admin_sid(sid: PSID) -> bool {
    // SAFETY: the caller guarantees a valid SID.
    unsafe {
        IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
            || IsWellKnownSid(sid, WinLocalSystemSid) != 0
    }
}

/// A SID as `S-1-…` text.
///
/// # Safety
/// `sid` must point to a valid SID.
pub(crate) unsafe fn sid_string(sid: PSID) -> Result<String> {
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: the caller guarantees a valid SID; `text` is a valid out pointer.
    let ok = unsafe { ConvertSidToStringSidW(sid, &raw mut text) };
    last_error_if_false(ok, "ConvertSidToStringSidW")?;
    let text = LocalBox(text.cast());
    // SAFETY: ConvertSidToStringSidW returns a NUL-terminated string, freed by `text`.
    let s = unsafe { from_wide_ptr(text.0.cast::<u16>()) }?;
    Ok(s.to_string_lossy().into_owned())
}

/// A security descriptor from SDDL, freed on drop.
pub(crate) fn descriptor(sddl: &str) -> Result<LocalBox> {
    let sddl = WideCString::new(sddl)?;
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
    Ok(LocalBox(sd))
}

/// A `LocalAlloc`ed buffer from a security API, freed with `LocalFree`.
pub(crate) struct LocalBox(pub(crate) PSECURITY_DESCRIPTOR);

impl Drop for LocalBox {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: allocated with LocalAlloc by the security API and freed exactly once.
            // ignore: LocalFree only fails for an invalid handle.
            let _ = unsafe { LocalFree(self.0) };
        }
    }
}

pub(crate) fn io_error(e: &std::io::Error, api: &'static str) -> Error {
    let code = e
        .raw_os_error()
        .and_then(|c| u32::try_from(c).ok())
        .unwrap_or(0);
    OsError::from_win32(code, api).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("dpro-sec-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn descriptor_parses() {
        let sd = descriptor(ADMIN_ONLY).unwrap();
        assert!(!sd.0.is_null());
    }

    #[test]
    fn refuses_links() {
        let base = scratch("links");
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

    #[test]
    fn a_missing_folder_reads_as_none() {
        let base = scratch("missing");
        let read = read_admin_only_file(&base.join("no-dir").join("policy.toml"), 1024);
        std::fs::remove_dir_all(&base).ok();
        assert!(matches!(read, Ok(None)), "{read:?}");
    }

    /// A folder owned by a user is refused, so even a missing file in it is: the folder is
    /// checked first.
    #[test]
    fn a_file_in_a_user_folder_is_refused() {
        let base = scratch("user");
        give_to_current_user(&base);
        let file = base.join("policy.toml");
        let missing = read_admin_only_file(&file, 1024);
        std::fs::write(&file, "schema = 1\n").unwrap();
        let present = read_admin_only_file(&file, 1024);
        std::fs::remove_dir_all(&base).ok();
        for read in [missing, present] {
            assert!(matches!(read, Err(Error::UntrustedDir(_))), "{read:?}");
        }
    }

    /// Makes the running user own `dir`, as a user owns a folder in their profile. An elevated
    /// administrator's new folders belong to Administrators on Windows Server (the "default
    /// owner" policy; T3 run 41677219726), which the folder check rightly trusts.
    fn give_to_current_user(dir: &Path) {
        let owned_by_admins = |dir: &Path| {
            let folder = open_no_follow(dir, READ_CONTROL).unwrap();
            Security::of(&folder).unwrap().owned_by_admins()
        };
        if !owned_by_admins(dir) {
            return;
        }
        let sid = crate::pipe::current_user_sid().unwrap();
        let status = std::process::Command::new("icacls")
            .arg(dir)
            .args(["/setowner", &format!("*{sid}"), "/Q"])
            .status()
            .unwrap();
        assert!(status.success(), "icacls /setowner: {status}");
        assert!(
            !owned_by_admins(dir),
            "the user should own {}",
            dir.display()
        );
    }

    /// Needs elevation to make an admin-only folder: runs on T3 (the runner's tests run elevated)
    /// and passes trivially elsewhere.
    #[test]
    fn an_admin_only_file_reads_back() {
        if !crate::privilege::is_elevated().unwrap_or(false) {
            return;
        }
        let base = scratch("admin");
        let dir = base.join("DismPro");
        ensure_admin_only_dir(&dir).unwrap();
        let file = dir.join("agent-policy.toml");
        let missing = read_admin_only_file(&file, 1024);
        std::fs::write(&file, "schema = 1\n").unwrap();
        let present = read_admin_only_file(&file, 1024);
        let too_big = read_admin_only_file(&file, 4);
        std::fs::remove_dir_all(&base).ok();
        assert!(matches!(missing, Ok(None)), "{missing:?}");
        assert_eq!(present.unwrap().as_deref(), Some(&b"schema = 1\n"[..]));
        assert!(
            matches!(&too_big, Err(Error::UntrustedFile { why, .. }) if why.contains("limit")),
            "{too_big:?}"
        );
    }
}
