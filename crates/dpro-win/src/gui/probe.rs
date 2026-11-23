//! G1's measurements (TD-0009 §5.5, §7 U4, U6, U7): `--ui-probe first-frame|idle` with
//! `--probe-out <file>`. Every probe target (the raw-API `bare_window` example and the toolkit's
//! windows) parses the same arguments with [`Args::parse`] and reports the same way, so G1's
//! ratios compare like with like.
//!
//! - **first-frame** (U4): from the process's creation time (`GetProcessTimes`) to the end of its
//!   window's first `WM_PAINT`, with the frame on screen. Writes `first_frame_us=<n>` and exits, so
//!   teardown is never measured.
//! - **idle** (U6, U7): from the first frame, 3 s to settle, then 5 s untouched. Writes the CPU time
//!   the process used, the messages its loop took (with the timers and paints among them), and its
//!   working set and private bytes at the end, then exits. A message another process *sends* is
//!   handled inside `GetMessageW` without being taken, so only the CPU time shows it.
//!
//! Both reports end with the presenter that drew the frames: `composed=1` for `DirectComposition`
//! (`warp=1` on WARP), `composed=0` for GDI. G1 checks it, so a glass run that fell back to GDI
//! can't pass as a measurement of the composition path.
//!
//! A probe target calls [`arm`] before its window exists, [`message`] for every message its loop
//! takes, and [`painted`] after every `WM_PAINT`; all three cost an atomic or two when unarmed. Its
//! window calls [`presenter`] once it knows how it presents.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::FILETIME,
    System::{
        ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        SystemInformation::GetSystemTimePreciseAsFileTime,
        Threading::{GetCurrentProcess, GetProcessTimes},
    },
    UI::WindowsAndMessaging::{WM_PAINT, WM_TIMER},
};

/// How long the idle probe lets the window settle, then how long it watches.
const SETTLE: Duration = Duration::from_secs(3);
const WATCH: Duration = Duration::from_secs(5);

/// What `--ui-probe` measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    FirstFrame,
    Idle,
}

/// `--materials`: the materials a window shows, forced for tests (TD-0009 §6). `bare_window` has
/// none and ignores it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ForceMaterials {
    /// As the user's preference and Windows' settings say.
    #[default]
    Auto,
    Solid,
    /// Glass wherever the presenter can compose, on WARP too.
    Glass,
}

/// A probe target's arguments: `[--size WxH] [--materials auto|solid|glass]
/// [--ui-probe first-frame|idle --probe-out <file>]`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// The client area at 100 % scale, if given.
    pub size: Option<(u32, u32)>,
    pub materials: ForceMaterials,
    pub probe: Option<(Kind, PathBuf)>,
}

impl Args {
    /// Parses the arguments after the program name. The error names the argument at fault.
    ///
    /// # Errors
    /// An unknown argument, a missing or malformed value, or `--ui-probe` without `--probe-out`.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut parsed = Self::default();
        let (mut kind, mut out) = (None, None);
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let arg = arg.to_string_lossy().into_owned();
            let mut value = || {
                args.next()
                    .ok_or_else(|| format!("{arg} needs a value"))
                    .map(|v| v.to_string_lossy().into_owned())
            };
            match arg.as_str() {
                "--size" => {
                    let v = value()?;
                    let size = v
                        .split_once('x')
                        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                        .filter(|&(w, h)| w > 0 && h > 0);
                    parsed.size = Some(size.ok_or_else(|| format!("--size {v}: expected WxH"))?);
                }
                "--materials" => {
                    parsed.materials = match value()?.as_str() {
                        "auto" => ForceMaterials::Auto,
                        "solid" => ForceMaterials::Solid,
                        "glass" => ForceMaterials::Glass,
                        other => {
                            return Err(format!(
                                "--materials {other}: expected auto, solid or glass"
                            ));
                        }
                    };
                }
                "--ui-probe" => {
                    kind = Some(match value()?.as_str() {
                        "first-frame" => Kind::FirstFrame,
                        "idle" => Kind::Idle,
                        other => {
                            return Err(format!(
                                "--ui-probe {other}: expected first-frame or idle"
                            ));
                        }
                    });
                }
                "--probe-out" => out = Some(PathBuf::from(value()?)),
                _ => return Err(format!("unknown argument {arg}")),
            }
        }
        parsed.probe = match (kind, out) {
            (Some(kind), Some(out)) => Some((kind, out)),
            (None, None) => None,
            (Some(_), None) => return Err("--ui-probe needs --probe-out <file>".into()),
            (None, Some(_)) => return Err("--probe-out needs --ui-probe".into()),
        };
        Ok(parsed)
    }
}

