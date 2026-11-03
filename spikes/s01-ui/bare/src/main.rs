//! The ADR-0002 baseline: a bare Win32 window that paints one solid fill. Its startup time and
//! size are what the prototypes' "startup ratio ≤ 3×" and "size delta ≤ 3 MiB" are measured against.
//!
//! `bare [--first-frame <file>] [--size WxH]`

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
mod win {
    use std::ptr::null;

    use windows_sys::{
        Win32::{
            Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
            Graphics::Gdi::CreateSolidBrush,
            System::LibraryLoader::GetModuleHandleW,
            UI::{
                HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem},
                WindowsAndMessaging::{
                    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
                    IDC_ARROW, LoadCursorW, MSG, PostQuitMessage, RegisterClassExW,
                    TranslateMessage, WM_DESTROY, WNDCLASSEXW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
                },
            },
        },
        core::w,
    };

    /// bg.app (light), as a COLORREF (0x00BBGGRR).
    const BG: u32 = 0x00F9_F7F6;

    pub fn run() {
        let args: Vec<String> = std::env::args().collect();
        probe::arm(&args);
        probe::hook_first_paint();
        let (w, h) = probe::size(&args, (1280, 800));
        // SAFETY: plain Win32 calls with valid arguments; every pointer is to a live local or a
        // static wide string, and the class outlives the window (it's never unregistered).
        unsafe {
            let instance = GetModuleHandleW(null());
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc),
                hInstance: instance,
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                // DefWindowProcW paints this in WM_PAINT (via WM_ERASEBKGND): the whole frame.
                hbrBackground: CreateSolidBrush(BG),
                lpszClassName: w!("s01-bare"),
                ..std::mem::zeroed()
            };
            RegisterClassExW(&raw const class);
            let dpi = GetDpiForSystem();
            let mut rc = RECT {
                left: 0,
                top: 0,
                right: (w * dpi / 96) as i32,
                bottom: (h * dpi / 96) as i32,
            };
            AdjustWindowRectExForDpi(&raw mut rc, WS_OVERLAPPEDWINDOW, 0, 0, dpi);
            let hwnd = CreateWindowExW(
                0,
                w!("s01-bare"),
                w!("S-01 bare"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                rc.right - rc.left,
                rc.bottom - rc.top,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                null(),
            );
            if hwnd.is_null() {
                std::process::exit(1);
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&raw mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }
        }
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
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
