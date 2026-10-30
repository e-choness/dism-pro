//! Spike S-04 (roadmap 1.7): how fast can DISM Pro enumerate an NTFS tree, by which method, and
//! what can detect a stale plan (TD-0001 §12)? Throwaway measurement code, never shipped.
//!
//!   fsscan make-corpus <dir> [--files N] [--seed S] [--threads T]
//!   fsscan bench <dir> [--runs R] [--methods m,…] [--cold] [--threads T] [--corpus N --seed S]
//!   fsscan fingerprint <dir> [--paths N] [--changes K]
//!   fsscan usn-noise <dir> [--seconds S]
//!   fsscan probe <dir>
//!
//! Methods: find (`FindFirstFileExW` walk: the baseline every ratio divides by), find-mt,
//! dirinfo (`GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)` walk), dirinfo-mt,
//! usn (`FSCTL_ENUM_USN_DATA` over the volume), mft (raw `$MFT` read and parse).

#![cfg_attr(not(windows), allow(dead_code))]

mod corpus;
mod mft;
mod tally;
#[cfg(windows)]
mod win;

use std::fmt::Write as _;
use std::time::Duration;

use tally::Tally;

pub type Result<T> = std::result::Result<T, String>;

/// One measured scan.
#[derive(Debug, Default)]
pub struct Outcome {
    pub tally: Tally,
    /// Named parts of the elapsed time (e.g. MFT read vs parse).
    pub phases: Vec<(&'static str, Duration)>,
    pub notes: Vec<String>,
}

pub const METHODS: [&str; 6] = ["find", "find-mt", "dirinfo", "dirinfo-mt", "usn", "mft"];

#[derive(Debug)]
struct Args {
    command: String,
    dir: String,
    runs: usize,
    methods: Vec<String>,
    cold: bool,
    threads: usize,
    files: u64,
    seed: u64,
    corpus: Option<u64>,
    paths: usize,
    changes: usize,
    seconds: u64,
}

fn parse_args(raw: &[String]) -> Result<Args> {
    let [command, dir, rest @ ..] = raw else {
        return Err(
            "usage: fsscan <make-corpus|bench|fingerprint|usn-noise|probe> <dir> [options]".into(),
        );
    };
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let mut a = Args {
        command: command.clone(),
        dir: dir.clone(),
        runs: 5,
        methods: METHODS.iter().map(|m| (*m).to_owned()).collect(),
        cold: false,
        threads,
        files: 1_200_000,
        seed: 0x5045,
        corpus: None,
        paths: 100_000,
        changes: 100,
        seconds: 30,
    };
    let mut it = rest.iter();
    while let Some(flag) = it.next() {
        if flag == "--cold" {
            a.cold = true;
            continue;
        }
        let value = it.next().ok_or(format!("{flag} needs a value"))?;
        let number = || {
            value
                .parse::<u64>()
                .map_err(|e| format!("{flag} {value}: {e}"))
        };
        match flag.as_str() {
            "--runs" => a.runs = number()? as usize,
            "--threads" => a.threads = number()? as usize,
            "--files" => a.files = number()?,
            "--seed" => a.seed = number()?,
            "--corpus" => a.corpus = Some(number()?),
            "--paths" => a.paths = number()? as usize,
            "--changes" => a.changes = number()? as usize,
            "--seconds" => a.seconds = number()?,
            "--methods" => {
                a.methods = value.split(',').map(str::to_owned).collect();
                if let Some(bad) = a.methods.iter().find(|m| !METHODS.contains(&m.as_str())) {
                    return Err(format!(
                        "unknown method {bad}; known: {}",
                        METHODS.join(", ")
                    ));
                }
            }
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    Ok(a)
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&raw).and_then(|a| run(&a)) {
        Ok(report) => print!("{report}"),
        Err(e) => {
            eprintln!("fsscan: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
fn run(_: &Args) -> Result<String> {
    Err("fsscan measures NTFS and runs on Windows only".into())
}

#[cfg(windows)]
fn run(a: &Args) -> Result<String> {
    let dir = std::path::Path::new(&a.dir);
    match a.command.as_str() {
        "make-corpus" => {
            let plan = corpus::Plan::new(a.files, a.seed);
            let expected = plan.expected();
            let (dirs, files) = win::make_corpus(dir, &plan, a.threads)?;
            let names = expected.files;
            Ok(format!(
                "corpus {}: {} dirs in {:.1} s, {names} file names ({} files + {} hard links) in {:.1} s ({:.0} names/s, {} threads); {} logical, {} unique\n",
                a.dir,
                expected.dirs,
                dirs.as_secs_f64(),
                a.files,
                names - a.files,
                files.as_secs_f64(),
                names as f64 / files.as_secs_f64(),
                a.threads,
                gib(expected.bytes),
                gib(expected.unique_bytes),
            ))
        }
        "bench" => bench(a, dir),
        "fingerprint" => win::fingerprint(dir, a.paths, a.changes),
        "usn-noise" => win::usn_noise(dir, Duration::from_secs(a.seconds)),
        "probe" => win::probe(dir),
        other => Err(format!("unknown command {other}")),
    }
}

/// One method's runs in a bench.
#[derive(Debug, Default)]
struct Series {
    method: String,
    times: Vec<Duration>,
    phases: Vec<Vec<(&'static str, Duration)>>,
    last: Option<Outcome>,
    /// A method that fails (e.g. no volume access) is reported and dropped; the others go on.
    failed: Option<String>,
}

/// Interleaved runs (doc 04 §3: ratio gates take the median of interleaved runs in one job), each
/// checked against the corpus plan or, without one, against the `find` baseline.
#[cfg(windows)]
fn bench(a: &Args, dir: &std::path::Path) -> Result<String> {
    let device = win::volume_device(dir)?;
    let mut series: Vec<Series> = a
        .methods
        .iter()
        .map(|m| Series {
            method: m.clone(),
            ..Series::default()
        })
        .collect();
    if !a.cold {
        for s in &mut series {
            // Warm-up: caches loaded, not timed.
            if let Err(e) = win::scan(&s.method, dir, a.threads) {
                s.failed = Some(e);
            }
        }
    }
    let n = series.len();
    for run in 0..a.runs {
        for k in 0..n {
            let s = &mut series[(k + run) % n];
            if s.failed.is_some() {
                continue;
            }
            if a.cold {
                win::dismount(&device)?;
            }
            let start = std::time::Instant::now();
            match win::scan(&s.method, dir, a.threads) {
                Ok(outcome) => {
                    s.times.push(start.elapsed());
                    s.phases.push(outcome.phases.clone());
                    s.last = Some(outcome);
                }
                Err(e) => s.failed = Some(e),
            }
        }
    }
    let expected = a.corpus.map(|n| corpus::Plan::new(n, a.seed).expected());
    Ok(report(a, &series, expected))
}

fn report(a: &Args, series: &[Series], expected: Option<Tally>) -> String {
    let mode = if a.cold {
        "cold (volume dismounted before each run)"
    } else {
        "warm"
    };
    let mut out = format!(
        "### {} — {mode}, {} interleaved runs, {} threads for -mt\n\n",
        a.dir, a.runs, a.threads
    );
    let find = series
        .iter()
        .find(|s| s.method == "find" && !s.times.is_empty());
    let reference = expected.or_else(|| find.and_then(|s| s.last.as_ref().map(|o| o.tally)));
    if let Some(e) = expected {
        let _ = writeln!(
            out,
            "expected (corpus plan): {} files, {} dirs, {} logical, {} unique\n",
            e.files,
            e.dirs,
            gib(e.bytes),
            gib(e.unique_bytes)
        );
    }
    let base = find.map(|s| median(&s.times));
    out.push_str("| method | median ms | ÷ find | min | max | files | dirs | logical | unique | denied | check |\n");
    out.push_str("|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|---|\n");
    for s in series {
        let m = &s.method;
        if s.times.is_empty() {
            let _ = writeln!(out, "| {m} | failed | | | | | | | | | |");
            continue;
        }
        let med = median(&s.times);
        let ratio = base.map_or("-".into(), |b| {
            format!("{:.2}", med.as_secs_f64() / b.as_secs_f64())
        });
        let t = s.last.as_ref().map(|o| o.tally).unwrap_or_default();
        let _ = writeln!(
            out,
            "| {m} | {} | {ratio} | {} | {} | {} | {} | {} | {} | {} | {} |",
            ms(med),
            ms(s.times.iter().min().copied().unwrap_or_default()),
            ms(s.times.iter().max().copied().unwrap_or_default()),
            t.files,
            t.dirs,
            gib(t.bytes),
            gib(t.unique_bytes),
            t.denied,
            reference.map_or("-".into(), |r| check(&t, &r)),
        );
    }
    for s in series {
        let names: Vec<&str> = s
            .phases
            .first()
            .map(|p| p.iter().map(|(n, _)| *n).collect())
            .unwrap_or_default();
        let parts: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(k, n)| {
                let each: Vec<Duration> = s
                    .phases
                    .iter()
                    .filter_map(|p| p.get(k).map(|x| x.1))
                    .collect();
                format!("{n} {} ms", ms(median(&each)))
            })
            .collect();
        let mut notes = s
            .last
            .as_ref()
            .map(|o| o.notes.join("; "))
            .unwrap_or_default();
        if let Some(e) = &s.failed {
            let _ = write!(notes, " FAILED: {e}");
        }
        if !parts.is_empty() || !notes.is_empty() {
            let _ = writeln!(out, "\n{}: {} {notes}", s.method, parts.join(", "));
        }
    }
    out
}

/// "ok", or which totals differ from the reference.
fn check(t: &Tally, r: &Tally) -> String {
    let mut diffs = Vec::new();
    let mut cmp = |name: &str, a: u64, b: u64| {
        if a != b {
            diffs.push(format!(
                "{name} {a:+}",
                a = a.cast_signed() - b.cast_signed()
            ));
        }
    };
    cmp("files", t.files, r.files);
    cmp("dirs", t.dirs, r.dirs);
    cmp("chars", t.name_chars, r.name_chars);
    if let (Some(a), Some(b)) = (t.bytes, r.bytes) {
        cmp("bytes", a, b);
    }
    if let (Some(a), Some(b)) = (t.unique_bytes, r.unique_bytes) {
        cmp("unique", a, b);
    }
    if diffs.is_empty() {
        "ok".into()
    } else {
        diffs.join(", ")
    }
}

fn median(d: &[Duration]) -> Duration {
    let mut v = d.to_vec();
    v.sort();
    v.get(v.len() / 2).copied().unwrap_or_default()
}

fn ms(d: Duration) -> String {
    format!("{:.0}", d.as_secs_f64() * 1000.0)
}

fn gib(b: Option<u64>) -> String {
    b.map_or("-".into(), |b| {
        format!("{:.3} GiB", b as f64 / f64::from(1u32 << 30))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_names_each_difference() {
        let r = Tally {
            files: 10,
            dirs: 2,
            name_chars: 50,
            bytes: Some(100),
            unique_bytes: Some(90),
            denied: 0,
        };
        assert_eq!(check(&r, &r), "ok");
        let t = Tally {
            files: 9,
            bytes: None,
            ..r
        };
        assert_eq!(check(&t, &r), "files -1");
    }

    #[test]
    fn options_parse_and_unknown_methods_fail() {
        let raw: Vec<String> = [
            "bench",
            "S:\\c",
            "--runs",
            "3",
            "--methods",
            "find,mft",
            "--cold",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        let a = parse_args(&raw).unwrap();
        assert_eq!((a.runs, a.cold, a.methods.len()), (3, true, 2));
        let bad: Vec<String> = ["bench", "x", "--methods", "magic"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert!(parse_args(&bad).unwrap_err().contains("unknown method"));
    }
}