static ARMED: OnceLock<(Kind, PathBuf)> = OnceLock::new();
static FIRST_PAINT: AtomicBool = AtomicBool::new(false);
static MESSAGES: AtomicU64 = AtomicU64::new(0);
static TIMERS: AtomicU64 = AtomicU64::new(0);
static PAINTS: AtomicU64 = AtomicU64::new(0);
static COMPOSED: AtomicBool = AtomicBool::new(false);
static WARP: AtomicBool = AtomicBool::new(false);

/// Arms the probe `args` asked for, if any. Call it before the window exists.
pub fn arm(args: &Args) {
    if let Some(probe) = &args.probe {
        let _ = ARMED.set(probe.clone());
    }
}

/// How the window presents: through `DirectComposition` (`warp`: on the WARP software device), or
/// GDI. The last call before a report is what it says.
pub fn presenter(composed: bool, warp: bool) {
    COMPOSED.store(composed, Ordering::Relaxed);
    WARP.store(composed && warp, Ordering::Relaxed);
}

/// The report's presenter fields.
fn presented() -> String {
    format!(
        "composed={} warp={}",
        u8::from(COMPOSED.load(Ordering::Relaxed)),
        u8::from(WARP.load(Ordering::Relaxed))
    )
}

/// Counts one message the UI thread woke for.
pub fn message(msg: u32) {
    if ARMED.get().is_none() {
        return;
    }
    MESSAGES.fetch_add(1, Ordering::Relaxed);
    match msg {
        WM_TIMER => TIMERS.fetch_add(1, Ordering::Relaxed),
        WM_PAINT => PAINTS.fetch_add(1, Ordering::Relaxed),
        _ => 0,
    };
}

/// A `WM_PAINT` was handled and its frame is on screen. The first one ends a first-frame probe and
/// starts an idle probe's clock.
pub fn painted() {
    let Some((kind, out)) = ARMED.get() else {
        return;
    };
    if FIRST_PAINT.swap(true, Ordering::SeqCst) {
        return;
    }
    match kind {
        Kind::FirstFrame => {
            let us = since_process_start().as_micros();
            finish(out, &format!("first_frame_us={us} {}\n", presented()));
        }
        Kind::Idle => {
            // The watcher sleeps between its two readings, so it adds no wakes of its own.
            let out = out.clone();
            let spawned = thread::Builder::new()
                .name("ui-probe".into())
                .spawn(move || watch_idle(&out));
            if spawned.is_err() {
                std::process::exit(2);
            }
        }
    }
}

fn watch_idle(out: &Path) {
    thread::sleep(SETTLE);
    let (cpu, msgs, timers, paints) = (
        cpu_time(),
        counts(&MESSAGES),
        counts(&TIMERS),
        counts(&PAINTS),
    );
    thread::sleep(WATCH);
    let cpu = cpu_time().saturating_sub(cpu);
    let mem = memory();
    let line = format!(
        "idle_cpu_us={} messages={} timers={} paints={} working_set_kib={} private_kib={} \
         watched_ms={} {}\n",
        cpu.as_micros(),
        counts(&MESSAGES) - msgs,
        counts(&TIMERS) - timers,
        counts(&PAINTS) - paints,
        mem.working_set / 1024,
        mem.private / 1024,
        WATCH.as_millis(),
        presented(),
    );
    finish(out, &line);
}

