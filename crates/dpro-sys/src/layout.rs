//! Layout and signature tables for the S-03 header cross-check (`cargo xtask ffi-check`).
//!
//! [`c_check_source`] turns them into a C file of `_Static_assert`s that is compiled with
//! `clang-cl` against Microsoft's header. Any size, offset or prototype mismatch between this
//! crate and the header is then a compile error.

use core::mem::{offset_of, size_of};
use std::fmt::Write as _;

use crate::{dism, types, wim};

/// One C type mirrored in this crate.
#[derive(Debug)]
pub struct Layout {
    pub c_type: &'static str,
    pub size: usize,
    /// (field, offset); empty for typedefs and enums.
    pub fields: &'static [(&'static str, usize)],
}

/// A header and everything this crate mirrors from it.
#[derive(Debug)]
pub struct Header {
    pub file: &'static str,
    pub layouts: &'static [Layout],
    /// (function, C function-pointer type)
    pub prototypes: &'static [(&'static str, &'static str)],
    /// (C constant expression, value this crate uses). Compared as `long long`, so signed
    /// `HRESULT`s and unsigned `DWORD` flags both compare by value.
    pub constants: &'static [(&'static str, i64)],
}

const fn scalar(c_type: &'static str, size: usize) -> Layout {
    Layout {
        c_type,
        size,
        fields: &[],
    }
}

pub const DISMAPI: Header = Header {
    file: "dismapi.h",
    layouts: &[
        scalar("DismSession", size_of::<dism::DismSession>()),
        scalar("DismLogLevel", size_of::<dism::DismLogLevel>()),
        scalar(
            "DismImageIdentifier",
            size_of::<dism::DismImageIdentifier>(),
        ),
        scalar(
            "DismPackageIdentifier",
            size_of::<dism::DismPackageIdentifier>(),
        ),
        scalar(
            "DismPackageFeatureState",
            size_of::<dism::DismPackageFeatureState>(),
        ),
        scalar("DismReleaseType", size_of::<dism::DismReleaseType>()),
        scalar(
            "DismImageHealthState",
            size_of::<dism::DismImageHealthState>(),
        ),
        Layout {
            c_type: "DismString",
            size: size_of::<dism::DismString>(),
            fields: &[("Value", offset_of!(dism::DismString, Value))],
        },
        Layout {
            c_type: "DismPackage",
            size: size_of::<dism::DismPackage>(),
            fields: &[
                ("PackageName", offset_of!(dism::DismPackage, PackageName)),
                ("PackageState", offset_of!(dism::DismPackage, PackageState)),
                ("ReleaseType", offset_of!(dism::DismPackage, ReleaseType)),
                ("InstallTime", offset_of!(dism::DismPackage, InstallTime)),
            ],
        },
        Layout {
            c_type: "DismFeature",
            size: size_of::<dism::DismFeature>(),
            fields: &[
                ("FeatureName", offset_of!(dism::DismFeature, FeatureName)),
                ("State", offset_of!(dism::DismFeature, State)),
            ],
        },
        Layout {
            c_type: "SYSTEMTIME",
            size: size_of::<types::SYSTEMTIME>(),
            fields: &[
                ("wYear", offset_of!(types::SYSTEMTIME, wYear)),
                (
                    "wMilliseconds",
                    offset_of!(types::SYSTEMTIME, wMilliseconds),
                ),
            ],
        },
    ],
    prototypes: dism::C_PROTOTYPES,
    constants: &[
        ("DISM_SESSION_DEFAULT", dism::DISM_SESSION_DEFAULT as i64),
        ("DismLogErrors", dism::DismLogErrors as i64),
        ("DismLogErrorsWarnings", dism::DismLogErrorsWarnings as i64),
        (
            "DismLogErrorsWarningsInfo",
            dism::DismLogErrorsWarningsInfo as i64,
        ),
        ("DismImageIndex", dism::DismImageIndex as i64),
        ("DismImageName", dism::DismImageName as i64),
        ("DismPackageNone", dism::DismPackageNone as i64),
        ("DismPackageName", dism::DismPackageName as i64),
        ("DismPackagePath", dism::DismPackagePath as i64),
        ("DismStateNotPresent", dism::DismStateNotPresent as i64),
        (
            "DismStateUninstallPending",
            dism::DismStateUninstallPending as i64,
        ),
        ("DismStateStaged", dism::DismStateStaged as i64),
        ("DismStateRemoved", dism::DismStateRemoved as i64),
        ("DismStateInstalled", dism::DismStateInstalled as i64),
        (
            "DismStateInstallPending",
            dism::DismStateInstallPending as i64,
        ),
        ("DismStateSuperseded", dism::DismStateSuperseded as i64),
        (
            "DismStatePartiallyInstalled",
            dism::DismStatePartiallyInstalled as i64,
        ),
        ("DismImageHealthy", dism::DismImageHealthy as i64),
        ("DismImageRepairable", dism::DismImageRepairable as i64),
        (
            "DismImageNonRepairable",
            dism::DismImageNonRepairable as i64,
        ),
        ("DISM_MOUNT_READWRITE", dism::DISM_MOUNT_READWRITE as i64),
        ("DISM_MOUNT_READONLY", dism::DISM_MOUNT_READONLY as i64),
        ("DISM_MOUNT_OPTIMIZE", dism::DISM_MOUNT_OPTIMIZE as i64),
        (
            "DISM_MOUNT_CHECK_INTEGRITY",
            dism::DISM_MOUNT_CHECK_INTEGRITY as i64,
        ),
        ("DISM_COMMIT_IMAGE", dism::DISM_COMMIT_IMAGE as i64),
        ("DISM_DISCARD_IMAGE", dism::DISM_DISCARD_IMAGE as i64),
        (
            "DISMAPI_S_RELOAD_IMAGE_SESSION_REQUIRED",
            dism::DISMAPI_S_RELOAD_IMAGE_SESSION_REQUIRED as i64,
        ),
        (
            "ERROR_SUCCESS_REBOOT_REQUIRED",
            dism::ERROR_SUCCESS_REBOOT_REQUIRED as i64,
        ),
        (
            "__HRESULT_FROM_WIN32(ERROR_CANCELLED)",
            dism::HRESULT_ERROR_CANCELLED as i64,
        ),
    ],
};

