//! What every method reports, and the subtree count for the whole-volume methods (MFT read, USN
//! enumeration), which see every file record on the volume and must filter to the scanned folder.

/// Totals for one scan. Every method must agree on `files`, `dirs` and `name_chars`; the ones
/// that see sizes must agree on `bytes`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tally {
    /// Directory entries that are files (a hard-linked file counts once per name).
    pub files: u64,
    /// Directories below the root (the root itself isn't counted).
    pub dirs: u64,
    /// UTF-16 units over all entry names: proves every method saw the same names.
    pub name_chars: u64,
    /// Sum of file sizes per name, what a `FindFirstFileEx` walk adds up.
    pub bytes: Option<u64>,
    /// Sum of file sizes with each hard-linked file counted once.
    pub unique_bytes: Option<u64>,
    /// Directories that couldn't be listed (access denied); their contents are missing.
    pub denied: u64,
}

impl Tally {
    pub fn add(&mut self, other: &Tally) {
        self.files += other.files;
        self.dirs += other.dirs;
        self.name_chars += other.name_chars;
        self.bytes = sum(self.bytes, other.bytes);
        self.unique_bytes = sum(self.unique_bytes, other.unique_bytes);
        self.denied += other.denied;
    }
}

fn sum(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a + b),
        (a, None) => a,
        (None, b) => b,
    }
}

/// Low 48 bits of an NTFS file reference: the MFT record number (the high 16 are a sequence).
pub const RECORD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
/// Records 0..16 are NTFS metadata files (`$MFT`, `$LogFile`, …, `$Extend`); 5 is the volume root.
const FIRST_USER_RECORD: u64 = 16;
#[cfg(test)]
pub const VOLUME_ROOT: u64 = 5;

#[derive(Debug, Default, Clone, Copy)]
pub struct Record {
    pub in_use: bool,
    pub is_dir: bool,
    /// Unnamed `$DATA` length; alternate data streams don't count, as in `FindFirstFileEx`.
    pub size: u64,
}

/// One directory entry: `record` is listed in directory `parent` under a name of `len` units.
#[derive(Debug, Clone, Copy)]
pub struct Name {
    pub record: u64,
    pub parent: u64,
    #[cfg_attr(
        all(windows, not(test)),
        expect(dead_code, reason = "kept as an index keeps it; tests read it")
    )]
    pub start: usize,
    pub len: u16,
}

/// Every file record and name on a volume, as an MFT read or a USN enumeration finds them.
#[derive(Debug, Default)]
pub struct Volume {
    pub records: Vec<Record>,
    pub names: Vec<Name>,
    /// Name text, kept the way an index would keep it so the cost is part of the measurement.
    pub arena: Vec<u16>,
}

const UNKNOWN: u8 = 0;
const INSIDE: u8 = 1;
const OUTSIDE: u8 = 2;

impl Volume {
    pub fn record_mut(&mut self, number: u64) -> &mut Record {
        let i = usize::try_from(number).unwrap_or(usize::MAX);
        if i >= self.records.len() {
            self.records.resize(i + 1, Record::default());
        }
        &mut self.records[i]
    }

    pub fn push_name(&mut self, record: u64, parent: u64, text: impl IntoIterator<Item = u16>) {
        let start = self.arena.len();
        self.arena.extend(text);
        let len = u16::try_from(self.arena.len() - start).unwrap_or(u16::MAX);
        self.names.push(Name {
            record,
            parent,
            start,
            len,
        });
    }

    /// Counts everything below directory record `root`. NTFS metadata files are skipped (a
    /// directory listing never shows them). `sizes` is false for USN enumeration, which has none.
    pub fn tally(&self, root: u64, sizes: bool) -> Tally {
        let n = self.records.len();
        let index = |r: u64| usize::try_from(r).ok().filter(|&i| i < n);
        // A directory has one name (hard links to directories don't exist); follow it upward.
        let mut parent_of = vec![u64::MAX; n];
        for name in &self.names {
            if let Some(i) = index(name.record)
                && self.records[i].is_dir
                && parent_of[i] == u64::MAX
            {
                parent_of[i] = name.parent;
            }
        }
        let mut state = vec![UNKNOWN; n];
        for r in 0..FIRST_USER_RECORD.min(n as u64) {
            state[r as usize] = OUTSIDE;
        }
        let Some(root_i) = index(root) else {
            return Tally::default();
        };
        state[root_i] = INSIDE;

        let mut tally = Tally {
            bytes: sizes.then_some(0),
            unique_bytes: sizes.then_some(0),
            ..Tally::default()
        };
        let mut counted = vec![false; if sizes { n } else { 0 }];
        let mut chain = Vec::new();
        for name in &self.names {
            let (Some(r), Some(p)) = (index(name.record), index(name.parent)) else {
                continue;
            };
            let rec = self.records[r];
            if r == root_i || name.record < FIRST_USER_RECORD || !rec.in_use {
                continue;
            }
            if !inside(p, &parent_of, &mut state, &mut chain) {
                continue;
            }
            tally.name_chars += u64::from(name.len);
            if rec.is_dir {
                tally.dirs += 1;
                continue;
            }
            tally.files += 1;
            if sizes {
                tally.bytes = tally.bytes.map(|b| b + rec.size);
                if !counted[r] {
                    counted[r] = true;
                    tally.unique_bytes = tally.unique_bytes.map(|b| b + rec.size);
                }
            }
        }
        tally
    }
}

