//! DISM API (`dismapi.h`, `dismapi.dll`). First cut for S-03, **unverified** until
//! `cargo xtask ffi-check` passes against the real header.
//!
//! C enums are `i32` aliases plus constants, never Rust enums: the OS may return values
//! this file doesn't list. Memory the API returns is freed with `DismDelete`
//! (`dpro-win` does this in `Drop`).
//!
//! Packing: the structs assume `#pragma pack(push, 1)` in `dismapi.h` (verify: ffi-check).

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

use crate::types::{PCWSTR, PVOID, SYSTEMTIME, UINT};
#[cfg(windows)]
use crate::types::{BOOL, DWORD, HANDLE, HRESULT};

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

pub const DISM_MOUNT_READWRITE: u32 = 0x0000_0000;
pub const DISM_MOUNT_READONLY: u32 = 0x0000_0001;
pub const DISM_MOUNT_OPTIMIZE: u32 = 0x0000_0002;
pub const DISM_MOUNT_CHECK_INTEGRITY: u32 = 0x0000_0004;
pub const DISM_COMMIT_IMAGE: u32 = 0x0000_0000;
pub const DISM_DISCARD_IMAGE: u32 = 0x0000_0001;

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
    ("DismInitialize", "HRESULT (WINAPI *)(DismLogLevel, PCWSTR, PCWSTR)"),
    ("DismShutdown", "HRESULT (WINAPI *)(void)"),
    ("DismOpenSession", "HRESULT (WINAPI *)(PCWSTR, PCWSTR, PCWSTR, DismSession *)"),
    ("DismCloseSession", "HRESULT (WINAPI *)(DismSession)"),
    ("DismGetLastErrorMessage", "HRESULT (WINAPI *)(DismString **)"),
    ("DismDelete", "HRESULT (WINAPI *)(VOID *)"),
    ("DismGetPackages", "HRESULT (WINAPI *)(DismSession, DismPackage **, UINT *)"),
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
        "DismUnmountImage",
        "HRESULT (WINAPI *)(PCWSTR, DWORD, HANDLE, DISM_PROGRESS_CALLBACK, PVOID)",
    ),
];
