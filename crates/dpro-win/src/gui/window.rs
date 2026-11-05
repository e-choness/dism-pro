//! The top-level window, its message loop and the GDI presenter (TD-0009 §5.1, §5.2).
//!
//! **Ownership.** [`Window::new`] boxes the window's [`State`] and hands it to the window at
//! `WM_NCCREATE` (`GWLP_USERDATA`). The state is freed after `WM_NCDESTROY`. Only shared references
//! to it exist, and everything that changes is a cell, so the window procedure may be re-entered:
//! `DestroyWindow`, `SetWindowPos` and the modal move/size loop all call it again before they
//! return. A depth count keeps the state alive until the outermost call into it returns, so a
//! window destroyed in the middle of handling a message never frees memory a caller still uses.

use std::{
    cell::{Cell, RefCell},
    fmt,
    ptr::{null, null_mut},
    rc::Rc,
};

use windows_sys::{
    Win32::{
        Foundation::{
            ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, RECT,
            WPARAM,
        },
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CreateCompatibleDC,
            CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, EndPaint, GdiFlush, HBITMAP,
            HDC, HGDIOBJ, InvalidateRect, PAINTSTRUCT, SRCCOPY, SelectObject,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem, GetDpiForWindow},
            Input::KeyboardAndMouse::{
                GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT,
                TrackMouseEvent, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
            },
            WindowsAndMessaging::{
                CREATESTRUCTW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, GWLP_USERDATA, GetClientRect, GetMessageW, GetWindowLongPtrW,
                IDC_ARROW, KillTimer, LoadCursorW, MINMAXINFO, MSG, PostQuitMessage,
                RegisterClassExW, SIZE_MINIMIZED, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOMOVE,
                SWP_NOZORDER, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
                TranslateMessage, WM_CAPTURECHANGED, WM_CLOSE, WM_CREATE, WM_DESTROY,
                WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS,
                WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_NCDESTROY, WM_PAINT,
                WM_SETFOCUS, WM_SIZE, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WNDCLASSEXW,
                WS_OVERLAPPEDWINDOW,
            },
        },
    },
    core::w,
};

use super::{Event, Frame, Handler, Mods, PointPx, RectPx, SizePx, WindowOptions, probe};
use crate::{
    error::{Result, last_os_error},
    wide::WideCString,
};

const CLASS: *const u16 = w!("dismpro.window");
const FRAME_TIMER: usize = 1;
/// One frame at 60 Hz, rounded down: `SetTimer`'s resolution is the system tick anyway.
const FRAME_MS: u32 = 16;
/// The DPI that is 100 %.
const BASE_DPI: u32 = 96;
/// `WinUser.h`'s value; windows-sys files it under `Win32_UI_Controls`, a large feature for one
/// constant.
const WM_MOUSELEAVE: u32 = 0x02A3;

/// A top-level window that presents its [`Handler`]'s frames. `!Send`: a window belongs to the
/// thread that created it, which runs its loop.
pub struct Window {
    hwnd: HWND,
    life: Rc<Cell<Life>>,
}

