//! Job runner: applies a plan with write-ahead journaling (TD-0001 §5.2).
//!
//! Per batch of up to `batch` changes (or `max_batch_age` of inverse capture):
//! 1. capture each change's inverse and buffer its intent,
//! 2. flush the journal (one sync): every intent in the batch is now durable,
//! 3. apply the batch's changes in order, buffering a done record for each,
//!    checking the cancel token after each change.
//!
//! Plan staleness (OS build, fingerprint) is checked by the caller before `run`.

use std::time::{Duration, Instant};

use dpro_core::{
    OsError,
    cancel::CancelToken,
    plan::{Change, ChangeKind, Reversibility},
};

use crate::journal::{Done, Intent, JobStatus, JournalError, JournalSink, JournalWriter, Outcome};

/// Applies changes to a real or fake target.
pub trait Backend {
    /// Reads whatever is needed to undo `change` (old registry value, quarantine
    /// destination, service config…). `Ok(None)`: nothing to undo (e.g. deleting a value
    /// that doesn't exist).
    ///
    /// # Errors
    /// The state can't be read; the change is then not applied.
    fn capture_inverse(&mut self, change: &Change<String>) -> Result<Option<ChangeKind<String>>, OsError>;

    /// Applies one change. Called only after its intent is durable.
    ///
    /// # Errors
    /// The change failed; it is recorded and the job continues with the next one.
    fn apply(&mut self, change: &Change<String>) -> Result<(), OsError>;
}

/// Monotonic clock, injectable for tests.
pub trait Clock {
    fn now(&self) -> Duration;
}

/// `Instant`-based clock.
#[derive(Debug, Clone, Copy)]
pub struct MonotonicClock(Instant);

