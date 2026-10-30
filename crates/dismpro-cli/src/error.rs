//! CLI errors and their exit codes / diagnosis ids (TD-0002 §5.5, §9).

use dpro_cli_fmt::exit::ExitCode;
use dpro_core::OsError;

#[derive(Debug)]
pub(crate) enum CliError {
    Usage(String),
    NeedsElevation,
    #[cfg_attr(
        windows,
        expect(dead_code, reason = "only non-Windows builds construct it")
    )]
    Unsupported,
    UnknownName(String),
    Os(OsError, String),
    /// Ctrl+C during a read-only DISM call.
    Cancelled,
    /// Anything else, already worded (journal, plan, I/O with its path).
    Failed(String),
    Io(std::io::Error),
}

impl CliError {
    pub(crate) fn exit_code(&self) -> ExitCode {
        match self {
            Self::Usage(_) => ExitCode::Usage,
            Self::NeedsElevation => ExitCode::NeedsElevation,
            Self::Cancelled => ExitCode::Cancelled,
            Self::Unsupported
            | Self::UnknownName(_)
            | Self::Os(..)
            | Self::Failed(_)
            | Self::Io(_) => ExitCode::Failed,
        }
    }

    /// Fluent id of the diagnosis (also the JSON `diagnosis` field).
    pub(crate) fn diagnosis(&self) -> &'static str {
        match self {
            Self::Usage(_) => "cli-error-usage",
            Self::NeedsElevation => "cli-error-elevation",
            Self::Unsupported => "cli-error-unsupported",
            Self::Cancelled => "cli-error-cancelled",
            Self::UnknownName(_) | Self::Os(..) | Self::Failed(_) | Self::Io(_) => {
                "cli-error-failed"
            }
        }
    }

    pub(crate) fn os(&self) -> Option<OsError> {
        match self {
            Self::Os(e, _) => Some(*e),
            _ => None,
        }
    }

    /// The `{ $message }` / `{ $what }` argument for the diagnosis.
    pub(crate) fn detail(&self) -> String {
        match self {
            Self::Usage(m) | Self::UnknownName(m) | Self::Os(_, m) | Self::Failed(m) => m.clone(),
            Self::Io(e) => e.to_string(),
            Self::NeedsElevation | Self::Unsupported | Self::Cancelled => String::new(),
        }
    }

    /// An I/O error that names the file it happened on.
    pub(crate) fn io_at(path: &std::path::Path, e: &std::io::Error) -> Self {
        Self::Failed(format!("{}: {e}", path.display()))
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<dpro_engine::journal::JournalError> for CliError {
    fn from(e: dpro_engine::journal::JournalError) -> Self {
        Self::Failed(e.to_string())
    }
}

impl From<dpro_engine::ops::PlanError> for CliError {
    fn from(e: dpro_engine::ops::PlanError) -> Self {
        match e {
            dpro_engine::ops::PlanError::Unknown { what, name } => {
                Self::UnknownName(format!("unknown {what}: {name}"))
            }
            dpro_engine::ops::PlanError::Core(e) => Self::Failed(e.to_string()),
        }
    }
}

impl From<dpro_core::Error> for CliError {
    fn from(e: dpro_core::Error) -> Self {
        Self::Failed(e.to_string())
    }
}

#[cfg(windows)]
impl From<dpro_win::Error> for CliError {
    fn from(e: dpro_win::Error) -> Self {
        match e {
            dpro_win::Error::Dism { source, .. }
                if source.code == dpro_win::dism::HRESULT_ERROR_CANCELLED =>
            {
                Self::Cancelled
            }
            // DISM has no text for some failures; fall back to the API and code.
            dpro_win::Error::Dism { source, message } if message.is_empty() => {
                Self::Os(source, source.to_string())
            }
            dpro_win::Error::Dism { source, message } => Self::Os(source, message),
            dpro_win::Error::Os(source) => Self::Os(source, source.to_string()),
            other => Self::Failed(other.to_string()),
        }
    }
}
