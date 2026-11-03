//! DISM API sessions (doc 02 §5) over the S-03 bindings. Signatures and constants are
//! cross-checked against `dismapi.h` by `cargo xtask ffi-check` (CI, ADK headers).
//!
//! `DismApi` is the process-wide initialize/shutdown guard; sessions borrow it, so they can't
//! outlive `DismShutdown`. Buffers the API returns are freed with `DismDelete` in `Drop`.
//! The DISM API requires an elevated process.
//!
//! Long calls take a [`Call`]: a cancel event (bridged from the `CancelToken`, TD-0001 §5.3)
//! and a progress sink fed by `DISM_PROGRESS_CALLBACK`.

use std::{
    ffi::{OsStr, OsString, c_void},
    marker::PhantomData,
    path::Path,
    ptr,
};

use dpro_core::{OsError, progress::Progress};
use dpro_sys::{
    dism::{
        self, DISM_ONLINE_IMAGE, DISM_PROGRESS_CALLBACK, DISM_SESSION_DEFAULT,
        DISMAPI_S_RELOAD_IMAGE_SESSION_REQUIRED, DismDriver, DismDriverPackage,
        DismDriverSignature, DismFeature, DismPackage, DismPackageFeatureState, DismReleaseType,
        DismSession, DismString, ERROR_SUCCESS_REBOOT_REQUIRED,
    },
    types::{HANDLE, HRESULT, PCWSTR, PVOID, SYSTEMTIME},
};

/// `HRESULT_FROM_WIN32(ERROR_CANCELLED)`: what a call returns after its cancel event is set.
pub use dpro_sys::dism::HRESULT_ERROR_CANCELLED;

use crate::{
    error::{Error, Result},
    event::Event,
    wide::{WideCString, from_wide_ptr, long_path},
};

/// `E_HANDLE`: the session was closed by a failed reload.
const E_HANDLE: HRESULT = i32::from_ne_bytes(0x8007_0006_u32.to_ne_bytes());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Errors,
    ErrorsWarnings,
    ErrorsWarningsInfo,
}

/// How a successful servicing call ended. DISM reports these as success `HRESULT`s, so they
/// must never be treated as failures (`DismEnableFeature` "Return value").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    Done,
    /// `ERROR_SUCCESS_REBOOT_REQUIRED`: the change finishes at the next restart.
    RebootRequired,
    /// `DISMAPI_S_RELOAD_IMAGE_SESSION_REQUIRED`: call [`Session::reload`] before reusing it.
    ReloadSession,
}

/// Progress sink for a DISM call. DISM may call it from one of its own threads.
pub type ProgressFn<'a> = &'a (dyn Fn(Progress) + Sync);

/// Optional cancel event and progress sink for one long-running DISM call.
#[derive(Clone, Copy, Default)]
pub struct Call<'a> {
    /// Signalled to stop the call. DISM documents that a cancelled servicing call leaves the
    /// image in an unknown state, so callers pass it only to read-only or cancellable ops.
    pub cancel: Option<&'a Event>,
    pub progress: Option<ProgressFn<'a>>,
}

impl std::fmt::Debug for Call<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Call")
            .field("cancel", &self.cancel)
            .field("progress", &self.progress.is_some())
            .finish()
    }
}

impl Call<'_> {
    /// `(CancelEvent, Progress, UserData)` for a DISM call. `UserData` points into `self`,
    /// so `self` must outlive the call.
    fn raw(&self) -> (HANDLE, DISM_PROGRESS_CALLBACK, PVOID) {
        let cancel = self.cancel.map_or(ptr::null_mut(), Event::as_raw);
        match &self.progress {
            Some(sink) => (
                cancel,
                Some(progress_trampoline),
                ptr::from_ref::<ProgressFn<'_>>(sink)
                    .cast_mut()
                    .cast::<c_void>(),
            ),
            None => (cancel, None, ptr::null_mut()),
        }
    }
}

