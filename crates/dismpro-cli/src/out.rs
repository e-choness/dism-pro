//! Output helpers shared by all commands: JSON, errors, job summaries, confirmation, progress.

use std::{
    collections::HashMap,
    io::{self, BufRead, IsTerminal, Write},
    path::Path,
    sync::atomic::{AtomicU32, Ordering},
};

use dpro_cli_fmt::{
    exit::ExitCode,
    messages::Messages,
    plan::{action_id, subject},
    units::rfc3339,
};
use dpro_core::progress::Progress;
use dpro_engine::journal::{self, JobReport, JobStatus, JournalEntry, Record, Recovered};
use serde::Serialize;

use crate::{
    args::{ApplyMode, Global},
    error::CliError,
    json::{ErrorDoc, FailedJson, JobDoc, JournalJson, OsJson, hresult},
};

/// Prints one JSON document as one line on stdout (JSON Lines): a command that prints a plan
/// and then a job report emits two parseable lines, told apart by their `schema`.
///
/// # Errors
/// stdout is closed.
pub(crate) fn print_json<T: Serialize>(doc: &T) -> Result<(), CliError> {
    let text = serde_json::to_string(doc).map_err(|e| CliError::Failed(e.to_string()))?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{text}")?;
    Ok(())
}

/// Columns available for tables: `COLUMNS` if set, else 120.
pub(crate) fn width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(120)
}

pub(crate) fn report_error(e: &CliError, json: bool, m: &Messages) -> std::process::ExitCode {
    let code = e.exit_code();
    let detail = e.detail();
    let message = m.get(e.diagnosis(), &[("message", &detail), ("what", &detail)]);
    if json {
        let doc = ErrorDoc {
            schema: "error.v1",
            exit_code: code.code(),
            diagnosis: e.diagnosis(),
            message: message.clone(),
            os: e.os().map(|os| OsJson {
                code: hresult(os.code),
                api: os.api,
            }),
        };
        // ignore: if stdout is gone too there's nowhere left to report.
        let _ = print_json(&doc);
    }
    let line = m.get("cli-error", &[("message", &message)]);
    match e.os() {
        Some(os) => eprintln!("{line} ({} {})", os.api, hresult(os.code)),
        None => eprintln!("{line}"),
    }
    std::process::ExitCode::from(code.code())
}

/// Stable status name (JSON) and its Fluent id is `job-status-<name>`.
pub(crate) fn status_name(status: Option<JobStatus>) -> &'static str {
    match status {
        Some(JobStatus::Completed) => "completed",
        Some(JobStatus::Cancelled) => "cancelled",
        Some(JobStatus::Aborted) => "aborted",
        None => "interrupted",
    }
}

pub(crate) fn job_doc(rec: &Recovered, path: &Path) -> JobDoc {
    let report = journal::analyze(rec);
    JobDoc {
        schema: "job.v1",
        job: rec.begin.job.to_string(),
        target: rec.begin.target.to_string(),
        op: rec.begin.op.clone(),
        created: rfc3339(rec.begin.created),
        undoes: rec.begin.undoes.map(|j| j.to_string()),
        status: status_name(report.status),
        reboot_required: report.reboot_required,
        applied: report.applied,
        failed: report
            .failed
            .into_iter()
            .map(|(index, code)| FailedJson {
                index,
                code: hresult(code),
            })
            .collect(),
        without_outcome: report.without_outcome,
        journal: path.display().to_string(),
    }
}

/// Human job report: totals, each failed change, the restart notice and the journal path.
pub(crate) fn print_job(rec: &Recovered, path: &Path, m: &Messages) {
    let report = journal::analyze(rec);
    let count = report.applied.len() + report.failed.len() + report.without_outcome.len();
    println!(
        "{} {} ({})",
        m.text(&format!("job-status-{}", status_name(report.status))),
        m.get(
            "cli-applied",
            &[("applied", &report.applied.len()), ("count", &count)]
        ),
        rec.begin.job
    );
    if !report.failed.is_empty() {
        println!(
            "{}",
            m.get("cli-job-partial", &[("failed", &report.failed.len())])
        );
        let changes: HashMap<u32, _> = rec
            .records
            .iter()
            .filter_map(|r| match r {
                Record::Intent(i) => Some((i.index, &i.change.kind)),
                _ => None,
            })
            .collect();
        for (index, code) in &report.failed {
            let (action, object) = changes.get(index).map_or_else(
                || (format!("#{index}"), String::new()),
                |k| (m.text(&action_id(k)), subject(k)),
            );
            println!(
                "  {}",
                m.get(
                    "cli-job-failed-change",
                    &[
                        ("action", &action),
                        ("object", &object),
                        ("code", &hresult(*code))
                    ]
                )
            );
        }
    }
    if report.reboot_required {
        println!("{}", m.text("cli-reboot-required"));
    }
    println!(
        "{}",
        m.get("cli-journal-file", &[("path", &path.display())])
    );
}

