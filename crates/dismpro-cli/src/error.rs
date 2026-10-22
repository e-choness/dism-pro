//! CLI errors and their exit codes / diagnosis ids (TD-0002 §5.5, §9).

use dpro_cli_fmt::exit::ExitCode;
use dpro_core::OsError;

#[derive(Debug)]
pub(crate) enum CliError {
    Usage(String),
    NeedsElevation,
    Unsupported,
    NotImplemented(&'static str),
    UnknownName(String),
    Os(OsError, String),
    Journal(String),
    Io(std::io::Error),
}

impl CliError {
    pub(crate) fn exit_code(&self) -> ExitCode {
        match self {
            Self::Usage(_) => ExitCode::Usage,
            Self::NeedsElevation => ExitCode::NeedsElevation,
            Self::Unsupported | Self::NotImplemented(_) | Self::UnknownName(_) | Self::Os(..) | Self::Journal(_) | Self::Io(_) => {
                ExitCode::Failed
            }
        }
    }

    /// Fluent id of the diagnosis (also the JSON `diagnosis` field).
    pub(crate) fn diagnosis(&self) -> &'static str {
        match self {
            Self::Usage(_) => "cli-error-usage",
            Self::NeedsElevation => "cli-error-elevation",
            Self::Unsupported => "cli-error-unsupported",
            Self::NotImplemented(_) => "cli-error-not-implemented",
            Self::UnknownName(_) | Self::Os(..) | Self::Journal(_) | Self::Io(_) => "cli-error",
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
            Self::Usage(m) | Self::UnknownName(m) | Self::Os(_, m) | Self::Journal(m) => m.clone(),
            Self::NotImplemented(what) => (*what).to_owned(),
            Self::Io(e) => e.to_string(),
            Self::NeedsElevation | Self::Unsupported => String::new(),
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<dpro_engine::journal::JournalError> for CliError {
    fn from(e: dpro_engine::journal::JournalError) -> Self {
        Self::Journal(e.to_string())
    }
}

impl From<dpro_engine::ops::PlanError> for CliError {
    fn from(e: dpro_engine::ops::PlanError) -> Self {
        match e {
            dpro_engine::ops::PlanError::Unknown { what, name } => Self::UnknownName(format!("unknown {what}: {name}")),
            dpro_engine::ops::PlanError::Core(e) => Self::Journal(e.to_string()),
        }
    }
}

impl From<dpro_core::Error> for CliError {
    fn from(e: dpro_core::Error) -> Self {
        Self::Journal(e.to_string())
    }
}

#[cfg(windows)]
impl From<dpro_win::Error> for CliError {
    fn from(e: dpro_win::Error) -> Self {
        match e {
            dpro_win::Error::Dism { source, message } => Self::Os(source, message),
            dpro_win::Error::Os(source) => Self::Os(source, source.to_string()),
            other => Self::Journal(other.to_string()),
        }
    }
}
