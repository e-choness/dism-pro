//! Write-ahead job journal (TD-0001 §5.2, §5.4).
//!
//! File: `"DPJ1"`, schema `u16` LE, then records framed as
//! `[len u32 LE][crc32c u32 LE][kind u8][postcard payload]`, where `len` counts the kind
//! byte plus payload and the CRC covers the same bytes. A torn tail (short frame, bad CRC,
//! undecodable payload) ends the readable journal; recovery truncates the file there.
//!
//! Invariant (enforced by `job::run`): no change is applied before its intent record has
//! been synced. Intents are buffered and synced in batches, so a 100k-change job does
//! ~N/256 syncs.

use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
};

use dpro_core::{
    id::{JobId, TargetId},
    plan::{Change, ChangeKind, OsBuild, Reversibility},
    units::UnixMillis,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::crc32c::crc32c;

pub const MAGIC: [u8; 4] = *b"DPJ1";
pub const JOURNAL_SCHEMA: u16 = 1;
const FILE_HEADER_LEN: usize = 6;
const FRAME_HEADER_LEN: usize = 8;
/// Upper bound for one record; anything larger is treated as corruption.
const MAX_RECORD_LEN: usize = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("journal I/O failed")]
    Io(#[from] io::Error),
    #[error("journal record encoding failed")]
    Encode(#[source] postcard::Error),
    #[error("not a DISM Pro journal (bad magic)")]
    BadMagic,
    #[error("unsupported journal schema {0}")]
    UnsupportedSchema(u16),
    #[error("journal has no begin record")]
    NoBegin,
    #[error("record too large ({0} bytes)")]
    RecordTooLarge(usize),
}

/// Where journal bytes go. `sync` must make everything appended so far durable.
pub trait JournalSink {
    /// Appends `bytes` at the end.
    ///
    /// # Errors
    /// Any I/O error; the job stops before the next change.
    fn append(&mut self, bytes: &[u8]) -> io::Result<()>;

    /// Makes all appended bytes durable (fsync).
    ///
    /// # Errors
    /// Any I/O error; the job stops before the next change.
    fn sync(&mut self) -> io::Result<()>;
}

/// In-memory sink (tests, benches, `--dry-run` journaling); `sync` is a no-op.
impl JournalSink for Vec<u8> {
    fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.extend_from_slice(bytes);
        Ok(())
    }

    fn sync(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl JournalSink for std::fs::File {
    fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        io::Write::write_all(self, bytes)
    }

    fn sync(&mut self) -> io::Result<()> {
        self.sync_data()
    }
}

/// Job header, written as the first record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Begin {
    pub job: JobId,
    pub target: TargetId,
    pub os_build: OsBuild,
    pub created: UnixMillis,
    /// The plan's `OpDescriptor::id` (e.g. `features.enable`), for listings.
    pub op: String,
    /// For an undo job: the job it undoes. A job can be undone once; undo the undo to redo.
    pub undoes: Option<JobId>,
}

/// Change `index` of the plan is about to be applied; `inverse` undoes it (if any).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    pub index: u32,
    pub change: Change<String>,
    pub inverse: Option<ChangeKind<String>>,
}

/// Append new variants at the end: postcard encodes the variant index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Applied,
    /// Not applied; `code` is the OS error (`HRESULT`/Win32), 0 if none.
    Failed {
        code: i32,
    },
    /// Applied; finishes at the next restart (`ERROR_SUCCESS_REBOOT_REQUIRED`).
    AppliedRebootRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Done {
    pub index: u32,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Completed,
    Cancelled,
    /// Stopped early because of an error (e.g. disk full while journaling).
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    pub status: JobStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Begin(Begin),
    Intent(Intent),
    Done(Done),
    Commit(Commit),
}

// Kind bytes: part of the format; never reuse a value.
const KIND_BEGIN: u8 = 1;
const KIND_INTENT: u8 = 2;
const KIND_DONE: u8 = 3;
const KIND_COMMIT: u8 = 4;

/// Buffers records and appends + syncs them on [`JournalWriter::flush`].
#[derive(Debug)]
pub struct JournalWriter<W> {
    sink: W,
    buf: Vec<u8>,
    buffered_intents: usize,
    syncs: u64,
}