impl MonotonicClock {
    #[must_use]
    pub fn new() -> Self {
        Self(Instant::now())
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MonotonicClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

/// Batching policy (TD-0001 §5.2: 256 intents or 50 ms).
#[derive(Debug, Clone, Copy)]
pub struct RunConfig {
    pub batch: usize,
    pub max_batch_age: Duration,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self { batch: 256, max_batch_age: Duration::from_millis(50) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobSummary {
    pub status: JobStatus,
    pub applied: u32,
    pub failed: u32,
    pub syncs: u64,
}

/// Runs `changes` against `backend`, journaling to `journal`, and commits the journal.
///
/// # Errors
/// Journal I/O errors. The job stops before the next change; changes already applied
/// stay journaled and undoable (the commit record is missing, so the job reads as
/// interrupted).
pub fn run<B: Backend, W: JournalSink>(
    changes: &[Change<String>],
    backend: &mut B,
    mut journal: JournalWriter<W>,
    cancel: &CancelToken,
    clock: &impl Clock,
    config: RunConfig,
) -> Result<(JobSummary, W), JournalError> {
    let batch = config.batch.max(1);
    let mut summary = JobSummary { status: JobStatus::Completed, applied: 0, failed: 0, syncs: 0 };
    let mut next = 0;
    // Per batch: whether inverse capture failed (then the change is not applied).
    let mut capture_failed: Vec<Option<i32>> = Vec::with_capacity(batch);

    'job: while next < changes.len() {
        if cancel.is_cancelled() {
            summary.status = JobStatus::Cancelled;
            break;
        }
        let start = next;
        let started_at = clock.now();
        capture_failed.clear();
        while next < changes.len()
            && next - start < batch
            && (next == start || clock.now().saturating_sub(started_at) < config.max_batch_age)
        {
            let change = &changes[next];
            let (inverse, failure) = match capture(backend, change) {
                Ok(inverse) => (inverse, None),
                Err(e) => (None, Some(e.code)),
            };
            journal.intent(&Intent { index: index(next), change: change.clone(), inverse })?;
            capture_failed.push(failure);
            next += 1;
        }

        // Write-ahead: nothing below runs until every intent of this batch is durable.
        journal.flush()?;

        for (offset, failure) in capture_failed.iter().enumerate() {
            let i = start + offset;
            let outcome = match failure {
                Some(code) => Outcome::Failed { code: *code },
                None => match backend.apply(&changes[i]) {
                    Ok(()) => Outcome::Applied,
                    Err(e) => Outcome::Failed { code: e.code },
                },
            };
            match outcome {
                Outcome::Applied => summary.applied += 1,
                Outcome::Failed { .. } => summary.failed += 1,
            }
            journal.done(Done { index: index(i), outcome })?;
            if cancel.is_cancelled() {
                summary.status = JobStatus::Cancelled;
                break 'job;
            }
        }
    }

    summary.syncs = journal.syncs() + 1; // + the commit sync
    let sink = journal.commit(summary.status)?;
    Ok((summary, sink))
}

fn capture<B: Backend>(backend: &mut B, change: &Change<String>) -> Result<Option<ChangeKind<String>>, OsError> {
    match &change.reversibility {
        // Nothing to capture: irreversible, or the plan already carries the inverse.
        Reversibility::Irreversible { .. } | Reversibility::ReversibleVia { .. } => Ok(None),
        Reversibility::Reversible | Reversibility::ReversibleUntil { .. } => backend.capture_inverse(change),
    }
}

fn index(i: usize) -> u32 {
    // Plans are far below 2^32 changes; saturate rather than wrap if one ever isn't.
    u32::try_from(i).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, collections::BTreeMap};

    use dpro_core::{
        id::{JobId, TargetId},
        plan::{OsBuild, RegData},
        units::UnixMillis,
    };
    use proptest::prelude::*;

    use super::*;
    use crate::journal::{self, Begin};

    /// In-memory registry: (key, value) → DWORD data.
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub(crate) struct FakeRegistry {
        pub(crate) values: BTreeMap<(String, String), u32>,
        pub(crate) fail_on: Option<String>,
    }

    impl Backend for FakeRegistry {
        fn capture_inverse(&mut self, change: &Change<String>) -> Result<Option<ChangeKind<String>>, OsError> {
            let (key, value) = match &change.kind {
                ChangeKind::RegSet { key, value, .. } => (key, value),
                ChangeKind::RegDelete { key, value: Some(value) } => (key, value),
                other => panic!("fake registry can't capture {}", other.name()),
            };
            let old = self.values.get(&(key.clone(), value.clone()));
            Ok(Some(match old {
                Some(data) => ChangeKind::RegSet { key: key.clone(), value: value.clone(), new: RegData::Dword(*data) },
                None => ChangeKind::RegDelete { key: key.clone(), value: Some(value.clone()) },
            }))
        }

        fn apply(&mut self, change: &Change<String>) -> Result<(), OsError> {
            match &change.kind {
                ChangeKind::RegSet { key, value, new: RegData::Dword(data) } => {
                    if self.fail_on.as_ref() == Some(value) {
                        return Err(OsError { code: 5, api: "RegSetValueExW" });
                    }
                    self.values.insert((key.clone(), value.clone()), *data);
                }
                ChangeKind::RegDelete { key, value: Some(value) } => {
                    self.values.remove(&(key.clone(), value.clone()));
                }
                other => panic!("fake registry can't apply {}", other.name()),
            }
            Ok(())
        }
    }

    /// Advances by `step` on every read.
    pub(crate) struct StepClock {
        pub(crate) t: Cell<Duration>,
        pub(crate) step: Duration,
    }

    impl Clock for StepClock {
        fn now(&self) -> Duration {
            let t = self.t.get();
            self.t.set(t + self.step);
            t
        }
    }

    pub(crate) fn begin() -> Begin {
        Begin {
            job: JobId::from_parts(UnixMillis(1), 1),
            target: TargetId([9; 16]),
            os_build: OsBuild { major: 10, minor: 0, build: 26100, revision: 0 },
            created: UnixMillis(1),
        }
    }

    fn set(value: &str, data: u32) -> Change<String> {
        Change {
            kind: ChangeKind::RegSet { key: "K".into(), value: value.into(), new: RegData::Dword(data) },
            reversibility: Reversibility::Reversible,
            reboot_required: false,
        }
    }

    fn run_all(changes: &[Change<String>], reg: &mut FakeRegistry, config: RunConfig) -> (JobSummary, Vec<u8>) {
        let journal = JournalWriter::create(Vec::new(), &begin()).unwrap();
        let clock = StepClock { t: Cell::new(Duration::ZERO), step: Duration::ZERO };
        run(changes, reg, journal, &CancelToken::new(), &clock, config).unwrap()
    }

    #[test]
    fn batches_bound_the_number_of_syncs() {
        let changes: Vec<_> = (0..1000).map(|i| set(&format!("v{i}"), i)).collect();
        let mut reg = FakeRegistry::default();
        let (summary, _) = run_all(&changes, &mut reg, RunConfig::default());
        // begin + ceil(1000 / 256) batches + commit
        assert_eq!(summary.syncs, 1 + 4 + 1);
        assert_eq!(summary.applied, 1000);
    }

    #[test]
    fn slow_capture_flushes_early() {
        let changes: Vec<_> = (0..10).map(|i| set(&format!("v{i}"), i)).collect();
        let journal = JournalWriter::create(Vec::new(), &begin()).unwrap();
        // Each clock read advances 30 ms: a batch holds at most 2 captures under 50 ms.
        let clock = StepClock { t: Cell::new(Duration::ZERO), step: Duration::from_millis(30) };
        let mut reg = FakeRegistry::default();
        let (summary, _) = run(&changes, &mut reg, journal, &CancelToken::new(), &clock, RunConfig::default()).unwrap();
        assert!(summary.syncs > 3, "{summary:?}");
        assert_eq!(summary.applied, 10);
    }

    #[test]
    fn failures_are_recorded_and_the_job_continues() {
        let changes = [set("a", 1), set("bad", 2), set("c", 3)];
        let mut reg = FakeRegistry { fail_on: Some("bad".into()), ..FakeRegistry::default() };
        let (summary, bytes) = run_all(&changes, &mut reg, RunConfig::default());
        assert_eq!((summary.applied, summary.failed), (2, 1));
        let report = journal::analyze(&journal::read(&bytes).unwrap());
        assert_eq!(report.failed, [(1, 5)]);
        assert_eq!(report.status, Some(JobStatus::Completed));
    }

    #[test]
    fn cancel_stops_after_the_current_change() {
        struct CancelAfterFirst<'a>(FakeRegistry, &'a CancelToken);
        impl Backend for CancelAfterFirst<'_> {
            fn capture_inverse(&mut self, c: &Change<String>) -> Result<Option<ChangeKind<String>>, OsError> {
                self.0.capture_inverse(c)
            }
            fn apply(&mut self, c: &Change<String>) -> Result<(), OsError> {
                self.1.cancel();
                self.0.apply(c)
            }
        }
        let cancel = CancelToken::new();
        let mut backend = CancelAfterFirst(FakeRegistry::default(), &cancel);
        let changes = [set("a", 1), set("b", 2)];
        let journal = JournalWriter::create(Vec::new(), &begin()).unwrap();
        let clock = StepClock { t: Cell::new(Duration::ZERO), step: Duration::ZERO };
        let (summary, bytes) = run(&changes, &mut backend, journal, &cancel, &clock, RunConfig::default()).unwrap();
        assert_eq!(summary.status, JobStatus::Cancelled);
        assert_eq!(summary.applied, 1);
        let report = journal::analyze(&journal::read(&bytes).unwrap());
        assert_eq!(report.without_outcome, [1]); // committed, so: never applied
    }