/// Whether directory `dir` lies below the root, memoised for every directory on the way up.
fn inside(dir: usize, parent_of: &[u64], state: &mut [u8], chain: &mut Vec<usize>) -> bool {
    chain.clear();
    let mut cur = dir;
    let verdict = loop {
        match state.get(cur) {
            None => break OUTSIDE,
            Some(&s) if s != UNKNOWN => break s,
            Some(_) => {}
        }
        chain.push(cur);
        let parent = parent_of[cur];
        let next = usize::try_from(parent).unwrap_or(usize::MAX);
        // A self-parent (the volume root) or a cycle in a damaged volume ends the walk.
        if parent == u64::MAX || next == cur || chain.len() > 4096 {
            break OUTSIDE;
        }
        cur = next;
    };
    for c in chain.drain(..) {
        state[c] = verdict;
    }
    verdict == INSIDE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(v: &mut Volume, record: u64, parent: u64, name: &str) {
        *v.record_mut(record) = Record {
            in_use: true,
            is_dir: true,
            size: 0,
        };
        v.push_name(record, parent, name.encode_utf16());
    }

    fn file(v: &mut Volume, record: u64, size: u64, names: &[(u64, &str)]) {
        *v.record_mut(record) = Record {
            in_use: true,
            is_dir: false,
            size,
        };
        for (parent, name) in names {
            v.push_name(record, *parent, name.encode_utf16());
        }
    }

    fn sample() -> Volume {
        let mut v = Volume::default();
        dir(&mut v, VOLUME_ROOT, VOLUME_ROOT, ".");
        file(&mut v, 0, 1 << 20, &[(VOLUME_ROOT, "$MFT")]);
        dir(&mut v, 11, VOLUME_ROOT, "$Extend");
        file(&mut v, 12, 9, &[(11, "$UsnJrnl")]);
        dir(&mut v, 100, VOLUME_ROOT, "c");
        dir(&mut v, 101, 100, "sub");
        file(&mut v, 200, 10, &[(101, "a.dll")]);
        file(&mut v, 201, 7, &[(101, "b.mui"), (100, "link")]);
        file(&mut v, 202, 3, &[(VOLUME_ROOT, "outside.txt")]);
        v.record_mut(203).in_use = false;
        v.push_name(203, 101, "deleted".encode_utf16());
        v
    }

    #[test]
    fn counts_a_subtree_with_hard_links_once_for_unique_bytes() {
        let t = sample().tally(100, true);
        assert_eq!((t.files, t.dirs), (3, 1));
        assert_eq!(t.bytes, Some(10 + 7 + 7));
        assert_eq!(t.unique_bytes, Some(17));
        assert_eq!(
            t.name_chars,
            ("sub".len() + "a.dll".len() + "b.mui".len() + "link".len()) as u64
        );
    }

    #[test]
    fn the_volume_root_skips_metadata_files() {
        let t = sample().tally(VOLUME_ROOT, true);
        assert_eq!((t.files, t.dirs), (4, 2));
        assert_eq!(t.unique_bytes, Some(20));
    }

    #[test]
    fn without_sizes_only_counts() {
        let t = sample().tally(100, false);
        assert_eq!(
            (t.files, t.dirs, t.bytes, t.unique_bytes),
            (3, 1, None, None)
        );
    }

    #[test]
    fn unknown_roots_and_parent_cycles_count_nothing() {
        let mut v = sample();
        assert_eq!(v.tally(9999, true), Tally::default());
        dir(&mut v, 300, 301, "x");
        dir(&mut v, 301, 300, "y");
        file(&mut v, 302, 1, &[(300, "f")]);
        assert_eq!(v.tally(100, true).files, 3);
    }
}