pub const WIMGAPI: Header = Header {
    file: "wimgapi.h",
    layouts: &[],
    prototypes: wim::C_PROTOTYPES,
    constants: &[
        ("WIM_GENERIC_READ", wim::WIM_GENERIC_READ as i64),
        ("WIM_GENERIC_WRITE", wim::WIM_GENERIC_WRITE as i64),
        ("WIM_GENERIC_MOUNT", wim::WIM_GENERIC_MOUNT as i64),
        ("WIM_CREATE_NEW", wim::WIM_CREATE_NEW as i64),
        ("WIM_CREATE_ALWAYS", wim::WIM_CREATE_ALWAYS as i64),
        ("WIM_OPEN_EXISTING", wim::WIM_OPEN_EXISTING as i64),
        ("WIM_OPEN_ALWAYS", wim::WIM_OPEN_ALWAYS as i64),
        ("WIM_COMPRESS_NONE", wim::WIM_COMPRESS_NONE as i64),
        ("WIM_COMPRESS_XPRESS", wim::WIM_COMPRESS_XPRESS as i64),
        ("WIM_COMPRESS_LZX", wim::WIM_COMPRESS_LZX as i64),
        ("WIM_COMPRESS_LZMS", wim::WIM_COMPRESS_LZMS as i64),
        ("WIM_MSG", wim::WIM_MSG as i64),
        ("WIM_MSG_TEXT", wim::WIM_MSG_TEXT as i64),
        ("WIM_MSG_PROGRESS", wim::WIM_MSG_PROGRESS as i64),
        ("WIM_MSG_PROCESS", wim::WIM_MSG_PROCESS as i64),
        ("WIM_MSG_SUCCESS", wim::WIM_MSG_SUCCESS as i64),
        ("WIM_MSG_DONE", wim::WIM_MSG_DONE as i64),
        ("WIM_MSG_SKIP_ERROR", wim::WIM_MSG_SKIP_ERROR as i64),
        ("WIM_MSG_ABORT_IMAGE", wim::WIM_MSG_ABORT_IMAGE as i64),
    ],
};

pub const HEADERS: [Header; 2] = [DISMAPI, WIMGAPI];

/// C source that fails to compile unless `header` agrees with this crate.
#[must_use]
pub fn c_check_source(header: &Header) -> String {
    let mut c = String::new();
    // ignore: writing to a String cannot fail.
    let _ = writeln!(
        c,
        "// Generated by `cargo xtask ffi-check` (S-03). Do not edit."
    );
    let _ = writeln!(
        c,
        "#include <windows.h>\n#include <stddef.h>\n#include <{}>\n",
        header.file
    );
    for l in header.layouts {
        let _ = writeln!(
            c,
            "_Static_assert(sizeof({t}) == {s}, \"sizeof {t}\");",
            t = l.c_type,
            s = l.size
        );
        for (field, offset) in l.fields {
            let _ = writeln!(
                c,
                "_Static_assert(offsetof({t}, {field}) == {offset}, \"{t}.{field}\");",
                t = l.c_type
            );
        }
    }
    for (function, pointer_type) in header.prototypes {
        let _ = writeln!(
            c,
            "_Static_assert(__builtin_types_compatible_p(__typeof__(&{function}), {pointer_type}), \
             \"{function} signature\");"
        );
    }
    for (expr, value) in header.constants {
        let _ = writeln!(
            c,
            "_Static_assert((long long)({expr}) == {value}LL, \"{expr} == {value}\");"
        );
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_layouts_match_the_assumed_header() {
        // pack(1) is the assumption ffi-check verifies; these pin what Rust produces.
        assert_eq!(size_of::<dism::DismString>(), 8);
        assert_eq!(size_of::<dism::DismPackage>(), 32);
        assert_eq!(size_of::<dism::DismFeature>(), 12);
        assert_eq!(size_of::<types::SYSTEMTIME>(), 16);
    }

    #[test]
    fn generated_source_covers_every_item() {
        for header in &HEADERS {
            let c = c_check_source(header);
            assert!(c.contains(&format!("#include <{}>", header.file)));
            for l in header.layouts {
                assert!(c.contains(&format!("sizeof({})", l.c_type)));
            }
            for (function, _) in header.prototypes {
                assert!(c.contains(&format!("&{function})")));
            }
            for (expr, _) in header.constants {
                assert!(c.contains(&format!("(long long)({expr})")));
            }
        }
    }
}
