//! Startup probe for the ADR-0002 gate "startup ratio vs bare Win32 window exe ≤ 3×".
//!
//! Every exe measures the same span: from its process's creation time (`GetProcessTimes`) to the
//! end of the first frame it puts on screen. With `--first-frame <file>` the exe writes
//! `first_frame_us=<n>` there and exits at that point, so the harness never sees teardown or
//! console costs. The end of the first frame is the same event for every exe: its first top-level
//! window has handled its first `WM_PAINT` ([`hook_first_paint`]); all three present inside it.

use std::path::PathBuf;

/// `--flag value` lookup over the process arguments.
#[must_use]
pub fn value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

#[must_use]
pub fn flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

/// `--size WxH` (logical pixels), else the given default.
#[must_use]
pub fn size(args: &[String], default: (u32, u32)) -> (u32, u32) {
    value(args, "--size")
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or(default)
}

/// The `--first-frame <file>` target, armed for [`first_frame_done`].
pub fn arm(args: &[String]) -> Option<PathBuf> {
    let path = value(args, "--first-frame").map(PathBuf::from)?;
    let _ = imp::REPORT.set(path.clone());
    Some(path)
}

#[cfg(windows)]
pub use imp::{first_frame_done, hook_first_paint, since_process_start};

#[cfg(windows)]
mod imp {
    use std::{
        path::PathBuf,
        sync::{
            Mutex, OnceLock,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use windows_sys::Win32::{
        Foundation::{FILETIME, HWND, LPARAM, LRESULT, WPARAM},
        System::{
            SystemInformation::GetSystemTimePreciseAsFileTime,
            Threading::{ExitProcess, GetCurrentProcess, GetCurrentThreadId, GetProcessTimes},
        },
        UI::WindowsAndMessaging::{
            CBT_CREATEWNDW, CallNextHookEx, CallWindowProcW, DefWindowProcW, GWLP_WNDPROC,
            GetWindowLongPtrW, HCBT_CREATEWND, SetWindowLongPtrW, SetWindowsHookExW, WH_CBT,
            WM_PAINT, WNDPROC, WS_CHILD,
        },
    };

    pub(crate) static REPORT: OnceLock<PathBuf> = OnceLock::new();
    static DONE: AtomicBool = AtomicBool::new(false);
    /// Every top-level window this thread created, with its own procedure. All of them, because a
    /// toolkit may create hidden ones first (winit's event-target window); those never paint.
    static ORIGINALS: Mutex<Vec<(usize, isize)>> = Mutex::new(Vec::new());

    const fn ticks(ft: FILETIME) -> u64 {
        ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
    }

    /// Time since this process was created (100 ns resolution).
    #[must_use]
    pub fn since_process_start() -> Duration {
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user, mut now) =
            (zero, zero, zero, zero, zero);
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
        // SAFETY: the out-pointer is a live local.
        unsafe { GetSystemTimePreciseAsFileTime(&raw mut now) };
        if ok == 0 {
            return Duration::ZERO;
        }
        Duration::from_nanos(ticks(now).saturating_sub(ticks(created)) * 100)
    }

    /// Ends a `--first-frame` run: writes the time to the report and exits. A no-op without
    /// `--first-frame`, and after the first call.
    pub fn first_frame_done() {
        let Some(path) = REPORT.get() else { return };
        if DONE.swap(true, Ordering::SeqCst) {
            return;
        }
        let us = since_process_start().as_micros();
        let code = u32::from(std::fs::write(path, format!("first_frame_us={us}\n")).is_err());
        // SAFETY: ends the process on purpose; nothing is left to clean up in a measurement run.
        unsafe { ExitProcess(code) };
    }

    unsafe extern "system" fn hooked(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        // The lock is released before forwarding: the window's procedure may create windows.
        let original = ORIGINALS.lock().ok().and_then(|o| {
            o.iter()
                .find(|(h, _)| *h == hwnd as usize)
                .map(|&(_, proc)| proc)
        });
        let Some(original) = original else {
            // SAFETY: default handling (not reached: only recorded windows use this procedure).
            return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
        };
        // SAFETY: the value is the window's own procedure, read before this one replaced it.
        let original: WNDPROC = unsafe { std::mem::transmute(original) };
        // SAFETY: forwarding the message unchanged to the procedure that owns the window.
        let r = unsafe { CallWindowProcW(original, hwnd, msg, wp, lp) };
        if msg == WM_PAINT {
            first_frame_done();
        }
        r
    }

    /// Calls [`first_frame_done`] once any top-level window this thread creates has handled its
    /// first `WM_PAINT`. The same for every toolkit (Slint creates its window inside its event
    /// loop): a thread-local CBT hook subclasses each top-level window as it's created. Call it
    /// before the first window. A no-op without `--first-frame`.
    pub fn hook_first_paint() {
        if REPORT.get().is_none() {
            return;
        }
        let cbt: unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT = on_create;
        // SAFETY: a thread-local hook (this thread's id, no module) with a valid procedure.
        unsafe {
            SetWindowsHookExW(
                WH_CBT,
                Some(cbt),
                std::ptr::null_mut(),
                GetCurrentThreadId(),
            )
        };
    }

    unsafe extern "system" fn on_create(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code == HCBT_CREATEWND as i32 {
            // SAFETY: for HCBT_CREATEWND, lp points to a CBT_CREATEWNDW whose lpcs is valid.
            let style = unsafe { (*(*(lp as *const CBT_CREATEWNDW)).lpcs).style };
            if style as u32 & WS_CHILD == 0 {
                subclass(wp as HWND);
            }
        }
        // SAFETY: passes the event on unchanged (the hook handle argument is ignored).
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wp, lp) }
    }

    fn subclass(hwnd: HWND) {
        let ours: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT = hooked;
        // SAFETY: hwnd is a live window of this thread.
        let old = unsafe { GetWindowLongPtrW(hwnd, GWLP_WNDPROC) };
        let Ok(mut originals) = ORIGINALS.lock() else {
            return;
        };
        if old == 0 {
            return;
        }
        originals.push((hwnd as usize, old));
        drop(originals);
        // SAFETY: hwnd is a live window of this thread, and ORIGINALS already holds the procedure
        // `hooked` forwards to.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours as usize as isize) };
    }
}

#[cfg(not(windows))]
mod imp {
    use std::{path::PathBuf, sync::OnceLock};

    pub(crate) static REPORT: OnceLock<PathBuf> = OnceLock::new();
}