impl<W: JournalSink> JournalWriter<W> {
    /// Writes the file header and the begin record, and syncs.
    ///
    /// # Errors
    /// I/O or encoding errors.
    pub fn create(sink: W, begin: &Begin) -> Result<Self, JournalError> {
        let mut w = Self {
            sink,
            buf: Vec::with_capacity(64 * 1024),
            buffered_intents: 0,
            syncs: 0,
        };
        w.buf.extend_from_slice(&MAGIC);
        w.buf.extend_from_slice(&JOURNAL_SCHEMA.to_le_bytes());
        w.push(KIND_BEGIN, begin)?;
        w.flush()?;
        Ok(w)
    }

    /// Buffers an intent. It is durable only after the next [`JournalWriter::flush`].
    ///
    /// # Errors
    /// Encoding errors.
    pub fn intent(&mut self, intent: &Intent) -> Result<(), JournalError> {
        self.push(KIND_INTENT, intent)?;
        self.buffered_intents += 1;
        Ok(())
    }

    /// Buffers a done record.
    ///
    /// # Errors
    /// Encoding errors.
    pub fn done(&mut self, done: Done) -> Result<(), JournalError> {
        self.push(KIND_DONE, &done)
    }

    /// Appends everything buffered and syncs. No-op when nothing is buffered.
    ///
    /// # Errors
    /// I/O errors from the sink.
    pub fn flush(&mut self) -> Result<(), JournalError> {
        if self.buf.is_empty() {
            return Ok(());
        }
        self.sink.append(&self.buf)?;
        self.sink.sync()?;
        self.buf.clear();
        self.buffered_intents = 0;
        self.syncs += 1;
        Ok(())
    }

    /// Writes the commit record, syncs, and returns the sink.
    ///
    /// # Errors
    /// I/O or encoding errors.
    pub fn commit(mut self, status: JobStatus) -> Result<W, JournalError> {
        self.push(KIND_COMMIT, &Commit { status })?;
        self.flush()?;
        Ok(self.sink)
    }

    /// Intents buffered but not yet durable.
    #[must_use]
    pub fn buffered_intents(&self) -> usize {
        self.buffered_intents
    }

    /// Number of syncs so far (budget: ≤ 500 for a 100k-change job, TD-0001 §7.2).
    #[must_use]
    pub fn syncs(&self) -> u64 {
        self.syncs
    }

    fn push<T: Serialize>(&mut self, kind: u8, value: &T) -> Result<(), JournalError> {
        let frame_start = self.buf.len();
        // Reserve the frame header, then write kind + payload after it.
        self.buf.extend_from_slice(&[0; FRAME_HEADER_LEN]);
        self.buf.push(kind);
        let payload = postcard::to_allocvec(value).map_err(JournalError::Encode)?;
        self.buf.extend_from_slice(&payload);
        let body = &self.buf[frame_start + FRAME_HEADER_LEN..];
        if body.len() > MAX_RECORD_LEN {
            let len = body.len();
            self.buf.truncate(frame_start);
            return Err(JournalError::RecordTooLarge(len));
        }
        let len =
            u32::try_from(body.len()).map_err(|_| JournalError::RecordTooLarge(body.len()))?;
        let crc = crc32c(body);
        self.buf[frame_start..frame_start + 4].copy_from_slice(&len.to_le_bytes());
        self.buf[frame_start + 4..frame_start + 8].copy_from_slice(&crc.to_le_bytes());
        Ok(())
    }
}

/// What could be read back from a journal file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    pub begin: Begin,
    /// Records after `begin`, in file order.
    pub records: Vec<Record>,
    /// Bytes up to the end of the last valid record; truncate the file here.
    pub valid_len: usize,
    /// Whether bytes after `valid_len` were discarded.
    pub torn: bool,
}