impl fmt::Debug for Window {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Window")
            .field("hwnd", &self.hwnd)
            .field("life", &self.life.get())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Life {
    /// `CreateWindowExW` hasn't reached `WM_NCCREATE`: the state is still [`Window::new`]'s.
    Unborn,
    Alive,
    /// `WM_NCDESTROY` came; the state is freed once nothing uses it.
    Gone,
}

impl Window {
    /// Creates the window, hidden, with its client area at `options.size` scaled to its monitor.
    /// The handler gets [`Event::Dpi`] and [`Event::Resize`] before this returns.
    ///
    /// # Errors
    /// The title holds a NUL, or the class or window can't be created.
    pub fn new(options: &WindowOptions<'_>, handler: Box<dyn Handler>) -> Result<Self> {
        let title = WideCString::new(options.title)?;
        // SAFETY: no preconditions; a null name means this executable.
        let instance = unsafe { GetModuleHandleW(null()) };
        register_class(instance)?;
        // SAFETY: no preconditions.
        let system_dpi = unsafe { GetDpiForSystem() };
        let life = Rc::new(Cell::new(Life::Unborn));
        let state = Box::into_raw(Box::new(State {
            handler: RefCell::new(handler),
            canvas: RefCell::new(None),
            size: Cell::new(SizePx::default()),
            dpi: Cell::new(system_dpi),
            min_size: options.min_size,
            dirty: Cell::new(false),
            frames: Cell::new(false),
            close: Cell::new(false),
            tracking: Cell::new(false),
            pressed: Cell::new(false),
            depth: Cell::new(0),
            life: Rc::clone(&life),
        }));
        let (w, h) = outer_size(options.size, system_dpi);
        // SAFETY: the class is registered, the strings outlive the call, and `state` is a live
        // allocation the window takes at WM_NCCREATE.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                CLASS,
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                w,
                h,
                null_mut(),
                null_mut(),
                instance,
                state.cast(),
            )
        };
        if hwnd.is_null() {
            let error = last_os_error("CreateWindowExW");
            if life.get() == Life::Unborn {
                // SAFETY: the window never took the state, so this is its only owner.
                drop(unsafe { Box::from_raw(state) });
            }
            return Err(error.into());
        }
        let window = Self { hwnd, life };
        // SAFETY: hwnd is live.
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi != 0 && dpi != system_dpi {
            // It opened on a monitor whose scale isn't the primary's.
            let (w, h) = outer_size(options.size, dpi);
            // SAFETY: hwnd is live; WM_SIZE comes back through the procedure.
            unsafe {
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    0,
                    0,
                    w,
                    h,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
        }
        // WM_SIZE may not have come during creation.
        // SAFETY: hwnd is this thread's window of our class.
        unsafe { with_state(hwnd, |s| s.sync_size(hwnd)) };
        Ok(window)
    }

    /// Shows the window; its first frame paints once the loop runs.
    pub fn show(&self) {
        if self.life.get() == Life::Alive {
            // SAFETY: hwnd is live.
            unsafe { ShowWindow(self.hwnd, SW_SHOWNORMAL) };
        }
    }

    /// Runs this thread's message loop until the window is destroyed.
    ///
    /// # Errors
    /// `GetMessageW` failed.
    pub fn run(&self) -> Result<()> {
        // SAFETY: plain data; all-zero is a valid value.
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        loop {
            // SAFETY: msg is a live local.
            match unsafe { GetMessageW(&raw mut msg, null_mut(), 0, 0) } {
                0 => return Ok(()),
                -1 => return Err(last_os_error("GetMessageW").into()),
                _ => {}
            }
            probe::message(msg.message);
            // SAFETY: msg is the message GetMessageW just filled.
            unsafe {
                TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        if self.life.get() == Life::Alive {
            // SAFETY: hwnd is live and belongs to this thread; WM_NCDESTROY frees the state.
            unsafe { DestroyWindow(self.hwnd) };
        }
    }
}

/// Registers the window class, once per process.
fn register_class(instance: HINSTANCE) -> Result<()> {
    let class = WNDCLASSEXW {
        cbSize: u32::try_from(size_of::<WNDCLASSEXW>()).unwrap_or(0),
        lpfnWndProc: Some(proc),
        hInstance: instance,
        // SAFETY: a system cursor, loaded by id.
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        lpszClassName: CLASS,
        // SAFETY: plain data; all-zero is a valid value for the remaining fields.
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: the class's name is static and its procedure lives for the process.
    if unsafe { RegisterClassExW(&raw const class) } == 0 {
        // SAFETY: no preconditions; read right after the failed call.
        if unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS {
            return Err(last_os_error("RegisterClassExW").into());
        }
    }
    Ok(())
}

/// What the handler may ask of its window while it handles an event.
pub struct WindowCx<'a> {
    hwnd: HWND,
    state: &'a State,
}

impl fmt::Debug for WindowCx<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowCx")
            .field("hwnd", &self.hwnd)
            .finish_non_exhaustive()
    }
}