fn counts(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}

/// Writes the report and ends the process: a measurement run has nothing to clean up.
fn finish(out: &Path, line: &str) {
    let code = i32::from(std::fs::write(out, line).is_err());
    std::process::exit(code);
}

fn ticks(ft: FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

const ZERO: FILETIME = FILETIME {
    dwLowDateTime: 0,
    dwHighDateTime: 0,
};

/// The process's creation time and its kernel + user CPU time, in 100 ns ticks.
fn process_times() -> Option<(u64, u64)> {
    let (mut created, mut exited, mut kernel, mut user) = (ZERO, ZERO, ZERO, ZERO);
    // SAFETY: the pseudo-handle is always valid; every out-pointer is a live local.
    let ok = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &raw mut created,
            &raw mut exited,
            &raw mut kernel,
            &raw mut user,
        )
    };
    (ok != 0).then(|| (ticks(created), ticks(kernel) + ticks(user)))
}

/// Time since this process was created, at 100 ns resolution.
#[must_use]
pub fn since_process_start() -> Duration {
    let mut now = ZERO;
    // SAFETY: the out-pointer is a live local.
    unsafe { GetSystemTimePreciseAsFileTime(&raw mut now) };
    let created = process_times().map_or(ticks(now), |(created, _)| created);
    Duration::from_nanos(ticks(now).saturating_sub(created) * 100)
}

fn cpu_time() -> Duration {
    Duration::from_nanos(process_times().map_or(0, |(_, cpu)| cpu) * 100)
}

struct Memory {
    working_set: usize,
    private: usize,
}

/// The working set and private bytes (`PagefileUsage`), as `GetProcessMemoryInfo` reports them.
fn memory() -> Memory {
    // SAFETY: plain data; all-zero is a valid value.
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    let size = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>()).unwrap_or(0);
    counters.cb = size;
    // SAFETY: the pseudo-handle is always valid, and the out-pointer is a live local of `size`
    // bytes.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &raw mut counters, size) };
    if ok == 0 {
        return Memory {
            working_set: 0,
            private: 0,
        };
    }
    Memory {
        working_set: counters.WorkingSetSize,
        private: counters.PagefileUsage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(OsString::from))
    }

    #[test]
    fn parses_size_and_probe() {
        assert_eq!(parse(&[]), Ok(Args::default()));
        assert_eq!(
            parse(&[
                "--size",
                "1000x700",
                "--ui-probe",
                "idle",
                "--probe-out",
                "o.txt"
            ]),
            Ok(Args {
                size: Some((1000, 700)),
                materials: ForceMaterials::Auto,
                probe: Some((Kind::Idle, PathBuf::from("o.txt"))),
            })
        );
        for (flag, materials) in [
            ("auto", ForceMaterials::Auto),
            ("solid", ForceMaterials::Solid),
            ("glass", ForceMaterials::Glass),
        ] {
            assert_eq!(
                parse(&["--materials", flag]).map(|a| a.materials),
                Ok(materials)
            );
        }
        assert_eq!(
            parse(&["--probe-out", "o.txt", "--ui-probe", "first-frame"])
                .map(|a| a.probe.map(|p| p.0)),
            Ok(Some(Kind::FirstFrame))
        );
    }

    #[test]
    fn rejects_what_it_cannot_measure() {
        for bad in [
            &["--size"][..],
            &["--size", "1000"],
            &["--size", "0x700"],
            &["--ui-probe", "uia", "--probe-out", "o"],
            &["--ui-probe", "idle"],
            &["--probe-out", "o"],
            &["--materials"],
            &["--materials", "mica"],
            &["--frobnicate"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_process_started_before_now() {
        assert!(since_process_start() > Duration::ZERO);
        assert!(memory().working_set > 0);
    }
}
