//! Option A's window: Win32 + the software canvas, presented through DirectComposition (glass:
//! Mica behind a premultiplied-alpha client area, ADR-0007) or GDI (solid; WinPE, where `dcomp.dll`
//! doesn't exist). The solid fallback switches live when Windows' transparency setting changes. UI
//! Automation through AccessKit. `--palette` opens the command palette as a separate Acrylic popup.

use std::{error::Error, ffi::c_void, ptr::null_mut};

use accesskit_windows::Adapter;
use scene::{
    Screen, State,
    data::{self, Nav, Rows},
    layout,
    tokens::{Theme, font},
};
use windows_sys::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{
            Dwm::{
                DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
                DWMWA_WINDOW_CORNER_PREFERENCE, DwmExtendFrameIntoClientArea,
                DwmSetWindowAttribute,
            },
            Gdi::{
                BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CreateCompatibleDC,
                CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, EndPaint, HBITMAP, HDC,
                InvalidateRect, PAINTSTRUCT, SRCCOPY, SelectObject,
            },
        },
        System::{
            LibraryLoader::GetModuleHandleW,
            Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
        },
        UI::{
            Controls::MARGINS,
            HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem, GetDpiForWindow},
            Input::KeyboardAndMouse::{
                GetKeyState, VK_CONTROL, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_NEXT, VK_PRIOR,
                VK_UP,
            },
            WindowsAndMessaging::{
                CREATESTRUCTW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, GW_OWNER, GWLP_USERDATA, GetClientRect, GetMessageW, GetWindow,
                GetWindowLongPtrW, GetWindowRect, IDC_ARROW, LoadCursorW, MSG, PostMessageW,
                PostQuitMessage, RegisterClassExW, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
                SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
                WM_APP, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_GETOBJECT, WM_KEYDOWN,
                WM_KILLFOCUS, WM_LBUTTONDOWN, WM_MOUSEWHEEL, WM_NCCREATE, WM_PAINT, WM_SETFOCUS,
                WM_SETTINGCHANGE, WM_SIZE, WNDCLASSEXW, WS_EX_NOREDIRECTIONBITMAP,
                WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::{
    a11y,
    canvas::Canvas,
    draw, gpu,
    text::{Align, Style, Text},
};

const WM_A11Y_ACTION: u32 = WM_APP + 1;
const MAIN_CLASS: PCWSTR = w!("s01-a");
const PALETTE_CLASS: PCWSTR = w!("s01-a-palette");
/// The palette popup, logical px.
const PALETTE: (f32, f32) = (560.0, 300.0);

/// How frames reach the screen.
enum Present {
    /// DirectComposition: alpha reaches DWM, so Mica shows through the transparent base.
    Glass(gpu::Surface),
    /// GDI blit of an opaque frame (WinPE, or no DirectComposition).
    Gdi(Dib),
}

/// A top-down 32-bit DIB section and a memory DC holding it.
struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u8,
    w: u32,
    h: u32,
}

impl Dib {
    fn new(w: u32, h: u32) -> Option<Self> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = null_mut();
        // SAFETY: a valid header; out-pointer to a local; the section and DC are owned by Dib.
        unsafe {
            let dc = CreateCompatibleDC(null_mut());
            let bitmap = CreateDIBSection(
                dc,
                &raw const info,
                DIB_RGB_COLORS,
                &raw mut bits,
                null_mut(),
                0,
            );
            if bitmap.is_null() || bits.is_null() {
                DeleteDC(dc);
                return None;
            }
            SelectObject(dc, bitmap);
            Some(Self {
                dc,
                bitmap,
                bits: bits.cast(),
                w,
                h,
            })
        }
    }

    fn blit(&mut self, target: HDC, c: &Canvas) {
        // SAFETY: the section holds exactly w × h × 4 bytes for as long as Dib lives.
        let pixels =
            unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h * 4) as usize) };
        c.to_bgra(pixels);
        // SAFETY: both DCs are live for the duration of the call.
        unsafe {
            BitBlt(
                target,
                0,
                0,
                self.w as i32,
                self.h as i32,
                self.dc,
                0,
                0,
                SRCCOPY,
            )
        };
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        // SAFETY: the DC and bitmap are ours and no longer used.
        unsafe {
            DeleteDC(self.dc);
            DeleteObject(self.bitmap);
        }
    }
}