impl WindowCx<'_> {
    /// The handler changed `r`: the next `WM_PAINT` asks it to paint, then presents `r`.
    pub fn invalidate(&mut self, r: RectPx) {
        if r.x1 <= r.x0 || r.y1 <= r.y0 {
            return;
        }
        let rect = RECT {
            left: r.x0,
            top: r.y0,
            right: r.x1,
            bottom: r.y1,
        };
        // SAFETY: hwnd is live, and rect a live local. Sends no message.
        unsafe { InvalidateRect(self.hwnd, &raw const rect, 0) };
        self.state.dirty.set(true);
    }

    /// Whether the handler wants [`Event::Frame`]s, every 16 ms, until it says otherwise.
    pub fn want_frames(&mut self, on: bool) {
        if self.state.frames.get() == on {
            return;
        }
        let ok = if on {
            // SAFETY: hwnd is live; WM_TIMER comes through the loop (no callback).
            unsafe { SetTimer(self.hwnd, FRAME_TIMER, FRAME_MS, None) != 0 }
        } else {
            // SAFETY: hwnd is live; the timer is this window's.
            unsafe { KillTimer(self.hwnd, FRAME_TIMER) };
            true
        };
        self.state.frames.set(on && ok);
    }

    /// Destroys the window once the handler returns, ending [`Window::run`].
    pub fn close(&mut self) {
        self.state.close.set(true);
    }

    /// The window's DPI; 96 is 100 %.
    #[must_use]
    pub fn dpi(&self) -> u32 {
        self.state.dpi.get()
    }

    /// The client area, in physical pixels.
    #[must_use]
    pub fn size(&self) -> SizePx {
        self.state.size.get()
    }
}

/// The window's side of the handler: its canvas and what it tracks between messages.
struct State {
    handler: RefCell<Box<dyn Handler>>,
    /// The client area's pixels; `None` while the client area is empty.
    canvas: RefCell<Option<Dib>>,
    size: Cell<SizePx>,
    dpi: Cell<u32>,
    min_size: (u32, u32),
    /// The handler damaged something it hasn't painted yet.
    dirty: Cell<bool>,
    /// The frame timer runs.
    frames: Cell<bool>,
    /// The handler asked to close.
    close: Cell<bool>,
    /// `TrackMouseEvent` will send `WM_MOUSELEAVE`.
    tracking: Cell<bool>,
    /// The primary button is down, and the window holds the pointer.
    pressed: Cell<bool>,
    /// Calls into the state that haven't returned.
    depth: Cell<u32>,
    life: Rc<Cell<Life>>,
}

