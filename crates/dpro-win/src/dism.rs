//! DISM API sessions (doc 02 §5) over the S-03 bindings. **Unverified** until
//! `cargo xtask ffi-check` passes (CAVEATS.md §5).
//!
//! `DismApi` is the process-wide initialize/shutdown guard; sessions borrow it, so they can't
//! outlive `DismShutdown`. Buffers the API returns are freed with `DismDelete` in `Drop`.
//! The DISM API requires an elevated process.

use std::{
    ffi::{OsStr, OsString},
    marker::PhantomData,
    path::Path,
    ptr,
};

use dpro_core::OsError;
use dpro_sys::{
    dism::{
        self, DISM_ONLINE_IMAGE, DISM_SESSION_DEFAULT, DismFeature, DismPackage, DismPackageFeatureState, DismReleaseType,
        DismSession, DismString,
    },
    types::{HRESULT, S_OK, SYSTEMTIME},
};

use crate::{
    error::{Error, Result},
    wide::{WideCString, from_wide_ptr, long_path},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Errors,
    ErrorsWarnings,
    ErrorsWarningsInfo,
}

/// Initialized DISM API. Create once per process; dropping it calls `DismShutdown`.
#[derive(Debug)]
pub struct DismApi {
    // !Send + !Sync: initialize and shut down on the same thread.
    _not_send: PhantomData<*const ()>,
}

impl DismApi {
    /// `DismInitialize`. `log_file`/`scratch` default to DISM's own locations when `None`.
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message.
    pub fn initialize(level: LogLevel, log_file: Option<&Path>, scratch: Option<&Path>) -> Result<Self> {
        let level = match level {
            LogLevel::Errors => dism::DismLogErrors,
            LogLevel::ErrorsWarnings => dism::DismLogErrorsWarnings,
            LogLevel::ErrorsWarningsInfo => dism::DismLogErrorsWarningsInfo,
        };
        let log_file = log_file.map(|p| WideCString::new(long_path(p))).transpose()?;
        let scratch = scratch.map(|p| WideCString::new(long_path(p))).transpose()?;
        // SAFETY: the strings are NUL-terminated (or null) and outlive the call.
        let hr = unsafe { dism::DismInitialize(level, opt_ptr(log_file.as_ref()), opt_ptr(scratch.as_ref())) };
        check(hr, "DismInitialize")?;
        Ok(Self { _not_send: PhantomData })
    }
}

impl Drop for DismApi {
    fn drop(&mut self) {
        // SAFETY: DismInitialize succeeded; every Session borrowing `self` is already dropped.
        // ignore: nothing useful to do if shutdown fails during drop.
        let _ = unsafe { dism::DismShutdown() };
    }
}

/// An open DISM session on one target. Not `Send`: the target actor that opened it owns it
/// (doc 02 §3: one actor per target serializes its DISM calls).
#[derive(Debug)]
pub struct Session<'api> {
    id: DismSession,
    _api: PhantomData<&'api DismApi>,
    _not_send: PhantomData<*const ()>,
}

/// A package as reported by `DismGetPackages`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInfo {
    pub name: OsString,
    pub state: DismPackageFeatureState,
    pub release_type: DismReleaseType,
    pub install_time: SYSTEMTIME,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureInfo {
    pub name: OsString,
    pub state: DismPackageFeatureState,
}

impl<'api> Session<'api> {
    /// Session on the running OS.
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message.
    pub fn open_online(api: &'api DismApi) -> Result<Self> {
        Self::open(api, Path::new(DISM_ONLINE_IMAGE), None)
    }

    /// Session on an offline installation: `root` is its volume root (e.g. `D:\`) and
    /// `windows_dir` the Windows folder relative to it (default `Windows`).
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message.
    pub fn open_offline(api: &'api DismApi, root: &Path, windows_dir: Option<&Path>) -> Result<Self> {
        Self::open(api, root, windows_dir)
    }

    fn open(_api: &'api DismApi, image: &Path, windows_dir: Option<&Path>) -> Result<Self> {
        let image = WideCString::new(image)?;
        let windows_dir = windows_dir.map(WideCString::new).transpose()?;
        let mut id: DismSession = DISM_SESSION_DEFAULT;
        // SAFETY: strings are NUL-terminated (or null) and outlive the call; `id` is a valid out pointer.
        let hr = unsafe { dism::DismOpenSession(image.as_ptr(), opt_ptr(windows_dir.as_ref()), ptr::null(), &mut id) };
        check(hr, "DismOpenSession")?;
        Ok(Self { id, _api: PhantomData, _not_send: PhantomData })
    }

    /// # Errors
    /// The `HRESULT` with DISM's error message, or a string longer than the OS limit.
    pub fn packages(&self) -> Result<Vec<PackageInfo>> {
        let mut ptr: *mut DismPackage = ptr::null_mut();
        let mut count = 0_u32;
        // SAFETY: `self.id` is open; out pointers are valid.
        let hr = unsafe { dism::DismGetPackages(self.id, &mut ptr, &mut count) };
        check(hr, "DismGetPackages")?;
        let buf = DismBuf { ptr, count };
        buf.as_slice()
            .iter()
            .map(|p| {
                // Copy fields out of the packed struct before use.
                let (name, state, release_type, install_time) = (p.PackageName, p.PackageState, p.ReleaseType, p.InstallTime);
                // SAFETY: DISM returns NUL-terminated package names, valid until DismDelete (buf drop).
                let name = unsafe { from_wide_ptr(name) }?;
                Ok(PackageInfo { name, state, release_type, install_time })
            })
            .collect()
    }