struct Popup {
    hwnd: HWND,
    canvas: Canvas,
    surface: gpu::Surface,
}

struct App {
    hwnd: HWND,
    state: State,
    rows: Rows,
    canvas: Canvas,
    text: Text,
    gpu: Option<gpu::Gpu>,
    present: Option<Present>,
    /// Windows' transparency setting, when glass is possible at all.
    glass: bool,
    adapter: Option<Adapter>,
    palette: Option<Popup>,
    /// Where the palette opens (screen px), instead of over the window: the harness puts it over
    /// the bare desktop to see the system Acrylic sample what's behind it.
    palette_at: Option<(i32, i32)>,
}

fn transparency_on() -> bool {
    let mut value = 1_u32;
    let mut size = 4_u32;
    // SAFETY: valid key path and value name; out-pointers to locals of the size given.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("EnableTransparency"),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&raw mut value).cast(),
            &raw mut size,
        )
    };
    rc != 0 || value != 0
}

/// DWM backdrop and frame for a glass window: `kind` 2 = Mica, 3 = transient (Acrylic); 1 = none.
fn backdrop(hwnd: HWND, kind: i32, dark: bool) {
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    let dark = i32::from(dark);
    // SAFETY: a live window; each pointer is to a live local of the size passed.
    unsafe {
        DwmExtendFrameIntoClientArea(hwnd, &raw const margins);
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            (&raw const dark).cast(),
            4,
        );
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            (&raw const kind).cast(),
            4,
        );
    }
}

fn client_size(hwnd: HWND) -> (u32, u32) {
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: a live window; out-pointer to a local.
    unsafe { GetClientRect(hwnd, &raw mut rc) };
    (
        (rc.right - rc.left).max(1) as u32,
        (rc.bottom - rc.top).max(1) as u32,
    )
}

fn wh(hwnd: HWND) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(hwnd)
}

impl App {
    fn scale(&self) -> f32 {
        self.canvas.scale
    }

    /// Renders the current state and puts it on screen.
    fn paint(&mut self) {
        let (w, h) = client_size(self.hwnd);
        if (self.canvas.width(), self.canvas.height()) != (w, h)
            && let Some(c) = Canvas::new(w, h, self.scale())
        {
            self.canvas = c;
        }
        let glass = self.glass && matches!(self.present, Some(Present::Glass(_)));
        draw::frame(
            &mut self.canvas,
            &mut self.text,
            &self.state,
            &self.rows,
            glass,
        );
        let mut ps = PAINTSTRUCT::default();
        // SAFETY: the BeginPaint/EndPaint pair of this window's WM_PAINT.
        let dc = unsafe { BeginPaint(self.hwnd, &raw mut ps) };
        match (&mut self.present, &self.gpu) {
            (Some(Present::Glass(s)), Some(gpu)) => {
                let _ = s.present(gpu, &self.canvas);
            }
            (Some(Present::Gdi(dib)), _) if (dib.w, dib.h) == (w, h) => dib.blit(dc, &self.canvas),
            (present, _) => {
                if let Some(mut dib) = Dib::new(w, h) {
                    dib.blit(dc, &self.canvas);
                    *present = Some(Present::Gdi(dib));
                }
            }
        }
        // SAFETY: as above.
        unsafe { EndPaint(self.hwnd, &raw const ps) };
    }

