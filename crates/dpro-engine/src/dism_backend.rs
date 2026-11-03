//! `job::Backend` over a DISM session (Windows only). Applies `FeatureState`,
//! `PackageRemove`, `DriverAdd`, `DriverRemove` and the health restore `Native` op; any other
//! kind is refused with `E_NOTIMPL` rather than ignored.
//!
//! Drivers (TD-0002 §5.6): a removal first copies the package's store folder into the job's
//! quarantine, and that copy is its inverse. An add learns its inverse after the call, because
//! DISM picks the published name (`oemN.inf`) then.

use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use dpro_core::{
    OsError,
    plan::{Change, ChangeKind},
};
use dpro_win::{
    dism::{Call, Completion, DriverPackageInfo, ProgressFn, Session},
    security,
};

use crate::job::{Applied, Backend};

/// `E_NOTIMPL`.
const E_NOTIMPL: i32 = i32::from_ne_bytes(0x8000_4001_u32.to_ne_bytes());
/// `E_INVALIDARG`.
const E_INVALIDARG: i32 = i32::from_ne_bytes(0x8007_0057_u32.to_ne_bytes());
/// `HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)`.
const E_FILE_NOT_FOUND: i32 = i32::from_ne_bytes(0x8007_0002_u32.to_ne_bytes());
/// `E_FAIL`.
const E_FAIL: i32 = i32::from_ne_bytes(0x8000_4005_u32.to_ne_bytes());

pub struct DismBackend<'s, 'api, 'p> {
    pub session: &'s mut Session<'api>,
    /// Extra payload sources for feature enables (`--source`).
    pub sources: Vec<PathBuf>,
    pub limit_access: bool,
    /// Progress of the DISM call in flight.
    pub progress: Option<ProgressFn<'p>>,
    /// This job's quarantine folder (`paths::quarantine_dir`); driver removals copy the
    /// package there first. `None` refuses them.
    pub quarantine: Option<PathBuf>,
    /// DISM's message for each failed change, in order, for the report.
    pub errors: Vec<String>,
    /// Things the user should know about changes that did apply.
    pub notes: Vec<String>,
    /// The third-party driver packages, while nothing has changed them since the listing.
    staged: Option<Vec<DriverPackageInfo>>,
    /// The inverse the last apply learned.
    learned: Option<ChangeKind<String>>,
}

impl<'s, 'api> DismBackend<'s, 'api, '_> {
    #[must_use]
    pub fn new(session: &'s mut Session<'api>) -> Self {
        Self {
            session,
            sources: Vec::new(),
            limit_access: false,
            progress: None,
            quarantine: None,
            errors: Vec::new(),
            notes: Vec::new(),
            staged: None,
            learned: None,
        }
    }
}

impl std::fmt::Debug for DismBackend<'_, '_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DismBackend")
            .field("session", &self.session)
            .field("sources", &self.sources)
            .field("limit_access", &self.limit_access)
            .field("quarantine", &self.quarantine)
            .field("errors", &self.errors)
            .field("notes", &self.notes)
            .finish_non_exhaustive()
    }
}

impl Backend for DismBackend<'_, '_, '_> {
    fn capture_inverse(
        &mut self,
        change: &Change<String>,
    ) -> Result<Option<ChangeKind<String>>, OsError> {
        // Feature changes carry their inverse in the plan (ReversibleVia); package removal and
        // repair are irreversible; a driver add learns its inverse when it runs.
        match &change.kind {
            ChangeKind::DriverRemove { published } => self.quarantine_driver(published).map(Some),
            _ => Ok(None),
        }
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
                let sources: Vec<&Path> = self.sources.iter().map(PathBuf::as_path).collect();
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
            ChangeKind::DriverAdd { inf } => return self.add_driver(inf),
            ChangeKind::DriverRemove { published } => {
                self.staged = None;
                self.session.remove_driver(OsStr::new(published))
            }
            ChangeKind::Native { api, args } if api == crate::health::RESTORE_API => {
                let Some(a) = crate::health::RestoreArgs::decode(args) else {
                    return Err(OsError {
                        code: E_INVALIDARG,
                        api: "DismBackend::apply",
                    });
                };
                let sources: Vec<&Path> = a.sources.iter().map(PathBuf::as_path).collect();
                self.session.restore_health(&sources, a.limit_access, call)
            }
            _ => {
                return Err(OsError {
                    code: E_NOTIMPL,
                    api: "DismBackend::apply",
                });
            }
        };
        self.settle(result)
    }

    fn learned_inverse(&mut self) -> Option<ChangeKind<String>> {
        self.learned.take()
    }
}

