//! `job::Backend` over a DISM session (Windows only). Applies `FeatureState`,
//! `PackageRemove` and the health restore `Native` op; any other kind is refused with
//! `E_NOTIMPL` rather than ignored.

use std::{ffi::OsStr, path::Path};

use dpro_core::{
    OsError,
    plan::{Change, ChangeKind},
};
use dpro_win::dism::{Call, Completion, ProgressFn, Session};

use crate::job::{Applied, Backend};

/// `E_NOTIMPL`.
const E_NOTIMPL: i32 = i32::from_ne_bytes(0x8000_4001_u32.to_ne_bytes());
/// `E_INVALIDARG`.
const E_INVALIDARG: i32 = i32::from_ne_bytes(0x8007_0057_u32.to_ne_bytes());

pub struct DismBackend<'s, 'api, 'p> {
    pub session: &'s mut Session<'api>,
    /// Extra payload sources for feature enables (`--source`).
    pub sources: Vec<std::path::PathBuf>,
    pub limit_access: bool,
    /// Progress of the DISM call in flight.
    pub progress: Option<ProgressFn<'p>>,
    /// DISM's message for each failed change, in order, for the report.
    pub errors: Vec<String>,
}

impl std::fmt::Debug for DismBackend<'_, '_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DismBackend")
            .field("session", &self.session)
            .field("sources", &self.sources)
            .field("limit_access", &self.limit_access)
            .field("errors", &self.errors)
            .finish_non_exhaustive()
    }
}

impl Backend for DismBackend<'_, '_, '_> {
    fn capture_inverse(
        &mut self,
        _change: &Change<String>,
    ) -> Result<Option<ChangeKind<String>>, OsError> {
        // Feature changes carry their inverse in the plan (ReversibleVia); package removal and
        // repair are irreversible. Nothing to read from the target.
        Ok(None)
    }

    fn apply(&mut self, change: &Change<String>) -> Result<Applied, OsError> {
        // Servicing calls get no cancel event: DISM leaves a cancelled image in an unknown
        // state. Cancellation takes effect between changes (job::run).
        let call = Call {
            cancel: None,
            progress: self.progress,
        };
        let result = match &change.kind {
            ChangeKind::FeatureState {
                name, enable: true, ..
            } => {
                let sources: Vec<&Path> = self
                    .sources
                    .iter()
                    .map(std::path::PathBuf::as_path)
                    .collect();
                self.session
                    .enable_feature(OsStr::new(name), &sources, self.limit_access, call)
            }
            ChangeKind::FeatureState {
                name,
                enable: false,
                ..
            } => self.session.disable_feature(OsStr::new(name), call),
            ChangeKind::PackageRemove { identity } => {
                self.session.remove_package(OsStr::new(identity), call)
            }
            ChangeKind::Native { api, args } if api == crate::health::RESTORE_API => {
                let Some(a) = crate::health::RestoreArgs::decode(args) else {
                    return Err(OsError {
                        code: E_INVALIDARG,
                        api: "DismBackend::apply",
                    });
                };
                let sources: Vec<&Path> =
                    a.sources.iter().map(std::path::PathBuf::as_path).collect();
                self.session.restore_health(&sources, a.limit_access, call)
            }
            _ => {
                return Err(OsError {
                    code: E_NOTIMPL,
                    api: "DismBackend::apply",
                });
            }
        };
        match result {
            Ok(Completion::Done) => Ok(Applied::Done),
            Ok(Completion::RebootRequired) => Ok(Applied::RebootRequired),
            Ok(Completion::ReloadSession) => {
                // The change is applied; the session must be reopened before the next call.
                // If that fails, later changes fail with E_HANDLE and are journaled as such.
                if let Err(e) = self.session.reload() {
                    self.errors.push(e.to_string());
                }
                Ok(Applied::Done)
            }
            Err(e) => {
                self.errors.push(e.to_string());
                Err(match e {
                    dpro_win::Error::Dism { source, .. } | dpro_win::Error::Os(source) => source,
                    _ => OsError {
                        code: E_INVALIDARG,
                        api: "DismBackend::apply",
                    },
                })
            }
        }
    }
}
