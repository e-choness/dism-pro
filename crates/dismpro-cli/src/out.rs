//! Output helpers shared by all commands: JSON, errors, job summaries, confirmation.

use std::{
    io::{self, BufRead, IsTerminal, Write},
    path::Path,
};

use dpro_cli_fmt::{exit::ExitCode, messages::Messages};
use dpro_engine::journal::{self, JobStatus, Recovered};
use serde::Serialize;

use crate::{
    error::CliError,
    json::{ErrorDoc, FailedJson, JobDoc, OsJson, hresult},
};

/// Prints one JSON document to stdout.
///
/// # Errors
/// stdout is closed.
pub(crate) fn print_json<T: Serialize>(doc: &T) -> Result<(), CliError> {
    let text = serde_json::to_string_pretty(doc).map_err(|e| CliError::Journal(e.to_string()))?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{text}")?;
    Ok(())
}

/// Columns available for tables: `COLUMNS` if set, else 120.
pub(crate) fn width() -> usize {
    std::env::var("COLUMNS").ok().and_then(|c| c.parse().ok()).unwrap_or(120)
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
            os: e.os().map(|os| OsJson { code: hresult(os.code), api: os.api }),
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

pub(crate) fn job_doc(rec: &Recovered, path: &Path) -> JobDoc {
    let report = journal::analyze(rec);
    JobDoc {
        schema: "job.v1",
        job: rec.begin.job.to_string(),
        target: rec.begin.target.to_string(),
        status: match report.status {
            Some(JobStatus::Completed) => "completed",
            Some(JobStatus::Cancelled) => "cancelled",
            Some(JobStatus::Aborted) => "aborted",
            None => "interrupted",
        },
        applied: report.applied,
        failed: report.failed.into_iter().map(|(index, code)| FailedJson { index, code: hresult(code) }).collect(),
        without_outcome: report.without_outcome,
        journal: path.display().to_string(),
    }
}

pub(crate) fn print_job(doc: &JobDoc, m: &Messages) {
    let count = doc.applied.len() + doc.failed.len() + doc.without_outcome.len();
    println!("{} ({}, {})", m.get("cli-applied", &[("applied", &doc.applied.len()), ("count", &count)]), doc.status, doc.job);
    if !doc.failed.is_empty() {
        println!("{}", m.get("cli-job-partial", &[("failed", &doc.failed.len())]));
        for f in &doc.failed {
            println!("  #{} {}", f.index, f.code);
        }
    }
    println!("{}", doc.journal);
}

/// Exit code for a finished job (TD-0002 §5.2).
pub(crate) fn job_exit(status: JobStatus, applied: u32, failed: u32) -> ExitCode {
    match status {
        JobStatus::Cancelled => ExitCode::Cancelled,
        JobStatus::Aborted => ExitCode::Failed,
        JobStatus::Completed if failed == 0 => ExitCode::Ok,
        JobStatus::Completed if applied > 0 => ExitCode::Partial,
        JobStatus::Completed => ExitCode::Failed,
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
    pub(crate) dry_run: bool,
    pub(crate) yes: bool,
    pub(crate) allow_irreversible: bool,
    pub(crate) interactive: bool,
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
    if c.dry_run || count == 0 {
        return Decision::Stop(ExitCode::Ok);
    }
    if c.yes {
        return if irreversible > 0 && !c.allow_irreversible {
            Decision::Stop(ExitCode::ConfirmationRequired)
        } else {
            Decision::Apply
        };
    }
    if !c.interactive {
        return Decision::Stop(ExitCode::ConfirmationRequired);
    }
    let (prompt, expected) = if irreversible > 0 {
        (m.get("cli-confirm-irreversible", &[("irreversible", &irreversible), ("count", &count)]), count.to_string())
    } else {
        (m.get("cli-confirm-apply", &[("count", &count)]), "y".to_owned())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn d(dry: bool, yes: bool, allow: bool, tty: bool, irreversible: u64, answer: Option<&str>) -> Decision {
        let m = Messages::en_us();
        let c = Confirm { dry_run: dry, yes, allow_irreversible: allow, interactive: tty };
        decide(c, 5, irreversible, |_| answer.map(str::to_owned), &m)
    }

    #[test]
    fn confirmation_rules() {
        use Decision::{Apply, Stop};
        assert_eq!(d(true, false, false, true, 0, Some("y")), Stop(ExitCode::Ok));
        assert_eq!(d(false, true, false, false, 0, None), Apply);
        assert_eq!(d(false, true, false, false, 1, None), Stop(ExitCode::ConfirmationRequired));
        assert_eq!(d(false, true, true, false, 1, None), Apply);
        assert_eq!(d(false, false, false, false, 0, None), Stop(ExitCode::ConfirmationRequired));
        assert_eq!(d(false, false, false, true, 0, Some("Y\n")), Apply);
        assert_eq!(d(false, false, false, true, 0, Some("n")), Stop(ExitCode::ConfirmationRequired));
        // Irreversible: typing "y" isn't enough, the count is required.
        assert_eq!(d(false, false, false, true, 2, Some("y")), Stop(ExitCode::ConfirmationRequired));
        assert_eq!(d(false, false, false, true, 2, Some("5")), Apply);
    }

    #[test]
    fn exit_codes_for_jobs() {
        assert_eq!(job_exit(JobStatus::Completed, 3, 0), ExitCode::Ok);
        assert_eq!(job_exit(JobStatus::Completed, 2, 1), ExitCode::Partial);
        assert_eq!(job_exit(JobStatus::Completed, 0, 1), ExitCode::Failed);
        assert_eq!(job_exit(JobStatus::Cancelled, 1, 0), ExitCode::Cancelled);
    }
}
