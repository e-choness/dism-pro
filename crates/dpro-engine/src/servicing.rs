//! What a list read from DISM can be cached against (TD-0013 §7.1, roadmap 2.7a): the
//! servicing stamp, which moves when CBS changes a package's state and not when it's read, and a
//! cache that answers only while the stamp still matches and the entry is young.

use std::{
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use dpro_core::units::UnixMillis;

use crate::{paths, registry::Value};

/// CBS's key, relative to `HKLM\SOFTWARE` (or an offline image's `SOFTWARE` hive).
pub const CBS_KEY: &str = r"Microsoft\Windows\CurrentVersion\Component Based Servicing";

/// The CBS values that moved on both measured changes and on none of the reads or idle periods
/// (run 41698111296). `SessionIdLow` and the key's own last write move on every read.
pub const STAMP_VALUES: [&str; 2] = ["LastModified_UTC", "NextExecutionSequence"];

/// How long an entry answers even when nothing it's stamped with moved: the backstop for a
/// change the stamp misses (decisions-log #30a).
pub const TTL: Duration = Duration::from_mins(10);

/// Targets a cache keeps; the one filled longest ago goes first.
pub const MAX_ENTRIES: usize = 16;

/// A file or folder's last write and size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    modified: SystemTime,
    len: u64,
}

impl FileStamp {
    fn of(path: &Path) -> Option<Self> {
        let md = fs::metadata(path).ok()?;
        Some(Self {
            modified: md.modified().ok()?,
            len: md.len(),
        })
    }
}

/// The state of a target's servicing, as far as a cached list cares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    /// [`STAMP_VALUES`], when both were read with a known type.
    cbs: Option<[Value; 2]>,
    /// `servicing\Sessions\Sessions.xml`: CBS rewrites it for every session that changes
    /// something (run 41698111296), so it backs up the CBS pair.
    sessions: Option<FileStamp>,
    /// The target's journal folder: every DISM Pro job, from any process, adds a file to it,
    /// which moves the folder's last write. That drops the entry after each job.
    journal: Option<FileStamp>,
}

impl Stamp {
    /// `cbs` holds the [`STAMP_VALUES`] of the target's CBS key, in order, as read; the files
    /// are looked up under the target's Windows folder and volume.
    #[must_use]
    pub fn read(cbs: [Option<Value>; 2], windows_dir: &Path, volume_root: &Path) -> Self {
        let known = |v: &Option<Value>| v.as_ref().is_some_and(|v| *v != Value::Other);
        let cbs = if cbs.iter().all(known) {
            let [a, b] = cbs;
            a.zip(b).map(|(a, b)| [a, b])
        } else {
            None
        };
        let sessions = windows_dir
            .join("servicing")
            .join("Sessions")
            .join("Sessions.xml");
        Self {
            cbs,
            sessions: FileStamp::of(&sessions),
            journal: FileStamp::of(&paths::journal_dir(volume_root)),
        }
    }

    /// Whether the stamp can see a change at all. Without the CBS pair and without
    /// `Sessions.xml` it can't, and nothing is cached against it.
    #[must_use]
    pub fn is_usable(&self) -> bool {
        self.cbs.is_some() || self.sessions.is_some()
    }
}

#[derive(Debug)]
struct Entry<V> {
    key: String,
    stamp: Stamp,
    filled: Instant,
    read_at: UnixMillis,
    value: Arc<V>,
}

/// Lists keyed by target, each answered while its [`Stamp`] matches and for at most [`TTL`].
/// Small and owned by the one thread that talks to DISM, so it takes no locks.
#[derive(Debug)]
pub struct StampedCache<V> {
    entries: Vec<Entry<V>>,
}

impl<V> Default for StampedCache<V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<V> StampedCache<V> {
    /// The value cached for `key` and when it was read, if `stamp` is usable and equal to the
    /// one it was stored with, and it was filled less than [`TTL`] before `now`.
    #[must_use]
    pub fn get(&self, key: &str, stamp: &Stamp, now: Instant) -> Option<(Arc<V>, UnixMillis)> {
        if !stamp.is_usable() {
            return None;
        }
        self.entries
            .iter()
            .find(|e| e.key == key)
            .filter(|e| e.stamp == *stamp && now.saturating_duration_since(e.filled) < TTL)
            .map(|e| (Arc::clone(&e.value), e.read_at))
    }

