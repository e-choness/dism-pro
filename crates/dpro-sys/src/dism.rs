//! DISM API (`dismapi.h`, `dismapi.dll`). First cut for S-03, **unverified** until
//! `cargo xtask ffi-check` passes against the real header.
//!
//! C enums are `i32` aliases plus constants, never Rust enums: the OS may return values
//! this file doesn't list. Memory the API returns is freed with `DismDelete`
//! (`dpro-win` does this in `Drop`).
//!
//! Packing: the structs assume `#pragma pack(push, 1)` in `dismapi.h` (verify: ffi-check).

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

#[cfg(windows)]
use crate::types::{BOOL, DWORD, HANDLE};
use crate::types::{HRESULT, PCWSTR, PVOID, SYSTEMTIME, UINT};

/// Opaque session handle (`typedef UINT DismSession`).
pub type DismSession = UINT;

/// `DISM_ONLINE_IMAGE`: pass as `ImagePath` to service the running OS.
pub const DISM_ONLINE_IMAGE: &str = "DISM_{53BFAE52-B167-4E2F-A258-0A37B57FF845}";
pub const DISM_SESSION_DEFAULT: DismSession = 0;

pub type DismLogLevel = i32;
pub const DismLogErrors: DismLogLevel = 0;
pub const DismLogErrorsWarnings: DismLogLevel = 1;
pub const DismLogErrorsWarningsInfo: DismLogLevel = 2;

pub type DismImageIdentifier = i32;
pub const DismImageIndex: DismImageIdentifier = 0;
pub const DismImageName: DismImageIdentifier = 1;

pub type DismPackageIdentifier = i32;
pub const DismPackageNone: DismPackageIdentifier = 0;
pub const DismPackageName: DismPackageIdentifier = 1;
pub const DismPackagePath: DismPackageIdentifier = 2;

pub type DismPackageFeatureState = i32;
pub const DismStateNotPresent: DismPackageFeatureState = 0;
pub const DismStateUninstallPending: DismPackageFeatureState = 1;
pub const DismStateStaged: DismPackageFeatureState = 2;
pub const DismStateRemoved: DismPackageFeatureState = 3;
pub const DismStateInstalled: DismPackageFeatureState = 4;
pub const DismStateInstallPending: DismPackageFeatureState = 5;
pub const DismStateSuperseded: DismPackageFeatureState = 6;
pub const DismStatePartiallyInstalled: DismPackageFeatureState = 7;

pub type DismReleaseType = i32;

pub type DismImageHealthState = i32;
pub const DismImageHealthy: DismImageHealthState = 0;
pub const DismImageRepairable: DismImageHealthState = 1;
pub const DismImageNonRepairable: DismImageHealthState = 2;

pub const DISM_MOUNT_READWRITE: u32 = 0x0000_0000;
pub const DISM_MOUNT_READONLY: u32 = 0x0000_0001;
pub const DISM_MOUNT_OPTIMIZE: u32 = 0x0000_0002;
pub const DISM_MOUNT_CHECK_INTEGRITY: u32 = 0x0000_0004;
pub const DISM_COMMIT_IMAGE: u32 = 0x0000_0000;
pub const DISM_DISCARD_IMAGE: u32 = 0x0000_0001;

/// Success code: the session must be closed and reopened before it is used again
/// (`DismEnableFeature` and other servicing calls, per their "Return value" docs).
pub const DISMAPI_S_RELOAD_IMAGE_SESSION_REQUIRED: HRESULT = 0x0000_0001;
/// Success code `(HRESULT)ERROR_SUCCESS_REBOOT_REQUIRED` (`winerror.h`): an online change
/// finishes at the next restart. Not `HRESULT_FROM_WIN32`: the value stays positive.
pub const ERROR_SUCCESS_REBOOT_REQUIRED: HRESULT = 3010;
/// `HRESULT_FROM_WIN32(ERROR_CANCELLED)`: a call stopped because its `CancelEvent` was set.
pub const HRESULT_ERROR_CANCELLED: HRESULT = i32::from_ne_bytes(0x8007_04C7_u32.to_ne_bytes());

/// `DISM_PROGRESS_CALLBACK`.
pub type DISM_PROGRESS_CALLBACK =
    Option<unsafe extern "system" fn(Current: UINT, Total: UINT, UserData: PVOID)>;

#[repr(C, packed(1))]
#[derive(Debug, Clone, Copy)]
pub struct DismString {
    pub Value: PCWSTR,
}

#[repr(C, packed(1))]
#[derive(Debug, Clone, Copy)]
pub struct DismPackage {
    pub PackageName: PCWSTR,
    pub PackageState: DismPackageFeatureState,
    pub ReleaseType: DismReleaseType,
    pub InstallTime: SYSTEMTIME,
}

#[repr(C, packed(1))]
#[derive(Debug, Clone, Copy)]
pub struct DismFeature {
    pub FeatureName: PCWSTR,
    pub State: DismPackageFeatureState,
}