/// Reads a journal, stopping at the first torn or corrupt record. Never panics on
/// arbitrary input (fuzzed in Phase 6).
///
/// # Errors
/// [`JournalError::BadMagic`], [`JournalError::UnsupportedSchema`], or
/// [`JournalError::NoBegin`] if not even the begin record survived.
pub fn read(bytes: &[u8]) -> Result<Recovered, JournalError> {
    if bytes.len() < FILE_HEADER_LEN || bytes[..4] != MAGIC {
        return Err(JournalError::BadMagic);
    }
    let schema = u16::from_le_bytes([bytes[4], bytes[5]]);
    if schema != JOURNAL_SCHEMA {
        return Err(JournalError::UnsupportedSchema(schema));
    }

    let mut offset = FILE_HEADER_LEN;
    let mut begin = None;
    let mut records = Vec::new();
    let mut torn = false;
    while offset < bytes.len() {
        let Some((record, next)) = read_frame(bytes, offset) else {
            torn = true;
            break;
        };
        match (record, begin.is_some()) {
            (Record::Begin(b), false) => begin = Some(b),
            (Record::Begin(_), true) | (_, false) => {
                // A second begin, or a record before begin: not a journal we wrote.
                torn = true;
                break;
            }
            (r, true) => records.push(r),
        }
        offset = next;
    }
    let begin = begin.ok_or(JournalError::NoBegin)?;
    Ok(Recovered {
        begin,
        records,
        valid_len: offset,
        torn,
    })
}

fn read_frame(bytes: &[u8], offset: usize) -> Option<(Record, usize)> {
    let header = bytes.get(offset..offset.checked_add(FRAME_HEADER_LEN)?)?;
    let len = usize::try_from(u32::from_le_bytes([
        header[0], header[1], header[2], header[3],
    ]))
    .ok()?;
    let crc = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    if len == 0 || len > MAX_RECORD_LEN {
        return None;
    }
    let body_start = offset + FRAME_HEADER_LEN;
    let body = bytes.get(body_start..body_start.checked_add(len)?)?;
    if crc32c(body) != crc {
        return None;
    }
    let (kind, payload) = body.split_first()?;
    let record = match *kind {
        KIND_BEGIN => Record::Begin(decode(payload)?),
        KIND_INTENT => Record::Intent(decode(payload)?),
        KIND_DONE => Record::Done(decode(payload)?),
        KIND_COMMIT => Record::Commit(decode(payload)?),
        _ => return None,
    };
    Some((record, body_start + len))
}

fn decode<T: DeserializeOwned>(payload: &[u8]) -> Option<T> {
    postcard::from_bytes(payload).ok()
}

/// How a journaled job ended, as far as the journal can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobReport {
    /// `None`: no commit record, the job was interrupted (power loss, crash).
    pub status: Option<JobStatus>,
    pub applied: Vec<u32>,
    pub failed: Vec<(u32, i32)>,
    /// Intents without a done record. After a commit these were never applied; for an
    /// interrupted job they must be probed against the target.
    pub without_outcome: Vec<u32>,
    /// An applied change finishes at the next restart.
    pub reboot_required: bool,
}

#[must_use]
pub fn analyze(rec: &Recovered) -> JobReport {
    let mut report = JobReport {
        status: None,
        applied: Vec::new(),
        failed: Vec::new(),
        without_outcome: Vec::new(),
        reboot_required: false,
    };
    let mut intents = Vec::new();
    for r in &rec.records {
        match r {
            Record::Intent(i) => intents.push(i.index),
            Record::Done(Done {
                index,
                outcome: Outcome::Applied,
            }) => report.applied.push(*index),
            Record::Done(Done {
                index,
                outcome: Outcome::AppliedRebootRequired,
            }) => {
                report.applied.push(*index);
                report.reboot_required = true;
            }
            Record::Done(Done {
                index,
                outcome: Outcome::Failed { code },
            }) => {
                report.failed.push((*index, *code));
            }
            Record::Commit(c) => report.status = Some(c.status),
            Record::Begin(_) => {}
        }
    }
    // Sets, not Vec::contains: recovery of 100k records has a 300 ms budget (§7.2).
    let decided: HashSet<u32> = report
        .applied
        .iter()
        .copied()
        .chain(report.failed.iter().map(|(i, _)| *i))
        .collect();
    report.without_outcome = intents
        .into_iter()
        .filter(|i| !decided.contains(i))
        .collect();
    report
}

