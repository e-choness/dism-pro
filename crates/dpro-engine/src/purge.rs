//! `purge` (roadmap 2.8, Vision principle 4): plan the removal of every file DISM Pro created
//! on a volume, and the filesystem backend that applies `FileDelete` changes.
//!
//! The purge job's own journal is kept in memory: writing it to disk would recreate the very
//! state being removed (deviation from TD-0001, noted in CAVEATS.md).

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use dpro_core::{
    OsError,
    id::TargetId,
    plan::{Change, ChangeKind, OpDescriptor, OsBuild, Plan, PlanBuilder, Reversibility},
    units::Bytes,
};

use crate::{job::Backend, paths};

#[derive(Debug, thiserror::Error)]
pub enum PurgeError {
    #[error("reading {}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error(transparent)]
    Core(#[from] dpro_core::Error),
}

/// Folders DISM Pro owns on a volume.
#[must_use]
pub fn owned_dirs(volume_root: &Path) -> [PathBuf; 2] {
    [paths::state_dir(volume_root), volume_root.join("$DismProQuarantine")]
}

/// One irreversible `FileDelete` per file under [`owned_dirs`]. Links are listed as files
/// (deleting a link never touches its target) and never followed.
///
/// # Errors
/// A folder can't be read.
pub fn plan_purge(target: TargetId, os_build: OsBuild, volume_root: &Path) -> Result<Plan, PurgeError> {
    let op = OpDescriptor { id: "purge".into(), title_key: "op-purge".into(), category: "app".into(), cancellable: true };
    let mut b = PlanBuilder::new(target, os_build, op);
    let reason = b.atom("rev-purge")?;
    let mut files = Vec::new();
    for dir in owned_dirs(volume_root) {
        walk(&dir, &mut files)?;
    }
    files.sort();
    for (path, size) in files {
        let atom = b.atom(&path.to_string_lossy())?;
        b.push(Change {
            kind: ChangeKind::FileDelete { path: atom, bytes: Bytes(size), unique_bytes: Bytes(size) },
            reversibility: Reversibility::Irreversible { reason },
            reboot_required: false,
        })?;
    }
    Ok(b.finish())
}

fn walk(dir: &Path, out: &mut Vec<(PathBuf, u64)>) -> Result<(), PurgeError> {
    let io_err = |source| PurgeError::Io { path: dir.to_path_buf(), source };
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_err(e)),
    };
    for entry in entries {
        let entry = entry.map_err(io_err)?;
        let meta = fs::symlink_metadata(entry.path()).map_err(io_err)?;
        if meta.is_dir() {
            walk(&entry.path(), out)?;
        } else {
            out.push((entry.path(), meta.len()));
        }
    }
    Ok(())
}

/// Removes now-empty folders under `root` (deepest first), then `root` itself if empty.
/// Best effort: a folder that still has content is left alone.
pub fn remove_empty_dirs(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let is_real_dir = fs::symlink_metadata(entry.path()).is_ok_and(|m| m.is_dir());
        if is_real_dir {
            remove_empty_dirs(&entry.path());
        }
    }
    let _ = fs::remove_dir(root); // ignore: fails (correctly) when not empty
}

/// Applies `FileDelete` with `std::fs`. Other kinds are refused.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsBackend;

impl Backend for FsBackend {
    fn capture_inverse(&mut self, _change: &Change<String>) -> Result<Option<ChangeKind<String>>, OsError> {
        Ok(None) // FileDelete is irreversible by definition
    }

    fn apply(&mut self, change: &Change<String>) -> Result<(), OsError> {
        let ChangeKind::FileDelete { path, .. } = &change.kind else {
            return Err(OsError { code: i32::from_ne_bytes(0x8000_4001_u32.to_ne_bytes()), api: "FsBackend::apply" });
        };
        fs::remove_file(path).map_err(|e| {
            let code = e.raw_os_error().map_or(1, |c| u32::try_from(c).unwrap_or(1));
            OsError::from_win32(code, "DeleteFileW")
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dpro_core::{cancel::CancelToken, units::UnixMillis};

    use super::*;
    use crate::{
        job::{self, RunConfig},
        journal::{Begin, JournalWriter},
    };

    struct Zero;
    impl crate::job::Clock for Zero {
        fn now(&self) -> Duration {
            Duration::ZERO
        }
    }

    fn scratch() -> PathBuf {
        let d = std::env::temp_dir().join(format!("dpro-purge-{}-{}", std::process::id(), paths::now().0));
        let _ = fs::remove_dir_all(&d); // ignore: may not exist
        d
    }

    #[test]
    fn plans_and_purges_everything_it_owns() {
        let root = scratch();
        let journal = paths::journal_dir(&root);
        fs::create_dir_all(&journal).unwrap();
        fs::write(journal.join("a.dpj"), b"12345").unwrap();
        fs::create_dir_all(root.join("$DismProQuarantine").join("job")).unwrap();
        fs::write(root.join("$DismProQuarantine").join("job").join("f"), b"x").unwrap();
        fs::write(root.join("keep.txt"), b"not ours").unwrap();

        let build = OsBuild { major: 10, minor: 0, build: 1, revision: 0 };
        let plan = plan_purge(TargetId([0; 16]), build, &root).unwrap();
        assert_eq!(plan.summary().changes, 2);
        assert_eq!(plan.summary().bytes, Bytes(6));
        assert_eq!(plan.summary().irreversible, 2);

        let doc = plan.to_doc().unwrap();
        let begin = Begin { job: paths::new_job_id(), target: TargetId([0; 16]), os_build: build, created: UnixMillis(0) };
        let writer = JournalWriter::create(Vec::new(), &begin).unwrap();
        let (summary, _) = job::run(&doc.changes, &mut FsBackend, writer, &CancelToken::new(), &Zero, RunConfig::default()).unwrap();
        assert_eq!(summary.applied, 2);
        for dir in owned_dirs(&root) {
            remove_empty_dirs(&dir);
            assert!(!dir.exists(), "{}", dir.display());
        }
        assert!(root.join("keep.txt").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn nothing_to_purge_is_an_empty_plan() {
        let root = scratch();
        let plan = plan_purge(TargetId([0; 16]), OsBuild { major: 10, minor: 0, build: 1, revision: 0 }, &root).unwrap();
        assert_eq!(plan.summary().changes, 0);
    }
}