#[cfg(windows)]
#[link(name = "dismapi", kind = "raw-dylib")]
unsafe extern "system" {
    pub fn DismInitialize(
        LogLevel: DismLogLevel,
        LogFilePath: PCWSTR,
        ScratchDirectory: PCWSTR,
    ) -> HRESULT;
    pub fn DismShutdown() -> HRESULT;
    pub fn DismOpenSession(
        ImagePath: PCWSTR,
        WindowsDirectory: PCWSTR,
        SystemDrive: PCWSTR,
        Session: *mut DismSession,
    ) -> HRESULT;
    pub fn DismCloseSession(Session: DismSession) -> HRESULT;
    pub fn DismGetLastErrorMessage(ErrorMessage: *mut *mut DismString) -> HRESULT;
    pub fn DismDelete(DismStructure: PVOID) -> HRESULT;
    pub fn DismGetPackages(
        Session: DismSession,
        Package: *mut *mut DismPackage,
        Count: *mut UINT,
    ) -> HRESULT;
    pub fn DismGetFeatures(
        Session: DismSession,
        Identifier: PCWSTR,
        PackageIdentifier: DismPackageIdentifier,
        Feature: *mut *mut DismFeature,
        Count: *mut UINT,
    ) -> HRESULT;
    pub fn DismMountImage(
        ImageFilePath: PCWSTR,
        MountPath: PCWSTR,
        ImageIndex: UINT,
        ImageName: PCWSTR,
        ImageIdentifier: DismImageIdentifier,
        Flags: DWORD,
        CancelEvent: HANDLE,
        Progress: DISM_PROGRESS_CALLBACK,
        UserData: PVOID,
    ) -> HRESULT;
    pub fn DismEnableFeature(
        Session: DismSession,
        FeatureName: PCWSTR,
        Identifier: PCWSTR,
        PackageIdentifier: DismPackageIdentifier,
        LimitAccess: BOOL,
        SourcePaths: *const PCWSTR,
        SourcePathCount: UINT,
        EnableAll: BOOL,
        CancelEvent: HANDLE,
        Progress: DISM_PROGRESS_CALLBACK,
        UserData: PVOID,
    ) -> HRESULT;
    pub fn DismDisableFeature(
        Session: DismSession,
        FeatureName: PCWSTR,
        PackageName: PCWSTR,
        RemovePayload: BOOL,
        CancelEvent: HANDLE,
        Progress: DISM_PROGRESS_CALLBACK,
        UserData: PVOID,
    ) -> HRESULT;
    pub fn DismRemovePackage(
        Session: DismSession,
        Identifier: PCWSTR,
        PackageIdentifier: DismPackageIdentifier,
        CancelEvent: HANDLE,
        Progress: DISM_PROGRESS_CALLBACK,
        UserData: PVOID,
    ) -> HRESULT;
    pub fn DismCheckImageHealth(
        Session: DismSession,
        ScanImage: BOOL,
        CancelEvent: HANDLE,
        Progress: DISM_PROGRESS_CALLBACK,
        UserData: PVOID,
        ImageHealth: *mut DismImageHealthState,
    ) -> HRESULT;
    pub fn DismRestoreImageHealth(
        Session: DismSession,
        SourcePaths: *const PCWSTR,
        SourcePathCount: UINT,
        LimitAccess: BOOL,
        CancelEvent: HANDLE,
        Progress: DISM_PROGRESS_CALLBACK,
        UserData: PVOID,
    ) -> HRESULT;
    pub fn DismUnmountImage(
        MountPath: PCWSTR,
        Flags: DWORD,
        CancelEvent: HANDLE,
        Progress: DISM_PROGRESS_CALLBACK,
        UserData: PVOID,
    ) -> HRESULT;
}

/// C prototypes of the functions above, for the ffi-check signature test. Keep in the same
/// order and in sync with the `extern` block.
pub const C_PROTOTYPES: &[(&str, &str)] = &[
    (
        "DismInitialize",
        "HRESULT (WINAPI *)(DismLogLevel, PCWSTR, PCWSTR)",
    ),
    ("DismShutdown", "HRESULT (WINAPI *)(void)"),
    (
        "DismOpenSession",
        "HRESULT (WINAPI *)(PCWSTR, PCWSTR, PCWSTR, DismSession *)",
    ),
    ("DismCloseSession", "HRESULT (WINAPI *)(DismSession)"),
    (
        "DismGetLastErrorMessage",
        "HRESULT (WINAPI *)(DismString **)",
    ),
    ("DismDelete", "HRESULT (WINAPI *)(VOID *)"),
    (
        "DismGetPackages",
        "HRESULT (WINAPI *)(DismSession, DismPackage **, UINT *)",
    ),
    (
        "DismGetFeatures",
        "HRESULT (WINAPI *)(DismSession, PCWSTR, DismPackageIdentifier, DismFeature **, UINT *)",
    ),
    (
        "DismMountImage",
        "HRESULT (WINAPI *)(PCWSTR, PCWSTR, UINT, PCWSTR, DismImageIdentifier, DWORD, HANDLE, \
         DISM_PROGRESS_CALLBACK, PVOID)",
    ),
    (
        "DismEnableFeature",
        "HRESULT (WINAPI *)(DismSession, PCWSTR, PCWSTR, DismPackageIdentifier, BOOL, PCWSTR *, UINT, BOOL, \
         HANDLE, DISM_PROGRESS_CALLBACK, PVOID)",
    ),
    (
        "DismDisableFeature",
        "HRESULT (WINAPI *)(DismSession, PCWSTR, PCWSTR, BOOL, HANDLE, DISM_PROGRESS_CALLBACK, PVOID)",
    ),
    (
        "DismRemovePackage",
        "HRESULT (WINAPI *)(DismSession, PCWSTR, DismPackageIdentifier, HANDLE, DISM_PROGRESS_CALLBACK, PVOID)",
    ),
    (
        "DismCheckImageHealth",
        "HRESULT (WINAPI *)(DismSession, BOOL, HANDLE, DISM_PROGRESS_CALLBACK, PVOID, DismImageHealthState *)",
    ),
    (
        "DismRestoreImageHealth",
        "HRESULT (WINAPI *)(DismSession, PCWSTR *, UINT, BOOL, HANDLE, DISM_PROGRESS_CALLBACK, PVOID)",
    ),
    (
        "DismUnmountImage",
        "HRESULT (WINAPI *)(PCWSTR, DWORD, HANDLE, DISM_PROGRESS_CALLBACK, PVOID)",
    ),
];
