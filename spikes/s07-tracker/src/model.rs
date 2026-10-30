//! The pure part: what a trace observed, what an installer really wrote (ground truth), the
//! attribution rule of TD-0015 §5.7, and the checks. No OS calls, so it's tested on Linux.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Write as _,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    FileNew,
    FileWrite,
    FileDelete,
    FileRename,
    FileLink,
    KeyCreate,
    KeyDelete,
    ValueSet,
    ValueDelete,
    KeyInfo,
    KeySecurity,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::FileNew => "file new",
            Self::FileWrite => "file write",
            Self::FileDelete => "file delete",
            Self::FileRename => "file rename",
            Self::FileLink => "file link",
            Self::KeyCreate => "key create",
            Self::KeyDelete => "key delete",
            Self::ValueSet => "value set",
            Self::ValueDelete => "value delete",
            Self::KeyInfo => "key info",
            Self::KeySecurity => "key security",
        }
    }

    pub fn is_file(self) -> bool {
        matches!(
            self,
            Self::FileNew | Self::FileWrite | Self::FileDelete | Self::FileRename | Self::FileLink
        )
    }
}

/// One distinct operation: a process did `kind` to `path` (and `value`, for registry values).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Op {
    pub pid: u32,
    pub kind: Kind,
    pub path: String,
    pub value: String,
}

/// When an [`Op`] was first and last seen (QPC ticks) and how often.
#[derive(Clone, Copy, Debug)]
pub struct Seen {
    pub first: i64,
    pub last: i64,
    pub count: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Proc {
    pub parent: u32,
    pub image: String,
    pub start: Option<i64>,
    pub stop: Option<i64>,
}

pub fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// `\Device\HarddiskVolume3\x` → `C:\x`, from (device, drive) pairs (`QueryDosDeviceW`).
pub fn dos_path(nt: &str, devices: &[(String, String)]) -> String {
    for (device, drive) in devices {
        if let Some(rest) = strip_prefix_ci(nt, device)
            && (rest.is_empty() || rest.starts_with('\\'))
        {
            return format!("{drive}{rest}");
        }
    }
    nt.to_owned()
}

/// `\REGISTRY\MACHINE\X` → `HKLM\X`, `\REGISTRY\USER\X` → `HKU\X`; other names unchanged.
pub fn reg_path(kernel: &str) -> String {
    for (from, to) in [(r"\REGISTRY\MACHINE", "HKLM"), (r"\REGISTRY\USER", "HKU")] {
        if let Some(rest) = strip_prefix_ci(kernel, from)
            && (rest.is_empty() || rest.starts_with('\\'))
        {
            return format!("{to}{rest}");
        }
    }
    kernel.to_owned()
}

pub fn is_absolute_key(path: &str) -> bool {
    ["HKLM", "HKU"].iter().any(|root| {
        strip_prefix_ci(path, root).is_some_and(|rest| rest.is_empty() || rest.starts_with('\\'))
    })
}

/// The key a `CreateKey`/`OpenKey` event names: `base\relative`, or `relative` alone when it's
/// already absolute. `base` is the resolved name of the handle the call was relative to.
pub fn join_key(base: &str, relative: &str) -> String {
    let relative = reg_path(relative);
    let base = reg_path(base);
    if base.is_empty() || is_absolute_key(&relative) {
        return relative;
    }
    if relative.is_empty() {
        return base;
    }
    format!(
        "{}\\{}",
        base.trim_end_matches('\\'),
        relative.trim_start_matches('\\')
    )
}

pub fn file_name(image: &str) -> &str {
    image.rsplit(['\\', '/']).next().unwrap_or(image)
}

pub fn is_msiexec(image: &str) -> bool {
    file_name(image).eq_ignore_ascii_case("msiexec.exe")
}

/// TD-0015 §5.7: the job's processes, plus every `msiexec.exe` (the Windows Installer service
/// works outside the job) and the descendants of all of those.
pub fn attributed(procs: &HashMap<u32, Proc>, job: &HashSet<u32>) -> HashSet<u32> {
    let mut set = job.clone();
    set.extend(
        procs
            .iter()
            .filter(|(_, p)| is_msiexec(&p.image))
            .map(|(pid, _)| *pid),
    );
    loop {
        let more: Vec<u32> = procs
            .iter()
            .filter(|(pid, p)| !set.contains(pid) && set.contains(&p.parent))
            .map(|(pid, _)| *pid)
            .collect();
        if more.is_empty() {
            return set;
        }
        set.extend(more);
    }
}

/// What an installer really wrote. Paths are DOS paths, keys `HKLM\…`.
#[derive(Clone, Debug, Default)]
pub struct Truth {
    pub files: Vec<String>,
    pub modified: Vec<String>,
    pub keys: Vec<String>,
    pub values: Vec<(String, String)>,
}

/// How many truth items a source saw.
#[derive(Clone, Debug, Default)]
pub struct Recall {
    pub total: usize,
    /// Seen with the full path, by an attributed process.
    pub found: usize,
    /// Seen only as a relative registry name whose tail matches, by an attributed process.
    pub partial: usize,
    /// Seen, but only from processes the rule doesn't attribute.
    pub other_pid: usize,
    pub missing: Vec<String>,
}

impl Recall {
    fn add(&mut self, item: &str, hit: Hit) {
        self.total += 1;
        match hit {
            Hit::Full => self.found += 1,
            Hit::Partial => self.partial += 1,
            Hit::OtherPid => self.other_pid += 1,
            Hit::None => self.missing.push(item.to_owned()),
        }
    }