/// `journal list` rows, oldest first, each with the job that undid it.
pub(crate) fn journal_rows(entries: &[JournalEntry]) -> Vec<JournalJson> {
    entries
        .iter()
        .map(|e| {
            let JobReport {
                status,
                applied,
                failed,
                reboot_required,
                ..
            } = &e.report;
            JournalJson {
                job: e.begin.job.to_string(),
                op: e.begin.op.clone(),
                created: rfc3339(e.begin.created),
                status: status_name(*status),
                applied: applied.len(),
                failed: failed.len(),
                reboot_required: *reboot_required,
                undoes: e.begin.undoes.map(|j| j.to_string()),
                undone_by: journal::undone_by(entries, e.begin.job).map(|j| j.to_string()),
                journal: e.path.display().to_string(),
            }
        })
        .collect()
}

/// Exit code for a finished job (TD-0002 §5.2).
pub(crate) fn job_exit(status: JobStatus, applied: u32, failed: u32) -> ExitCode {
    match status {
        JobStatus::Cancelled => ExitCode::Cancelled,
        JobStatus::Completed if failed == 0 => ExitCode::Ok,
        JobStatus::Completed if applied > 0 => ExitCode::Partial,
        // Nothing applied, or stopped early by an error.
        JobStatus::Completed | JobStatus::Aborted => ExitCode::Failed,
    }
}

/// What to do after printing a plan (TD-0002 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Apply,
    Stop(ExitCode),
}

/// The confirmation inputs: flags plus whether stdin is a console.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Confirm {
    pub(crate) mode: ApplyMode,
    pub(crate) allow_irreversible: bool,
    pub(crate) interactive: bool,
}

impl Confirm {
    /// From the global flags; asking needs a console, and `--json` output never prompts.
    pub(crate) fn new(g: &Global) -> Self {
        Self {
            mode: g.mode,
            allow_irreversible: g.allow_irreversible,
            interactive: !g.json && stdin_is_console(),
        }
    }
}

/// `--dry-run` → stop (0); `--yes` → apply unless irreversible without `--allow-irreversible`;
/// a console → ask; otherwise refuse (exit 3) instead of blocking on stdin.
pub(crate) fn decide(
    c: Confirm,
    count: u64,
    irreversible: u64,
    answer: impl FnOnce(&str) -> Option<String>,
    m: &Messages,
) -> Decision {
    if count == 0 {
        return Decision::Stop(ExitCode::Ok);
    }
    match c.mode {
        ApplyMode::DryRun => return Decision::Stop(ExitCode::Ok),
        ApplyMode::Yes if irreversible > 0 && !c.allow_irreversible => {
            return Decision::Stop(ExitCode::ConfirmationRequired);
        }
        ApplyMode::Yes => return Decision::Apply,
        ApplyMode::Ask if !c.interactive => return Decision::Stop(ExitCode::ConfirmationRequired),
        ApplyMode::Ask => {}
    }
    let (prompt, expected) = if irreversible > 0 {
        (
            m.get(
                "cli-confirm-irreversible",
                &[("irreversible", &irreversible), ("count", &count)],
            ),
            count.to_string(),
        )
    } else {
        (
            m.get("cli-confirm-apply", &[("count", &count)]),
            "y".to_owned(),
        )
    };
    match answer(&prompt) {
        Some(a) if a.trim().eq_ignore_ascii_case(&expected) => Decision::Apply,
        _ => Decision::Stop(ExitCode::ConfirmationRequired),
    }
}

/// Asks on stderr, reads one line from stdin.
pub(crate) fn ask(prompt: &str) -> Option<String> {
    eprint!("{prompt} ");
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).ok()?;
    Some(line)
}

pub(crate) fn stdin_is_console() -> bool {
    io::stdin().is_terminal()
}

/// A one-line percentage on stderr for long DISM calls (`[2/5] 40%`). Silent unless stderr is
/// a console and the output isn't JSON. DISM calls [`ProgressLine::report`] from its own
/// thread, hence the atomics.
#[derive(Debug)]
pub(crate) struct ProgressLine<'m> {
    enabled: bool,
    m: &'m Messages,
    /// `step << 16 | total` of the change in flight; 0 for a single call.
    step: AtomicU32,
    /// Last percentage printed + 1; 0 = nothing on the line.
    shown: AtomicU32,
}

impl<'m> ProgressLine<'m> {
    pub(crate) fn new(json: bool, m: &'m Messages) -> Self {
        Self {
            enabled: !json && io::stderr().is_terminal(),
            m,
            step: AtomicU32::new(0),
            shown: AtomicU32::new(0),
        }
    }

    /// The next change of a job starts (1-based).
    pub(crate) fn step(&self, step: usize, total: usize) {
        let pack = |n: usize| u32::try_from(n.min(0xFFFF)).unwrap_or(0xFFFF);
        self.step
            .store(pack(step) << 16 | pack(total), Ordering::Relaxed);
        self.shown.store(0, Ordering::Relaxed);
    }