    /// Stores `value` for `key`, read at `read_at` under `stamp`. Read the stamp *before* the
    /// value: a change that lands during the read then leaves a stamp that no longer matches.
    /// A stamp that can't see changes stores nothing. Returns the value either way.
    pub fn put(
        &mut self,
        key: &str,
        stamp: Stamp,
        filled: Instant,
        read_at: UnixMillis,
        value: V,
    ) -> Arc<V> {
        self.forget(key);
        let value = Arc::new(value);
        if !stamp.is_usable() {
            return value;
        }
        if self.entries.len() >= MAX_ENTRIES
            && let Some(oldest) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.filled)
                .map(|(i, _)| i)
        {
            self.entries.swap_remove(oldest);
        }
        self.entries.push(Entry {
            key: key.to_owned(),
            stamp,
            filled,
            read_at,
            value: Arc::clone(&value),
        });
        value
    }

    /// Drops what's cached for `key`.
    pub fn forget(&mut self, key: &str) {
        self.entries.retain(|e| e.key != key);
    }

    /// Entries held, for tests and diagnostics.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// A scratch target: `<root>\Windows` and the volume `<root>`.
    fn target(name: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("dpro-stamp-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root); // ignore: a leftover from an earlier run, or nothing
        let windows = root.join("Windows");
        fs::create_dir_all(windows.join("servicing").join("Sessions")).unwrap();
        (windows, root)
    }

    fn sessions_xml(windows: &Path, text: &str) {
        let path = windows
            .join("servicing")
            .join("Sessions")
            .join("Sessions.xml");
        fs::write(path, text).unwrap();
    }

    const CBS: [Option<Value>; 2] = [Some(Value::Qword(133)), Some(Value::Dword(7))];

    #[test]
    fn a_stamp_needs_the_cbs_pair_or_sessions_xml() {
        let (windows, volume) = target("usable");
        assert!(Stamp::read(CBS, &windows, &volume).is_usable());
        let binary = [Some(Value::Binary(vec![1, 2])), Some(Value::Dword(7))];
        assert!(Stamp::read(binary, &windows, &volume).is_usable());
        let half = [Some(Value::Qword(133)), None];
        assert!(!Stamp::read(half, &windows, &volume).is_usable());
        let unknown = [Some(Value::Other), Some(Value::Dword(7))];
        assert!(!Stamp::read(unknown.clone(), &windows, &volume).is_usable());
        sessions_xml(&windows, "<Sessions/>");
        assert!(Stamp::read(unknown, &windows, &volume).is_usable());
        fs::remove_dir_all(&volume).unwrap();
    }

    #[test]
    fn a_change_anywhere_in_the_stamp_is_a_new_stamp() {
        let (windows, volume) = target("moves");
        sessions_xml(&windows, "<Sessions/>");
        let first = Stamp::read(CBS, &windows, &volume);
        assert_eq!(first, Stamp::read(CBS, &windows, &volume));
        let next = [Some(Value::Qword(134)), Some(Value::Dword(7))];
        assert_ne!(first, Stamp::read(next, &windows, &volume));
        // A DISM Pro job creates the journal folder, or a file in it.
        fs::create_dir_all(paths::journal_dir(&volume)).unwrap();
        let journaled = Stamp::read(CBS, &windows, &volume);
        assert_ne!(first, journaled);
        // CBS rewrote Sessions.xml.
        sessions_xml(&windows, "<Sessions><Session/></Sessions>");
        assert_ne!(journaled, Stamp::read(CBS, &windows, &volume));
        fs::remove_dir_all(&volume).unwrap();
    }

    #[test]
    fn the_cache_answers_while_the_stamp_matches_and_the_entry_is_young() {
        let (windows, volume) = target("cache");
        let stamp = Stamp::read(CBS, &windows, &volume);
        let moved = Stamp::read(
            [Some(Value::Qword(1)), Some(Value::Dword(1))],
            &windows,
            &volume,
        );
        let t0 = Instant::now();
        let mut cache = StampedCache::default();
        assert!(cache.get("online", &stamp, t0).is_none());
        let put = cache.put("online", stamp.clone(), t0, UnixMillis(42), vec![1, 2, 3]);
        assert_eq!(*put, [1, 2, 3]);

        let (hit, read_at) = cache
            .get("online", &stamp, t0 + Duration::from_secs(1))
            .unwrap();
        assert_eq!(*hit, [1, 2, 3]);
        assert_eq!(read_at, UnixMillis(42));
        assert!(cache.get("online", &moved, t0).is_none(), "the stamp moved");
        assert!(
            cache.get("d:/image/windows", &stamp, t0).is_none(),
            "another target"
        );
        assert!(cache.get("online", &stamp, t0 + TTL).is_none(), "too old");

        cache.forget("online");
        assert!(cache.is_empty());
        fs::remove_dir_all(&volume).unwrap();
    }

    #[test]
    fn a_blind_stamp_stores_nothing_and_the_oldest_entry_goes_first() {
        let (windows, volume) = target("bounded");
        let blind = Stamp::read([None, None], &windows, &volume);
        let t0 = Instant::now();
        let mut cache = StampedCache::default();
        assert_eq!(*cache.put("online", blind.clone(), t0, UnixMillis(1), 5), 5);
        assert!(cache.is_empty());
        assert!(cache.get("online", &blind, t0).is_none());

        let stamp = Stamp::read(CBS, &windows, &volume);
        for i in 0..=MAX_ENTRIES {
            let at = t0 + Duration::from_millis(u64::try_from(i).unwrap());
            cache.put(&format!("t{i}"), stamp.clone(), at, UnixMillis(1), i);
        }
        assert_eq!(cache.len(), MAX_ENTRIES);
        assert!(cache.get("t0", &stamp, t0).is_none(), "the oldest went");
        assert!(cache.get(&format!("t{MAX_ENTRIES}"), &stamp, t0).is_some());
        // Refilling a target replaces its entry.
        cache.put("t1", stamp.clone(), t0, UnixMillis(2), 99);
        assert_eq!(cache.len(), MAX_ENTRIES);
        assert_eq!(*cache.get("t1", &stamp, t0).unwrap().0, 99);
        fs::remove_dir_all(&volume).unwrap();
    }
}
