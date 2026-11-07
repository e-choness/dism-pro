//! The top-level window, its message loop and its presenters (TD-0009 §5.1, §5.2).
//!
//! **Presenter.** [`Window::new`] makes the devices [`Compose`] allows before the window, because
//! a composed window needs `WS_EX_NOREDIRECTIONBITMAP` from creation, and GDI can't draw such a
//! window. Without devices it makes a GDI window. If the swap chain can't be shown in the new
//! window, that window is destroyed and a GDI one made for the same handler. A composed window's
//! canvas is plain memory; a GDI window's is the DIB. A lost device is rebuilt in the same window
//! (TD-0009 §5.2), and the handler hears the new [`Presenter`].
//!
//! **Ownership.** [`Window::new`] boxes the window's [`State`] and hands it to the window at
//! `WM_NCCREATE` (`GWLP_USERDATA`). The state is freed after `WM_NCDESTROY`. Only shared references
//! to it exist, and everything that changes is a cell, so the window procedure may be re-entered:
//! `DestroyWindow`, `SetWindowPos` and the modal move/size loop all call it again before they
//! return. A depth count keeps the state alive until the outermost call into it returns, so a
//! window destroyed in the middle of handling a message never frees memory a caller still uses.
//!
//! **UI Automation.** The adapter is made before the window shows, as `accesskit_windows`
//! requires. Answering `WM_GETOBJECT` and raising UI Automation events can both bring a nested
//! `WM_GETOBJECT`, so both happen with nothing borrowed.
//!
//! **Environment.** At `WM_CREATE` the window subscribes to session changes (remote desktop) and to
//! battery saver, the two signals that aren't broadcast to every window, and unsubscribes at
//! `WM_DESTROY`. Every signal re-reads the [`Env`]; only a changed one reaches the handler, so the
//! bursts of `WM_SETTINGCHANGE` that one switch in Settings sends cost no repaint.

use std::{
    cell::{Cell, RefCell},
    fmt,
    ptr::{null, null_mut},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use accesskit_windows::Adapter;
use windows::Win32::UI::Accessibility::{IRawElementProviderSimple, UiaReturnRawElementProvider};

use windows_sys::{
    Win32::{
        Foundation::{
            ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, RECT,
            WPARAM,
        },
        Graphics::{
            Dwm::{
                DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE,
                DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
            },
            Gdi::{
                BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CreateCompatibleDC,
                CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, EndPaint, GdiFlush,
                HBITMAP, HDC, HGDIOBJ, InvalidateRect, PAINTSTRUCT, SRCCOPY, SelectObject,
            },
        },
        System::{
            LibraryLoader::GetModuleHandleW,
            Power::{
                HPOWERNOTIFY, RegisterPowerSettingNotification, UnregisterPowerSettingNotification,
            },
            RemoteDesktop::{
                NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
                WTSUnRegisterSessionNotification,
            },
        },
        UI::{
            HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem, GetDpiForWindow},
            Input::KeyboardAndMouse::{
                GetDoubleClickTime, GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE,
                TRACKMOUSEEVENT, TrackMouseEvent, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
            },
            WindowsAndMessaging::{
                CREATESTRUCTW, CW_USEDEFAULT, CreateWindowExW, DEVICE_NOTIFY_WINDOW_HANDLE,
                DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetClientRect,
                GetMessageTime, GetMessageW, GetSystemMetrics, GetWindowLongPtrW, HCURSOR,
                HTCLIENT, IDC_ARROW, IDC_IBEAM, KillTimer, LoadCursorW, MINMAXINFO, MSG,
                PBT_POWERSETTINGCHANGE, PostQuitMessage, RegisterClassExW, SIZE_MINIMIZED,
                SM_CXDOUBLECLK, SM_CYDOUBLECLK, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOMOVE,
                SWP_NOZORDER, SetCursor, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
                TranslateMessage, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE, WM_CREATE, WM_DESTROY,
                WM_DPICHANGED, WM_DWMCOMPOSITIONCHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO,
                WM_GETOBJECT, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP,
                WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_PAINT,
                WM_POWERBROADCAST, WM_SETCURSOR, WM_SETFOCUS, WM_SETTINGCHANGE, WM_SIZE,
                WM_SYSCOLORCHANGE, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_THEMECHANGED, WM_TIMER,
                WM_WTSSESSION_CHANGE, WNDCLASS_STYLES, WNDCLASSEXW, WS_EX_NOREDIRECTIONBITMAP,
                WS_OVERLAPPEDWINDOW,
            },
        },
    },
    core::{BOOL, HRESULT, w},
};

use super::{
    Backdrop, Cursor, Event, Frame, Handler, Mods, PointPx, PopupKind, Presenter, RectPx, SizePx,
    Surface, WindowOptions,
    a11y::{Initial, Requests, ToUiThread, WM_APP_A11Y},
    clipboard,
    compose::{Composer, Device, Gpu, Rects, after_loss},
    env::{Env, GUID_POWER_SAVING_STATUS, WM_APP_ENV, Watch},
    probe,
};
use crate::{
    error::{Result, last_os_error},
    wide::WideCString,
};

mod popup;

use popup::Popups;

const CLASS: *const u16 = w!("dismpro.window");
const FRAME_TIMER: usize = 1;
/// One frame at 60 Hz, rounded down: `SetTimer`'s resolution is the system tick anyway.
const FRAME_MS: u32 = 16;
/// A presenter rebuild that comes sooner than [`REBUILD_GAP`] after the last one waits on this
/// timer, so a device that fails at once costs a rebuild a second, not one per paint.
const REBUILD_TIMER: usize = 2;
const REBUILD_GAP: Duration = Duration::from_secs(1);
/// The one-shot timer behind [`WindowCx::wake_after`].
const WAKE_TIMER: usize = 3;
/// The DPI that is 100 %.
const BASE_DPI: u32 = 96;
/// `WinUser.h`'s value; windows-sys files it under `Win32_UI_Controls`, a large feature for one
/// constant.
const WM_MOUSELEAVE: u32 = 0x02A3;

/// `uxtheme.h`'s `MARGINS`, which windows-sys also files under `Win32_UI_Controls`.
#[repr(C)]
struct Margins {
    left: i32,
    right: i32,
    top: i32,
    bottom: i32,
}