    pub(crate) fn report(&self, p: Progress) {
        let Some(permille) = p.permille() else {
            return;
        };
        let percent = u32::from(permille / 10);
        if !self.enabled || self.shown.swap(percent + 1, Ordering::Relaxed) == percent + 1 {
            return;
        }
        let step = self.step.load(Ordering::Relaxed);
        let text = if step == 0 {
            self.m.get("cli-progress", &[("percent", &percent)])
        } else {
            self.m.get(
                "cli-progress-step",
                &[
                    ("step", &(step >> 16)),
                    ("total", &(step & 0xFFFF)),
                    ("percent", &percent),
                ],
            )
        };
        let mut err = io::stderr().lock();
        // ignore: progress is cosmetic.
        let _ = write!(err, "\r{text}  ");
        let _ = err.flush();
    }

    /// Clears the line so the next output starts at column 0.
    pub(crate) fn finish(&self) {
        if self.enabled && self.shown.swap(0, Ordering::Relaxed) != 0 {
            eprint!("\r{:24}\r", "");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(
        mode: ApplyMode,
        allow: bool,
        tty: bool,
        irreversible: u64,
        answer: Option<&str>,
    ) -> Decision {
        let m = Messages::en_us();
        let c = Confirm {
            mode,
            allow_irreversible: allow,
            interactive: tty,
        };
        decide(c, 5, irreversible, |_| answer.map(str::to_owned), &m)
    }

    #[test]
    fn confirmation_rules() {
        use ApplyMode::{Ask, DryRun, Yes};
        use Decision::{Apply, Stop};
        let refused = Stop(ExitCode::ConfirmationRequired);
        assert_eq!(d(DryRun, false, true, 0, Some("y")), Stop(ExitCode::Ok));
        assert_eq!(d(Yes, false, false, 0, None), Apply);
        assert_eq!(d(Yes, false, false, 1, None), refused);
        assert_eq!(d(Yes, true, false, 1, None), Apply);
        assert_eq!(d(Ask, false, false, 0, None), refused);
        assert_eq!(d(Ask, false, true, 0, Some("Y\n")), Apply);
        assert_eq!(d(Ask, false, true, 0, Some("n")), refused);
        assert_eq!(d(Ask, false, true, 0, None), refused);
        // Irreversible: typing "y" isn't enough, the count is required.
        assert_eq!(d(Ask, false, true, 2, Some("y")), refused);
        assert_eq!(d(Ask, false, true, 2, Some("5")), Apply);
    }

    #[test]
    fn empty_plans_never_ask() {
        let c = Confirm {
            mode: ApplyMode::Ask,
            allow_irreversible: false,
            interactive: true,
        };
        let asked = decide(c, 0, 0, |_| panic!("asked"), &Messages::en_us());
        assert_eq!(asked, Decision::Stop(ExitCode::Ok));
    }

    #[test]
    fn exit_codes_for_jobs() {
        assert_eq!(job_exit(JobStatus::Completed, 3, 0), ExitCode::Ok);
        assert_eq!(job_exit(JobStatus::Completed, 2, 1), ExitCode::Partial);
        assert_eq!(job_exit(JobStatus::Completed, 0, 1), ExitCode::Failed);
        assert_eq!(job_exit(JobStatus::Aborted, 1, 0), ExitCode::Failed);
        assert_eq!(job_exit(JobStatus::Cancelled, 1, 0), ExitCode::Cancelled);
    }

    #[test]
    fn journal_rows_link_undos() {
        use dpro_core::{
            id::{JobId, TargetId},
            plan::OsBuild,
            units::UnixMillis,
        };
        use dpro_engine::journal::Begin;

        let entry = |random: u128, undoes: Option<JobId>| JournalEntry {
            path: format!("{random}.dpj").into(),
            begin: Begin {
                job: JobId::from_parts(UnixMillis(1), random),
                target: TargetId([0; 16]),
                os_build: OsBuild {
                    major: 10,
                    minor: 0,
                    build: 1,
                    revision: 0,
                },
                created: UnixMillis(0),
                op: "features.enable".into(),
                undoes,
            },
            report: JobReport {
                status: Some(JobStatus::Completed),
                applied: vec![0],
                failed: Vec::new(),
                without_outcome: Vec::new(),
                reboot_required: false,
            },
            torn: false,
        };
        let first = entry(1, None);
        let undo = entry(2, Some(first.begin.job));
        let rows = journal_rows(&[first.clone(), undo.clone()]);
        assert_eq!(rows[0].undone_by, Some(undo.begin.job.to_string()));
        assert_eq!(rows[1].undoes, Some(first.begin.job.to_string()));
        assert_eq!(rows[1].undone_by, None);
        assert_eq!(rows[0].created, "1970-01-01T00:00:00Z");
        assert_eq!(rows[0].status, "completed");
    }
}