/// Runs `f` on `hwnd`'s state, if it still has one. The state outlives `f` even if `f` destroys
/// the window: the outermost call frees it once it returns.
///
/// # Safety
/// `hwnd` is a window of this module's class, on this thread (or a destroyed one).
unsafe fn with_state<R>(hwnd: HWND, f: impl FnOnce(&State) -> R) -> Option<R> {
    // SAFETY: the caller's contract: the user data is 0 or the State set at WM_NCCREATE, which is
    // freed only below.
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const State;
    if ptr.is_null() {
        return None;
    }
    // SAFETY: live, as above. Only shared references to it exist.
    let s = unsafe { &*ptr };
    s.depth.set(s.depth.get() + 1);
    let r = f(s);
    s.depth.set(s.depth.get() - 1);
    if s.depth.get() == 0 && s.life.get() == Life::Gone {
        // SAFETY: the window is gone and cleared its user data, and no call into the state is
        // left: this was the last reference, and `s` isn't used again.
        drop(unsafe { Box::from_raw(ptr.cast_mut()) });
    }
    Some(r)
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        // SAFETY: for WM_NCCREATE, lp points to this CreateWindowExW call's CREATESTRUCTW, whose
        // lpCreateParams is the State that Window::new allocated.
        let state = unsafe { (*(lp as *const CREATESTRUCTW)).lpCreateParams }.cast::<State>();
        // SAFETY: hwnd is the window being created; the window owns the state from here.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize) };
        // SAFETY: live; Window::new holds no reference to it while CreateWindowExW runs.
        unsafe { (*state).life.set(Life::Alive) };
    }
    let on_state = |s: &State| {
        if msg == WM_NCDESTROY {
            // SAFETY: the window is being destroyed; nothing reads its user data again.
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            s.life.set(Life::Gone);
            return None;
        }
        s.handle(hwnd, msg, wp, lp)
    };
    // SAFETY: hwnd is a window of our class on this thread: the system calls us for no other.
    let handled = unsafe { with_state(hwnd, on_state) };
    match handled.flatten() {
        Some(result) => result,
        // SAFETY: default handling, with the arguments we were given.
        None => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

impl State {
    /// The message's result, or `None` for default handling.
    fn handle(&self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_CREATE => {
                // SAFETY: hwnd is live.
                let dpi = unsafe { GetDpiForWindow(hwnd) };
                if dpi != 0 {
                    self.dpi.set(dpi);
                }
                self.send(hwnd, Event::Dpi(self.dpi.get()));
            }
            WM_SIZE => {
                // Minimized, the canvas is kept for the restore.
                if u32::try_from(wp).ok() != Some(SIZE_MINIMIZED) {
                    self.resize(hwnd, low(lp), high(lp));
                }
            }
            WM_DPICHANGED => {
                let dpi = u32::try_from((wp >> 16) & 0xFFFF).unwrap_or(BASE_DPI);
                self.dpi.set(dpi);
                self.send(hwnd, Event::Dpi(dpi));
                // SAFETY: for WM_DPICHANGED, lp points to the suggested window rectangle.
                let r = unsafe { *(lp as *const RECT) };
                // SAFETY: hwnd is live (or destroyed, and the call fails). WM_SIZE follows.
                unsafe {
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        r.left,
                        r.top,
                        r.right.saturating_sub(r.left),
                        r.bottom.saturating_sub(r.top),
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    )
                };
            }
            WM_GETMINMAXINFO => {
                let (w, h) = outer_size(self.min_size, self.dpi.get());
                // SAFETY: for WM_GETMINMAXINFO, lp points to the MINMAXINFO to fill.
                let info = unsafe { &mut *(lp as *mut MINMAXINFO) };
                info.ptMinTrackSize.x = w;
                info.ptMinTrackSize.y = h;
            }
            // The canvas covers the client area; erasing would only flicker.
            WM_ERASEBKGND => return Some(1),
            WM_PAINT => self.paint(hwnd),
            WM_TIMER if wp == FRAME_TIMER => {
                self.send(hwnd, Event::Frame);
            }
            WM_SETFOCUS | WM_KILLFOCUS => {
                self.send(hwnd, Event::Focus(msg == WM_SETFOCUS));
            }
            WM_CLOSE => {
                self.send(hwnd, Event::Close);
            }
            WM_DESTROY => {
                // SAFETY: ends this thread's message loop.
                unsafe { PostQuitMessage(0) };
            }
            _ => return self.input(hwnd, msg, wp, lp),
        }
        Some(0)
    }

    /// Pointer and keyboard messages, as [`handle`](Self::handle).
    fn input(&self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_MOUSEMOVE => {
                if !self.tracking.get() {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: u32::try_from(size_of::<TRACKMOUSEEVENT>()).unwrap_or(0),
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    // SAFETY: track is a live local naming this window.
                    let ok = unsafe { TrackMouseEvent(&raw mut track) };
                    self.tracking.set(ok != 0);
                }
                self.send(hwnd, Event::PointerMove(point(lp)));
            }
            WM_MOUSELEAVE => {
                self.tracking.set(false);
                if !self.pressed.get() {
                    self.send(hwnd, Event::PointerLeave);
                }
            }
            WM_LBUTTONDOWN => {
                self.pressed.set(true);
                // SAFETY: hwnd is live; WM_CAPTURECHANGED goes to the window losing the capture.
                unsafe { SetCapture(hwnd) };
                self.send(hwnd, Event::PointerDown(point(lp)));
            }
            WM_LBUTTONUP => {
                // Cleared first: releasing sends WM_CAPTURECHANGED, which must not cancel.
                if self.pressed.replace(false) {
                    // SAFETY: no preconditions.
                    unsafe { ReleaseCapture() };
                }
                self.send(hwnd, Event::PointerUp(point(lp)));
            }
            WM_CAPTURECHANGED => {
                // Another window took the pointer mid-press: the press is cancelled.
                if self.pressed.replace(false) {
                    self.send(hwnd, Event::PointerLeave);
                }
            }
            WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
                let event = Event::Key {
                    vk: u16::try_from(wp & 0xFFFF).unwrap_or(0),
                    down: matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN),
                    mods: mods(),
                };
                // A key the handler didn't use goes on to the system (Alt+F4, Alt+Space, F10).
                if self.send(hwnd, event) != Some(true) {
                    return None;
                }
            }
            _ => return None,
        }
        Some(0)
    }

    /// Hands `event` to the handler, then destroys the window if the handler asked. Returns whether
    /// the handler used it, or `None` if the handler is busy: it pumped messages while handling an
    /// earlier one, and this one is dropped.
    fn send(&self, hwnd: HWND, event: Event) -> Option<bool> {
        let used = {
            let mut handler = self.handler.try_borrow_mut().ok()?;
            let mut cx = WindowCx { hwnd, state: self };
            handler.event(event, &mut cx)
        };
        if self.close.take() {
            // SAFETY: hwnd is live. The state outlives this call (with_state's depth count).
            unsafe { DestroyWindow(hwnd) };
        }
        Some(used)
    }

    /// Recreates the canvas at the client area's new size and tells the handler.
    fn resize(&self, hwnd: HWND, w: u32, h: u32) {
        let size = SizePx { w, h };
        let Ok(mut canvas) = self.canvas.try_borrow_mut() else {
            return;
        };
        if w == 0 || h == 0 || (size == self.size.get() && canvas.is_some()) {
            return;
        }
        // The old buffer goes first, so there's never more than one (TD-0009 §7 U7).
        *canvas = None;
        *canvas = Dib::new(size);
        drop(canvas);
        self.size.set(size);
        self.send(hwnd, Event::Resize(size));
    }

    /// Resizes to the client area as it is now.
    fn sync_size(&self, hwnd: HWND) {
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: hwnd is live, and r a live local.
        if unsafe { GetClientRect(hwnd, &raw mut r) } != 0 {
            let w = u32::try_from(r.right).unwrap_or(0);
            let h = u32::try_from(r.bottom).unwrap_or(0);
            self.resize(hwnd, w, h);
        }
    }

    /// `WM_PAINT`: the handler paints its damage into the canvas, then the update region is
    /// copied to the screen. A region the system invalidated is in the canvas already.
    fn paint(&self, hwnd: HWND) {
        // SAFETY: plain data; all-zero is a valid value.
        let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
        // SAFETY: hwnd is live, and ps a live local for BeginPaint to fill.
        let dc = unsafe { BeginPaint(hwnd, &raw mut ps) };
        if !dc.is_null()
            && let Ok(mut canvas) = self.canvas.try_borrow_mut()
            && let Some(dib) = canvas.as_mut()
        {
            if self.dirty.get()
                && let Ok(mut handler) = self.handler.try_borrow_mut()
            {
                self.dirty.set(false);
                // SAFETY: no preconditions. GDI may still be reading the bitmap for an earlier
                // copy; this waits for it before the handler writes the pixels.
                unsafe { GdiFlush() };
                let size = dib.size;
                handler.paint(&mut Frame {
                    pixels: dib.pixels(),
                    size,
                });
            }
            let r = ps.rcPaint;
            // SAFETY: both DCs are live, and BeginPaint clipped dc to the update region.
            unsafe {
                BitBlt(
                    dc,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    dib.dc,
                    r.left,
                    r.top,
                    SRCCOPY,
                )
            };
        }
        // SAFETY: pairs with BeginPaint above.
        unsafe { EndPaint(hwnd, &raw const ps) };
        if self.dirty.get() {
            // The handler was busy: its damage waits for the next paint, of the whole window.
            // SAFETY: hwnd is live. Sends no message.
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
        probe::painted();
    }
}

