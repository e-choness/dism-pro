//! `job::Backend` over a DISM session (Windows only). Applies `FeatureState` and
//! `PackageRemove`; any other kind is refused with `E_NOTIMPL` rather than ignored.

use std::{ffi::OsStr, path::Path};

use dpro_core::{
    OsError,
    plan::{Change, ChangeKind},
};
use dpro_win::dism::Session;

use crate::job::Backend;

/// `E_NOTIMPL`.
const E_NOTIMPL: i32 = i32::from_ne_bytes(0x8000_4001_u32.to_ne_bytes());

#[derive(Debug)]
pub struct DismBackend<'s, 'api> {
    pub session: &'s Session<'api>,
    /// Extra payload sources for feature enables (`--source`).
    pub sources: Vec<std::path::PathBuf>,
    pub limit_access: bool,
}

impl Backend for DismBackend<'_, '_> {
    fn capture_inverse(&mut self, _change: &Change<String>) -> Result<Option<ChangeKind<String>>, OsError> {
        // Feature changes carry their inverse in the plan (ReversibleVia); package removal is
        // irreversible. Nothing to read from the target.
        Ok(None)
    }

    fn apply(&mut self, change: &Change<String>) -> Result<(), OsError> {
        let result = match &change.kind {
            ChangeKind::FeatureState { name, enable: true, .. } => {
                let sources: Vec<&Path> = self.sources.iter().map(std::path::PathBuf::as_path).collect();
                self.session.enable_feature(OsStr::new(name), &sources, self.limit_access)
            }
            ChangeKind::FeatureState { name, enable: false, .. } => self.session.disable_feature(OsStr::new(name)),
            ChangeKind::PackageRemove { identity } => self.session.remove_package(OsStr::new(identity)),
            _ => return Err(OsError { code: E_NOTIMPL, api: "DismBackend::apply" }),
        };
        result.map_err(|e| match e {
            dpro_win::Error::Dism { source, .. } | dpro_win::Error::Os(source) => source,
            _ => OsError { code: E_NOTIMPL, api: "DismBackend::apply" },
        })
    }
}
