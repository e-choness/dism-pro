//! Health & Repair results (TD-0006 §5.1). Pure data; the engine produces it.

use serde::{Deserialize, Serialize};

use crate::OsError;

/// `DismImageHealthState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImageHealth {
    Healthy,
    Repairable,
    NonRepairable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogFile {
    Cbs,
    Dism,
}

/// Where a finding came from, for "Show log lines". Lines are 1-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogRef {
    pub file: LogFile,
    pub first_line: u64,
    pub last_line: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    /// Fluent id of the explanation, e.g. `health-finding-source-missing`.
    pub diagnosis: &'static str,
    pub code: Option<OsError>,
    pub source: LogRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    pub state: ImageHealth,
    /// `true` for a full scan (`ScanHealth`), `false` for the quick check.
    pub scanned: bool,
    pub findings: Vec<Finding>,
}

/// Why a reboot is pending (TD-0006 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "reason")]
pub enum RebootReason {
    Cbs,
    CbsPackagesPending,
    PendingXml,
    WindowsUpdate,
    FileRename { count: u32 },
    ComputerRename,
    UpdateExeVolatile,
}

impl RebootReason {
    /// Fluent id of the explanation.
    #[must_use]
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Cbs => "reboot-reason-cbs",
            Self::CbsPackagesPending => "reboot-reason-cbs-packages",
            Self::PendingXml => "reboot-reason-pending-xml",
            Self::WindowsUpdate => "reboot-reason-wu",
            Self::FileRename { .. } => "reboot-reason-file-rename",
            Self::ComputerRename => "reboot-reason-computer-rename",
            Self::UpdateExeVolatile => "reboot-reason-update-exe",
        }
    }
}