    fn paint_palette(&mut self) {
        let Some(p) = self.palette.as_mut() else {
            return;
        };
        let Some(gpu) = self.gpu.as_ref() else { return };
        let pal = self.state.theme.palette();
        let c = &mut p.canvas;
        // Acrylic shows through the transparent base; solid when transparency is off.
        if self.glass {
            c.clear(pal.bg_surface, 0);
        } else {
            c.clear(pal.bg_surface, 255);
        }
        let (w, _) = PALETTE;
        let search = layout::Rect::new(16.0, 16.0, w - 32.0, 36.0);
        c.bordered(search, [6.0; 4], (pal.bg_surface, 255), (pal.accent, 255));
        self.text.draw(
            c,
            "uninst",
            search.inset(12.0, 0.0),
            Style::new(font::SUBTITLE, pal.text_primary),
        );
        let hits = [
            "Apps \u{b7} Uninstall a program",
            "Apps \u{b7} Uninstall leftovers",
            "Activity \u{b7} Undo the last job",
            "Recipes \u{b7} Uninstall bundle",
        ];
        for (i, label) in hits.into_iter().enumerate() {
            let r = layout::Rect::new(16.0, 68.0 + i as f32 * 40.0, w - 32.0, 36.0);
            if i == 0 {
                c.rrect(
                    r,
                    [6.0; 4],
                    pal.selected,
                    if self.glass { 200 } else { 255 },
                );
            }
            self.text.draw(
                c,
                label,
                r.inset(12.0, 0.0),
                Style::new(font::BODY, pal.text_primary),
            );
        }
        self.text.draw(
            c,
            "Enter to run \u{b7} Esc to close",
            layout::Rect::new(16.0, 248.0, w - 32.0, 36.0),
            Style::new(font::CAPTION, pal.text_secondary).align(Align::Right),
        );
        self.text.end_frame();
        let mut ps = PAINTSTRUCT::default();
        // SAFETY: the BeginPaint/EndPaint pair of the popup's WM_PAINT.
        unsafe { BeginPaint(p.hwnd, &raw mut ps) };
        let _ = p.surface.present(gpu, &p.canvas);
        // SAFETY: as above.
        unsafe { EndPaint(p.hwnd, &raw const ps) };
    }

    fn invalidate(&self) {
        // SAFETY: a live window.
        unsafe { InvalidateRect(self.hwnd, std::ptr::null(), 0) };
        if let Some(p) = &self.palette {
            // SAFETY: a live window.
            unsafe { InvalidateRect(p.hwnd, std::ptr::null(), 0) };
        }
    }

    /// Applies a state change: repaint, and tell UI Automation. The events are raised by the
    /// caller, outside any borrow of the app (raising can re-enter the window procedure).
    fn changed(&mut self) -> Option<accesskit_windows::QueuedEvents> {
        self.invalidate();
        let (state, rows, scale) = (&self.state, &self.rows, self.scale());
        self.adapter
            .as_mut()?
            .update_if_active(|| a11y::tree(state, rows, scale))
    }

    fn scroll_to(&mut self, v: f32) -> Option<accesskit_windows::QueuedEvents> {
        self.state.scroll = v.clamp(0.0, layout::max_scroll(self.rows.len()));
        self.changed()
    }

    fn select(&mut self, i: usize) -> Option<accesskit_windows::QueuedEvents> {
        let i = i.min(self.rows.len().saturating_sub(1));
        self.state.selected = Some(i);
        let r = layout::row(i, self.state.scroll);
        if r.y < layout::VIEWPORT.y {
            self.state.scroll = i as f32 * layout::ROW_H;
        } else if r.bottom() > layout::VIEWPORT.bottom() {
            self.state.scroll = (i + 1) as f32 * layout::ROW_H - layout::VIEWPORT.h;
        }
        self.changed()
    }

    fn click(&mut self, x: f32, y: f32) -> Option<accesskit_windows::QueuedEvents> {
        if self.state.screen == Screen::Drawer {
            if layout::DRAWER_CANCEL.contains(x, y) {
                self.state.screen = Screen::List;
                return self.changed();
            }
            if layout::DRAWER.contains(x, y) {
                return None;
            }
        }
        for (i, e, r) in layout::nav_entries() {
            if matches!(e, Nav::Item(_)) && r.contains(x, y) {
                return self.nav(i);
            }
        }
        if self.state.screen != Screen::Shell && layout::VIEWPORT.contains(x, y) {
            let i = ((y - layout::VIEWPORT.y + self.state.scroll) / layout::ROW_H) as usize;
            if i < self.rows.len() {
                self.state.screen = Screen::Drawer;
                return self.select(i);
            }
        }
        None
    }

    fn nav(&mut self, i: usize) -> Option<accesskit_windows::QueuedEvents> {
        self.state.nav = i;
        self.state.screen = if i == data::NAV_APPS {
            Screen::List
        } else {
            Screen::Shell
        };
        self.changed()
    }

