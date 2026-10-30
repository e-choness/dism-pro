//! Where DISM Pro keeps state for a target (TD-0001 §5.4, Vision §2.4), and job ids.

use std::{
    collections::hash_map::RandomState,
    hash::{BuildHasher, Hasher},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use dpro_core::{id::JobId, units::UnixMillis};

/// `<root>\ProgramData\DismPro`: the online OS uses its system drive root, an offline install
/// its volume root, so undo information travels with the OS it belongs to.
#[must_use]
pub fn state_dir(volume_root: &Path) -> PathBuf {
    volume_root.join("ProgramData").join("DismPro")
}

#[must_use]
pub fn journal_dir(volume_root: &Path) -> PathBuf {
    state_dir(volume_root).join("journal")
}

#[must_use]
pub fn journal_file(volume_root: &Path, job: JobId) -> PathBuf {
    journal_dir(volume_root).join(format!("{job}.dpj"))
}

/// Milliseconds since the Unix epoch (0 if the clock is before it).
#[must_use]
pub fn now() -> UnixMillis {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    UnixMillis(u64::try_from(ms).unwrap_or(u64::MAX))
}

/// A new job id. Randomness comes from std's per-process random hasher keys: enough for
/// unique, unguessable-enough ids; not a security token.
#[must_use]
pub fn new_job_id() -> JobId {
    let time = now();
    let mut a = RandomState::new().build_hasher();
    a.write_u64(time.0);
    let mut b = RandomState::new().build_hasher();
    b.write_u64(!time.0);
    JobId::from_parts(
        time,
        (u128::from(a.finish()) << 64) | u128::from(b.finish()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_paths_are_under_program_data() {
        let job = JobId::from_parts(UnixMillis(0), 0);
        let p = journal_file(Path::new("D:"), job);
        assert!(
            p.ends_with(
                Path::new("ProgramData")
                    .join("DismPro")
                    .join("journal")
                    .join(format!("{job}.dpj"))
            )
        );
    }

    #[test]
    fn job_ids_differ() {
        assert_ne!(new_job_id(), new_job_id());
    }
}