// Linked as windows-sys links dwmapi (raw-dylib, verbatim), so the import table has one entry
// for it.
#[link(name = "dwmapi.dll", kind = "raw-dylib", modifiers = "+verbatim")]
unsafe extern "system" {
    fn DwmExtendFrameIntoClientArea(hwnd: HWND, margins: *const Margins) -> HRESULT;
}

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
    /// Creates the window, hidden, with its client area at `options.size` scaled to its monitor,
    /// presenting as `options.compose` allows. The handler gets [`Event::Presenter`],
    /// [`Event::Env`], [`Event::Dpi`] and [`Event::Resize`] before this returns.
    ///
    /// # Errors
    /// The title holds a NUL, or the class or window can't be created.
    pub fn new(options: &WindowOptions<'_>, handler: Box<dyn Handler>) -> Result<Self> {
        let (mut handler, mut gpu) = (handler, Gpu::open(options.compose));
        // Twice at most: a window without devices never hands the handler back.
        loop {
            match Self::create(options, handler, gpu.take())? {
                Made::Window(window) => return Ok(window),
                Made::Gdi(back) => handler = back,
            }
        }
    }

    /// The window, composed with `gpu` if given. If the swap chain can't be shown in it, it's
    /// destroyed, and the handler comes back for a GDI window.
    fn create(
        options: &WindowOptions<'_>,
        handler: Box<dyn Handler>,
        gpu: Option<Gpu>,
    ) -> Result<Made> {
        let title = WideCString::new(options.title)?;
        // SAFETY: no preconditions; a null name means this executable.
        let instance = unsafe { GetModuleHandleW(null()) };
        register_class(instance, CLASS, 0)?;
        // SAFETY: no preconditions.
        let system_dpi = unsafe { GetDpiForSystem() };
        let life = Rc::new(Cell::new(Life::Unborn));
        let composed = gpu.is_some();
        let presenter = Presenter {
            composed,
            hardware: gpu.as_ref().is_some_and(|g| !g.warp()),
            device_lost: false,
        };
        let state = Box::into_raw(Box::new(State::new(
            handler,
            presenter,
            options.min_size,
            system_dpi,
            Rc::clone(&life),
        )));
        let (w, h) = outer_size(options.size, system_dpi);
        let ex_style = if composed {
            WS_EX_NOREDIRECTIONBITMAP
        } else {
            0
        };
        // SAFETY: the class is registered, the strings outlive the call, and `state` is a live
        // allocation the window takes at WM_NCCREATE.
        let hwnd = unsafe {
            CreateWindowExW(
                ex_style,
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
        if let Some(gpu) = gpu {
            // SAFETY: as above.
            let shown = unsafe { with_state(hwnd, |s| s.show_swap_chain(hwnd, gpu)) };
            if shown != Some(true) {
                // SAFETY: as above. No message is being handled, so nothing holds the handler.
                if let Some(handler) = unsafe { with_state(hwnd, State::give_back) }.flatten() {
                    // Destroying the window frees its state, which now holds an inert handler.
                    drop(window);
                    return Ok(Made::Gdi(handler));
                }
            }
        } else {
            probe::presenter(false, false);
        }
        // SAFETY: as above. Not inside WM_GETOBJECT, and before the window shows, as
        // `Adapter::new` requires: UI Automation learns the window serves it before any client asks.
        unsafe {
            with_state(hwnd, |s| {
                s.requests.attach(hwnd);
                let actions = ToUiThread(Arc::clone(&s.requests));
                let adapter = Adapter::new(accesskit_windows::HWND(hwnd), false, actions);
                if let Ok(mut slot) = s.a11y.try_borrow_mut() {
                    *slot = Some(adapter);
                }
            })
        };
        Ok(Made::Window(window))
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
            // SAFETY: hwnd is live and belongs to this thread; a main window's popups go first.
            // WM_NCDESTROY frees the state.
            unsafe {
                with_state(self.hwnd, |s| {
                    if s.kind == Kind::Main {
                        s.destroy_popups();
                    }
                });
                DestroyWindow(self.hwnd);
            }
        }
    }
}

/// What [`Window::create`] made.
enum Made {
    Window(Window),
    /// The composed window couldn't show its swap chain and is gone; its handler, for a GDI one.
    Gdi(Box<dyn Handler>),
}

/// Holds a destroyed window's place after its handler moved to a new window.
struct Inert;

impl Handler for Inert {
    fn event(&mut self, _: Event, _: &mut WindowCx<'_>) -> bool {
        false
    }

    fn paint(&mut self, _: &mut Frame<'_>) {}
}

/// Registers a window class of this module (`dismpro.window`, or a popup's), once per process.
fn register_class(instance: HINSTANCE, name: *const u16, style: WNDCLASS_STYLES) -> Result<()> {
    let class = WNDCLASSEXW {
        cbSize: u32::try_from(size_of::<WNDCLASSEXW>()).unwrap_or(0),
        style,
        lpfnWndProc: Some(proc),
        hInstance: instance,
        // SAFETY: a system cursor, loaded by id.
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        lpszClassName: name,
        // SAFETY: plain data; all-zero is a valid value for the remaining fields.
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: the class's name is a static and its procedure lives for the process.
    if unsafe { RegisterClassExW(&raw const class) } == 0 {
        // SAFETY: no preconditions; read right after the failed call.
        if unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS {
            return Err(last_os_error("RegisterClassExW").into());
        }
    }
    Ok(())
}

/// What the handler may ask of its window while it handles an event. An event for a popup gets
/// its owner's: the window's size, DPI and timers are the main window's, and the popup is reached
/// through [`surface`](Self::surface) and [`invalidate_in`](Self::invalidate_in).
pub struct WindowCx<'a> {
    /// The main window.
    hwnd: HWND,
    state: &'a State,
    surface: Surface,
}

impl fmt::Debug for WindowCx<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowCx")
            .field("hwnd", &self.hwnd)
            .field("surface", &self.surface)
            .finish_non_exhaustive()
    }
}

impl WindowCx<'_> {
    /// The handler changed `r` of the main window: the next `WM_PAINT` asks it to paint, then
    /// presents `r`. A popup's are [`invalidate_in`](Self::invalidate_in)'s.
    pub fn invalidate(&mut self, r: RectPx) {
        self.state.invalidate_rect(self.hwnd, r);
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

    /// Sends [`Event::Wake`] once, `ms` milliseconds from now (the system makes it at least 10),
    /// replacing an earlier request; `None` cancels it. A caret blinks on it, and stops asking.
    pub fn wake_after(&mut self, ms: Option<u32>) {
        match ms {
            Some(ms) => {
                // SAFETY: hwnd is live; WM_TIMER comes through the loop (no callback). Setting the
                // same id again replaces the timer.
                let ok = unsafe { SetTimer(self.hwnd, WAKE_TIMER, ms, None) } != 0;
                self.state.wake.set(ok);
            }
            None => {
                if self.state.wake.replace(false) {
                    // SAFETY: hwnd is live; the timer is this window's.
                    unsafe { KillTimer(self.hwnd, WAKE_TIMER) };
                }
            }
        }
    }

    /// The clipboard's text, its first `max_units` UTF-16 code units at most; `None` if it holds
    /// none, or another window kept it open.
    #[must_use]
    pub fn clipboard_text(&self, max_units: usize) -> Option<String> {
        clipboard::read(self.hwnd, max_units)
    }

    /// Puts `text` on the clipboard. Returns whether it's there.
    pub fn set_clipboard_text(&mut self, text: &str) -> bool {
        clipboard::write(self.hwnd, text)
    }

    /// Destroys the window, and its popups, once the handler returns, ending [`Window::run`].
    pub fn close(&mut self) {
        self.state.close.set(true);
    }

    /// The main window's DPI; 96 is 100 %.
    #[must_use]
    pub fn dpi(&self) -> u32 {
        self.state.dpi.get()
    }

    /// The main window's client area, in physical pixels.
    #[must_use]
    pub fn size(&self) -> SizePx {
        self.state.size.get()
    }

    /// Windows' settings as the handler last heard them ([`Event::Env`]).
    #[must_use]
    pub fn env(&self) -> Env {
        self.state.env.get()
    }

    /// How the window presents, as [`Event::Presenter`] last said.
    #[must_use]
    pub fn presenter(&self) -> Presenter {
        self.state.presenter.get()
    }

    /// The system backdrop behind the client area, for the materials the handler paints: Mica
    /// under glass, none under solid. A GDI window is opaque and has none, so it ignores this.
    pub fn set_backdrop(&mut self, backdrop: Backdrop) {
        self.state.backdrop.set(backdrop);
        if self.state.composed {
            apply_backdrop(self.hwnd, backdrop);
        }
    }

    /// Draws the title bar and frame dark or light, to match the client area (and a composed
    /// window's Mica). Windows before 10 20H1, and `WinPE` without DWM, keep their frame.
    pub fn set_dark_frame(&mut self, dark: bool) {
        let on = BOOL::from(dark);
        // SAFETY: hwnd is live; the attribute's value is one BOOL, read during the call.
        unsafe {
            DwmSetWindowAttribute(
                self.hwnd,
                u32::try_from(DWMWA_USE_IMMERSIVE_DARK_MODE).unwrap_or(0),
                (&raw const on).cast(),
                u32::try_from(size_of::<BOOL>()).unwrap_or(0),
            )
        };
    }
}

