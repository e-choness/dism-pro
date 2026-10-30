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
    /// The plan's operation id (`features.enable`, `journal.undo`, …).
    pub(crate) op: String,
    /// RFC 3339 UTC.
    pub(crate) created: String,
    /// For an undo job: the job it undid.
    pub(crate) undoes: Option<String>,
    /// `completed`, `cancelled`, `aborted`, or `interrupted` (no commit record).
    pub(crate) status: &'static str,
    pub(crate) applied: Vec<u32>,
    pub(crate) failed: Vec<FailedJson>,
    pub(crate) without_outcome: Vec<u32>,
    /// Windows reported that an applied change finishes at the next restart.
    pub(crate) reboot_required: bool,
    pub(crate) journal: String,
}

/// `journal list` (`journals.v1`), oldest job first.
#[derive(Debug, Serialize)]
pub(crate) struct JournalsDoc {
    pub(crate) schema: &'static str,
    pub(crate) jobs: Vec<JournalJson>,
}

#[derive(Debug, Serialize)]
pub(crate) struct JournalJson {
    pub(crate) job: String,
    pub(crate) op: String,
    pub(crate) created: String,
    pub(crate) status: &'static str,
    /// Number of applied changes.
    pub(crate) applied: usize,
    /// Number of failed changes.
    pub(crate) failed: usize,
    pub(crate) reboot_required: bool,
    pub(crate) undoes: Option<String>,
    /// The job that undid this one, if any.
    pub(crate) undone_by: Option<String>,
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

#[derive(Debug, Serialize)]
pub(crate) struct HealthDoc {
    pub(crate) schema: &'static str,
    /// `healthy`, `repairable`, `non-repairable`.
    pub(crate) state: dpro_core::health::ImageHealth,
    pub(crate) scanned: bool,
    pub(crate) findings: Vec<FindingJson>,
}

#[derive(Debug, Serialize)]
pub(crate) struct FindingJson {
    pub(crate) severity: dpro_core::health::Severity,
    pub(crate) diagnosis: &'static str,
    pub(crate) message: String,
    pub(crate) code: Option<String>,
    pub(crate) log: dpro_core::health::LogFile,
    /// Relative to where the log was when the check started.
    pub(crate) first_line: u64,
    pub(crate) last_line: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct RebootDoc {
    pub(crate) schema: &'static str,
    pub(crate) pending: bool,
    pub(crate) reasons: Vec<dpro_core::health::RebootReason>,
    pub(crate) messages: Vec<String>,
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
        assert_eq!(
            hresult(i32::from_ne_bytes(0x8007_0005_u32.to_ne_bytes())),
            "0x80070005"
        );
        assert_eq!(hresult(0), "0x00000000");
    }

    #[test]
    fn states() {
        assert_eq!(state_name(4), "installed");
        assert_eq!(state_name(42), "unknown");
    }
}