    fn arb_changes() -> impl Strategy<Value = Vec<Change<String>>> {
        let one = (0_u8..6, any::<u32>(), any::<bool>()).prop_map(|(v, data, delete)| {
            let value = format!("v{v}");
            Change {
                kind: if delete {
                    ChangeKind::RegDelete { key: "K".into(), value: Some(value) }
                } else {
                    ChangeKind::RegSet { key: "K".into(), value, new: RegData::Dword(data) }
                },
                reversibility: Reversibility::Reversible,
                reboot_required: false,
            }
        });
        proptest::collection::vec(one, 0..60)
    }

    fn arb_registry() -> impl Strategy<Value = FakeRegistry> {
        proptest::collection::btree_map((0_u8..6).prop_map(|v| ("K".to_owned(), format!("v{v}"))), any::<u32>(), 0..6)
            .prop_map(|values| FakeRegistry { values, fail_on: None })
    }

    proptest! {
        /// TD-0001 §10: apply → undo ⇒ state equal.
        #[test]
        fn apply_then_undo_restores_the_state(
            initial in arb_registry(),
            changes in arb_changes(),
            batch in 1_usize..10,
        ) {
            let mut reg = initial.clone();
            let config = RunConfig { batch, max_batch_age: Duration::from_secs(1) };
            let (_, bytes) = run_all(&changes, &mut reg, config);
            let undo = journal::undo_changes(&journal::read(&bytes).unwrap(), &[], UnixMillis(0));
            run_all(&undo, &mut reg, config);
            prop_assert_eq!(reg, initial);
        }

        /// Crash at any byte: every change the backend applied has a durable intent with
        /// its inverse, so undo of the recovered journal still restores the state.
        #[test]
        fn crash_anywhere_keeps_applied_changes_undoable(
            initial in arb_registry(),
            changes in arb_changes(),
            batch in 1_usize..10,
            cut_permille in 0_u32..=1000,
        ) {
            // The journal is written into a shared buffer so the backend can see its length.
            struct Shared(std::rc::Rc<std::cell::RefCell<Vec<u8>>>);
            impl JournalSink for Shared {
                fn append(&mut self, b: &[u8]) -> std::io::Result<()> { self.0.borrow_mut().extend_from_slice(b); Ok(()) }
                fn sync(&mut self) -> std::io::Result<()> { Ok(()) }
            }
            // Records the journal length at every apply: that prefix was synced before it.
            struct Recording<'a> { reg: FakeRegistry, sink_len: &'a dyn Fn() -> usize, applied_at: Vec<(usize, usize)> }
            impl Backend for Recording<'_> {
                fn capture_inverse(&mut self, c: &Change<String>) -> Result<Option<ChangeKind<String>>, OsError> {
                    self.reg.capture_inverse(c)
                }
                fn apply(&mut self, c: &Change<String>) -> Result<(), OsError> {
                    self.applied_at.push((self.applied_at.len(), (self.sink_len)()));
                    self.reg.apply(c)
                }
            }
            let shared = std::rc::Rc::new(std::cell::RefCell::new(Vec::<u8>::new()));
            let len_view = { let s = shared.clone(); move || s.borrow().len() };
            let mut backend = Recording { reg: initial.clone(), sink_len: &len_view, applied_at: Vec::new() };
            let journal = JournalWriter::create(Shared(shared.clone()), &begin()).unwrap();
            let clock = StepClock { t: Cell::new(Duration::ZERO), step: Duration::ZERO };
            let config = RunConfig { batch, max_batch_age: Duration::from_secs(1) };
            run(&changes, &mut backend, journal, &CancelToken::new(), &clock, config).unwrap();

            let full = shared.borrow().clone();
            let cut = full.len() * usize::try_from(cut_permille).unwrap() / 1000;
            // Simulate the crash: the target keeps only changes applied before the cut.
            let mut crashed = initial.clone();
            let mut applied_before_cut = Vec::new();
            for (i, synced_len) in &backend.applied_at {
                if *synced_len <= cut {
                    // Write-ahead: the intent was already in the synced prefix.
                    applied_before_cut.push(u32::try_from(*i).unwrap());
                    crashed.apply(&changes[*i]).unwrap();
                }
            }
            let Ok(rec) = journal::read(&full[..cut]) else {
                // Not even the begin record survived: then nothing can have been applied.
                prop_assert!(applied_before_cut.is_empty());
                return Ok(());
            };
            let report = journal::analyze(&rec);
            // Probing would find exactly the in-doubt changes that really were applied.
            let probed: Vec<u32> = report.without_outcome.iter().copied().filter(|i| applied_before_cut.contains(i)).collect();
            let undo = journal::undo_changes(&rec, &probed, UnixMillis(0));
            run_all(&undo, &mut crashed, config);
            prop_assert_eq!(crashed, initial);
        }
    }
}