/// The changes that undo a job: the inverses of applied changes, newest first.
/// `also_applied` lists interrupted intents that probing found applied. Expired
/// (`ReversibleUntil` before `now`) and irreversible changes are skipped. Each undo
/// change is itself undoable, so an undo can be undone: a captured inverse becomes
/// `Reversible` (its own inverse is captured when it runs), a `ReversibleVia` change
/// becomes `ReversibleVia` the original change.
#[must_use]
pub fn undo_changes(rec: &Recovered, also_applied: &[u32], now: UnixMillis) -> Vec<Change<String>> {
    let report = analyze(rec);
    let in_doubt: HashSet<u32> = report.without_outcome.iter().copied().collect();
    let mut order: Vec<u32> = report.applied;
    order.extend(
        also_applied
            .iter()
            .copied()
            .filter(|i| in_doubt.contains(i)),
    );
    let intents: HashMap<u32, &Intent> = rec
        .records
        .iter()
        .filter_map(|r| match r {
            Record::Intent(i) => Some((i.index, i)),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for index in order.into_iter().rev() {
        let Some(intent) = intents.get(&index) else {
            continue;
        };
        let undo = match &intent.change.reversibility {
            Reversibility::Irreversible { .. } => None,
            Reversibility::ReversibleVia { inverse } => Some((
                (**inverse).clone(),
                Reversibility::ReversibleVia {
                    inverse: Box::new(intent.change.kind.clone()),
                },
            )),
            Reversibility::ReversibleUntil { until } if *until < now => None,
            Reversibility::ReversibleUntil { .. } | Reversibility::Reversible => intent
                .inverse
                .clone()
                .map(|kind| (kind, Reversibility::Reversible)),
        };
        if let Some((kind, reversibility)) = undo {
            out.push(Change {
                kind,
                reversibility,
                reboot_required: intent.change.reboot_required,
            });
        }
    }
    out
}

/// A journal file and what it says (`journal list`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    pub path: PathBuf,
    pub begin: Begin,
    pub report: JobReport,
    /// Bytes after the last valid record were discarded (crash mid-write).
    pub torn: bool,
}

/// Every readable journal (`*.dpj`) in `dir`, oldest first. Files that aren't DISM Pro
/// journals are skipped; a missing folder is an empty list.
///
/// # Errors
/// The folder exists but can't be read.
pub fn list(dir: &Path) -> io::Result<Vec<JournalEntry>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path
            .extension()
            .is_none_or(|e| !e.eq_ignore_ascii_case("dpj"))
        {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if let Ok(rec) = read(&bytes) {
            out.push(JournalEntry {
                report: analyze(&rec),
                torn: rec.torn,
                begin: rec.begin,
                path,
            });
        }
    }
    out.sort_by_key(|e| e.begin.job);
    Ok(out)
}

/// The job that undid `job`, if any: an undo job naming it that applied at least one change.
#[must_use]
pub fn undone_by(entries: &[JournalEntry], job: JobId) -> Option<JobId> {
    entries
        .iter()
        .rev()
        .find(|e| e.begin.undoes == Some(job) && !e.report.applied.is_empty())
        .map(|e| e.begin.job)
}

#[cfg(test)]
mod tests {
    use dpro_core::{plan::RegData, units::Bytes};

    use super::*;

    fn begin() -> Begin {
        Begin {
            job: JobId::from_parts(UnixMillis(1_700_000_000_000), 42),
            target: TargetId([3; 16]),
            os_build: OsBuild {
                major: 10,
                minor: 0,
                build: 26100,
                revision: 2,
            },
            created: UnixMillis(1_700_000_000_000),
            op: "test".into(),
            undoes: None,
        }
    }

    fn reg_set(index: u32) -> Intent {
        Intent {
            index,
            change: Change {
                kind: ChangeKind::RegSet {
                    key: r"HKLM\SOFTWARE\X".into(),
                    value: format!("v{index}"),
                    new: RegData::Dword(1),
                },
                reversibility: Reversibility::Reversible,
                reboot_required: false,
            },
            inverse: Some(ChangeKind::RegDelete {
                key: r"HKLM\SOFTWARE\X".into(),
                value: Some(format!("v{index}")),
            }),
        }
    }