    fn key(&mut self, vk: u16) -> Option<accesskit_windows::QueuedEvents> {
        let page = ((layout::VIEWPORT.h / layout::ROW_H) as usize).saturating_sub(1);
        let sel = self.state.selected.unwrap_or(0);
        match vk {
            VK_DOWN => self.select(sel + 1),
            VK_UP => self.select(sel.saturating_sub(1)),
            VK_NEXT => self.select(sel + page),
            VK_PRIOR => self.select(sel.saturating_sub(page)),
            VK_HOME => self.select(0),
            VK_END => self.select(usize::MAX),
            VK_ESCAPE if self.palette.is_some() => {
                if let Some(p) = self.palette.take() {
                    // SAFETY: our own popup.
                    unsafe { DestroyWindow(p.hwnd) };
                }
                None
            }
            VK_ESCAPE if self.state.screen == Screen::Drawer => {
                self.state.screen = Screen::List;
                self.changed()
            }
            0x4B if ctrl_down() => {
                self.open_palette(false);
                None
            }
            _ => None,
        }
    }

    fn focus(&mut self, focused: bool) -> Option<accesskit_windows::QueuedEvents> {
        self.adapter.as_mut()?.update_window_focus_state(focused)
    }

    /// The command palette: its own top-level window with the system Acrylic backdrop.
    fn open_palette(&mut self, at_start: bool) {
        if self.palette.is_some() || self.gpu.is_none() {
            return;
        }
        let s = self.scale();
        let (pw, ph) = ((PALETTE.0 * s) as i32, (PALETTE.1 * s) as i32);
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: a live window; out-pointer to a local.
        unsafe { GetWindowRect(self.hwnd, &raw mut rc) };
        let (x, y) = self.palette_at.unwrap_or((
            rc.left + ((layout::WIDTH - PALETTE.0) / 2.0 * s) as i32,
            rc.top + (96.0 * s) as i32,
        ));
        // SAFETY: a registered class; owned by the main window; no create parameter.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW,
                PALETTE_CLASS,
                w!("S01 palette"),
                WS_POPUP,
                x,
                y,
                pw,
                ph,
                self.hwnd,
                null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return;
        }
        backdrop(
            hwnd,
            if self.glass { 3 } else { 1 },
            self.state.theme == Theme::Dark,
        );
        let round = 2_i32; // DWMWCP_ROUND
        // SAFETY: a live window; pointer to a live local.
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                (&raw const round).cast(),
                4,
            )
        };
        let (Some(gpu), Some(canvas)) = (self.gpu.as_ref(), Canvas::new(pw as u32, ph as u32, s))
        else {
            return;
        };
        let Ok(surface) = gpu::Surface::new(gpu, wh(hwnd), pw as u32, ph as u32) else {
            return;
        };
        // SAFETY: stores our app pointer for the popup's procedure (same thread, same lifetime).
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, std::ptr::from_mut(self) as isize) };
        self.palette = Some(Popup {
            hwnd,
            canvas,
            surface,
        });
        // SAFETY: a live window.
        unsafe { ShowWindow(hwnd, if at_start { SW_SHOWNOACTIVATE } else { SW_SHOW }) };
    }

    /// The live solid fallback: follows Windows' transparency setting without a restart.
    fn settings_changed(&mut self) {
        let on = transparency_on();
        if self.gpu.is_none() || on == self.glass {
            return;
        }
        self.glass = on;
        let dark = self.state.theme == Theme::Dark;
        backdrop(self.hwnd, if on { 2 } else { 1 }, dark);
        if let Some(p) = &self.palette {
            backdrop(p.hwnd, if on { 3 } else { 1 }, dark);
        }
        eprintln!(
            "s01-a: transparency {}, glass {on}",
            if on { "on" } else { "off" }
        );
        self.invalidate();
    }
}

fn ctrl_down() -> bool {
    // SAFETY: reads this thread's keyboard state.
    unsafe { GetKeyState(i32::from(VK_CONTROL)) < 0 }
}