    pub fn cell(&self) -> String {
        let mut s = format!("{}/{}", self.found, self.total);
        if self.partial > 0 {
            let _ = write!(s, " (+{} partial)", self.partial);
        }
        if self.other_pid > 0 {
            let _ = write!(s, " (+{} other pid)", self.other_pid);
        }
        s
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Hit {
    None,
    OtherPid,
    Partial,
    Full,
}

#[derive(Clone, Debug, Default)]
pub struct Check {
    pub files: Recall,
    pub modified: Recall,
    pub keys: Recall,
    pub values: Recall,
}

fn leaf(path: &str) -> String {
    path.rsplit('\\').next().unwrap_or(path).to_lowercase()
}

/// Matches an observed key name against a truth key: equal, or (for a relative name) a tail.
fn key_hit(observed: &str, truth: &str) -> Option<Hit> {
    if observed.eq_ignore_ascii_case(truth) {
        return Some(Hit::Full);
    }
    let tail = observed.trim_start_matches('\\');
    let lower = truth.to_lowercase();
    (!is_absolute_key(observed)
        && !tail.is_empty()
        && lower.ends_with(&format!("\\{}", tail.to_lowercase())))
    .then_some(Hit::Partial)
}

/// Recall of the truth in an ETW trace, under the attribution rule.
pub fn check_etw(truth: &Truth, ops: &HashMap<Op, Seen>, attributed: &HashSet<u32>) -> Check {
    let mut files: HashMap<String, Vec<(u32, Kind)>> = HashMap::new();
    let mut keys: HashMap<String, Vec<&Op>> = HashMap::new();
    for op in ops.keys() {
        if op.kind.is_file() {
            files
                .entry(op.path.to_lowercase())
                .or_default()
                .push((op.pid, op.kind));
        } else {
            keys.entry(leaf(&op.path)).or_default().push(op);
        }
    }
    let file_hit = |path: &str, kinds: &[Kind]| {
        let seen = files
            .get(&path.to_lowercase())
            .map_or(&[][..], Vec::as_slice);
        seen.iter()
            .filter(|(_, k)| kinds.contains(k))
            .fold(Hit::None, |best, (pid, _)| {
                best.max(if attributed.contains(pid) {
                    Hit::Full
                } else {
                    Hit::OtherPid
                })
            })
    };
    let reg_hit = |key: &str, kind: Kind, value: Option<&str>| {
        let seen = keys.get(&leaf(key)).map_or(&[][..], Vec::as_slice);
        seen.iter()
            .filter(|op| op.kind == kind && value.is_none_or(|v| op.value.eq_ignore_ascii_case(v)))
            .filter_map(|op| {
                let hit = key_hit(&op.path, key)?;
                Some(if attributed.contains(&op.pid) {
                    hit
                } else {
                    Hit::OtherPid
                })
            })
            .max()
            .unwrap_or(Hit::None)
    };
    let written = [
        Kind::FileNew,
        Kind::FileWrite,
        Kind::FileRename,
        Kind::FileLink,
    ];
    let mut check = Check::default();
    for f in &truth.files {
        check.files.add(f, file_hit(f, &written));
    }
    for f in &truth.modified {
        check.modified.add(f, file_hit(f, &[Kind::FileWrite]));
    }
    for k in &truth.keys {
        check.keys.add(k, reg_hit(k, Kind::KeyCreate, None));
    }
    for (k, v) in &truth.values {
        check
            .values
            .add(&format!("{k} : {v}"), reg_hit(k, Kind::ValueSet, Some(v)));
    }
    check
}

/// Which processes wrote the truth files: (pid, count of truth files), most first.
pub fn writers(truth: &Truth, ops: &HashMap<Op, Seen>) -> Vec<(u32, usize)> {
    let wanted: HashSet<String> = truth.files.iter().map(|f| f.to_lowercase()).collect();
    let mut by_pid: HashMap<u32, HashSet<String>> = HashMap::new();
    for op in ops.keys() {
        let path = op.path.to_lowercase();
        if op.kind.is_file() && op.kind != Kind::FileDelete && wanted.contains(&path) {
            by_pid.entry(op.pid).or_default().insert(path);
        }
    }
    let mut out: Vec<(u32, usize)> = by_pid.into_iter().map(|(p, s)| (p, s.len())).collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

// ---------------------------------------------------------------------------------------------
// Registry snapshots

/// Keys (lowercase full paths) new, deleted, or with a changed last-write time.
#[derive(Clone, Debug, Default)]
pub struct RegDiff {
    pub new: Vec<String>,
    pub deleted: Vec<String>,
    pub changed: Vec<String>,
}

impl RegDiff {
    pub fn touched(&self) -> HashSet<String> {
        self.new.iter().chain(&self.changed).cloned().collect()
    }
}

pub fn reg_diff(before: &HashMap<String, u64>, after: &HashMap<String, u64>) -> RegDiff {
    let mut d = RegDiff::default();
    for (k, t) in after {
        match before.get(k) {
            None => d.new.push(k.clone()),
            Some(old) if old != t => d.changed.push(k.clone()),
            Some(_) => {}
        }
    }
    d.deleted = before
        .keys()
        .filter(|k| !after.contains_key(*k))
        .cloned()
        .collect();
    d.new.sort();
    d.changed.sort();
    d.deleted.sort();
    d
}

/// Would a walk that descends only into keys whose last-write time changed (TD-0015 §5.7 step 3)
/// reach `key`? The root is always enumerated; every ancestor between it and `key` must be
/// touched. `key` and `roots` are lowercase.
pub fn pruned_reach(key: &str, touched: &HashSet<String>, roots: &[String]) -> bool {
    let Some(root) = roots.iter().find(|r| key.starts_with(&format!("{r}\\"))) else {
        return false;
    };
    let mut path = key;
    while let Some((parent, _)) = path.rsplit_once('\\') {
        if parent.len() <= root.len() {
            return true;
        }
        if !touched.contains(parent) {
            return false;
        }
        path = parent;
    }
    true
}

// ---------------------------------------------------------------------------------------------
// USN journal

/// One `USN_RECORD_V2`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsnRecord {
    pub frn: u64,
    pub parent: u64,
    pub usn: i64,
    pub reason: u32,
    pub name: String,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// One `FSCTL_READ_USN_JOURNAL` output: the next USN, then records (only V2 is decoded).
pub fn parse_usn(buf: &[u8]) -> (i64, Vec<UsnRecord>) {
    let next = u64_at(buf, 0).unwrap_or(0) as i64;
    let mut out = Vec::new();
    let mut at = 8;
    while let Some(len) = u32_at(buf, at).map(|l| l as usize) {
        let Some(rec) = buf.get(at..at + len).filter(|_| len >= 60) else {
            break;
        };
        if u16_at(rec, 4) == Some(2) {
            let name_len = usize::from(u16_at(rec, 56).unwrap_or(0));
            let name_at = usize::from(u16_at(rec, 58).unwrap_or(0));
            let units: Vec<u16> = rec
                .get(name_at..name_at + name_len)
                .unwrap_or_default()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|u| u16::from_le_bytes(*u))
                .collect();
            out.push(UsnRecord {
                frn: u64_at(rec, 8).unwrap_or(0),
                parent: u64_at(rec, 16).unwrap_or(0),
                usn: u64_at(rec, 24).unwrap_or(0) as i64,
                reason: u32_at(rec, 40).unwrap_or(0),
                name: String::from_utf16_lossy(&units),
            });
        }
        at += len;
    }
    (next, out)
}

/// A file's records in a delta, merged: its last name and parent, and every reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsnFile {
    pub frn: u64,
    pub parent: u64,
    pub name: String,
    pub reasons: u32,
}

pub fn merge_usn(records: &[UsnRecord]) -> Vec<UsnFile> {
    let mut by_frn: BTreeMap<u64, UsnFile> = BTreeMap::new();
    for r in records {
        let f = by_frn.entry(r.frn).or_insert_with(|| UsnFile {
            frn: r.frn,
            parent: r.parent,
            name: r.name.clone(),
            reasons: 0,
        });
        f.parent = r.parent;
        f.name.clone_from(&r.name);
        f.reasons |= r.reason;
    }
    by_frn.into_values().collect()
}

pub const USN_DATA_OVERWRITE: u32 = 0x1;
pub const USN_DATA_EXTEND: u32 = 0x2;
pub const USN_DATA_TRUNCATION: u32 = 0x4;
pub const USN_FILE_CREATE: u32 = 0x100;
pub const USN_FILE_DELETE: u32 = 0x200;

/// Up to `n` parent folders with the most entries, as "`folder` (count)".
pub fn top_folders<'a>(paths: impl Iterator<Item = &'a str>, n: usize) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for p in paths {
        let dir = p.rsplit_once('\\').map_or(p, |(d, _)| d);
        *counts.entry(dir).or_default() += 1;
    }
    let mut v: Vec<(&str, usize)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v.iter()
        .take(n)
        .map(|(d, c)| format!("`{d}` ({c})"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::{
        Hit, Kind, Op, Proc, Seen, Truth, UsnRecord, attributed, check_etw, dos_path, join_key,
        key_hit, merge_usn, parse_usn, pruned_reach, reg_diff, reg_path,
    };
    use std::collections::{HashMap, HashSet};

    #[test]
    fn paths() {
        let devices = vec![(r"\Device\HarddiskVolume3".to_owned(), "C:".to_owned())];
        assert_eq!(
            dos_path(r"\Device\HarddiskVolume3\x\y", &devices),
            r"C:\x\y"
        );
        assert_eq!(dos_path(r"\device\harddiskvolume3", &devices), "C:");
        assert_eq!(
            dos_path(r"\Device\HarddiskVolume33\x", &devices),
            r"\Device\HarddiskVolume33\x"
        );
        assert_eq!(
            reg_path(r"\REGISTRY\MACHINE\SOFTWARE\A"),
            r"HKLM\SOFTWARE\A"
        );
        assert_eq!(reg_path(r"\Registry\User\S-1-5-18\X"), r"HKU\S-1-5-18\X");
        assert_eq!(
            join_key(r"\REGISTRY\MACHINE\SOFTWARE", r"S07Test\k"),
            r"HKLM\SOFTWARE\S07Test\k"
        );
        assert_eq!(join_key("", r"S07Test\k"), r"S07Test\k");
        assert_eq!(
            join_key(r"\REGISTRY\MACHINE", r"\REGISTRY\MACHINE\SOFTWARE"),
            r"HKLM\SOFTWARE"
        );
        assert_eq!(
            key_hit(r"S07Test\k", r"HKLM\SOFTWARE\S07Test\k"),
            Some(Hit::Partial)
        );
        assert_eq!(
            key_hit(r"hklm\software\s07test\k", r"HKLM\SOFTWARE\S07Test\k"),
            Some(Hit::Full)
        );
        assert_eq!(
            key_hit(r"HKLM\SOFTWARE\Other\k", r"HKLM\SOFTWARE\S07Test\k"),
            None
        );
    }

    fn proc(parent: u32, image: &str) -> Proc {
        Proc {
            parent,
            image: image.into(),
            start: None,
            stop: None,
        }
    }

    #[test]
    fn attribution_follows_msiexec_and_children() {
        let procs = HashMap::from([
            (
                10,
                proc(1, r"\Device\HarddiskVolume3\Windows\System32\msiexec.exe"),
            ),
            (11, proc(10, "msiexec.exe")),
            (12, proc(11, "rundll32.exe")),
            (20, proc(5, "svchost.exe")),
            (31, proc(30, "child.exe")),
        ]);
        let set = attributed(&procs, &HashSet::from([30]));
        assert_eq!(set, HashSet::from([10, 11, 12, 30, 31]));
    }

    fn op(pid: u32, kind: Kind, path: &str, value: &str) -> (Op, Seen) {
        (
            Op {
                pid,
                kind,
                path: path.into(),
                value: value.into(),
            },
            Seen {
                first: 0,
                last: 0,
                count: 1,
            },
        )
    }

    #[test]
    fn etw_recall() {
        let truth = Truth {
            files: vec![
                r"C:\P\a.bin".into(),
                r"C:\P\b.bin".into(),
                r"C:\P\c.bin".into(),
            ],
            modified: vec![r"C:\cfg.ini".into()],
            keys: vec![
                r"HKLM\SOFTWARE\S07Test\k1".into(),
                r"HKLM\SOFTWARE\S07Test\k2".into(),
            ],
            values: vec![(r"HKLM\SOFTWARE\S07Test\k1".into(), "v".into())],
        };
        let ops = HashMap::from([
            op(7, Kind::FileNew, r"c:\p\a.bin", ""),
            op(9, Kind::FileWrite, r"C:\P\b.bin", ""),
            op(7, Kind::FileWrite, r"C:\cfg.ini", ""),
            op(7, Kind::KeyCreate, r"HKLM\SOFTWARE\S07Test\k1", ""),
            op(7, Kind::KeyCreate, r"S07Test\k2", ""),
            op(7, Kind::ValueSet, r"HKLM\SOFTWARE\S07Test\k1", "V"),
        ]);
        let c = check_etw(&truth, &ops, &HashSet::from([7]));
        assert_eq!(
            (c.files.found, c.files.other_pid, c.files.missing.len()),
            (1, 1, 1)
        );
        assert_eq!(c.modified.found, 1);
        assert_eq!((c.keys.found, c.keys.partial), (1, 1));
        assert_eq!(c.values.found, 1);
        assert_eq!(c.files.cell(), "1/3 (+1 other pid)");
    }

    #[test]
    fn registry_diff_and_pruned_walk() {
        let before = HashMap::from([
            ("hklm\\software".to_owned(), 1),
            ("hklm\\software\\microsoft".to_owned(), 1),
            ("hklm\\software\\microsoft\\run".to_owned(), 1),
            ("hklm\\software\\gone".to_owned(), 1),
        ]);
        let mut after = before.clone();
        after.remove("hklm\\software\\gone");
        after.insert("hklm\\software".into(), 2);
        after.insert("hklm\\software\\microsoft\\run".into(), 2);
        after.insert("hklm\\software\\s07".into(), 2);
        after.insert("hklm\\software\\s07\\k".into(), 2);
        let d = reg_diff(&before, &after);
        assert_eq!(d.new, vec!["hklm\\software\\s07", "hklm\\software\\s07\\k"]);
        assert_eq!(
            d.changed,
            vec!["hklm\\software", "hklm\\software\\microsoft\\run"]
        );
        assert_eq!(d.deleted, vec!["hklm\\software\\gone"]);
        let touched = d.touched();
        let roots = vec!["hklm\\software".to_owned()];
        assert!(pruned_reach("hklm\\software\\s07\\k", &touched, &roots));
        // A value written into an existing deep key doesn't touch its ancestors.
        assert!(!pruned_reach(
            "hklm\\software\\microsoft\\run",
            &touched,
            &roots
        ));
        assert!(pruned_reach("hklm\\software\\microsoft", &touched, &roots));
    }

    fn usn_record(frn: u64, parent: u64, usn: i64, reason: u32, name: &str) -> Vec<u8> {
        let name: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let len = (60 + name.len()).next_multiple_of(8);
        let mut r = vec![0u8; len];
        r[0..4].copy_from_slice(&(len as u32).to_le_bytes());
        r[4..6].copy_from_slice(&2u16.to_le_bytes());
        r[8..16].copy_from_slice(&frn.to_le_bytes());
        r[16..24].copy_from_slice(&parent.to_le_bytes());
        r[24..32].copy_from_slice(&usn.to_le_bytes());
        r[40..44].copy_from_slice(&reason.to_le_bytes());
        r[56..58].copy_from_slice(&(name.len() as u16).to_le_bytes());
        r[58..60].copy_from_slice(&60u16.to_le_bytes());
        r[60..60 + name.len()].copy_from_slice(&name);
        r
    }

    #[test]
    fn usn_parse_and_merge() {
        let mut buf = 500i64.to_le_bytes().to_vec();
        buf.extend(usn_record(1, 5, 100, 0x100, "a.tmp"));
        buf.extend(usn_record(1, 6, 200, 0x2000, "a.bin"));
        buf.extend(usn_record(2, 5, 300, 0x2, "b"));
        let (next, recs) = parse_usn(&buf);
        assert_eq!(next, 500);
        assert_eq!(recs.len(), 3);
        assert_eq!(
            recs[1],
            UsnRecord {
                frn: 1,
                parent: 6,
                usn: 200,
                reason: 0x2000,
                name: "a.bin".into()
            }
        );
        let files = merge_usn(&recs);
        assert_eq!(files.len(), 2);
        assert_eq!(
            (files[0].parent, files[0].name.as_str(), files[0].reasons),
            (6, "a.bin", 0x2100)
        );
        // A truncated buffer stops cleanly.
        assert_eq!(parse_usn(&buf[..buf.len() - 4]).1.len(), 2);
    }
}