/// `DISM_PROGRESS_CALLBACK` → the caller's [`ProgressFn`].
unsafe extern "system" fn progress_trampoline(current: u32, total: u32, user_data: PVOID) {
    if user_data.is_null() {
        return;
    }
    // SAFETY: `Call::raw` passed a pointer to a `ProgressFn` that lives in the `Call`, which
    // outlives the DISM call that invokes this callback; the closure is `Sync`.
    let sink = unsafe { &*user_data.cast::<ProgressFn<'_>>() };
    sink(Progress {
        done: u64::from(current),
        total: u64::from(total),
    });
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
    pub fn initialize(
        level: LogLevel,
        log_file: Option<&Path>,
        scratch: Option<&Path>,
    ) -> Result<Self> {
        let level = match level {
            LogLevel::Errors => dism::DismLogErrors,
            LogLevel::ErrorsWarnings => dism::DismLogErrorsWarnings,
            LogLevel::ErrorsWarningsInfo => dism::DismLogErrorsWarningsInfo,
        };
        let log_file = log_file
            .map(|p| WideCString::new(long_path(p)))
            .transpose()?;
        let scratch = scratch
            .map(|p| WideCString::new(long_path(p)))
            .transpose()?;
        // SAFETY: the strings are NUL-terminated (or null) and outlive the call.
        let hr = unsafe {
            dism::DismInitialize(level, opt_ptr(log_file.as_ref()), opt_ptr(scratch.as_ref()))
        };
        check(hr, "DismInitialize")?;
        Ok(Self {
            _not_send: PhantomData,
        })
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
    /// `None` after a reload failed to reopen the image.
    id: Option<DismSession>,
    image: WideCString,
    windows_dir: Option<WideCString>,
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

/// A driver package (one INF) as reported by `DismGetDrivers` or `DismGetDriverInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverPackageInfo {
    /// `oem<N>.inf` for a third-party package in the store; an inbox package keeps its INF name.
    pub published_name: OsString,
    /// The INF's path: its copy in the driver store for a staged package, otherwise the path
    /// passed to [`Session::driver_info`].
    pub original_file: OsString,
    pub inbox: bool,
    pub catalog_file: OsString,
    pub class_name: OsString,
    pub class_guid: OsString,
    pub class_description: OsString,
    pub boot_critical: bool,
    pub signature: DismDriverSignature,
    pub provider: OsString,
    /// `DriverVer` date.
    pub date: SYSTEMTIME,
    /// `DriverVer` version: major, minor, build, revision.
    pub version: [u32; 4],
}

impl DriverPackageInfo {
    /// Copies a package out of a buffer the API returned.
    ///
    /// # Safety
    /// Every string field of `p` must be null or NUL-terminated, and stay valid for the call.
    unsafe fn from_raw(p: &DismDriverPackage) -> Result<Self> {
        // A by-value copy: fields of a packed struct can't be borrowed.
        let p = *p;
        // SAFETY: by the caller's guarantee for every string field.
        let s = |ptr: PCWSTR| unsafe { from_wide_ptr(ptr) };
        Ok(Self {
            published_name: s(p.PublishedName)?,
            original_file: s(p.OriginalFileName)?,
            inbox: p.InBox != 0,
            catalog_file: s(p.CatalogFile)?,
            class_name: s(p.ClassName)?,
            class_guid: s(p.ClassGuid)?,
            class_description: s(p.ClassDescription)?,
            boot_critical: p.BootCritical != 0,
            signature: p.DriverSignature,
            provider: s(p.ProviderName)?,
            date: p.Date,
            version: [p.MajorVersion, p.MinorVersion, p.Build, p.Revision],
        })
    }
}

/// One device an INF installs (a model line), as reported by `DismGetDriverInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverDevice {
    pub manufacturer: OsString,
    pub description: OsString,
    pub hardware_id: OsString,
    /// `PROCESSOR_ARCHITECTURE_*` of the model section.
    pub architecture: u32,
    pub service: OsString,
    pub compatible_ids: OsString,
    pub exclude_ids: OsString,
}

impl DriverDevice {
    /// # Safety
    /// As [`DriverPackageInfo::from_raw`].
    unsafe fn from_raw(d: &DismDriver) -> Result<Self> {
        let d = *d;
        // SAFETY: by the caller's guarantee for every string field.
        let s = |ptr: PCWSTR| unsafe { from_wide_ptr(ptr) };
        Ok(Self {
            manufacturer: s(d.ManufacturerName)?,
            description: s(d.HardwareDescription)?,
            hardware_id: s(d.HardwareId)?,
            architecture: d.Architecture,
            service: s(d.ServiceName)?,
            compatible_ids: s(d.CompatibleIds)?,
            exclude_ids: s(d.ExcludeIds)?,
        })
    }
}

impl<'api> Session<'api> {
    /// Session on the running OS.
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message.
    pub fn open_online(api: &'api DismApi) -> Result<Self> {
        Self::open(api, Path::new(DISM_ONLINE_IMAGE), None)
    }

    /// Session on an offline installation or mounted image: `root` is its root (e.g. `D:\`)
    /// and `windows_dir` the Windows folder relative to it (default `Windows`).
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message.
    pub fn open_offline(
        api: &'api DismApi,
        root: &Path,
        windows_dir: Option<&Path>,
    ) -> Result<Self> {
        Self::open(api, root, windows_dir)
    }

    fn open(_api: &'api DismApi, image: &Path, windows_dir: Option<&Path>) -> Result<Self> {
        let mut session = Self {
            id: None,
            image: WideCString::new(image)?,
            windows_dir: windows_dir.map(WideCString::new).transpose()?,
            _api: PhantomData,
            _not_send: PhantomData,
        };
        session.id = Some(session.open_id()?);
        Ok(session)
    }

    fn open_id(&self) -> Result<DismSession> {
        let mut id: DismSession = DISM_SESSION_DEFAULT;
        // SAFETY: strings are NUL-terminated (or null) and outlive the call; `id` is a valid
        // out pointer.
        let hr = unsafe {
            dism::DismOpenSession(
                self.image.as_ptr(),
                opt_ptr(self.windows_dir.as_ref()),
                ptr::null(),
                &raw mut id,
            )
        };
        check(hr, "DismOpenSession")?;
        Ok(id)
    }

    fn id(&self) -> Result<DismSession> {
        self.id.ok_or(Error::Dism {
            source: OsError {
                code: E_HANDLE,
                api: "DismSession",
            },
            message: String::new(),
        })
    }

    /// Closes and reopens the session, as required after
    /// [`Completion::ReloadSession`]. If reopening fails the session stays closed and every
    /// later call fails with `E_HANDLE`.
    ///
    /// # Errors
    /// The `HRESULT` of `DismOpenSession`.
    pub fn reload(&mut self) -> Result<()> {
        if let Some(id) = self.id.take() {
            // SAFETY: `id` came from a successful DismOpenSession and is closed exactly once.
            // ignore: a close failure doesn't stop the reopen, which reports its own error.
            let _ = unsafe { dism::DismCloseSession(id) };
        }
        self.id = Some(self.open_id()?);
        Ok(())
    }

    /// # Errors
    /// The `HRESULT` with DISM's error message, or a string longer than the OS limit.
    pub fn packages(&self) -> Result<Vec<PackageInfo>> {
        let id = self.id()?;
        let mut ptr: *mut DismPackage = ptr::null_mut();
        let mut count = 0_u32;
        // SAFETY: `id` is open; out pointers are valid.
        let hr = unsafe { dism::DismGetPackages(id, &raw mut ptr, &raw mut count) };
        check(hr, "DismGetPackages")?;
        let buf = DismBuf { ptr, count };
        buf.as_slice()
            .iter()
            .map(|p| {
                // Copy fields out of the packed struct before use.
                let (name, state, release_type, install_time) =
                    (p.PackageName, p.PackageState, p.ReleaseType, p.InstallTime);
                // SAFETY: DISM returns NUL-terminated package names, valid until DismDelete
                // (buf drop).
                let name = unsafe { from_wide_ptr(name) }?;
                Ok(PackageInfo {
                    name,
                    state,
                    release_type,
                    install_time,
                })
            })
            .collect()
    }

    /// Features of the image (no package filter).
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message, or a string longer than the OS limit.
    pub fn features(&self) -> Result<Vec<FeatureInfo>> {
        let id = self.id()?;
        let mut ptr: *mut DismFeature = ptr::null_mut();
        let mut count = 0_u32;
        // SAFETY: `id` is open; null identifier + DismPackageNone means "all features".
        let hr = unsafe {
            dism::DismGetFeatures(
                id,
                ptr::null(),
                dism::DismPackageNone,
                &raw mut ptr,
                &raw mut count,
            )
        };
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

    /// `DismCheckImageHealth`: `scan = false` is the quick check, `true` the full scan.
    /// Read-only, so cancelling it is safe. Returns the raw `DismImageHealthState`.
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message (`HRESULT_ERROR_CANCELLED` when cancelled).
    pub fn check_health(&self, scan: bool, call: Call<'_>) -> Result<i32> {
        let id = self.id()?;
        let mut state = dism::DismImageHealthy;
        let (cancel, progress, user_data) = call.raw();
        // SAFETY: `id` is open; `state` is a valid out pointer; `call` outlives the call.
        let hr = unsafe {
            dism::DismCheckImageHealth(
                id,
                i32::from(scan),
                cancel,
                progress,
                user_data,
                &raw mut state,
            )
        };
        check(hr, "DismCheckImageHealth")?;
        Ok(state)
    }
}

/// Mutations: only for `Backend::apply` implementations (TD-0001; services never call these).
impl Session<'_> {
    /// `DismEnableFeature`. On Windows 8.1 and later DISM ignores `sources` and
    /// `limit_access` for this call (they're honoured by [`Session::restore_health`]).
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message.
    pub fn enable_feature(
        &self,
        name: &OsStr,
        sources: &[&Path],
        limit_access: bool,
        call: Call<'_>,
    ) -> Result<Completion> {
        let id = self.id()?;
        let name = WideCString::new(name)?;
        let sources = WideList::new(sources)?;
        let (cancel, progress, user_data) = call.raw();
        // SAFETY: every string is NUL-terminated and outlives the call; `sources.ptr()` points
        // to `sources.count()` valid string pointers (or is null with count 0).
        let hr = unsafe {
            dism::DismEnableFeature(
                id,
                name.as_ptr(),
                ptr::null(),
                dism::DismPackageNone,
                i32::from(limit_access),
                sources.ptr(),
                sources.count(),
                0,
                cancel,
                progress,
                user_data,
            )
        };
        check(hr, "DismEnableFeature")
    }

    /// `DismDisableFeature`, keeping the payload so the change stays reversible.
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message.
    pub fn disable_feature(&self, name: &OsStr, call: Call<'_>) -> Result<Completion> {
        let id = self.id()?;
        let name = WideCString::new(name)?;
        let (cancel, progress, user_data) = call.raw();
        // SAFETY: `name` is NUL-terminated and outlives the call; null package name = any.
        let hr = unsafe {
            dism::DismDisableFeature(
                id,
                name.as_ptr(),
                ptr::null(),
                0,
                cancel,
                progress,
                user_data,
            )
        };
        check(hr, "DismDisableFeature")
    }

    /// `DismRestoreImageHealth` with optional repair sources; `limit_access` keeps Windows
    /// Update out of it.
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message.
    pub fn restore_health(
        &self,
        sources: &[&Path],
        limit_access: bool,
        call: Call<'_>,
    ) -> Result<Completion> {
        let id = self.id()?;
        let sources = WideList::new(sources)?;
        let (cancel, progress, user_data) = call.raw();
        // SAFETY: `sources.ptr()` points to `sources.count()` NUL-terminated strings that
        // outlive the call (or is null).
        let hr = unsafe {
            dism::DismRestoreImageHealth(
                id,
                sources.ptr(),
                sources.count(),
                i32::from(limit_access),
                cancel,
                progress,
                user_data,
            )
        };
        check(hr, "DismRestoreImageHealth")
    }

    /// `DismRemovePackage` by package identity name.
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message.
    pub fn remove_package(&self, identity: &OsStr, call: Call<'_>) -> Result<Completion> {
        let id = self.id()?;
        let identity = WideCString::new(identity)?;
        let (cancel, progress, user_data) = call.raw();
        // SAFETY: `identity` is NUL-terminated and outlives the call.
        let hr = unsafe {
            dism::DismRemovePackage(
                id,
                identity.as_ptr(),
                dism::DismPackageName,
                cancel,
                progress,
                user_data,
            )
        };
        check(hr, "DismRemovePackage")
    }

    /// `DismGetDrivers`: the third-party packages in the driver store, and with `all` the
    /// inbox ones too.
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message, or a string longer than the OS limit.
    pub fn drivers(&self, all: bool) -> Result<Vec<DriverPackageInfo>> {
        let id = self.id()?;
        let mut ptr: *mut DismDriverPackage = ptr::null_mut();
        let mut count = 0_u32;
        // SAFETY: `id` is open; out pointers are valid.
        let hr = unsafe { dism::DismGetDrivers(id, i32::from(all), &raw mut ptr, &raw mut count) };
        check(hr, "DismGetDrivers")?;
        let buf = DismBuf { ptr, count };
        buf.as_slice()
            .iter()
            // SAFETY: DISM returns NUL-terminated (or null) strings, valid until DismDelete
            // (buf drop).
            .map(|p| unsafe { DriverPackageInfo::from_raw(p) })
            .collect()
    }

    /// `DismGetDriverInfo` for an INF path or a published name (`oem3.inf`): the devices it
    /// installs and, when DISM reports it, the package.
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message, or a string longer than the OS limit.
    pub fn driver_info(
        &self,
        inf: &OsStr,
    ) -> Result<(Vec<DriverDevice>, Option<DriverPackageInfo>)> {
        let id = self.id()?;
        let inf = WideCString::new(inf)?;
        let mut devices: *mut DismDriver = ptr::null_mut();
        let mut count = 0_u32;
        let mut package: *mut DismDriverPackage = ptr::null_mut();
        // SAFETY: `id` is open; `inf` is NUL-terminated and outlives the call; out pointers
        // are valid.
        let hr = unsafe {
            dism::DismGetDriverInfo(
                id,
                inf.as_ptr(),
                &raw mut devices,
                &raw mut count,
                &raw mut package,
            )
        };
        // Both buffers are owned (and freed) from here on, even if the call failed.
        let devices = DismBuf {
            ptr: devices,
            count,
        };
        let package = DismBuf {
            ptr: package,
            count: 1,
        };
        check(hr, "DismGetDriverInfo")?;
        let list = devices
            .as_slice()
            .iter()
            // SAFETY: as in `drivers`.
            .map(|d| unsafe { DriverDevice::from_raw(d) })
            .collect::<Result<_>>()?;
        let package = match package.as_slice().first() {
            // SAFETY: as in `drivers`.
            Some(p) => Some(unsafe { DriverPackageInfo::from_raw(p) }?),
            None => None,
        };
        Ok((list, package))
    }

    /// `DismAddDriver`: stages the package of one INF into the store. `force_unsigned` lets
    /// an offline image take an unsigned package; DISM ignores it online. The API has no
    /// cancel event and no progress callback.
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message.
    pub fn add_driver(&self, inf: &Path, force_unsigned: bool) -> Result<Completion> {
        let id = self.id()?;
        // No `\\?\` prefix: DISM's driver paths aren't documented to take one, and the
        // store's paths stay short.
        let inf = WideCString::new(inf)?;
        // SAFETY: `inf` is NUL-terminated and outlives the call.
        let hr = unsafe { dism::DismAddDriver(id, inf.as_ptr(), i32::from(force_unsigned)) };
        check(hr, "DismAddDriver")
    }

    /// `DismRemoveDriver` by published name (`oem3.inf`). Inbox packages can't be removed.
    ///
    /// # Errors
    /// The `HRESULT` with DISM's error message.
    pub fn remove_driver(&self, published: &OsStr) -> Result<Completion> {
        let id = self.id()?;
        let published = WideCString::new(published)?;
        // SAFETY: `published` is NUL-terminated and outlives the call.
        let hr = unsafe { dism::DismRemoveDriver(id, published.as_ptr()) };
        check(hr, "DismRemoveDriver")
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            // SAFETY: `id` came from a successful DismOpenSession and is closed exactly once.
            // ignore: nothing useful to do if closing fails during drop.
            let _ = unsafe { dism::DismCloseSession(id) };
        }
    }
}

