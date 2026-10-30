//! The synthetic corpus: a deterministic WinSxS-like tree (doc 04 §2: "Cleanup scan, ~1.2 M
//! files"). The plan is pure, so the expected totals are known without touching a disk, and every
//! method's result is checked against them.

use crate::tally::Tally;

/// Directory fan-out: 40 × 40 × `leaves_per_branch` leaf directories, ~50 files each.
const TOP: usize = 40;
const MID: usize = 40;
const FILES_PER_LEAF: u64 = 50;
/// Every 64th file gets a second name (hard link) in another leaf.
const LINK_EVERY: u64 = 64;
const EXTENSIONS: [&str; 10] = [
    "dll", "mui", "cat", "manifest", "mum", "exe", "sys", "tmp", "log", "dat",
];

/// splitmix64: small, fast, and the same everywhere.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next_u64() % (hi - lo + 1)
    }
    fn word(&mut self, min: u64, max: u64) -> String {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789-_";
        let len = self.range(min, max);
        (0..len)
            .map(|_| char::from(ALPHABET[(self.next_u64() % ALPHABET.len() as u64) as usize]))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// Path relative to the corpus root.
    pub path: String,
    pub size: u64,
    /// Second name for the same file, relative to the corpus root.
    pub link: Option<String>,
}

#[derive(Debug)]
pub struct Plan {
    seed: u64,
    /// Every directory, parents first, relative to the corpus root.
    pub dirs: Vec<String>,
    /// Indexes into `dirs` of the leaf directories, which hold the files.
    pub leaves: Vec<usize>,
    /// Files in each leaf, and the global index of each leaf's first file.
    counts: Vec<u64>,
    first: Vec<u64>,
}

impl Plan {
    pub fn new(files: u64, seed: u64) -> Self {
        let leaves_wanted = files.div_ceil(FILES_PER_LEAF).max(1);
        let per_branch = leaves_wanted.div_ceil((TOP * MID) as u64).max(1) as usize;
        let mut rng = Rng::new(seed);
        let mut dirs = Vec::new();
        let mut leaves = Vec::new();
        'tree: for t in 0..TOP {
            let top = format!("{}_{t:02}", rng.word(4, 12));
            dirs.push(top.clone());
            for m in 0..MID {
                let mid = format!("{top}\\{}-{m:02}", rng.word(6, 20));
                dirs.push(mid.clone());
                for l in 0..per_branch {
                    if leaves.len() as u64 == leaves_wanted {
                        break 'tree;
                    }
                    let word = rng.word(10, 40);
                    let hash = rng.next_u64();
                    leaves.push(dirs.len());
                    dirs.push(format!("{mid}\\amd64_{word}_{hash:016x}_{l}"));
                }
            }
        }
        // Even split, then pairs of leaves trade up to 80 % of the base: varied, exact total.
        let n = leaves.len() as u64;
        let (base, extra) = (files / n, files % n);
        let mut counts: Vec<u64> = (0..n).map(|i| base + u64::from(i < extra)).collect();
        for pair in counts.as_chunks_mut::<2>().0 {
            let d = rng.range(0, base * 4 / 5);
            pair[0] += d;
            pair[1] -= d;
        }
        let mut first = Vec::with_capacity(counts.len());
        let mut acc = 0;
        for &n in &counts {
            first.push(acc);
            acc += n;
        }
        Self {
            seed,
            dirs,
            leaves,
            counts,
            first,
        }
    }

    /// The files of leaf `leaf` (an index into `leaves`), generated on demand so creation can
    /// run one leaf per task without holding 1.2 M paths in memory.
    pub fn files(&self, leaf: usize) -> Vec<File> {
        let mut rng = Rng::new(self.seed ^ (leaf as u64).wrapping_mul(0xA24B_AED4_963E_E407));
        let dir = &self.dirs[self.leaves[leaf]];
        let link_dir = &self.dirs[self.leaves[(leaf + 7) % self.leaves.len()]];
        (0..self.counts[leaf])
            .map(|i| {
                let global = self.first[leaf] + i;
                let ext = EXTENSIONS[(rng.next_u64() % EXTENSIONS.len() as u64) as usize];
                let name = format!("{}-{i:x}.{ext}", rng.word(6, 40));
                let size = size(&mut rng);
                let link = (global % LINK_EVERY == LINK_EVERY - 1)
                    .then(|| format!("{link_dir}\\link-{global:x}-{name}"));
                File {
                    path: format!("{dir}\\{name}"),
                    size,
                    link,
                }
            })
            .collect()
    }

    /// What a correct scan of the corpus root must report.
    pub fn expected(&self) -> Tally {
        let mut t = Tally {
            dirs: self.dirs.len() as u64,
            bytes: Some(0),
            unique_bytes: Some(0),
            ..Tally::default()
        };
        t.name_chars = self.dirs.iter().map(|d| leaf_len(d)).sum();
        for leaf in 0..self.leaves.len() {
            for f in self.files(leaf) {
                let names = 1 + u64::from(f.link.is_some());
                t.files += names;
                t.bytes = t.bytes.map(|b| b + names * f.size);
                t.unique_bytes = t.unique_bytes.map(|b| b + f.size);
                t.name_chars += leaf_len(&f.path) + f.link.as_deref().map_or(0, leaf_len);
            }
        }
        t
    }
}

fn leaf_len(path: &str) -> u64 {
    path.rsplit('\\')
        .next()
        .map_or(0, |n| n.encode_utf16().count() as u64)
}

/// 40 % empty, 30 % small enough to stay resident in the MFT record, 30 % 1–64 KiB
/// (log-uniform), so both `$DATA` forms are exercised. ~16 KiB mean for the non-resident ones.
fn size(rng: &mut Rng) -> u64 {
    match rng.range(0, 9) {
        0..=3 => 0,
        4..=6 => rng.range(1, 700),
        _ => {
            let base = 1u64 << rng.range(10, 15);
            base + rng.range(0, base - 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn plans_are_deterministic_and_hit_the_file_count() {
        let a = Plan::new(10_000, 7);
        let b = Plan::new(10_000, 7);
        assert_eq!(a.dirs, b.dirs);
        assert_eq!(a.files(3), b.files(3));
        let primary: u64 = (0..a.leaves.len()).map(|l| a.files(l).len() as u64).sum();
        assert_eq!(primary, 10_000);
        let e = a.expected();
        assert_eq!(e.files, 10_000 + 10_000 / LINK_EVERY);
        assert_eq!(e, b.expected());
        assert_ne!(e.bytes, e.unique_bytes);
    }

    #[test]
    fn names_are_unique_per_directory_and_parents_come_first() {
        let p = Plan::new(5_000, 1);
        let mut seen = HashSet::new();
        for d in &p.dirs {
            if let Some((parent, _)) = d.rsplit_once('\\') {
                assert!(seen.contains(parent), "{parent} before {d}");
            }
            assert!(seen.insert(d.clone()));
        }
        let mut paths = HashSet::new();
        for l in 0..p.leaves.len() {
            for f in p.files(l) {
                assert!(paths.insert(f.path));
                if let Some(link) = f.link {
                    assert!(paths.insert(link));
                }
            }
        }
    }

    #[test]
    fn the_full_corpus_has_the_documented_shape() {
        let p = Plan::new(1_200_000, 0x5045);
        assert_eq!(p.leaves.len(), 24_000);
        assert_eq!(p.dirs.len(), 24_000 + TOP * MID + TOP);
        assert!(p.counts.iter().all(|&n| n <= 2 * FILES_PER_LEAF));
    }
}
