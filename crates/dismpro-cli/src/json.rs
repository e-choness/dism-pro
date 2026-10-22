//! `--json` documents (TD-0002 §5.4). Each names its schema; files in `schemas/`.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct TargetsDoc {
    pub(crate) schema: &'static str,
    pub(crate) targets: Vec<TargetJson>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TargetJson {
    pub(crate) kind: &'static str,
    /// The value to pass as `--target`.
    pub(crate) selector: String,
    pub(crate) windows_dir: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct PackagesDoc {
    pub(crate) schema: &'static str,
    pub(crate) packages: Vec<PackageJson>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PackageJson {
    pub(crate) identity: String,
    pub(crate) state: &'static str,
    pub(crate) release_type: i32,
    /// `YYYY-MM-DDTHH:MM:SS` as reported by DISM (UTC, verify).
    pub(crate) installed: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct FeaturesDoc {
    pub(crate) schema: &'static str,
    pub(crate) features: Vec<FeatureJson>,
}

#[derive(Debug, Serialize)]
pub(crate) struct FeatureJson {
    pub(crate) name: String,
    pub(crate) state: &'static str,
    pub(crate) enabled: bool,
}

/// Outcome of an apply or the content of a journal (`job.v1`).
#[derive(Debug, Serialize)]
pub(crate) struct JobDoc {
    pub(crate) schema: &'static str,
    pub(crate) job: String,
    pub(crate) target: String,
    /// `completed`, `cancelled`, `aborted`, or `interrupted` (no commit record).
    pub(crate) status: &'static str,
    pub(crate) applied: Vec<u32>,
    pub(crate) failed: Vec<FailedJson>,
    pub(crate) without_outcome: Vec<u32>,
    pub(crate) journal: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct FailedJson {
    pub(crate) index: u32,
    pub(crate) code: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ErrorDoc {
    pub(crate) schema: &'static str,
    pub(crate) exit_code: u8,
    pub(crate) diagnosis: &'static str,
    pub(crate) message: String,
    pub(crate) os: Option<OsJson>,
}

#[derive(Debug, Serialize)]
pub(crate) struct OsJson {
    pub(crate) code: String,
    pub(crate) api: &'static str,
}

/// `0x80070005` form for HRESULTs in JSON.
pub(crate) fn hresult(code: i32) -> String {
    format!("{:#010x}", u32::from_ne_bytes(code.to_ne_bytes()))
}

/// DISM `DismPackageFeatureState` → stable JSON name + Fluent id suffix.
pub(crate) fn state_name(state: i32) -> &'static str {
    match state {
        0 => "not-present",
        1 => "uninstall-pending",
        2 => "staged",
        3 => "removed",
        4 => "installed",
        5 => "install-pending",
        6 => "superseded",
        7 => "partially-installed",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hresult_text() {
        assert_eq!(hresult(i32::from_ne_bytes(0x8007_0005_u32.to_ne_bytes())), "0x80070005");
        assert_eq!(hresult(0), "0x00000000");
    }

    #[test]
    fn states() {
        assert_eq!(state_name(4), "installed");
        assert_eq!(state_name(42), "unknown");
    }
}