    /// Features of the image (no package filter).
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message, or a string longer than the OS limit.
    pub fn features(&self) -> Result<Vec<FeatureInfo>> {
        let mut ptr: *mut DismFeature = ptr::null_mut();
        let mut count = 0_u32;
        // SAFETY: `self.id` is open; null identifier + DismPackageNone means "all features".
        let hr = unsafe { dism::DismGetFeatures(self.id, ptr::null(), dism::DismPackageNone, &mut ptr, &mut count) };
        check(hr, "DismGetFeatures")?;
        let buf = DismBuf { ptr, count };
        buf.as_slice()
            .iter()
            .map(|f| {
                let (name, state) = (f.FeatureName, f.State);
                // SAFETY: as in `packages`.
                let name = unsafe { from_wide_ptr(name) }?;
                Ok(FeatureInfo { name, state })
            })
            .collect()
    }
}

/// Mutations: only for `Backend::apply` implementations (TD-0001; services never call these).
/// No progress callback or cancel event yet (CAVEATS.md §7).
impl Session<'_> {
    /// `DismEnableFeature`. `sources` are extra payload locations (`/Source`); with
    /// `limit_access` Windows Update isn't contacted.
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message.
    pub fn enable_feature(&self, name: &OsStr, sources: &[&Path], limit_access: bool) -> Result<()> {
        let name = WideCString::new(name)?;
        let sources: Vec<WideCString> = sources.iter().map(|p| WideCString::new(long_path(p))).collect::<Result<_>>()?;
        let source_ptrs: Vec<*const u16> = sources.iter().map(WideCString::as_ptr).collect();
        let count = u32::try_from(source_ptrs.len()).unwrap_or(u32::MAX);
        let sources_arg = if source_ptrs.is_empty() { ptr::null() } else { source_ptrs.as_ptr() };
        // SAFETY: every string is NUL-terminated and outlives the call; `sources_arg` points to
        // `count` valid string pointers (or is null with count 0).
        let hr = unsafe {
            dism::DismEnableFeature(
                self.id,
                name.as_ptr(),
                ptr::null(),
                dism::DismPackageNone,
                i32::from(limit_access),
                sources_arg,
                count,
                0,
                ptr::null_mut(),
                None,
                ptr::null_mut(),
            )
        };
        check(hr, "DismEnableFeature")
    }

    /// `DismDisableFeature`, keeping the payload so the change stays reversible.
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message.
    pub fn disable_feature(&self, name: &OsStr) -> Result<()> {
        let name = WideCString::new(name)?;
        // SAFETY: `name` is NUL-terminated and outlives the call; null package name = any.
        let hr = unsafe {
            dism::DismDisableFeature(self.id, name.as_ptr(), ptr::null(), 0, ptr::null_mut(), None, ptr::null_mut())
        };
        check(hr, "DismDisableFeature")
    }

    /// `DismRemovePackage` by package identity name.
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message.
    pub fn remove_package(&self, identity: &OsStr) -> Result<()> {
        let identity = WideCString::new(identity)?;
        // SAFETY: `identity` is NUL-terminated and outlives the call.
        let hr = unsafe {
            dism::DismRemovePackage(
                self.id,
                identity.as_ptr(),
                dism::DismPackageName,
                ptr::null_mut(),
                None,
                ptr::null_mut(),
            )
        };
        check(hr, "DismRemovePackage")
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        // SAFETY: `self.id` came from a successful DismOpenSession and is closed exactly once.
        // ignore: nothing useful to do if closing fails during drop.
        let _ = unsafe { dism::DismCloseSession(self.id) };
    }
}

/// An array allocated by the DISM API, freed with `DismDelete`.
struct DismBuf<T> {
    ptr: *mut T,
    count: u32,
}

impl<T> DismBuf<T> {
    fn as_slice(&self) -> &[T] {
        if self.ptr.is_null() || self.count == 0 {
            return &[];
        }
        let len = usize::try_from(self.count).unwrap_or(0);
        // SAFETY: DISM returned `count` contiguous elements at `ptr`, alive until DismDelete.
        // The structs are packed(1), so any alignment is valid.
        unsafe { std::slice::from_raw_parts(self.ptr, len) }
    }
}

impl<T> Drop for DismBuf<T> {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // SAFETY: `ptr` was allocated by the DISM API and is freed exactly once.
            // ignore: nothing useful to do if freeing fails during drop.
            let _ = unsafe { dism::DismDelete(self.ptr.cast()) };
        }
    }
}

fn opt_ptr(s: Option<&WideCString>) -> *const u16 {
    s.map_or(ptr::null(), WideCString::as_ptr)
}

/// `Err` with DISM's last error message unless `hr` is `S_OK`.
fn check(hr: HRESULT, api: &'static str) -> Result<()> {
    if hr == S_OK {
        return Ok(());
    }
    let source = OsError { code: hr, api };
    Err(Error::Dism { source, message: last_error_message().unwrap_or_default() })
}

fn last_error_message() -> Option<String> {
    let mut msg: *mut DismString = ptr::null_mut();
    // SAFETY: `msg` is a valid out pointer.
    let hr = unsafe { dism::DismGetLastErrorMessage(&mut msg) };
    if hr != S_OK || msg.is_null() {
        return None;
    }
    // SAFETY: `msg` points to a DismString from the API; copy its Value before freeing.
    let value = unsafe { (*msg).Value };
    // SAFETY: DISM strings are NUL-terminated.
    let text = unsafe { from_wide_ptr(value) }.ok().map(|s| s.to_string_lossy().into_owned());
    // SAFETY: `msg` was allocated by the DISM API and is freed exactly once.
    let _ = unsafe { dism::DismDelete(msg.cast()) }; // ignore: best effort
    text
}