/// Owned wide strings plus the pointer array DISM's `PCWSTR*` parameters take.
struct WideList {
    _strings: Vec<WideCString>,
    ptrs: Vec<*const u16>,
}

impl WideList {
    fn new(paths: &[&Path]) -> Result<Self> {
        let strings: Vec<WideCString> = paths
            .iter()
            .map(|p| WideCString::new(long_path(p)))
            .collect::<Result<_>>()?;
        let ptrs = strings.iter().map(WideCString::as_ptr).collect();
        Ok(Self {
            _strings: strings,
            ptrs,
        })
    }

    /// Null for an empty list, as DISM expects with a count of 0.
    fn ptr(&self) -> *const *const u16 {
        if self.ptrs.is_empty() {
            ptr::null()
        } else {
            self.ptrs.as_ptr()
        }
    }

    fn count(&self) -> u32 {
        u32::try_from(self.ptrs.len()).unwrap_or(u32::MAX)
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

/// Classifies `hr` like `SUCCEEDED()`; failures carry DISM's last error message.
fn check(hr: HRESULT, api: &'static str) -> Result<Completion> {
    match completion(hr) {
        Some(c) => Ok(c),
        None => Err(Error::Dism {
            source: OsError { code: hr, api },
            message: last_error_message().unwrap_or_default(),
        }),
    }
}

/// `None` for a failure `HRESULT`. Unknown success codes count as plain success.
fn completion(hr: HRESULT) -> Option<Completion> {
    match hr {
        ERROR_SUCCESS_REBOOT_REQUIRED => Some(Completion::RebootRequired),
        DISMAPI_S_RELOAD_IMAGE_SESSION_REQUIRED => Some(Completion::ReloadSession),
        hr if hr >= 0 => Some(Completion::Done),
        _ => None,
    }
}

fn last_error_message() -> Option<String> {
    let mut msg: *mut DismString = ptr::null_mut();
    // SAFETY: `msg` is a valid out pointer.
    let hr = unsafe { dism::DismGetLastErrorMessage(&raw mut msg) };
    if hr < 0 || msg.is_null() {
        return None;
    }
    // SAFETY: `msg` points to a DismString from the API; copy its Value before freeing.
    let value = unsafe { (*msg).Value };
    // SAFETY: DISM strings are NUL-terminated.
    let text = unsafe { from_wide_ptr(value) }
        .ok()
        .map(|s| s.to_string_lossy().trim_end().to_owned());
    // SAFETY: `msg` was allocated by the DISM API and is freed exactly once.
    let _ = unsafe { dism::DismDelete(msg.cast()) }; // ignore: best effort
    text
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn success_codes_are_not_failures() {
        assert_eq!(completion(0), Some(Completion::Done));
        assert_eq!(completion(3010), Some(Completion::RebootRequired));
        assert_eq!(completion(1), Some(Completion::ReloadSession));
        assert_eq!(completion(2), Some(Completion::Done));
        // HRESULT_FROM_WIN32(ERROR_SUCCESS_REBOOT_REQUIRED) is a failure code, unlike 3010.
        assert_eq!(
            completion(i32::from_ne_bytes(0x8007_0BC2_u32.to_ne_bytes())),
            None
        );
        assert_eq!(completion(dism::HRESULT_ERROR_CANCELLED), None);
    }

    #[test]
    fn progress_reaches_the_sink_through_the_trampoline() {
        let seen = Mutex::new(Vec::new());
        let sink = |p: Progress| seen.lock().unwrap().push(p);
        let call = Call {
            cancel: None,
            progress: Some(&sink),
        };
        let (cancel, callback, user_data) = call.raw();
        assert!(cancel.is_null());
        // SAFETY: exactly what DISM does: invoke the callback with the user data it was given,
        // while `call` is alive.
        unsafe { callback.unwrap()(3, 10, user_data) };
        assert_eq!(*seen.lock().unwrap(), [Progress { done: 3, total: 10 }]);
        assert!(Call::default().raw().1.is_none());
    }
}