/// The window's side of the handler: its canvas and what it tracks between messages.
struct State {
    handler: RefCell<Box<dyn Handler>>,
    /// The client area's pixels; `None` while the client area is empty.
    canvas: RefCell<Option<Canvas>>,
    /// The composition presenter; `None` in a GDI window, and in a composed one until its swap
    /// chain shows (or after every rebuild failed: the next paint tries again).
    composer: RefCell<Option<Composer>>,
    /// Made to be composed (`WS_EX_NOREDIRECTIONBITMAP`): GDI can't draw it.
    composed: bool,
    /// As the handler last heard it.
    presenter: Cell<Presenter>,
    /// When the device was last lost.
    lost_at: Cell<Option<Instant>>,
    /// When the presenter was last rebuilt.
    rebuilt_at: Cell<Option<Instant>>,
    /// A rebuild waits on [`REBUILD_TIMER`], for this device.
    rebuild_on: Cell<Option<Device>>,
    /// What the handler damaged since the last present, for a composed window.
    damage: RefCell<Rects>,
    /// The system backdrop the handler asked for.
    backdrop: Cell<Backdrop>,
    /// Destroying the window ends the loop; not for a composed window whose handler moved to a
    /// GDI one.
    quit_on_destroy: Cell<bool>,
    size: Cell<SizePx>,
    dpi: Cell<u32>,
    min_size: (u32, u32),
    /// The handler damaged something it hasn't painted yet.
    dirty: Cell<bool>,
    /// The frame timer runs.
    frames: Cell<bool>,
    /// The wake timer runs.
    wake: Cell<bool>,
    /// The first half of a surrogate pair a `WM_CHAR` brought; 0 if none.
    surrogate: Cell<u16>,
    /// The system's arrow and I-beam.
    cursors: [HCURSOR; 2],
    /// The handler asked to close.
    close: Cell<bool>,
    /// `TrackMouseEvent` will send `WM_MOUSELEAVE`.
    tracking: Cell<bool>,
    /// The primary button is down, and the window holds the pointer.
    pressed: Cell<bool>,
    /// The last press that could begin a double-click: where, and its message time. A
    /// double-click's second press clears it, so a third begins anew.
    last_press: Cell<Option<(PointPx, i32)>>,
    /// Serves the handler's tree to UI Automation; made once the window exists.
    a11y: RefCell<Option<Adapter>>,
    /// Clients' requests, queued on UI Automation's threads.
    requests: Arc<Requests>,
    /// The adapter serves a placeholder (the handler was busy at the first request), so the next
    /// update must be the whole tree.
    a11y_whole: Cell<bool>,
    /// UI Automation holds a provider of ours; it's told to let go when the window is destroyed.
    a11y_served: Cell<bool>,
    /// Windows' settings, as last sent to the handler.
    env: Cell<Env>,
    /// The thread watching the settings Windows doesn't broadcast (the text size); `None` if it
    /// couldn't start, or after `WM_DESTROY`.
    watch: Cell<Option<Watch>>,
    /// The battery saver subscription; 0 if there is none.
    power: Cell<HPOWERNOTIFY>,
    /// Subscribed to session changes.
    sessions: Cell<bool>,
    /// The main window, or which popup of which window.
    kind: Kind,
    /// A main window's popups (`popup.rs`); empty for a popup.
    popups: Popups,
    /// A popup is being hidden: losing the activation then is no dismiss.
    hiding: Cell<bool>,
    /// A popup is losing the activation: hiding it then gives nothing back.
    deactivating: Cell<bool>,
    /// A popup's presenter was lost for good: its next show makes a new window.
    broken: Cell<bool>,
    /// Calls into the state that haven't returned.
    depth: Cell<u32>,
    life: Rc<Cell<Life>>,
}

/// What a window of this module is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Main,
    /// A popup, owned by `owner`, whose handler serves it.
    Popup {
        owner: HWND,
        kind: PopupKind,
    },
}

impl Kind {
    fn surface(self) -> Surface {
        match self {
            Self::Main => Surface::Main,
            Self::Popup { kind, .. } => Surface::Popup(kind),
        }
    }
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
    /// A new window's state, presenting as `presenter` says; the window takes it at `WM_NCCREATE`.
    fn new(
        handler: Box<dyn Handler>,
        presenter: Presenter,
        min_size: (u32, u32),
        dpi: u32,
        life: Rc<Cell<Life>>,
    ) -> Self {
        Self {
            handler: RefCell::new(handler),
            canvas: RefCell::new(None),
            composer: RefCell::new(None),
            composed: presenter.composed,
            presenter: Cell::new(presenter),
            lost_at: Cell::new(None),
            rebuilt_at: Cell::new(None),
            rebuild_on: Cell::new(None),
            damage: RefCell::new(Rects::default()),
            backdrop: Cell::new(Backdrop::None),
            quit_on_destroy: Cell::new(true),
            size: Cell::new(SizePx::default()),
            dpi: Cell::new(dpi),
            min_size,
            dirty: Cell::new(false),
            frames: Cell::new(false),
            wake: Cell::new(false),
            surrogate: Cell::new(0),
            // SAFETY: system cursors, by their predefined ids; shared, never destroyed.
            cursors: unsafe {
                [
                    LoadCursorW(null_mut(), IDC_ARROW),
                    LoadCursorW(null_mut(), IDC_IBEAM),
                ]
            },
            close: Cell::new(false),
            tracking: Cell::new(false),
            pressed: Cell::new(false),
            last_press: Cell::new(None),
            a11y: RefCell::new(None),
            requests: Arc::new(Requests::default()),
            a11y_whole: Cell::new(false),
            a11y_served: Cell::new(false),
            env: Cell::new(Env::default()),
            watch: Cell::new(None),
            power: Cell::new(0),
            sessions: Cell::new(false),
            kind: Kind::Main,
            popups: Popups::default(),
            hiding: Cell::new(false),
            deactivating: Cell::new(false),
            broken: Cell::new(false),
            depth: Cell::new(0),
            life,
        }
    }