    fn write_job(intents: &[Intent], done: &[Done], status: Option<JobStatus>) -> Vec<u8> {
        let mut w = JournalWriter::create(Vec::new(), &begin()).unwrap();
        for i in intents {
            w.intent(i).unwrap();
        }
        w.flush().unwrap();
        for d in done {
            w.done(*d).unwrap();
        }
        if let Some(s) = status {
            w.commit(s).unwrap()
        } else {
            w.flush().unwrap();
            w.sink
        }
    }

    #[test]
    fn round_trip_and_report() {
        let intents = [reg_set(0), reg_set(1), reg_set(2)];
        let done = [
            Done {
                index: 0,
                outcome: Outcome::Applied,
            },
            Done {
                index: 1,
                outcome: Outcome::Failed { code: 5 },
            },
        ];
        let bytes = write_job(&intents, &done, Some(JobStatus::Cancelled));
        let rec = read(&bytes).unwrap();
        assert_eq!(rec.begin, begin());
        assert!(!rec.torn);
        assert_eq!(rec.valid_len, bytes.len());
        let report = analyze(&rec);
        assert_eq!(report.status, Some(JobStatus::Cancelled));
        assert_eq!(report.applied, [0]);
        assert_eq!(report.failed, [(1, 5)]);
        assert_eq!(report.without_outcome, [2]);
    }

    #[test]
    fn every_truncation_point_is_recoverable() {
        let intents = [reg_set(0), reg_set(1)];
        let done = [
            Done {
                index: 0,
                outcome: Outcome::Applied,
            },
            Done {
                index: 1,
                outcome: Outcome::Applied,
            },
        ];
        let bytes = write_job(&intents, &done, Some(JobStatus::Completed));
        let full = read(&bytes).unwrap();
        let begin_end = {
            let mut w = JournalWriter::create(Vec::new(), &begin()).unwrap();
            w.flush().unwrap();
            w.sink.len()
        };
        for cut in 0..bytes.len() {
            match read(&bytes[..cut]) {
                Ok(rec) => {
                    assert!(cut >= begin_end, "cut {cut}");
                    assert!(rec.valid_len <= cut);
                    assert_eq!(rec.torn, rec.valid_len < cut);
                    assert!(full.records.starts_with(&rec.records), "cut {cut}");
                }
                Err(_) => assert!(cut < begin_end, "cut {cut} lost a complete begin record"),
            }
        }
    }

    #[test]
    fn flipped_bytes_end_the_journal_without_panicking() {
        let bytes = write_job(
            &[reg_set(0)],
            &[Done {
                index: 0,
                outcome: Outcome::Applied,
            }],
            Some(JobStatus::Completed),
        );
        for i in FILE_HEADER_LEN..bytes.len() {
            let mut b = bytes.clone();
            b[i] ^= 0x40;
            // CRC-32C detects every single-bit error: a flip either loses the begin record
            // or ends the journal early, never yields a silently different record.
            if let Ok(rec) = read(&b) {
                assert!(rec.torn, "flip at {i} went undetected");
            }
        }
    }

    #[test]
    fn rejects_foreign_files() {
        assert!(matches!(
            read(b"PK\x03\x04...."),
            Err(JournalError::BadMagic)
        ));
        let mut bytes = write_job(&[], &[], Some(JobStatus::Completed));
        bytes[4] = 9;
        assert!(matches!(
            read(&bytes),
            Err(JournalError::UnsupportedSchema(9))
        ));
    }