impl DismBackend<'_, '_, '_> {
    /// How a finished DISM call ends for the job.
    fn settle(&mut self, result: dpro_win::error::Result<Completion>) -> Result<Applied, OsError> {
        match result {
            Ok(Completion::Done) => Ok(Applied::Done),
            Ok(Completion::RebootRequired) => Ok(Applied::RebootRequired),
            Ok(Completion::ReloadSession) => {
                // The change is applied; the session must be reopened before the next call.
                // If that fails, later changes fail with E_HANDLE and are journaled as such.
                if let Err(e) = self.session.reload() {
                    self.errors.push(e.to_string());
                }
                self.staged = None;
                Ok(Applied::Done)
            }
            Err(e) => Err(self.fail(&e)),
        }
    }

    /// Records DISM's message and returns the code the journal keeps.
    fn fail(&mut self, e: &dpro_win::Error) -> OsError {
        self.errors.push(e.to_string());
        match e {
            dpro_win::Error::Dism { source, .. } | dpro_win::Error::Os(source) => *source,
            _ => OsError {
                code: E_INVALIDARG,
                api: "DismBackend::apply",
            },
        }
    }

    /// The third-party packages, listed once and reused until a change invalidates them.
    fn staged(&mut self) -> Result<&[DriverPackageInfo], OsError> {
        if self.staged.is_none() {
            match self.session.drivers(false) {
                Ok(list) => self.staged = Some(list),
                Err(e) => return Err(self.fail(&e)),
            }
        }
        Ok(self.staged.as_deref().unwrap_or_default())
    }

    /// `DriverAdd` (TD-0002 §5.6): `DismAddDriver` with `ForceUnsigned`, since the planner's
    /// signature gate is the consent. Then one listing finds the published name DISM picked:
    /// the package that wasn't there before, and the inverse removes it.
    fn add_driver(&mut self, inf: &str) -> Result<Applied, OsError> {
        let before: Vec<String> = self
            .staged()?
            .iter()
            .map(|p| p.published_name.to_string_lossy().to_lowercase())
            .collect();
        self.staged = None;
        let result = self.session.add_driver(Path::new(inf), true);
        let applied = self.settle(result)?;
        let after = match self.session.drivers(false) {
            Ok(after) => after,
            Err(e) => {
                self.notes.push(format!(
                    "{inf} was added, but listing the driver store failed ({e}): undo can't \
                     remove it"
                ));
                return Ok(applied);
            }
        };
        let name = file_name(inf);
        let fresh: Vec<&DriverPackageInfo> = after
            .iter()
            .filter(|p| !before.contains(&p.published_name.to_string_lossy().to_lowercase()))
            .collect();
        let found = match fresh.as_slice() {
            [one] => Some(*one),
            many => {
                let mut same_inf = many.iter().filter(|p| {
                    file_name(&p.original_file.to_string_lossy()).eq_ignore_ascii_case(name)
                });
                match (same_inf.next(), same_inf.next()) {
                    (Some(p), None) => Some(*p),
                    _ => None,
                }
            }
        };
        match found {
            Some(p) => {
                self.learned = Some(ChangeKind::DriverRemove {
                    published: p.published_name.to_string_lossy().into_owned(),
                });
            }
            // DISM accepted the INF without staging anything new: the package was already
            // there, so undo must leave it alone.
            None => self.notes.push(format!(
                "{inf}: no new package in the driver store, so undo leaves the store as it is"
            )),
        }
        self.staged = Some(after);
        Ok(applied)
    }

    /// Before a `DriverRemove`: copies the package's store folder (the parent of its store INF)
    /// to `<quarantine>\drivers\<published>\`. The copy re-adds as the same package, so
    /// `DriverAdd` of the copied INF is the inverse. A copy that fails (disk full) fails the
    /// change before it's applied.
    fn quarantine_driver(&mut self, published: &str) -> Result<ChangeKind<String>, OsError> {
        let Some(job_dir) = self.quarantine.clone() else {
            return Err(OsError {
                code: E_NOTIMPL,
                api: "DismBackend::quarantine_driver",
            });
        };
        let store_inf = self
            .staged()?
            .iter()
            .find(|p| p.published_name.eq_ignore_ascii_case(published))
            .map(|p| PathBuf::from(&p.original_file));
        let Some(store_inf) = store_inf else {
            self.errors
                .push(format!("{published} isn't in the target's driver store"));
            return Err(OsError {
                code: E_FILE_NOT_FOUND,
                api: "DismGetDrivers",
            });
        };
        let (Some(store), Some(inf_name)) = (store_inf.parent(), store_inf.file_name()) else {
            self.errors.push(format!(
                "{published}: unexpected store path {}",
                store_inf.display()
            ));
            return Err(OsError {
                code: E_INVALIDARG,
                api: "DismBackend::quarantine_driver",
            });
        };
        if let Some(root) = job_dir.parent()
            && let Err(e) = security::ensure_hidden_admin_only_dir(root)
        {
            return Err(self.fail(&e));
        }
        let dest = job_dir.join("drivers").join(published);
        if let Err(e) = copy_tree(store, &dest) {
            self.errors.push(format!(
                "copying {} to {}: {e}",
                store.display(),
                dest.display()
            ));
            return Err(io_os_error(&e, "CopyFileW"));
        }
        Ok(ChangeKind::DriverAdd {
            inf: dest.join(inf_name).to_string_lossy().into_owned(),
        })
    }
}

/// The last component of a Windows path.
fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Copies `from` into `to`, recursively, replacing what an earlier attempt left there. Links are
/// not followed: the driver store holds none.
fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    match fs::remove_dir_all(to) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn io_os_error(e: &io::Error, api: &'static str) -> OsError {
    e.raw_os_error()
        .and_then(|code| u32::try_from(code).ok())
        .map_or(OsError { code: E_FAIL, api }, |code| {
            OsError::from_win32(code, api)
        })
}