/// The canvas: a top-down 32-bit DIB section selected into its own memory DC.
struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    /// The DC's bitmap before ours, put back before the DC is deleted.
    old: HGDIOBJ,
    bits: *mut u32,
    len: usize,
    size: SizePx,
}

impl Dib {
    /// `None` if the size doesn't fit GDI or memory runs out; the window then shows nothing until
    /// its next resize.
    fn new(size: SizePx) -> Option<Self> {
        let len = usize::try_from(u64::from(size.w) * u64::from(size.h)).ok()?;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).ok()?,
                biWidth: i32::try_from(size.w).ok()?,
                // Negative: rows run top to bottom, as the canvas's do.
                biHeight: -i32::try_from(size.h).ok()?,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                // SAFETY: plain data; all-zero is a valid value for the remaining fields.
                ..unsafe { std::mem::zeroed() }
            },
            // SAFETY: as above; a 32-bit BI_RGB DIB has no color table.
            ..unsafe { std::mem::zeroed() }
        };
        // SAFETY: a memory DC compatible with the screen; deleted on every path below.
        let dc = unsafe { CreateCompatibleDC(null_mut()) };
        if dc.is_null() {
            return None;
        }
        let mut bits = null_mut();
        // SAFETY: dc is live, info describes a 32-bit DIB, and bits is a live out-pointer.
        let bitmap = unsafe {
            CreateDIBSection(
                dc,
                &raw const info,
                DIB_RGB_COLORS,
                &raw mut bits,
                null_mut(),
                0,
            )
        };
        if bitmap.is_null() || bits.is_null() {
            // SAFETY: what was created above, deleted once.
            unsafe {
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                DeleteDC(dc);
            }
            return None;
        }
        // SAFETY: both are live, and the bitmap is in no other DC.
        let old = unsafe { SelectObject(dc, bitmap) };
        Some(Self {
            dc,
            bitmap,
            old,
            bits: bits.cast(),
            len,
            size,
        })
    }

    fn pixels(&mut self) -> &mut [u32] {
        // SAFETY: bits is the DIB's memory: `len` 32-bit pixels (rows of a 32-bit DIB need no
        // padding), page-aligned, alive until drop. `&mut self` makes this the only view.
        unsafe { std::slice::from_raw_parts_mut(self.bits, self.len) }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        // SAFETY: the objects new() created, released once, the bitmap after leaving the DC.
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

/// The window size for a client area of `client` at 100 %, at `dpi`.
fn outer_size(client: (u32, u32), dpi: u32) -> (i32, i32) {
    let scale = |v: u32| i32::try_from(u64::from(v) * u64::from(dpi) / u64::from(BASE_DPI));
    let mut r = RECT {
        left: 0,
        top: 0,
        right: scale(client.0).unwrap_or(i32::MAX / 2),
        bottom: scale(client.1).unwrap_or(i32::MAX / 2),
    };
    // SAFETY: r is a live local. On failure r stays the client area, which is close enough.
    unsafe { AdjustWindowRectExForDpi(&raw mut r, WS_OVERLAPPEDWINDOW, 0, 0, dpi) };
    (
        r.right.saturating_sub(r.left),
        r.bottom.saturating_sub(r.top),
    )
}

/// The low word of an `LPARAM`, unsigned (`LOWORD`).
fn low(lp: LPARAM) -> u32 {
    u32::try_from(lp & 0xFFFF).unwrap_or(0)
}

/// The high word of an `LPARAM`, unsigned (`HIWORD`).
fn high(lp: LPARAM) -> u32 {
    u32::try_from((lp >> 16) & 0xFFFF).unwrap_or(0)
}

/// A word of an `LPARAM` as a signed coordinate (`GET_X_LPARAM`): positions left of or above the
/// client area, while the window holds the pointer, are negative.
fn signed(word: u32) -> i32 {
    let w = i32::try_from(word).unwrap_or(0);
    if w >= 0x8000 { w - 0x1_0000 } else { w }
}

fn point(lp: LPARAM) -> PointPx {
    PointPx {
        x: signed(low(lp)),
        y: signed(high(lp)),
    }
}

fn mods() -> Mods {
    // SAFETY: no preconditions; the high bit is set while the key is down.
    let down = |vk: u16| unsafe { GetKeyState(i32::from(vk)) } < 0;
    Mods {
        shift: down(VK_SHIFT),
        ctrl: down(VK_CONTROL),
        alt: down(VK_MENU),
        win: down(VK_LWIN) || down(VK_RWIN),
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        PM_REMOVE, PeekMessageW, PostMessageW, SendMessageW,
    };

    use super::*;

    #[test]
    fn words_of_an_lparam() {
        let lp: LPARAM = 0x0002_FFFF;
        assert_eq!(low(lp), 0xFFFF);
        assert_eq!(high(lp), 2);
        assert_eq!(point(lp), PointPx { x: -1, y: 2 });
        assert_eq!(signed(0x7FFF), 0x7FFF);
        assert_eq!(signed(0x8000), -0x8000);
    }

    #[test]
    fn a_dib_is_one_top_down_buffer_of_the_client_size() {
        let mut dib = Dib::new(SizePx { w: 3, h: 2 }).expect("a 3x2 DIB");
        assert_eq!(dib.pixels().len(), 6);
        dib.pixels()[5] = 0xFF11_2233;
        assert_eq!(dib.pixels()[5], 0xFF11_2233);
        assert!(
            Dib::new(SizePx {
                w: u32::MAX,
                h: u32::MAX
            })
            .is_none()
        );
    }

    /// The events a [`Recorder`] got, shared with the test.
    type Log = Rc<RefCell<Vec<Event>>>;

    /// Records what its window tells it. It damages the whole canvas when resized, wants frames
    /// until it has had `frames` of them, and closes when asked.
    struct Recorder {
        events: Log,
        paints: Rc<Cell<u32>>,
        frames: u32,
    }

    impl Handler for Recorder {
        fn event(&mut self, event: Event, cx: &mut WindowCx<'_>) -> bool {
            self.events.borrow_mut().push(event);
            match event {
                Event::Resize(size) => {
                    let (x1, y1) = (i32::try_from(size.w), i32::try_from(size.h));
                    cx.invalidate(RectPx {
                        x0: 0,
                        y0: 0,
                        x1: x1.unwrap_or(0),
                        y1: y1.unwrap_or(0),
                    });
                    cx.want_frames(self.frames > 0);
                }
                Event::Frame => {
                    self.frames = self.frames.saturating_sub(1);
                    if self.frames == 0 {
                        cx.want_frames(false);
                        // SAFETY: a posted message to this test's live window.
                        unsafe { PostMessageW(cx.hwnd, WM_CLOSE, 0, 0) };
                    }
                }
                Event::Close => cx.close(),
                _ => {}
            }
            true
        }

        fn paint(&mut self, frame: &mut Frame<'_>) {
            frame.pixels.fill(0xFF33_6699);
            self.paints.set(self.paints.get() + 1);
        }
    }

    fn open(frames: u32) -> (Window, Log, Rc<Cell<u32>>) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let paints = Rc::new(Cell::new(0));
        let handler = Recorder {
            events: Rc::clone(&events),
            paints: Rc::clone(&paints),
            frames,
        };
        let options = WindowOptions {
            title: "dpro-win gui test",
            size: (320, 200),
            min_size: (100, 100),
        };
        let window = Window::new(&options, Box::new(handler)).expect("a window");
        (window, events, paints)
    }

    #[test]
    fn the_handler_learns_the_dpi_then_the_size_and_paints_its_damage() {
        let (window, events, paints) = open(0);
        let recorded = events.borrow().clone();
        let Some(&Event::Dpi(dpi)) = recorded.first() else {
            panic!("the first event isn't the DPI: {recorded:?}");
        };
        let scaled = |v: u32| v * dpi / BASE_DPI;
        assert_eq!(
            recorded.last(),
            Some(&Event::Resize(SizePx {
                w: scaled(320),
                h: scaled(200)
            })),
            "{recorded:?}"
        );
        // The handler damaged the canvas; painting asks it once, and only once.
        assert_eq!(paints.get(), 0);
        // SAFETY: a message sent to this test's live window, on its thread.
        unsafe { SendMessageW(window.hwnd, WM_PAINT, 0, 0) };
        assert_eq!(paints.get(), 1);
        // SAFETY: as above.
        unsafe { SendMessageW(window.hwnd, WM_PAINT, 0, 0) };
        assert_eq!(paints.get(), 1, "nothing new was damaged");
    }

    #[test]
    fn frames_tick_until_the_handler_stops_them_and_closing_ends_the_loop() {
        let (window, events, _) = open(3);
        window.run().expect("the loop");
        let recorded = events.borrow().clone();
        let frames = recorded.iter().filter(|e| **e == Event::Frame).count();
        assert_eq!(frames, 3, "{recorded:?}");
        assert_eq!(recorded.last(), Some(&Event::Close));
        // The window is gone, and with it the state and the handler.
        assert_eq!(window.life.get(), Life::Gone);
        assert_eq!(Rc::strong_count(&events), 1);
        drop(window);
    }

    #[test]
    fn dropping_an_open_window_destroys_it_and_frees_the_handler() {
        let (window, events, _) = open(0);
        assert_eq!(Rc::strong_count(&events), 2);
        drop(window);
        assert_eq!(Rc::strong_count(&events), 1);
        // Destroying it posted WM_QUIT; take it, so the next test's loop doesn't end at once.
        // SAFETY: plain data; all-zero is a valid value.
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: msg is a live local; PM_REMOVE takes the message.
        while unsafe { PeekMessageW(&raw mut msg, null_mut(), 0, 0, PM_REMOVE) } != 0 {}
    }
}