    #[test]
    fn undo_reverses_applied_changes_newest_first_and_skips_the_rest() {
        let quarantine = Intent {
            index: 2,
            change: Change {
                kind: ChangeKind::FileQuarantine {
                    path: "a".into(),
                    bytes: Bytes(1),
                    unique_bytes: Bytes(1),
                },
                reversibility: Reversibility::ReversibleUntil {
                    until: UnixMillis(100),
                },
                reboot_required: false,
            },
            inverse: Some(ChangeKind::FileRestore {
                quarantined: "q/a".into(),
                path: "a".into(),
            }),
        };
        let intents = [reg_set(0), reg_set(1), quarantine];
        let done = [
            Done {
                index: 0,
                outcome: Outcome::Applied,
            },
            Done {
                index: 1,
                outcome: Outcome::Failed { code: 5 },
            },
            Done {
                index: 2,
                outcome: Outcome::Applied,
            },
        ];
        let rec = read(&write_job(&intents, &done, Some(JobStatus::Completed))).unwrap();

        let undo = undo_changes(&rec, &[], UnixMillis(50));
        let kinds: Vec<&str> = undo.iter().map(|c| c.kind.name()).collect();
        assert_eq!(kinds, ["file_restore", "reg_delete"]);

        // After the quarantine TTL only the registry change can be undone.
        let late = undo_changes(&rec, &[], UnixMillis(101));
        assert_eq!(late.len(), 1);
    }

    fn feature(enable: bool) -> ChangeKind<String> {
        ChangeKind::FeatureState {
            name: "TelnetClient".into(),
            enable,
            source: None,
        }
    }

    /// Records one applied change and returns the journal of the job.
    fn applied_job(change: Change<String>, outcome: Outcome) -> Recovered {
        let intent = Intent {
            index: 0,
            change,
            inverse: None,
        };
        let done = Done { index: 0, outcome };
        read(&write_job(&[intent], &[done], Some(JobStatus::Completed))).unwrap()
    }

    #[test]
    fn undo_of_an_undo_restores_the_original_change() {
        let original = Change {
            kind: feature(true),
            reversibility: Reversibility::ReversibleVia {
                inverse: Box::new(feature(false)),
            },
            reboot_required: false,
        };
        let undo = undo_changes(
            &applied_job(original.clone(), Outcome::Applied),
            &[],
            UnixMillis(0),
        );
        assert_eq!(undo.len(), 1);
        assert_eq!(undo[0].kind, feature(false));
        let redo = undo_changes(
            &applied_job(undo[0].clone(), Outcome::Applied),
            &[],
            UnixMillis(0),
        );
        assert_eq!(redo, [original]);
    }

    #[test]
    fn reboot_required_counts_as_applied() {
        let change = Change {
            kind: feature(true),
            reversibility: Reversibility::ReversibleVia {
                inverse: Box::new(feature(false)),
            },
            reboot_required: false,
        };
        let rec = applied_job(change, Outcome::AppliedRebootRequired);
        let report = analyze(&rec);
        assert_eq!(report.applied, [0]);
        assert!(report.reboot_required);
        assert_eq!(undo_changes(&rec, &[], UnixMillis(0)).len(), 1);
    }

    #[test]
    fn list_reads_journals_and_finds_undos() {
        let dir = std::env::temp_dir().join(format!("dpro-journals-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir); // ignore: may not exist
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(list(&dir.join("missing")).unwrap(), []);

        let first = begin();
        let mut undo = begin();
        undo.job = JobId::from_parts(UnixMillis(1_700_000_000_001), 1);
        undo.undoes = Some(first.job);
        for b in [&undo, &first] {
            let mut w = JournalWriter::create(Vec::new(), b).unwrap();
            w.intent(&reg_set(0)).unwrap();
            w.flush().unwrap();
            w.done(Done {
                index: 0,
                outcome: Outcome::Applied,
            })
            .unwrap();
            let bytes = w.commit(JobStatus::Completed).unwrap();
            std::fs::write(dir.join(format!("{}.dpj", b.job)), bytes).unwrap();
        }
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        std::fs::write(dir.join("broken.dpj"), b"DPJ1").unwrap();

        let entries = list(&dir).unwrap();
        let jobs: Vec<JobId> = entries.iter().map(|e| e.begin.job).collect();
        assert_eq!(
            jobs,
            [first.job, undo.job],
            "oldest first, foreign files skipped"
        );
        assert_eq!(undone_by(&entries, first.job), Some(undo.job));
        assert_eq!(undone_by(&entries, undo.job), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