    /// The message's result, or `None` for default handling.
    fn handle(&self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        let own = match self.kind {
            Kind::Main => self.owner_message(hwnd, msg, wp, lp),
            Kind::Popup { owner, kind } => self.popup_message(hwnd, owner, kind, msg, wp, lp),
        };
        if let Some(result) = own {
            return result;
        }
        match msg {
            WM_CREATE => {
                self.subscribe(hwnd);
                self.send(hwnd, Event::Presenter(self.presenter.get()));
                let env = Env::read();
                self.env.set(env);
                self.send(hwnd, Event::Env(env));
                // SAFETY: hwnd is live.
                let dpi = unsafe { GetDpiForWindow(hwnd) };
                if dpi != 0 {
                    self.dpi.set(dpi);
                }
                self.send(hwnd, Event::Dpi(self.dpi.get()));
            }
            // Broadcast to every top-level window: theme, colors, accessibility settings.
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE | WM_THEMECHANGED | WM_WTSSESSION_CHANGE => {
                self.env_changed(hwnd);
                return None;
            }
            WM_POWERBROADCAST if u32::try_from(wp).ok() == Some(PBT_POWERSETTINGCHANGE) => {
                self.env_changed(hwnd);
                return Some(1);
            }
            WM_SIZE => {
                // Resized or minimized: its popups no longer stand by their anchors.
                if self.kind == Kind::Main {
                    self.dismiss_popups(hwnd);
                }
                // Minimized, the canvas is kept for the restore.
                if u32::try_from(wp).ok() != Some(SIZE_MINIMIZED) {
                    self.resize(hwnd, low(lp), high(lp));
                }
            }
            WM_DPICHANGED => {
                self.dismiss_popups(hwnd);
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
            WM_PAINT if self.composed => self.compose(hwnd),
            WM_PAINT => self.paint(hwnd),
            WM_DWMCOMPOSITIONCHANGED if self.composed => self.dwm_restarted(hwnd),
            WM_TIMER if wp == FRAME_TIMER => {
                self.send(hwnd, Event::Frame);
            }
            WM_TIMER if wp == REBUILD_TIMER => self.rebuild_due(hwnd),
            WM_TIMER if wp == WAKE_TIMER => {
                // One shot: the handler asks again if it needs to.
                self.wake.set(false);
                // SAFETY: hwnd is live; the timer is this window's.
                unsafe { KillTimer(hwnd, WAKE_TIMER) };
                self.send(hwnd, Event::Wake);
            }
            // Over the client area the handler picks the shape; elsewhere the system does.
            WM_SETCURSOR if low(lp) == HTCLIENT => {
                if !self.set_cursor() {
                    return None;
                }
                return Some(1);
            }
            WM_SETFOCUS | WM_KILLFOCUS => {
                let focused = msg == WM_SETFOCUS;
                self.send(hwnd, Event::Focus(focused));
                self.a11y_focus(focused);
            }
            WM_CLOSE => {
                self.send(hwnd, Event::Close);
            }
            WM_GETOBJECT => return self.get_object(wp, lp),
            WM_APP_A11Y => {
                for request in self.requests.drain() {
                    self.dispatch(hwnd, |h, cx| h.a11y_action(&request, cx));
                }
            }
            WM_APP_ENV => self.env_changed(hwnd),
            WM_DESTROY => self.destroyed(hwnd),
            _ => return self.input(hwnd, msg, wp, lp),
        }
        Some(0)
    }

    /// `WM_DESTROY`: unsubscribes, lets UI Automation go, and ends the loop. Windows has destroyed
    /// a main window's popups already, as it does owned windows.
    fn destroyed(&self, hwnd: HWND) {
        if self.kind == Kind::Main {
            self.destroy_popups();
        }
        self.unsubscribe(hwnd);
        self.requests.detach();
        if self.a11y_served.get() {
            // UI Automation may release what it holds for this window (the documented call for a
            // window that served it).
            // SAFETY: hwnd is live until this message returns.
            unsafe {
                UiaReturnRawElementProvider(
                    accesskit_windows::HWND(hwnd),
                    accesskit_windows::WPARAM(0),
                    accesskit_windows::LPARAM(0),
                    None::<&IRawElementProviderSimple>,
                )
            };
        }
        if self.quit_on_destroy.get() {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
        }
    }

    /// `WM_DWMCOMPOSITIONCHANGED` (DWM restarted): the frame and backdrop are set again, and a
    /// device it invalidated is rebuilt on the same kind (not a loss: the adapter is fine).
    fn dwm_restarted(&self, hwnd: HWND) {
        extend_frame(hwnd);
        apply_backdrop(hwnd, self.backdrop.get());
        let healthy = self
            .composer
            .try_borrow()
            .ok()
            .and_then(|c| c.as_ref().map(Composer::healthy));
        if healthy == Some(false) {
            let device = if self.presenter.get().hardware {
                Device::Hardware
            } else {
                Device::Warp
            };
            self.rebuild(hwnd, device);
        }
    }

    /// [`REBUILD_TIMER`] fired: the rebuild that waited runs now.
    fn rebuild_due(&self, hwnd: HWND) {
        // SAFETY: hwnd is live; the timer is this window's. One shot.
        unsafe { KillTimer(hwnd, REBUILD_TIMER) };
        if let Some(device) = self.rebuild_on.take() {
            self.rebuilt_at.set(None);
            self.rebuild(hwnd, device);
        }
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
                // WM_SETCURSOR came before the handler saw the move; and while the window holds
                // the pointer (selecting text), it doesn't come at all.
                self.set_cursor();
            }
            WM_MOUSELEAVE => {
                self.tracking.set(false);
                if !self.pressed.get() {
                    self.send(hwnd, Event::PointerLeave);
                }
            }
            // The surface under the pointer hears it; handled either way, so DefWindowProc never
            // passes it on to an owner as well.
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let main = match self.kind {
                    Kind::Main => hwnd,
                    Kind::Popup { owner, .. } => owner,
                };
                self.with_owner(|o| o.wheel(main, msg == WM_MOUSEHWHEEL, wp, lp));
            }
            WM_LBUTTONDOWN => {
                self.pressed.set(true);
                // SAFETY: hwnd is live; WM_CAPTURECHANGED goes to the window losing the capture.
                unsafe { SetCapture(hwnd) };
                let at = point(lp);
                let double = self.double_click(at);
                let held = mods();
                let mods = Mods {
                    shift: wp & MK_SHIFT != 0,
                    ctrl: wp & MK_CONTROL != 0,
                    ..held
                };
                self.send(hwnd, Event::PointerDown { at, mods, double });
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
            WM_CHAR => {
                let Some(c) = self.char_of(u16::try_from(wp & 0xFFFF).unwrap_or(0)) else {
                    return Some(0);
                };
                if self.send(hwnd, Event::Char(c)) != Some(true) {
                    return None;
                }
            }
            _ => return None,
        }
        Some(0)
    }

    /// The character a `WM_CHAR` code unit completes: a surrogate pair's first half waits for its
    /// second, and an unpaired half is dropped.
    fn char_of(&self, unit: u16) -> Option<char> {
        match unit {
            0xD800..=0xDBFF => {
                self.surrogate.set(unit);
                None
            }
            0xDC00..=0xDFFF => match self.surrogate.replace(0) {
                0 => None,
                high => char::decode_utf16([high, unit]).next()?.ok(),
            },
            _ => {
                self.surrogate.set(0);
                char::from_u32(u32::from(unit))
            }
        }
    }

    /// Shows the handler's cursor. False if the handler is busy, and the class's arrow shows.
    fn set_cursor(&self) -> bool {
        let Ok(handler) = self.handler.try_borrow() else {
            return false;
        };
        let cursor = match handler.cursor() {
            Cursor::Arrow => self.cursors[0],
            Cursor::IBeam => self.cursors[1],
        };
        // SAFETY: a system cursor; no other preconditions.
        unsafe { SetCursor(cursor) };
        true
    }

    /// Hands `event` to the handler. Returns whether the handler used it, or `None` if the handler
    /// is busy: it pumped messages while handling an earlier one, and this one is dropped.
    fn send(&self, hwnd: HWND, event: Event) -> Option<bool> {
        self.dispatch(hwnd, |h, cx| h.event(event, cx))
    }

    /// Subscribes to the signals that aren't broadcast: session changes, battery saver and the
    /// text size, before the first reading of [`Env`] so no change falls between. Each may be
    /// unavailable (no session manager); the window then misses that change only.
    fn subscribe(&self, hwnd: HWND) {
        self.watch.set(Watch::start(hwnd));
        // SAFETY: hwnd is live; unsubscribed at WM_DESTROY.
        self.sessions
            .set(unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } != 0);
        // SAFETY: as above; the GUID is a static. Windows sends the current state at once, which
        // matches the first reading and so reaches no one.
        self.power.set(unsafe {
            RegisterPowerSettingNotification(
                hwnd,
                &raw const GUID_POWER_SAVING_STATUS,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            )
        });
    }

    fn unsubscribe(&self, hwnd: HWND) {
        // Stops the watch thread and waits for it: it only posts, so it never waits on this one.
        drop(self.watch.take());
        if self.sessions.take() {
            // SAFETY: hwnd is live until WM_DESTROY returns, and subscribed.
            unsafe { WTSUnRegisterSessionNotification(hwnd) };
        }
        let power = self.power.take();
        if power != 0 {
            // SAFETY: the handle RegisterPowerSettingNotification returned, released once.
            unsafe { UnregisterPowerSettingNotification(power) };
        }
    }

    /// Windows signaled a change: the handler hears of it if a setting it sees changed. A busy
    /// handler misses it, and the next signal tries again.
    fn env_changed(&self, hwnd: HWND) {
        let env = Env::read();
        if env == self.env.get() {
            return;
        }
        let heard = self.env.replace(env);
        if self.send(hwnd, Event::Env(env)).is_none() {
            self.env.set(heard);
        }
    }

    /// Runs `f` on the handler for this window: its own, or a popup's owner's, as
    /// [`dispatch_as`](Self::dispatch_as) does. `None` if the handler is busy, or a popup's owner
    /// is going.
    fn dispatch<R>(
        &self,
        hwnd: HWND,
        f: impl FnOnce(&mut dyn Handler, &mut WindowCx<'_>) -> R,
    ) -> Option<R> {
        match self.kind {
            Kind::Main => self.dispatch_as(hwnd, Surface::Main, f),
            Kind::Popup { owner, kind } => {
                // SAFETY: a popup's owner is a window of this module's class on this thread,
                // alive while its popups are (it destroys them first).
                unsafe { with_state(owner, |o| o.dispatch_as(owner, Surface::Popup(kind), f)) }
                    .flatten()
            }
        }
    }

    /// On a main window's state: runs `f` on the handler for `surface`, then does what it asked
    /// of the popups, serves what changed to UI Automation (the window's tree and each popup's),
    /// and destroys the window if the handler asked. `None` if the handler is busy, or (for a
    /// popup) the window is being destroyed.
    fn dispatch_as<R>(
        &self,
        hwnd: HWND,
        surface: Surface,
        f: impl FnOnce(&mut dyn Handler, &mut WindowCx<'_>) -> R,
    ) -> Option<R> {
        if surface != Surface::Main && self.popups.closing.get() {
            return None;
        }
        let r = {
            let mut handler = self.handler.try_borrow_mut().ok()?;
            let mut cx = WindowCx {
                hwnd,
                state: self,
                surface,
            };
            f(&mut **handler, &mut cx)
        };
        self.apply_popups(hwnd);
        self.a11y_sync();
        self.a11y_sync_popups();
        if self.close.take() {
            self.destroy_popups();
            // SAFETY: hwnd is live. The state outlives this call (with_state's depth count).
            unsafe { DestroyWindow(hwnd) };
        }
        Some(r)
    }

    /// Runs `f` on the state whose handler serves this window: its own, or a popup's owner's.
    /// `None` once a popup's owner is gone.
    fn with_owner<R>(&self, f: impl FnOnce(&Self) -> R) -> Option<R> {
        match self.kind {
            Kind::Main => Some(f(self)),
            // SAFETY: as in `dispatch`; after the owner's WM_NCDESTROY its user data is 0, and
            // this is None.
            Kind::Popup { owner, .. } => unsafe { with_state(owner, f) },
        }
    }

    /// The handler changed `r`: the next `WM_PAINT` asks it to paint, then presents `r`.
    fn invalidate_rect(&self, hwnd: HWND, r: RectPx) {
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
        unsafe { InvalidateRect(hwnd, &raw const rect, 0) };
        self.dirty.set(true);
        if self.composed
            && let Ok(mut damage) = self.damage.try_borrow_mut()
        {
            damage.push(r);
        }
    }

    /// Asks the handler (this window's, or a popup's owner's) to paint this window's canvas.
    /// False if it's busy.
    fn paint_canvas(&self, pixels: &mut [u32], size: SizePx) -> bool {
        let surface = self.kind.surface();
        self.with_owner(|o| {
            let Ok(mut handler) = o.handler.try_borrow_mut() else {
                return false;
            };
            handler.paint(&mut Frame {
                pixels,
                size,
                surface,
            });
            true
        }) == Some(true)
    }

    /// `WM_GETOBJECT`: the adapter answers, asking the handler (a popup's owner's) for this
    /// surface's tree on the first request.
    fn get_object(&self, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        self.with_owner(|o| {
            let answer = {
                let mut adapter = self.a11y.try_borrow_mut().ok()?;
                let adapter = adapter.as_mut()?;
                let mut initial = Initial {
                    handler: o.handler.try_borrow_mut().ok(),
                    surface: self.kind.surface(),
                    placeholder: false,
                };
                let answer = adapter.handle_wm_getobject(
                    accesskit_windows::WPARAM(wp),
                    accesskit_windows::LPARAM(lp),
                    &mut initial,
                )?;
                if initial.placeholder {
                    self.a11y_whole.set(true);
                }
                answer
            };
            self.a11y_served.set(true);
            // With nothing borrowed: handing UI Automation the provider may bring WM_GETOBJECT
            // again.
            let result: accesskit_windows::LRESULT = answer.into();
            Some(result.0)
        })
        .flatten()
    }

    /// Gives the adapter what changed in this surface's tree (the handler's, or a popup's
    /// owner's), then raises the events that makes. Until a client has asked for the tree,
    /// nothing is asked of the handler.
    fn a11y_sync(&self) {
        if !self.a11y_served.get() {
            return;
        }
        let surface = self.kind.surface();
        let events = self
            .with_owner(|o| {
                let mut adapter = self.a11y.try_borrow_mut().ok()?;
                let adapter = adapter.as_mut()?;
                let mut handler = o.handler.try_borrow_mut().ok()?;
                let update = if self.a11y_whole.get() {
                    let tree = handler.a11y_tree(surface);
                    self.a11y_whole.set(tree.is_none());
                    tree
                } else {
                    handler.a11y_update(surface)
                }?;
                adapter.update_if_active(|| update)
            })
            .flatten();
        // With nothing borrowed: raising events may bring WM_GETOBJECT.
        if let Some(events) = events {
            events.raise();
        }
    }

    /// Tells UI Automation whether the window has the keyboard focus.
    fn a11y_focus(&self, focused: bool) {
        let events = {
            let Ok(mut adapter) = self.a11y.try_borrow_mut() else {
                return;
            };
            adapter
                .as_mut()
                .and_then(|a| a.update_window_focus_state(focused))
        };
        if let Some(events) = events {
            events.raise();
        }
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
        *canvas = Canvas::new(size, self.composed);
        drop(canvas);
        self.size.set(size);
        if self.composed {
            // The handler damages everything for the new canvas; the old damage is moot.
            if let Ok(mut damage) = self.damage.try_borrow_mut() {
                damage.clear();
            }
            let lost = self
                .composer
                .try_borrow_mut()
                .ok()
                .and_then(|mut c| c.as_mut().map(|c| c.resize(size).is_err()));
            if lost == Some(true) {
                self.lost(hwnd);
            }
        }
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
            && let Some(Canvas::Dib(dib)) = canvas.as_mut()
        {
            if self.dirty.get() {
                // SAFETY: no preconditions. GDI may still be reading the bitmap for an earlier
                // copy; this waits for it before the handler writes the pixels.
                unsafe { GdiFlush() };
                let size = dib.size;
                if self.paint_canvas(dib.pixels(), size) {
                    self.dirty.set(false);
                }
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
        self.repaint_if_busy(hwnd);
        probe::painted();
    }

    /// `WM_PAINT` in a composed window: the handler paints its damage into the canvas, and the
    /// composer uploads and presents it. DWM keeps what's on screen, so a region the system
    /// invalidated needs nothing.
    fn compose(&self, hwnd: HWND) {
        // SAFETY: plain data; all-zero is a valid value.
        let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
        // SAFETY: hwnd is live, and ps a live local. Validates the update region: the window has
        // no redirection bitmap, so its DC draws nothing.
        unsafe {
            BeginPaint(hwnd, &raw mut ps);
            EndPaint(hwnd, &raw const ps);
        }
        if self.composer.try_borrow().is_ok_and(|c| c.is_none()) {
            // Every rebuild failed so far: try again.
            self.rebuild(hwnd, after_loss(self.lost_at.get(), Instant::now()));
        }
        let lost = {
            let Ok(mut canvas) = self.canvas.try_borrow_mut() else {
                return;
            };
            let Some(canvas) = canvas.as_mut() else {
                return;
            };
            let Ok(mut damage) = self.damage.try_borrow_mut() else {
                return;
            };
            let mut painted = Rects::default();
            if self.dirty.get() {
                let size = canvas.size();
                if self.paint_canvas(canvas.pixels(), size) {
                    self.dirty.set(false);
                    painted = std::mem::take(&mut *damage);
                }
            }
            let Ok(mut composer) = self.composer.try_borrow_mut() else {
                return;
            };
            composer.as_mut().is_some_and(|c| {
                (!painted.is_empty() || c.due()) && c.present(canvas.pixels(), &painted).is_err()
            })
        };
        if lost {
            self.lost(hwnd);
        }
        self.repaint_if_busy(hwnd);
        probe::painted();
    }

    /// The handler was busy at a paint: its damage waits for the next one, of the whole window.
    fn repaint_if_busy(&self, hwnd: HWND) {
        if self.dirty.get() {
            // SAFETY: hwnd is live. Sends no message.
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    /// Shows `gpu`'s swap chain in this new window, over its system backdrop. False if it can't.
    fn show_swap_chain(&self, hwnd: HWND, gpu: Gpu) -> bool {
        let warp = gpu.warp();
        let Some(composer) = Composer::new(gpu, hwnd, self.size.get()) else {
            return false;
        };
        let Ok(mut slot) = self.composer.try_borrow_mut() else {
            return false;
        };
        *slot = Some(composer);
        extend_frame(hwnd);
        apply_backdrop(hwnd, self.backdrop.get());
        probe::presenter(true, warp);
        true
    }

    /// Takes the handler out of a window about to be destroyed, for a GDI window: an [`Inert`] one
    /// takes its place, and destroying this window doesn't end the loop.
    fn give_back(&self) -> Option<Box<dyn Handler>> {
        let mut handler = self.handler.try_borrow_mut().ok()?;
        self.quit_on_destroy.set(false);
        Some(std::mem::replace(&mut *handler, Box::new(Inert)))
    }

    /// The device was lost (TD-0009 §5.2): rebuilt on hardware, or on WARP after a second loss
    /// within a minute.
    fn lost(&self, hwnd: HWND) {
        if let Kind::Popup { owner, .. } = self.kind {
            self.popup_lost(hwnd, owner);
            return;
        }
        let now = Instant::now();
        let device = after_loss(self.lost_at.replace(Some(now)), now);
        self.rebuild(hwnd, device);
    }

    /// A new presenter in place of the old one, on `device` or else WARP, presented whole at the
    /// next paint. The handler hears if the presenter changed; if no device can be made, the next
    /// paint tries again. Within [`REBUILD_GAP`] of the last rebuild, the old one goes now and
    /// the new one waits on [`REBUILD_TIMER`].
    fn rebuild(&self, hwnd: HWND, device: Device) {
        // A popup presents on its owner's devices, whatever kind they are.
        if let Kind::Popup { owner, .. } = self.kind {
            self.popup_lost(hwnd, owner);
            return;
        }
        let now = Instant::now();
        let soon = self
            .rebuilt_at
            .get()
            .is_some_and(|t| now.saturating_duration_since(t) < REBUILD_GAP);
        let warp = {
            let Ok(mut slot) = self.composer.try_borrow_mut() else {
                return;
            };
            // The old one goes first: a window has one composition target at a time.
            *slot = None;
            if soon || self.rebuild_on.get().is_some() {
                if self.rebuild_on.replace(Some(device)).is_none() {
                    let gap = u32::try_from(REBUILD_GAP.as_millis()).unwrap_or(u32::MAX);
                    // SAFETY: hwnd is live; WM_TIMER comes through the loop (no callback).
                    if unsafe { SetTimer(hwnd, REBUILD_TIMER, gap, None) } == 0 {
                        // No timer: the next paint tries again.
                        self.rebuild_on.set(None);
                    }
                }
                return;
            }
            self.rebuilt_at.set(Some(now));
            let size = self.size.get();
            let made = match device {
                Device::Hardware => Composer::open(hwnd, size, Device::Hardware),
                Device::Warp => None,
            }
            .or_else(|| Composer::open(hwnd, size, Device::Warp));
            let Some(composer) = made else {
                return;
            };
            let on_warp = composer.warp();
            *slot = Some(composer);
            on_warp
        };
        probe::presenter(true, warp);
        let was = self.presenter.get();
        let now = Presenter {
            composed: true,
            hardware: !warp,
            // Hardware lost for WARP: the materials fall back to solid (TD-0009 §5.2).
            device_lost: warp && (was.hardware || was.device_lost),
        };
        if now != was {
            self.presenter.set(now);
            self.send(hwnd, Event::Presenter(now));
        }
        // SAFETY: hwnd is live. Sends no message; the paint presents the canvas whole.
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}

/// Lets DWM draw the system backdrop under all of a composed window's client area.
fn extend_frame(hwnd: HWND) {
    let all = Margins {
        left: -1,
        right: -1,
        top: -1,
        bottom: -1,
    };
    // SAFETY: hwnd is live, and the margins a live local read during the call. Without DWM
    // (`WinPE`) it fails, and nothing changes.
    unsafe { DwmExtendFrameIntoClientArea(hwnd, &raw const all) };
}

/// Sets the system backdrop DWM draws behind a composed window. Before Windows 11 22H2 the
/// attribute doesn't exist and nothing changes; the caller composes only from there.
fn apply_backdrop(hwnd: HWND, backdrop: Backdrop) {
    let kind = match backdrop {
        Backdrop::None => DWMSBT_NONE,
        Backdrop::Mica => DWMSBT_MAINWINDOW,
        Backdrop::Transient => DWMSBT_TRANSIENTWINDOW,
    };
    // SAFETY: hwnd is live; the attribute's value is one DWM_SYSTEMBACKDROP_TYPE, read during the
    // call.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            u32::try_from(DWMWA_SYSTEMBACKDROP_TYPE).unwrap_or(0),
            (&raw const kind).cast(),
            u32::try_from(size_of_val(&kind)).unwrap_or(0),
        )
    };
}

/// The client area's pixels: GDI's DIB section, or memory a composed window uploads from.
enum Canvas {
    Dib(Dib),
    Memory(Vec<u32>, SizePx),
}

impl Canvas {
    /// `None` if the size doesn't fit or memory runs out; the window then shows nothing until its
    /// next resize.
    fn new(size: SizePx, composed: bool) -> Option<Self> {
        if !composed {
            return Dib::new(size).map(Self::Dib);
        }
        let len = usize::try_from(u64::from(size.w) * u64::from(size.h)).ok()?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(len).ok()?;
        pixels.resize(len, 0);
        Some(Self::Memory(pixels, size))
    }

    fn size(&self) -> SizePx {
        match self {
            Self::Dib(dib) => dib.size,
            Self::Memory(_, size) => *size,
        }
    }

    fn pixels(&mut self) -> &mut [u32] {
        match self {
            Self::Dib(dib) => dib.pixels(),
            Self::Memory(pixels, _) => pixels,
        }
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

/// `WM_LBUTTONDOWN`'s `wParam` flags for Shift and Ctrl (WinUser.h's `MK_SHIFT`, `MK_CONTROL`).
const MK_SHIFT: WPARAM = 0x0004;
const MK_CONTROL: WPARAM = 0x0008;

impl State {
    /// Whether a press at `at`, now (the message's time), is a double-click's second: within
    /// `GetDoubleClickTime` of the last press that could begin one, and within the double-click
    /// rectangle (`SM_CXDOUBLECLK` by `SM_CYDOUBLECLK`) centered on it.
    fn double_click(&self, at: PointPx) -> bool {
        // SAFETY: no preconditions; the time of the message being handled.
        let now = unsafe { GetMessageTime() };
        // SAFETY: no preconditions.
        let (limit, w, h) = unsafe {
            (
                GetDoubleClickTime(),
                GetSystemMetrics(SM_CXDOUBLECLK),
                GetSystemMetrics(SM_CYDOUBLECLK),
            )
        };
        let double = self.last_press.get().is_some_and(|(p, then)| {
            // Message times wrap after 49.7 days; the difference doesn't.
            let elapsed = now.wrapping_sub(then).unsigned_abs();
            elapsed <= limit && 2 * (at.x - p.x).abs() <= w && 2 * (at.y - p.y).abs() <= h
        });
        self.last_press.set((!double).then_some((at, now)));
        double
    }
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
    use crate::gui::{Compose, compose::on_wine};

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
        open_with(frames, Compose::Never)
    }

    fn open_with(frames: u32, compose: Compose) -> (Window, Log, Rc<Cell<u32>>) {
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
            compose,
        };
        let window = Window::new(&options, Box::new(handler)).expect("a window");
        (window, events, paints)
    }

    #[test]
    fn the_handler_learns_the_env_the_dpi_then_the_size_and_paints_its_damage() {
        let (window, events, paints) = open(0);
        let recorded = events.borrow().clone();
        let [
            Event::Presenter(presenter),
            Event::Env(env),
            Event::Dpi(dpi),
            ..,
        ] = recorded[..]
        else {
            panic!(
                "the first events aren't the presenter, the environment and the DPI: {recorded:?}"
            );
        };
        assert_eq!(presenter, Presenter::default(), "GDI");
        assert_eq!(env, Env::read());
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

    /// The last presenter the handler heard of.
    fn last_presenter(events: &Log) -> Option<Presenter> {
        events.borrow().iter().rev().find_map(|e| match e {
            Event::Presenter(p) => Some(*p),
            _ => None,
        })
    }

    /// T2 and T3: a window that may compose does, or (Wine) falls back to GDI for the same handler;
    /// either way the handler heard which, and paints its damage once.
    #[test]
    fn a_window_that_may_compose_presents_or_falls_back_to_gdi() {
        let (window, events, paints) = open_with(0, Compose::Any);
        let presenter = last_presenter(&events).expect("a presenter");
        println!("presenter: {presenter:?}");
        // SAFETY: this test's window, on its thread.
        let (composed, has_composer) =
            unsafe { with_state(window.hwnd, |s| (s.composed, s.composer.borrow().is_some())) }
                .expect("a live window");
        assert_eq!(presenter.composed, composed, "{:?}", events.borrow());
        assert_eq!(has_composer, composed);
        if !on_wine() {
            assert!(composed, "Windows always has WARP and DirectComposition");
        }
        assert!(!presenter.device_lost);
        // SAFETY: messages sent to this test's live window, on its thread.
        unsafe { SendMessageW(window.hwnd, WM_PAINT, 0, 0) };
        assert_eq!(paints.get(), 1);
        // SAFETY: as above.
        unsafe { SendMessageW(window.hwnd, WM_PAINT, 0, 0) };
        assert_eq!(paints.get(), 1, "nothing new was damaged");
    }

    /// T3: a lost device is rebuilt in the same window, and a move from hardware to WARP reaches
    /// the handler, so its materials go solid (TD-0009 §5.2).
    #[test]
    fn a_lost_device_is_rebuilt_and_leaving_hardware_is_heard() {
        let (window, events, paints) = open_with(0, Compose::Any);
        let hwnd = window.hwnd;
        // SAFETY: this test's window, on its thread (for every block below).
        if unsafe { with_state(hwnd, |s| s.composed) } != Some(true) {
            assert!(on_wine(), "{:?}", events.borrow());
            return;
        }
        // SAFETY: as above.
        let composer = |f: fn(&Composer) -> bool| unsafe {
            with_state(hwnd, |s| s.composer.borrow().as_ref().map(f)).flatten()
        };
        // SAFETY: as above.
        unsafe { SendMessageW(hwnd, WM_PAINT, 0, 0) };
        assert_eq!((paints.get(), composer(Composer::due)), (1, Some(false)));
        // As if it had composed on hardware: the runner has no GPU, so rebuilds land on WARP.
        let hardware = Presenter {
            composed: true,
            hardware: true,
            device_lost: false,
        };
        // SAFETY: as above.
        unsafe {
            with_state(hwnd, |s| {
                s.presenter.set(hardware);
                s.lost(hwnd);
            })
        };
        let warp = Presenter {
            composed: true,
            hardware: false,
            device_lost: true,
        };
        assert_eq!(last_presenter(&events), Some(warp));
        assert_eq!(composer(Composer::warp), Some(true));
        // A second loss within a minute, and a second of the last rebuild: the lost presenter
        // goes at once, and the new one, on WARP, waits on the timer.
        let heard = events.borrow().len();
        // SAFETY: as above.
        let waiting = unsafe {
            with_state(hwnd, |s| {
                s.lost(hwnd);
                s.rebuild_on.get()
            })
        };
        assert_eq!(waiting, Some(Some(Device::Warp)));
        assert_eq!(composer(Composer::warp), None);
        // Paints meanwhile neither rebuild nor spin.
        // SAFETY: as above.
        unsafe { SendMessageW(hwnd, WM_PAINT, 0, 0) };
        assert_eq!(composer(Composer::warp), None);
        // SAFETY: as above; the timer's message, as the loop would dispatch it.
        unsafe { SendMessageW(hwnd, WM_TIMER, REBUILD_TIMER, 0) };
        assert_eq!(composer(Composer::warp), Some(true));
        assert_eq!(events.borrow().len(), heard, "{:?}", events.borrow());
        // The new buffers show the canvas without the handler painting again.
        assert_eq!(composer(Composer::due), Some(true));
        // SAFETY: as above.
        unsafe { SendMessageW(hwnd, WM_PAINT, 0, 0) };
        assert_eq!((paints.get(), composer(Composer::due)), (1, Some(false)));
    }

    /// A settings signal reaches the handler only when a setting it sees changed.
    #[test]
    fn only_a_changed_environment_reaches_the_handler() {
        let (window, events, _) = open(0);
        let before = events.borrow().len();
        let power = WPARAM::try_from(PBT_POWERSETTINGCHANGE).expect("a WPARAM");
        // SAFETY: messages sent to this test's live window, on its thread.
        unsafe {
            SendMessageW(window.hwnd, WM_SETTINGCHANGE, 0, 0);
            SendMessageW(window.hwnd, WM_SYSCOLORCHANGE, 0, 0);
            SendMessageW(window.hwnd, WM_POWERBROADCAST, power, 0);
        };
        assert_eq!(events.borrow().len(), before, "{:?}", events.borrow());
        // As if the user switched apps to dark since the last reading.
        let real = Env::read();
        // SAFETY: this test's window, on its thread.
        unsafe {
            with_state(window.hwnd, |s| {
                s.env.set(Env {
                    dark: !real.dark,
                    ..real
                });
            })
        };
        // SAFETY: as above.
        unsafe { SendMessageW(window.hwnd, WM_SETTINGCHANGE, 0, 0) };
        assert_eq!(events.borrow().last(), Some(&Event::Env(real)));
        // SAFETY: as above.
        let heard = unsafe { with_state(window.hwnd, |s| s.env.get()) };
        assert_eq!(heard, Some(real));
    }

    /// Characters reach the handler whole: a surrogate pair as one, an unpaired half not at all.
    /// The wake timer fires once.
    #[test]
    fn typed_characters_arrive_whole_and_a_wake_fires_once() {
        let (window, events, _) = open(0);
        let hwnd = window.hwnd;
        let before = events.borrow().len();
        // SAFETY: messages sent to this test's live window, on its thread.
        unsafe {
            SendMessageW(hwnd, WM_CHAR, 0x61, 0);
            // U+1F600: D83D DE00. Then a lone low half, and a high half another character ends.
            SendMessageW(hwnd, WM_CHAR, 0xD83D, 0);
            SendMessageW(hwnd, WM_CHAR, 0xDE00, 0);
            SendMessageW(hwnd, WM_CHAR, 0xDE00, 0);
            SendMessageW(hwnd, WM_CHAR, 0xD83D, 0);
            SendMessageW(hwnd, WM_CHAR, 0x62, 0);
        };
        let chars: Vec<Event> = events.borrow()[before..].to_vec();
        assert_eq!(
            chars,
            [Event::Char('a'), Event::Char('\u{1F600}'), Event::Char('b')]
        );
        // SAFETY: this test's window, on its thread.
        unsafe {
            with_state(hwnd, |state| {
                WindowCx {
                    hwnd,
                    state,
                    surface: Surface::Main,
                }
                .wake_after(Some(10));
            })
        };
        let pump = || {
            // SAFETY: plain data; all-zero is a valid value.
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            // SAFETY: msg is a live local; the timer's message is dispatched to the window.
            while unsafe { PeekMessageW(&raw mut msg, null_mut(), 0, 0, PM_REMOVE) } != 0 {
                // SAFETY: msg is the message PeekMessageW just filled.
                unsafe { DispatchMessageW(&raw const msg) };
            }
        };
        let wakes = || {
            events
                .borrow()
                .iter()
                .filter(|e| **e == Event::Wake)
                .count()
        };
        let start = Instant::now();
        while wakes() == 0 && start.elapsed() < Duration::from_secs(5) {
            pump();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(wakes(), 1, "{:?}", events.borrow());
        // One shot: nothing more comes.
        std::thread::sleep(Duration::from_millis(60));
        pump();
        assert_eq!(wakes(), 1);
        // SAFETY: as above.
        assert_eq!(unsafe { with_state(hwnd, |s| s.wake.get()) }, Some(false));
        drop(window);
        // Take the WM_QUIT the destroyed window posted.
        pump();
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

    /// Serves a window with one button whose label counts its clicks, and logs what it's asked.
    struct Accessible {
        log: Rc<RefCell<Vec<String>>>,
        clicks: u32,
    }

    impl Accessible {
        fn tree(&self) -> accesskit::TreeUpdate {
            use accesskit::{Action, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
            let mut root = Node::new(Role::Window);
            root.set_children(vec![NodeId(1)]);
            let mut button = Node::new(Role::Button);
            button.set_label(format!("clicked {}", self.clicks));
            button.add_action(Action::Click);
            TreeUpdate {
                nodes: vec![(NodeId(0), root), (NodeId(1), button)],
                tree: Some(TreeInfo::new(NodeId(0))),
                tree_id: TreeId::ROOT,
                focus: NodeId(0),
            }
        }
    }

    impl Handler for Accessible {
        fn event(&mut self, _: Event, _: &mut WindowCx<'_>) -> bool {
            true
        }

        fn paint(&mut self, _: &mut Frame<'_>) {}

        fn a11y_tree(&mut self, _: Surface) -> Option<accesskit::TreeUpdate> {
            self.log.borrow_mut().push("tree".into());
            Some(self.tree())
        }

        fn a11y_update(&mut self, _: Surface) -> Option<accesskit::TreeUpdate> {
            self.log.borrow_mut().push("update".into());
            Some(self.tree())
        }

        fn a11y_action(&mut self, request: &accesskit::ActionRequest, _: &mut WindowCx<'_>) {
            self.log
                .borrow_mut()
                .push(format!("{:?} {}", request.action, request.target_node.0));
            self.clicks += 1;
        }
    }

    /// T2 (Wine's UI Automation): a client's first request gets the handler's tree; a request
    /// from another thread reaches the handler on the UI thread, and the change is served after.
    #[test]
    fn ui_automation_gets_the_tree_and_requests_reach_the_ui_thread() {
        use accesskit::{Action, ActionHandler, ActionRequest, NodeId, TreeId};
        let log = Rc::new(RefCell::new(Vec::new()));
        let handler = Accessible {
            log: Rc::clone(&log),
            clicks: 0,
        };
        let options = WindowOptions {
            title: "dpro-win a11y test",
            size: (200, 100),
            min_size: (100, 100),
            compose: Compose::Never,
        };
        let window = Window::new(&options, Box::new(handler)).expect("a window");
        // Events before any client asked don't ask the handler for its tree.
        // SAFETY: a message sent to this test's live window, on its thread.
        unsafe { SendMessageW(window.hwnd, WM_SETFOCUS, 0, 0) };
        assert_eq!(log.borrow().len(), 0, "{:?}", log.borrow());
        // UiaRootObjectId, as a UI Automation client's first request sends it.
        // SAFETY: as above.
        unsafe { SendMessageW(window.hwnd, WM_GETOBJECT, 0, -25) };
        assert_eq!(log.borrow().first().map(String::as_str), Some("tree"));
        // SAFETY: as above.
        let requests =
            unsafe { with_state(window.hwnd, |s| Arc::clone(&s.requests)) }.expect("a live window");
        std::thread::spawn(move || {
            ToUiThread(requests).do_action(ActionRequest {
                action: Action::Click,
                target_tree: TreeId::ROOT,
                target_node: NodeId(1),
                data: None,
            });
        })
        .join()
        .expect("the client thread");
        // SAFETY: plain data; all-zero is a valid value.
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: msg is a live local; the posted wake is dispatched to the window.
        while unsafe { PeekMessageW(&raw mut msg, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            // SAFETY: msg is the message PeekMessageW just filled.
            unsafe { DispatchMessageW(&raw const msg) };
        }
        assert_eq!(*log.borrow(), ["tree", "Click 1", "update"]);
        drop(window);
        // SAFETY: as above; takes the WM_QUIT the destroyed window posted.
        while unsafe { PeekMessageW(&raw mut msg, null_mut(), 0, 0, PM_REMOVE) } != 0 {}
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

    /// TD-0009 A2.4, slice 5b1: a press carries the Shift and Ctrl that Windows sends with it
    /// (`MK_SHIFT`, `MK_CONTROL`), and a second press within the double-click rectangle and time
    /// is a double-click; a third starts over, as Windows counts them. The window has no
    /// `CS_DBLCLKS`, so a popup's watch for presses outside it sees every press as a button-down.
    ///
    /// The presses are sent, not posted, so they all share the last queued message's time.
    #[test]
    fn a_press_carries_its_modifiers_and_a_quick_second_one_is_a_double_click() {
        const MK_SHIFT: WPARAM = 0x0004;
        const MK_CONTROL: WPARAM = 0x0008;
        let (window, events, _) = open(0);
        let press = |x: i16, y: i16, keys: WPARAM| {
            let lp = (LPARAM::from(y) << 16) | LPARAM::from(x);
            // SAFETY: messages sent to this test's live window, on its thread.
            unsafe {
                SendMessageW(window.hwnd, WM_LBUTTONDOWN, keys, lp);
                SendMessageW(window.hwnd, WM_LBUTTONUP, 0, lp);
            }
        };
        events.borrow_mut().clear();
        press(10, 10, MK_CONTROL);
        press(11, 10, MK_SHIFT);
        press(11, 10, 0);
        press(200, 10, 0);
        press(10, 10, MK_SHIFT | MK_CONTROL);
        let presses: Vec<(i32, bool, bool, bool)> = events
            .borrow()
            .iter()
            .filter_map(|e| match *e {
                Event::PointerDown { at, mods, double } => {
                    Some((at.x, mods.shift, mods.ctrl, double))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            presses,
            [
                (10, false, true, false),
                (11, true, false, true),
                (11, false, false, false),
                (200, false, false, false),
                (10, true, true, false),
            ],
            "the last is too far from the one before"
        );
        drop(window);
        // SAFETY: plain data; all-zero is a valid value.
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: msg is a live local; PM_REMOVE takes the message.
        while unsafe { PeekMessageW(&raw mut msg, null_mut(), 0, 0, PM_REMOVE) } != 0 {}
    }
}
