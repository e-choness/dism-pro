//! What is spoken without a focus move, and when (TD-0009 A4.4, A4.5; slice 7b).
//!
//! An [`Announcement`] is what A5's `Effect::Announce` carries: the bin resolves its text through
//! the catalog ([`Announcement::id`]) and sends it with `Ui::notify`. The [`Announcer`] decides
//! when a job's progress is worth a word. Time is the monotonic milliseconds the progress
//! message carries, so the type stays pure and testable.

use dpro_core::{id::JobId, progress::Progress};

/// Jobs whose progress is spoken at once; a ninth gets no slot, and only its toast is heard.
pub const SLOTS: usize = 8;
/// The least time between two progress notices of one job, and from its start to its first.
pub const JOB_GAP_MS: u64 = 2_000;
/// The least time between two progress notices of any jobs.
pub const ANY_GAP_MS: u64 = 1_000;

/// What A5's `Effect::Announce` carries (TD-0009 §5.1, A4.4's table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Announcement {
    /// `a11y-job-progress`: a job's new 10 % step, as its current percentage (10 to 90).
    Progress {
        job: JobId,
        /// The job's [`Announcer`] slot, 0 to 7: its notices' activity, so each job's newest
        /// replaces only its own.
        slot: u8,
        percent: u8,
    },
    /// `a11y-copied`, where nothing visible changes.
    Copied,
    /// `a11y-results`: the palette's result count, 1 or more.
    Results(u32),
    /// `a11y-filter-count`: the rows a table's filter shows, of all.
    FilterCount { shown: u32, total: u32 },
    /// `a11y-listed`: a listing the user asked for that took 1 s or more.
    Listed(u32),
    /// `a11y-hold-released`: a hold let go before its end.
    HoldReleased,
}

impl Announcement {
    /// Its message's ID in the catalog (`i18n/en-US/ui.ftl`).
    #[must_use]
    pub const fn id(&self) -> &'static str {
        match self {
            Self::Progress { .. } => "a11y-job-progress",
            Self::Copied => "a11y-copied",
            Self::Results(_) => "a11y-results",
            Self::FilterCount { .. } => "a11y-filter-count",
            Self::Listed(_) => "a11y-listed",
            Self::HoldReleased => "a11y-hold-released",
        }
    }
}

/// Decides which progress events are spoken (A4.4): a job's new 10 % step, at least
/// [`JOB_GAP_MS`] after its start or its last notice and [`ANY_GAP_MS`] after any; nothing for an
/// unknown total, at 0 or at 100 % (completion is the toast's).
#[derive(Clone, Copy, Debug, Default)]
pub struct Announcer {
    jobs: [Slot; SLOTS],
    last_any_ms: Option<u64>,
}

/// One job's place, while it runs.
#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    job: Option<JobId>,
    /// Its start, or its last notice.
    last_ms: u64,
    /// The step last spoken, or the lower one a new phase came back to.
    last_step: u8,
}

impl Announcer {
    /// No job yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            jobs: [Slot {
                job: None,
                last_ms: 0,
                last_step: 0,
            }; SLOTS],
            last_any_ms: None,
        }
    }

    /// `job` started at `now_ms`: it takes the first free slot, if there is one. A job already
    /// running starts over in its own slot.
    pub fn started(&mut self, job: JobId, now_ms: u64) {
        let slot = match self.slot_of(job) {
            Some(i) => Some(i),
            None => self.jobs.iter().position(|s| s.job.is_none()),
        };
        if let Some(s) = slot.and_then(|i| self.jobs.get_mut(i)) {
            *s = Slot {
                job: Some(job),
                last_ms: now_ms,
                last_step: 0,
            };
        }
    }

    /// What to say for `job`'s progress `p` at `now_ms`, if anything: its current step, as a
    /// percentage, when it's a new one past 0 and short of 100 and both gaps have passed. A lower
    /// step is a new phase: it's recorded, and nothing is said. A held-back step waits for the
    /// next event; nothing sets a timer for it.
    pub fn progress(&mut self, job: JobId, p: Progress, now_ms: u64) -> Option<Announcement> {
        let i = self.slot_of(job)?;
        let step = u8::try_from(p.permille()? / 100).unwrap_or(10);
        let last_any = self.last_any_ms;
        let s = self.jobs.get_mut(i)?;
        if step < s.last_step {
            s.last_step = step;
            return None;
        }
        let new = step > s.last_step && step < 10;
        let job_gap = now_ms.saturating_sub(s.last_ms) >= JOB_GAP_MS;
        let any_gap = last_any.is_none_or(|t| now_ms.saturating_sub(t) >= ANY_GAP_MS);
        if !(new && job_gap && any_gap) {
            return None;
        }
        s.last_step = step;
        s.last_ms = now_ms;
        self.last_any_ms = Some(now_ms);
        Some(Announcement::Progress {
            job,
            slot: u8::try_from(i).unwrap_or(u8::MAX),
            percent: step * 10,
        })
    }

    /// `job` ended: its slot is free. Its completion is the toast's.
    pub fn finished(&mut self, job: JobId) {
        if let Some(s) = self.slot_of(job).and_then(|i| self.jobs.get_mut(i)) {
            *s = Slot::default();
        }
    }

    /// The slot `job` runs in.
    fn slot_of(&self, job: JobId) -> Option<usize> {
        self.jobs.iter().position(|s| s.job == Some(job))
    }
}