/// Recovers the app from a window's user data (set in `WM_NCCREATE` / at popup creation).
fn app_of(hwnd: HWND) -> *mut App {
    // SAFETY: reading a window's user data.
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App }
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        // SAFETY: for WM_NCCREATE, lp is the CREATESTRUCTW whose parameter is our app pointer.
        unsafe {
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                (*(lp as *const CREATESTRUCTW)).lpCreateParams as isize,
            )
        };
    }
    let ptr = app_of(hwnd);
    if ptr.is_null() {
        // SAFETY: default handling.
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    if msg == WM_GETOBJECT {
        // AccessKit answers outside any borrow of the app: answering can re-enter this procedure.
        let answer = {
            // SAFETY: the app outlives its window and nothing else borrows it on this thread now.
            let app = unsafe { &mut *ptr };
            let scale = app.scale();
            let App {
                adapter,
                state,
                rows,
                ..
            } = app;
            adapter.as_mut().and_then(|a| {
                a.handle_wm_getobject(
                    windows::Win32::Foundation::WPARAM(wp),
                    windows::Win32::Foundation::LPARAM(lp),
                    &mut a11y::Initial { state, rows, scale },
                )
            })
        };
        if let Some(answer) = answer {
            let r: windows::Win32::Foundation::LRESULT = answer.into();
            return r.0;
        }
        // SAFETY: default handling.
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    let (result, events) = {
        // SAFETY: as above.
        let app = unsafe { &mut *ptr };
        let scale = app.scale();
        let to_logical = move |v: isize| f32::from(v as i16) / scale;
        match msg {
            WM_PAINT => {
                app.paint();
                (Some(0), None)
            }
            WM_ERASEBKGND => (Some(1), None),
            WM_SIZE => {
                app.invalidate();
                (Some(0), None)
            }
            WM_DPICHANGED => {
                app.canvas.scale = f32::from((wp >> 16) as u16) / 96.0;
                // SAFETY: lp is the suggested window rectangle for the new DPI.
                let r = unsafe { &*(lp as *const RECT) };
                // SAFETY: a live window.
                unsafe {
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    )
                };
                (Some(0), app.changed())
            }
            WM_MOUSEWHEEL => {
                let delta = f32::from((wp >> 16) as u16 as i16) / 120.0;
                (
                    Some(0),
                    app.scroll_to(app.state.scroll - delta * 3.0 * layout::ROW_H),
                )
            }
            WM_LBUTTONDOWN => (
                Some(0),
                app.click(to_logical(lp & 0xFFFF), to_logical((lp >> 16) & 0xFFFF)),
            ),
            WM_KEYDOWN => (Some(0), app.key(wp as u16)),
            WM_SETFOCUS => (Some(0), app.focus(true)),
            WM_KILLFOCUS => (Some(0), app.focus(false)),
            WM_SETTINGCHANGE => {
                app.settings_changed();
                (None, None)
            }
            WM_A11Y_ACTION => {
                let ev = match a11y::target(accesskit::NodeId(wp as u64)) {
                    a11y::Target::Nav(i) => app.nav(i),
                    a11y::Target::Row(i) => {
                        app.state.screen = Screen::Drawer;
                        app.select(i)
                    }
                    a11y::Target::Cancel => {
                        app.state.screen = Screen::List;
                        app.changed()
                    }
                    a11y::Target::Other => None,
                };
                (Some(0), ev)
            }
            WM_DESTROY => {
                // SAFETY: ends the message loop.
                unsafe { PostQuitMessage(0) };
                (Some(0), None)
            }
            _ => (None, None),
        }
    };
    if let Some(events) = events {
        events.raise();
    }
    match result {
        Some(r) => r,
        // SAFETY: default handling.
        None => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

unsafe extern "system" fn palette_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ptr = app_of(hwnd);
    if !ptr.is_null() && msg == WM_PAINT {
        // SAFETY: the main window's app outlives the popup it owns; no other borrow is live.
        unsafe { (*ptr).paint_palette() };
        return 0;
    }
    match msg {
        WM_ERASEBKGND => 1,
        // Keys go to the main window, which owns the palette's state (Esc closes it).
        WM_KEYDOWN => {
            // SAFETY: posting to the owner window of this thread.
            unsafe { PostMessageW(GetWindow(hwnd, GW_OWNER), msg, wp, lp) };
            0
        }
        // SAFETY: default handling.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

fn register(name: PCWSTR, proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT) {
    // SAFETY: a static class name and a valid procedure; the class is never unregistered.
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: GetModuleHandleW(std::ptr::null()),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: name,
            ..Default::default()
        };
        RegisterClassExW(&raw const class);
    }
}

pub fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    probe::arm(args);
    probe::hook_first_paint();
    let screen = probe::value(args, "--screen")
        .and_then(|s| Screen::parse(&s))
        .unwrap_or(Screen::List);
    let theme = probe::value(args, "--theme")
        .and_then(|s| Theme::parse(&s))
        .unwrap_or(Theme::Light);
    let (w, h) = probe::size(args, (layout::WIDTH as u32, layout::HEIGHT as u32));
    let at = point(args, "--at");
    // Glass needs DirectComposition (absent in WinPE); --solid forces the GDI path.
    let gpu = if probe::flag(args, "--solid") {
        None
    } else {
        gpu::Gpu::new(false).ok()
    };
    let glass = gpu.is_some() && transparency_on();

    register(MAIN_CLASS, main_proc);
    register(PALETTE_CLASS, palette_proc);
    // SAFETY: plain query.
    let dpi = unsafe { GetDpiForSystem() };
    let scale = dpi as f32 / 96.0;
    let canvas =
        Canvas::new((w as f32 * scale) as u32, (h as f32 * scale) as u32, scale).ok_or("canvas")?;
    let app = Box::into_raw(Box::new(App {
        hwnd: null_mut(),
        // The snapshot state, so the harness's pixel probes land on known content.
        state: State::snapshot(screen, theme),
        rows: Rows::generate(data::ROWS),
        canvas,
        text: Text::new()?,
        present: None,
        glass,
        adapter: None,
        palette: None,
        palette_at: point(args, "--palette-at"),
        gpu,
    }));
    // SAFETY: from here the app is owned by the raw pointer and freed after the message loop.
    let app_ref = unsafe { &mut *app };
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: (w * dpi / 96) as i32,
        bottom: (h * dpi / 96) as i32,
    };
    let ex = if app_ref.gpu.is_some() {
        WS_EX_NOREDIRECTIONBITMAP
    } else {
        0
    };
    // SAFETY: valid arguments; the create parameter is the app pointer read in WM_NCCREATE.
    let hwnd = unsafe {
        AdjustWindowRectExForDpi(&raw mut rc, WS_OVERLAPPEDWINDOW, 0, ex, dpi);
        let (x, y) = at.unwrap_or((CW_USEDEFAULT, CW_USEDEFAULT));
        CreateWindowExW(
            ex,
            MAIN_CLASS,
            w!("S-01 option A"),
            WS_OVERLAPPEDWINDOW,
            x,
            y,
            rc.right - rc.left,
            rc.bottom - rc.top,
            null_mut(),
            null_mut(),
            GetModuleHandleW(std::ptr::null()),
            app.cast::<c_void>(),
        )
    };
    if hwnd.is_null() {
        return Err("CreateWindowExW failed".into());
    }
    app_ref.hwnd = hwnd;
    // SAFETY: a live window.
    let real = unsafe { GetDpiForWindow(hwnd) };
    if real != dpi && real != 0 {
        app_ref.canvas.scale = real as f32 / 96.0;
    }
    if let Some(gpu) = app_ref.gpu.as_ref() {
        backdrop(hwnd, if glass { 2 } else { 1 }, theme == Theme::Dark);
        let (cw, ch) = client_size(hwnd);
        match gpu::Surface::new(gpu, wh(hwnd), cw, ch) {
            Ok(s) => app_ref.present = Some(Present::Glass(s)),
            Err(e) => eprintln!("s01-a: DirectComposition surface: {e}"),
        }
    }
    // AccessKit must exist before the first WM_GETOBJECT, so before the window is shown.
    app_ref.adapter = Some(Adapter::new(
        wh(hwnd),
        true,
        a11y::Actions {
            hwnd: hwnd as isize,
            message: WM_A11Y_ACTION,
        },
    ));
    // SAFETY: a live window.
    unsafe { ShowWindow(hwnd, SW_SHOW) };
    if probe::flag(args, "--palette") {
        app_ref.open_palette(true);
    }
    report_mode(app_ref);

    let mut msg = MSG::default();
    // SAFETY: the standard message loop of this thread.
    unsafe {
        while GetMessageW(&raw mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&raw const msg);
            DispatchMessageW(&raw const msg);
        }
        drop(Box::from_raw(app));
    }
    Ok(())
}

/// One line on stderr for the harness: which device and presentation the window got.
fn report_mode(app: &App) {
    let device = match &app.gpu {
        Some(g) if g.warp => "D3D11 WARP",
        Some(_) => "D3D11 hardware",
        None => "none",
    };
    let present = match &app.present {
        Some(Present::Glass(_)) => "DirectComposition",
        Some(Present::Gdi(_)) | None => "GDI",
    };
    eprintln!(
        "s01-a: device {device}, present {present}, glass {}",
        app.glass
    );
}

/// `--flag X,Y` as a screen point.
fn point(args: &[String], flag: &str) -> Option<(i32, i32)> {
    let v = probe::value(args, flag)?;
    let (x, y) = v.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}
