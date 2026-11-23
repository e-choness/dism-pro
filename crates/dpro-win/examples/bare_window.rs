//! TD-0009 §7 U4, U6, U7: the raw-API baseline. A bare Win32 window that paints one solid fill,
//! with nothing a toolkit adds. G1 (`.github/scripts/ui-gates.ps1`) divides the GUI's startup by
//! this window's, in the same job on the same runner, so the gate holds wherever it runs.
//!
//! `bare_window [--size WxH] [--ui-probe first-frame|idle --probe-out <file>]`; the default size is
//! TD-0009's default window, 1280 × 800 at 100 %.
//!
//! It creates its window the way `dpro_win::gui::Window` does: hidden, sized for the system DPI,
//! then shown. Only the painting differs, so the ratio measures the toolkit.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(windows)]
    {
        bare::main()
    }
    #[cfg(not(windows))]
    {
        ExitCode::FAILURE
    }
}

#[cfg(windows)]
mod bare {
    use std::{
        process::ExitCode,
        ptr::{null, null_mut},
    };

    use dpro_win::gui::probe::{self, Args};
    use windows_sys::{
        Win32::{
            Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
            Graphics::Gdi::{
                BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, PAINTSTRUCT,
            },
            System::LibraryLoader::GetModuleHandleW,
            UI::{
                HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem},
                WindowsAndMessaging::{
                    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
                    IDC_ARROW, LoadCursorW, MSG, PostQuitMessage, RegisterClassExW, SW_SHOWNORMAL,
                    ShowWindow, TranslateMessage, WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WNDCLASSEXW,
                    WS_OVERLAPPEDWINDOW,
                },
            },
        },
        core::w,
    };

    /// The light theme's app background (`Tone::App`), as a COLORREF (`0x00BBGGRR`).
    const BG: u32 = 0x00F9_F7F6;

    pub(crate) fn main() -> ExitCode {
        let Ok(args) = Args::parse(std::env::args_os().skip(1)) else {
            return ExitCode::from(2);
        };
        probe::arm(&args);
        let (w, h) = args.size.unwrap_or((1280, 800));
        if run(w, h) {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    fn run(w: u32, h: u32) -> bool {
        // SAFETY: plain Win32 calls with valid arguments. Every pointer is to a live local or a
        // static wide string, and the class outlives the window (it's never unregistered).
        unsafe {
            let instance = GetModuleHandleW(null());
            let class = WNDCLASSEXW {
                cbSize: u32::try_from(size_of::<WNDCLASSEXW>()).unwrap_or(0),
                lpfnWndProc: Some(proc),
                hInstance: instance,
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                lpszClassName: w!("dismpro.bare"),
                ..std::mem::zeroed()
            };
            if RegisterClassExW(&raw const class) == 0 {
                return false;
            }
            let dpi = GetDpiForSystem();
            let scale =
                |v: u32| i32::try_from(u64::from(v) * u64::from(dpi) / 96).unwrap_or(i32::MAX);
            let mut rc = RECT {
                left: 0,
                top: 0,
                right: scale(w),
                bottom: scale(h),
            };
            AdjustWindowRectExForDpi(&raw mut rc, WS_OVERLAPPEDWINDOW, 0, 0, dpi);
            let hwnd = CreateWindowExW(
                0,
                w!("dismpro.bare"),
                w!("DISM Pro bare window"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                rc.right - rc.left,
                rc.bottom - rc.top,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            if hwnd.is_null() {
                return false;
            }
            ShowWindow(hwnd, SW_SHOWNORMAL);
            let mut msg: MSG = std::mem::zeroed();
            loop {
                match GetMessageW(&raw mut msg, null_mut(), 0, 0) {
                    0 => return true,
                    -1 => return false,
                    _ => {}
                }
                probe::message(msg.message);
                TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }
        }
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            // Painted whole in WM_PAINT instead.
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                // SAFETY: plain data; all-zero is a valid value.
                let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
                // SAFETY: hwnd is this procedure's live window, and ps a live local that
                // BeginPaint fills and EndPaint reads. The brush is deleted after use.
                unsafe {
                    let dc = BeginPaint(hwnd, &raw mut ps);
                    let brush = CreateSolidBrush(BG);
                    FillRect(dc, &raw const ps.rcPaint, brush);
                    DeleteObject(brush);
                    EndPaint(hwnd, &raw const ps);
                }
                probe::painted();
                0
            }
            WM_DESTROY => {
                // SAFETY: ends this thread's message loop.
                unsafe { PostQuitMessage(0) };
                0
            }
            // SAFETY: default handling with the arguments we were given.
            _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        }
    }
}
