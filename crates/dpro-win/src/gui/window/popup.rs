//! A window's popups (TD-0009 amendment A1.5): menus, a dropdown's list and tooltips, each a
//! `WS_POPUP` window of class `dismpro.popup` owned by the main window, on its thread.
//!
//! - **One handler.** A popup has no handler of its own. Its events, paints and accessibility
//!   trees go to its owner's handler, marked with the [`Surface`] they're for, and the
//!   [`WindowCx`] they get is the owner's. So the model and every UI live in one place, and an
//!   item's message can change the main window at once.
//! - **Show and hide** ([`WindowCx::show_popup`], [`WindowCx::hide_popup`]) are queued and done
//!   once the handler returns, because both send messages that come back to it (the popup's size,
//!   the keyboard focus moving).
//! - **Activation.** A menu or a list activates when it shows and takes the keyboard focus. The
//!   owner keeps drawing its frame active meanwhile (`WM_NCACTIVATE`), and hears
//!   [`Event::Focus`]`(false)` as usual. A tooltip never activates, and clicks fall through it to
//!   the owner (`HTTRANSPARENT`, `MA_NOACTIVATE`).
//! - **Light dismiss.** [`Event::Dismiss`] comes for a popup that's deactivated (a click outside
//!   it, another window activated) or cancelled (`WM_CANCELMODE`), and for every shown popup when
//!   the owner moves, resizes, minimizes or changes DPI. A click on the owner while a menu or a
//!   list is shown activates the owner and reaches nothing else, neither its press
//!   (`MA_ACTIVATEANDEAT`) nor its release, as with Windows' menus. One the handler hid while
//!   the owner was minimized isn't shown again at the restore. Alt+F4 in a popup closes the
//!   owner, not the popup's window.
//! - **Reuse.** Each kind's window is made at its first show and kept, hidden, for the next, so a
//!   later open makes no window, no swap chain and no UI Automation adapter. A popup presents on
//!   its owner's device when the owner is composed, else with GDI. One whose presenter is lost
//!   moves to the owner's current device, or else is closed and made again at its next show. One
//!   that can't be made at all is dismissed, so the handler doesn't hold it open.
//! - **Destruction.** The owner destroys its popups before itself, and from then on their messages
//!   no longer reach the handler.

use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    mem::transmute,
    ptr::{null, null_mut},
    rc::Rc,
    sync::{Arc, OnceLock},
    time::Instant,
};

use accesskit_windows::Adapter;
use windows_sys::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{
            Dwm::{
                DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DWMWCP_ROUNDSMALL,
                DwmSetWindowAttribute,
            },
            Gdi::{
                ClientToScreen, GetMonitorInfoW, HMONITOR, InvalidateRect,
                MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect, ScreenToClient,
            },
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{
                GetActiveWindow, GetCapture, ReleaseCapture, SetActiveWindow,
            },
            WindowsAndMessaging::{
                CS_DROPSHADOW, CreateWindowExW, DefWindowProcW, HTCLIENT, HTTRANSPARENT, HWND_TOP,
                MA_ACTIVATEANDEAT, MA_NOACTIVATE, PostMessageW, SC_CLOSE, SW_HIDE,
                SW_PARENTOPENING, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SendMessageW,
                SetCursor, SetWindowPos, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_CANCELMODE,
                WM_CONTEXTMENU, WM_CREATE, WM_DPICHANGED, WM_GETMINMAXINFO, WM_LBUTTONDOWN,
                WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOVE,
                WM_NCACTIVATE, WM_NCHITTEST, WM_POWERBROADCAST, WM_RBUTTONDOWN, WM_RBUTTONUP,
                WM_SETCURSOR, WM_SETTINGCHANGE, WM_SHOWWINDOW, WM_SYSCOLORCHANGE, WM_SYSCOMMAND,
                WM_THEMECHANGED, WM_WTSSESSION_CHANGE, WM_XBUTTONDOWN, WM_XBUTTONUP,
                WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::{HRESULT, w},
};

use super::{
    BASE_DPI, Inert, Kind, Life, REBUILD_GAP, State, Window, WindowCx, apply_backdrop,
    extend_frame, high, low, point, register_class, with_state,
};
use crate::{
    dll::{self, Proc},
    gui::{
        Backdrop, Event, PointPx, PopupKind, Presenter, RectPx, Surface,
        a11y::ToUiThread,
        compose::{Composer, Gpu},
    },
};

const POPUP_CLASS: *const u16 = w!("dismpro.popup");

/// How many ops one handler call may queue; more are dropped (a handler gone wild).
const MAX_OPS: usize = 16;

/// What the handler asked of a popup, done once it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Op {
    Show {
        kind: PopupKind,
        rect: RectPx,
        backdrop: Backdrop,
    },
    Hide(PopupKind),
}

/// A main window's popups. Only brief borrows are taken: showing, hiding and destroying a popup
/// all send messages that may come back here.
#[derive(Default)]
pub(super) struct Popups {
    /// Each kind's window, made at its first show.
    windows: RefCell<[Option<Window>; 3]>,
    /// Which kinds are shown, as of the last op done.
    shown: Cell<[bool; 3]>,
    ops: RefCell<Vec<Op>>,
    /// The ops are being done: a handler call this brings queues more for the same loop.
    applying: Cell<bool>,
    /// A menu or a list is being shown and takes the activation: the owner keeps its active frame.
    keep_active: Cell<bool>,
    /// The button-up message of a press on the owner that only dismissed a popup
    /// (`MA_ACTIVATEANDEAT` eats the press alone), or 0: that release reaches nothing either.
    eaten_up: Cell<u32>,
    /// The owner is being destroyed: its popups' messages no longer reach the handler.
    pub(super) closing: Cell<bool>,
}

impl Popups {
    pub(super) fn shown(&self, kind: PopupKind) -> bool {
        self.shown.get()[kind.index()]
    }

    fn set_shown(&self, kind: PopupKind, on: bool) {
        let mut shown = self.shown.get();
        shown[kind.index()] = on;
        self.shown.set(shown);
    }

    /// A menu or a list is shown, and so holds (or is taking) the activation.
    fn activating_shown(&self) -> bool {
        self.shown(PopupKind::Menu) || self.shown(PopupKind::List)
    }

    /// The window of `kind`, if it's made and alive.
    pub(super) fn hwnd(&self, kind: PopupKind) -> Option<HWND> {
        let windows = self.windows.try_borrow().ok()?;
        windows[kind.index()]
            .as_ref()
            .filter(|w| w.life.get() == Life::Alive)
            .map(|w| w.hwnd)
    }

    fn queue(&self, op: Op) {
        if let Ok(mut ops) = self.ops.try_borrow_mut()
            && ops.len() < MAX_OPS
        {
            ops.push(op);
        }
    }

    fn next(&self) -> Option<Op> {
        let mut ops = self.ops.try_borrow_mut().ok()?;
        (!ops.is_empty()).then(|| ops.remove(0))
    }
}

impl WindowCx<'_> {
    /// The surface this event is for: the window, or one of its popups.
    #[must_use]
    pub fn surface(&self) -> Surface {
        self.surface
    }

    /// As [`invalidate`](Self::invalidate), on `surface`'s canvas. A popup not made yet ignores
    /// it: it hears its size once it is, and paints all of it then.
    pub fn invalidate_in(&mut self, surface: Surface, r: RectPx) {
        match surface {
            Surface::Main => self.invalidate(r),
            Surface::Popup(kind) => {
                if let Some(hwnd) = self.state.popups.hwnd(kind) {
                    // SAFETY: a popup of this window: this module's class, on this thread.
                    unsafe { with_state(hwnd, |p| p.invalidate_rect(hwnd, r)) };
                }
            }
        }
    }

    /// Shows this window's popup of `kind` at `rect`, in screen pixels and already placed, over
    /// `backdrop` if the popup is composed; moves it if it's shown. A menu or a list takes the
    /// activation and the keyboard focus; a tooltip takes neither. This happens once the handler
    /// returns: the popup's surface then hears its [`Event::Presenter`] (when it's made) and its
    /// [`Event::Resize`] (when its size changes), and it paints what the handler invalidates in it.
    pub fn show_popup(&mut self, kind: PopupKind, rect: RectPx, backdrop: Backdrop) {
        self.state.popups.queue(Op::Show {
            kind,
            rect,
            backdrop,
        });
    }

    /// Hides this window's popup of `kind`, keeping it for the next show. A menu or a list gives
    /// the activation back to this window. This happens once the handler returns.
    pub fn hide_popup(&mut self, kind: PopupKind) {
        self.state.popups.queue(Op::Hide(kind));
    }

    /// Whether this window's popup of `kind` is shown, as of the last show or hide done.
    #[must_use]
    pub fn popup_shown(&self, kind: PopupKind) -> bool {
        self.state.popups.shown(kind)
    }

    /// `p`, in this window's client pixels, in screen pixels.
    #[must_use]
    pub fn client_to_screen(&self, p: PointPx) -> PointPx {
        let mut pt = POINT { x: p.x, y: p.y };
        // SAFETY: hwnd is live, and pt a live local; on failure pt stays as it was.
        unsafe { ClientToScreen(self.hwnd, &raw mut pt) };
        PointPx { x: pt.x, y: pt.y }
    }

    /// The work area (the monitor less the taskbar) of the monitor nearest `r`, in screen pixels,
    /// and that monitor's DPI: where a popup for `r` may go, and the scale to lay it out at.
    #[must_use]
    pub fn work_area(&self, r: RectPx) -> (RectPx, u32) {
        work_area(r, self.state.dpi.get())
    }
}

impl State {
    /// Does what the handler asked of the popups, once it has returned. A handler call this
    /// brings (a popup's size, the focus moving) may ask for more, which this loop does too.
    pub(super) fn apply_popups(&self, hwnd: HWND) {
        if self.popups.applying.replace(true) {
            return;
        }
        // Bounded: a handler that asks again at every answer can't hold the thread.
        for _ in 0..MAX_OPS * 4 {
            match self.popups.next() {
                Some(Op::Show {
                    kind,
                    rect,
                    backdrop,
                }) => self.show_now(hwnd, kind, rect, backdrop),
                Some(Op::Hide(kind)) => self.hide_now(hwnd, kind),
                None => break,
            }
        }
        if let Ok(mut ops) = self.popups.ops.try_borrow_mut() {
            ops.clear();
        }
        self.popups.applying.set(false);
    }

    fn show_now(&self, owner: HWND, kind: PopupKind, rect: RectPx, backdrop: Backdrop) {
        if self.popups.closing.get() || self.kind != Kind::Main {
            return;
        }
        let Some(hwnd) = self.popup_for(owner, kind, rect) else {
            // No window to show it in: the handler hears it dismissed, so what it holds open
            // closes, and its next show tries again (A1.8).
            self.dispatch_as(owner, Surface::Popup(kind), |h, cx| {
                h.event(Event::Dismiss, cx)
            });
            return;
        };
        // SAFETY: a popup of this window: this module's class, on this thread.
        unsafe {
            with_state(hwnd, |p| {
                if p.backdrop.replace(backdrop) != backdrop && p.composed {
                    apply_backdrop(hwnd, backdrop);
                }
            })
        };
        let (w, h) = (
            rect.x1.saturating_sub(rect.x0).max(1),
            rect.y1.saturating_sub(rect.y0).max(1),
        );
        // SAFETY: as above. WM_SIZE comes back through the procedure, and the handler hears it.
        unsafe { SetWindowPos(hwnd, HWND_TOP, rect.x0, rect.y0, w, h, SWP_NOACTIVATE) };
        self.popups.set_shown(kind, true);
        if kind == PopupKind::Tooltip {
            // SAFETY: as above.
            unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
        } else {
            // The owner gives up the activation, not its active frame.
            self.popups.keep_active.set(true);
            // SAFETY: as above. It activates: WM_ACTIVATE and WM_SETFOCUS reach the handler.
            unsafe { ShowWindow(hwnd, SW_SHOW) };
            self.popups.keep_active.set(false);
        }
    }

    fn hide_now(&self, owner: HWND, kind: PopupKind) {
        if !self.popups.shown(kind) {
            return;
        }
        self.popups.set_shown(kind, false);
        let Some(hwnd) = self.popups.hwnd(kind) else {
            return;
        };
        // SAFETY: a popup of this window: this module's class, on this thread (every block below).
        let deactivating = unsafe {
            with_state(hwnd, |p| {
                p.hiding.set(true);
                p.deactivating.get()
            })
        }
        .unwrap_or(false);
        // A menu or a list gives the activation back to its owner, so Windows doesn't pick
        // another window; unless it's losing it already, to the owner or to another app.
        // SAFETY: no preconditions.
        if !deactivating && unsafe { GetActiveWindow() } == hwnd {
            // SAFETY: owner is a live window of this thread.
            unsafe { SetActiveWindow(owner) };
        }
        // A press in it (an item held while Esc closed it) ends here: its release must not reach
        // the hidden window. WM_CAPTURECHANGED comes back through the procedure, and cancels it.
        // SAFETY: no preconditions; this thread's capture.
        if unsafe { GetCapture() } == hwnd {
            // SAFETY: no preconditions.
            unsafe { ReleaseCapture() };
        }
        // SAFETY: hwnd is a live popup of this window, on this thread.
        unsafe { ShowWindow(hwnd, SW_HIDE) };
        // SAFETY: as above.
        unsafe { with_state(hwnd, |p| p.hiding.set(false)) };
    }

    /// The popup of `kind`, made at `rect` if it doesn't exist yet or its presenter broke:
    /// composed on this window's devices if it's composed, else (or if that fails) with GDI.
    fn popup_for(&self, owner: HWND, kind: PopupKind, rect: RectPx) -> Option<HWND> {
        if let Some(hwnd) = self.popups.hwnd(kind) {
            // SAFETY: a popup of this window: this module's class, on this thread.
            let broken = unsafe { with_state(hwnd, |p| p.broken.get()) }.unwrap_or(true);
            if !broken {
                return Some(hwnd);
            }
        }
        // The old one goes first, outside the borrow: destroying it sends messages.
        let old = self.popups.windows.try_borrow_mut().ok()?[kind.index()].take();
        drop(old);
        let gpu = if self.composed {
            self.composer
                .try_borrow()
                .ok()
                .and_then(|c| c.as_ref().filter(|c| c.healthy()).map(Composer::gpu))
        } else {
            None
        };
        let composed = gpu.is_some();
        self.make_popup(owner, kind, rect, gpu).or_else(|| {
            if composed {
                self.make_popup(owner, kind, rect, None)
            } else {
                None
            }
        })
    }

    /// Makes the popup of `kind`, hidden, at `rect`, presenting on `gpu` if given. It's in its
    /// slot before it hears anything, so the handler can invalidate it at its first size.
    fn make_popup(
        &self,
        owner: HWND,
        kind: PopupKind,
        rect: RectPx,
        gpu: Option<Gpu>,
    ) -> Option<HWND> {
        let window = create(owner, kind, rect, gpu.as_ref())?;
        let hwnd = window.hwnd;
        self.popups.windows.try_borrow_mut().ok()?[kind.index()] = Some(window);
        // SAFETY: the popup just made: this module's class, on this thread.
        let ready = unsafe { with_state(hwnd, |p| p.init_popup(hwnd, kind, gpu)) } == Some(true);
        if !ready {
            let failed = self
                .popups
                .windows
                .try_borrow_mut()
                .ok()
                .and_then(|mut w| w[kind.index()].take());
            drop(failed);
            return None;
        }
        Some(hwnd)
    }

    /// A new popup's presenter, corners and adapter, before it ever shows. False if its swap
    /// chain can't be shown.
    fn init_popup(&self, hwnd: HWND, kind: PopupKind, gpu: Option<Gpu>) -> bool {
        self.sync_size(hwnd);
        // Its first size may have come during CreateWindowExW, before the handler could reach
        // it; a new canvas is painted whole anyway.
        let size = self.size.get();
        self.invalidate_rect(
            hwnd,
            RectPx {
                x0: 0,
                y0: 0,
                x1: i32::try_from(size.w).unwrap_or(i32::MAX),
                y1: i32::try_from(size.h).unwrap_or(i32::MAX),
            },
        );
        if let Some(gpu) = gpu {
            let Some(composer) = Composer::new(gpu, hwnd, self.size.get()) else {
                return false;
            };
            let Ok(mut slot) = self.composer.try_borrow_mut() else {
                return false;
            };
            *slot = Some(composer);
            drop(slot);
            extend_frame(hwnd);
            apply_backdrop(hwnd, self.backdrop.get());
        }
        round_corners(hwnd, kind);
        // Before the window shows, as `Adapter::new` requires.
        self.requests.attach(hwnd);
        let actions = ToUiThread(Arc::clone(&self.requests));
        let adapter = Adapter::new(accesskit_windows::HWND(hwnd), false, actions);
        if let Ok(mut slot) = self.a11y.try_borrow_mut() {
            *slot = Some(adapter);
        }
        self.send(hwnd, Event::Presenter(self.presenter.get()));
        true
    }

    /// A popup's own handling of a message, before what every window does. `Some(result)` ends
    /// it (`Some(None)`: default handling); `None` goes on.
    #[allow(
        clippy::option_option,
        reason = "handled with a result, handled by default, or not handled here"
    )]
    pub(super) fn popup_message(
        &self,
        hwnd: HWND,
        owner: HWND,
        kind: PopupKind,
        msg: u32,
        wp: WPARAM,
        lp: LPARAM,
    ) -> Option<Option<LRESULT>> {
        match msg {
            // No subscriptions and no environment: those are the owner's.
            WM_CREATE => {
                // SAFETY: hwnd is live.
                let dpi = unsafe { GetDpiForWindow(hwnd) };
                if dpi != 0 {
                    self.dpi.set(dpi);
                }
                Some(Some(0))
            }
            // Broadcasts reach the owner too, and a popup has no minimum size.
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE | WM_THEMECHANGED | WM_WTSSESSION_CHANGE
            | WM_POWERBROADCAST | WM_GETMINMAXINFO => Some(None),
            // Placed by the owner, at its monitor's DPI already: no suggested rectangle.
            WM_DPICHANGED => {
                self.dpi
                    .set(u32::try_from((wp >> 16) & 0xFFFF).unwrap_or(BASE_DPI));
                Some(Some(0))
            }
            WM_ACTIVATE if wp & 0xFFFF == usize::try_from(WA_INACTIVE).unwrap_or(0) => {
                self.deactivating.set(true);
                if !self.hiding.get() {
                    self.send(hwnd, Event::Dismiss);
                }
                self.deactivating.set(false);
                let other: HWND =
                    std::ptr::with_exposed_provenance_mut::<c_void>(lp.cast_unsigned());
                if other != owner {
                    // The owner kept its active frame for this popup; another app has the
                    // activation now, so the frame goes inactive too.
                    // SAFETY: owner is a live window of this thread.
                    unsafe { SendMessageW(owner, WM_NCACTIVATE, 0, 0) };
                }
                Some(None)
            }
            WM_CANCELMODE => {
                if !self.hiding.get() {
                    self.send(hwnd, Event::Dismiss);
                }
                Some(None)
            }
            // A tooltip is never the target of a click: it goes to the owner beneath, which
            // shares this thread (A1.5).
            WM_NCHITTEST if kind == PopupKind::Tooltip => {
                Some(Some(LRESULT::try_from(HTTRANSPARENT).unwrap_or(-1)))
            }
            WM_MOUSEACTIVATE if kind == PopupKind::Tooltip => {
                Some(Some(LRESULT::try_from(MA_NOACTIVATE).unwrap_or(3)))
            }
            WM_SETCURSOR if low(lp) == HTCLIENT => {
                // SAFETY: a system cursor; no other preconditions.
                unsafe { SetCursor(self.cursors[0]) };
                Some(Some(1))
            }
            // A popup has no context menu of its own.
            WM_CONTEXTMENU => Some(Some(0)),
            // Windows hides an owner's popups when it minimizes and shows them again when it's
            // restored. One the handler hid meanwhile (the minimize dismissed it) stays hidden.
            WM_SHOWWINDOW if wp != 0 && lp == LPARAM::try_from(SW_PARENTOPENING).unwrap_or(3) => {
                // SAFETY: the owner is a window of this module's class on this thread, alive
                // while its popups are.
                let shown = unsafe { with_state(owner, |o| o.popups.shown(kind)) }.unwrap_or(false);
                if shown { Some(None) } else { Some(Some(0)) }
            }
            // Alt+F4 in a menu or a list means the window: the owner hears it, after this message.
            // Never the popup's own close, which would destroy it.
            WM_SYSCOMMAND if u32::try_from(wp & 0xFFF0).ok() == Some(SC_CLOSE) => {
                // SAFETY: owner is a live window; a posted message carries no pointers.
                unsafe {
                    PostMessageW(
                        owner,
                        WM_SYSCOMMAND,
                        WPARAM::try_from(SC_CLOSE).unwrap_or(0xF060),
                        0,
                    )
                };
                Some(Some(0))
            }
            _ => None,
        }
    }

    /// A main window's handling of the messages about its popups, before what every window does.
    #[allow(
        clippy::option_option,
        reason = "handled with a result, handled by default, or not handled here"
    )]
    pub(super) fn owner_message(
        &self,
        hwnd: HWND,
        msg: u32,
        wp: WPARAM,
        lp: LPARAM,
    ) -> Option<Option<LRESULT>> {
        match msg {
            // Its menu or list is taking the activation: the frame stays active (A1.3).
            WM_NCACTIVATE if wp == 0 && self.popups.keep_active.get() => {
                // SAFETY: default handling, told the frame is active.
                Some(Some(unsafe { DefWindowProcW(hwnd, msg, 1, lp) }))
            }
            // A click on the window while its menu or list is open closes that (it's deactivated)
            // and reaches nothing else: the press is eaten here, its release below. Without that,
            // a right-click's release would ask for another menu at once (A1.4).
            WM_MOUSEACTIVATE if low(lp) == HTCLIENT && self.popups.activating_shown() => {
                self.popups.eaten_up.set(button_up(high(lp)));
                Some(Some(LRESULT::try_from(MA_ACTIVATEANDEAT).unwrap_or(2)))
            }
            // A press that reached the window: any release eaten earlier never came here.
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => {
                self.popups.eaten_up.set(0);
                None
            }
            WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP
                if self.popups.eaten_up.get() == msg =>
            {
                self.popups.eaten_up.set(0);
                // WM_XBUTTONUP's handled result is TRUE.
                Some(Some(LRESULT::from(msg == WM_XBUTTONUP)))
            }
            // Its popups no longer stand by their anchors.
            WM_MOVE => {
                self.dismiss_popups(hwnd);
                Some(Some(0))
            }
            WM_CONTEXTMENU => {
                let Some(at) = self.context_point(hwnd, lp) else {
                    // Not over the client area (the title bar): the system menu's.
                    return Some(None);
                };
                if self.send(hwnd, Event::ContextMenu(at)) == Some(true) {
                    Some(Some(0))
                } else {
                    Some(None)
                }
            }
            _ => None,
        }
    }

    /// Where a `WM_CONTEXTMENU` asks for a menu: `Some(None)` from the keyboard (the point is
    /// −1, −1), `Some(point)` in client pixels from the pointer, and `None` from outside the client
    /// area.
    #[allow(
        clippy::option_option,
        reason = "the keyboard's request, the pointer's, or none of ours"
    )]
    fn context_point(&self, hwnd: HWND, lp: LPARAM) -> Option<Option<PointPx>> {
        let p = point(lp);
        if p.x == -1 && p.y == -1 {
            return Some(None);
        }
        let mut pt = POINT { x: p.x, y: p.y };
        // SAFETY: hwnd is live, and pt a live local.
        if unsafe { ScreenToClient(hwnd, &raw mut pt) } == 0 {
            return None;
        }
        let size = self.size.get();
        let inside = |v: i32, len: u32| u32::try_from(v).is_ok_and(|v| v < len);
        (inside(pt.x, size.w) && inside(pt.y, size.h)).then_some(Some(PointPx { x: pt.x, y: pt.y }))
    }

    /// Every shown popup must close: the handler hears [`Event::Dismiss`] for each.
    pub(super) fn dismiss_popups(&self, hwnd: HWND) {
        for kind in PopupKind::ALL {
            if self.popups.shown(kind) {
                self.dispatch_as(hwnd, Surface::Popup(kind), |h, cx| {
                    h.event(Event::Dismiss, cx)
                });
            }
        }
    }

    /// Destroys the popups, before the window itself goes; their messages stop reaching the
    /// handler first.
    pub(super) fn destroy_popups(&self) {
        self.popups.closing.set(true);
        let windows = self
            .popups
            .windows
            .try_borrow_mut()
            .map(|mut w| std::mem::take(&mut *w))
            .unwrap_or_default();
        self.popups.shown.set([false; 3]);
        drop(windows);
    }

    /// Serves what changed in each popup's tree to that popup's adapter.
    pub(super) fn a11y_sync_popups(&self) {
        for kind in PopupKind::ALL {
            if let Some(hwnd) = self.popups.hwnd(kind) {
                // SAFETY: a popup of this window: this module's class, on this thread.
                unsafe { with_state(hwnd, State::a11y_sync) };
            }
        }
    }

    /// A popup's presenter was lost (a present or resize failed, or DWM restarted): it moves to
    /// its owner's current devices and is presented whole at the next paint. A second loss within
    /// [`REBUILD_GAP`], or no devices, marks it broken: the handler hears [`Event::Dismiss`], and
    /// its next show makes a new window.
    pub(super) fn popup_lost(&self, hwnd: HWND, owner: HWND) {
        if self.broken.get() {
            return;
        }
        let now = Instant::now();
        let again = self
            .lost_at
            .replace(Some(now))
            .is_some_and(|t| now.saturating_duration_since(t) < REBUILD_GAP);
        // SAFETY: the owner is a window of this module's class on this thread, alive while its
        // popups are.
        let gpu = (!again)
            .then(|| unsafe {
                with_state(owner, |o| {
                    o.composer
                        .try_borrow()
                        .ok()
                        .and_then(|c| c.as_ref().filter(|c| c.healthy()).map(Composer::gpu))
                })
            })
            .flatten()
            .flatten();
        let rebuilt = {
            let Ok(mut slot) = self.composer.try_borrow_mut() else {
                return;
            };
            // The old one goes first: a window has one composition target at a time.
            *slot = None;
            *slot = gpu.and_then(|g| Composer::new(g, hwnd, self.size.get()));
            slot.is_some()
        };
        if rebuilt {
            // SAFETY: hwnd is live. Sends no message; the paint presents the canvas whole.
            unsafe { InvalidateRect(hwnd, null(), 0) };
        } else {
            self.broken.set(true);
            self.send(hwnd, Event::Dismiss);
        }
    }
}

/// A new popup window of `kind` owned by `owner`, hidden, at `rect`, with a state that routes to
/// its owner's handler. `None` if it can't be made.
fn create(owner: HWND, kind: PopupKind, rect: RectPx, gpu: Option<&Gpu>) -> Option<Window> {
    // SAFETY: no preconditions; a null name means this executable.
    let instance = unsafe { GetModuleHandleW(null()) };
    register_class(instance, POPUP_CLASS, CS_DROPSHADOW).ok()?;
    let composed = gpu.is_some();
    let presenter = Presenter {
        composed,
        hardware: gpu.is_some_and(|g| !g.warp()),
        device_lost: false,
    };
    // SAFETY: owner is live.
    let dpi = unsafe { GetDpiForWindow(owner) };
    let life = Rc::new(Cell::new(Life::Unborn));
    let state = Box::into_raw(Box::new(State {
        kind: Kind::Popup { owner, kind },
        quit_on_destroy: Cell::new(false),
        ..State::new(
            Box::new(Inert),
            presenter,
            (0, 0),
            if dpi == 0 { BASE_DPI } else { dpi },
            Rc::clone(&life),
        )
    }));
    let mut ex_style = WS_EX_TOOLWINDOW;
    if kind == PopupKind::Tooltip {
        ex_style |= WS_EX_NOACTIVATE;
    }
    if composed {
        ex_style |= WS_EX_NOREDIRECTIONBITMAP;
    }
    let (w, h) = (
        rect.x1.saturating_sub(rect.x0).max(1),
        rect.y1.saturating_sub(rect.y0).max(1),
    );
    // SAFETY: the class is registered, the title is a static, and `state` is a live allocation
    // the window takes at WM_NCCREATE. `owner` (a WS_POPUP's parent) becomes its owner.
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            POPUP_CLASS,
            w!(""),
            WS_POPUP,
            rect.x0,
            rect.y0,
            w,
            h,
            owner,
            null_mut(),
            instance,
            state.cast(),
        )
    };
    if hwnd.is_null() {
        if life.get() == Life::Unborn {
            // SAFETY: the window never took the state, so this is its only owner.
            drop(unsafe { Box::from_raw(state) });
        }
        return None;
    }
    Some(Window { hwnd, life })
}

/// The release of the button whose press is the mouse message `down`, or 0. Neither class asks
/// for double-clicks (`CS_DBLCLKS`), so a press is always a button-down.
const fn button_up(down: u32) -> u32 {
    match down {
        WM_LBUTTONDOWN => WM_LBUTTONUP,
        WM_RBUTTONDOWN => WM_RBUTTONUP,
        WM_MBUTTONDOWN => WM_MBUTTONUP,
        WM_XBUTTONDOWN => WM_XBUTTONUP,
        _ => 0,
    }
}

/// Windows 11's rounded corners: a menu's and a list's 8 px, a tooltip's 4 px. Before Windows 11
/// the attribute doesn't exist, and the corners stay square.
fn round_corners(hwnd: HWND, kind: PopupKind) {
    let corner = if kind == PopupKind::Tooltip {
        DWMWCP_ROUNDSMALL
    } else {
        DWMWCP_ROUND
    };
    // SAFETY: hwnd is live; the value is one DWM_WINDOW_CORNER_PREFERENCE, read during the call.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            u32::try_from(DWMWA_WINDOW_CORNER_PREFERENCE).unwrap_or(0),
            (&raw const corner).cast(),
            u32::try_from(size_of_val(&corner)).unwrap_or(0),
        )
    };
}

/// The work area of the monitor nearest `r`, and its DPI (`fallback` if it can't be read).
fn work_area(r: RectPx, fallback: u32) -> (RectPx, u32) {
    let rect = RECT {
        left: r.x0,
        top: r.y0,
        right: r.x1,
        bottom: r.y1,
    };
    // SAFETY: rect is a live local; NEAREST always gives a monitor.
    let monitor = unsafe { MonitorFromRect(&raw const rect, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>()).unwrap_or(0),
        // SAFETY: plain data; all-zero is a valid value for the remaining fields.
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: a monitor handle, and a live MONITORINFO with its size set.
    let work = if unsafe { GetMonitorInfoW(monitor, &raw mut info) } != 0 {
        let w = info.rcWork;
        RectPx {
            x0: w.left,
            y0: w.top,
            x1: w.right,
            y1: w.bottom,
        }
    } else {
        // No monitor information: the popup isn't kept anywhere.
        RectPx {
            x0: i32::MIN / 4,
            y0: i32::MIN / 4,
            x1: i32::MAX / 4,
            y1: i32::MAX / 4,
        }
    };
    (work, monitor_dpi(monitor).unwrap_or(fallback))
}

type GetDpiForMonitor = unsafe extern "system" fn(HMONITOR, i32, *mut u32, *mut u32) -> HRESULT;

/// `monitor`'s effective DPI. `GetDpiForMonitor` is loaded at run time from System32's shcore,
/// whose API set a static import would require at start (`WinPE` may lack it).
fn monitor_dpi(monitor: HMONITOR) -> Option<u32> {
    static EXPORT: OnceLock<Option<GetDpiForMonitor>> = OnceLock::new();
    let get = (*EXPORT.get_or_init(|| {
        let shcore = dll::system("shcore.dll").ok()?;
        let export = dll::export(shcore, c"GetDpiForMonitor").ok()?;
        // SAFETY: the export has the signature shellscalingapi.h gives it.
        Some(unsafe { transmute::<Proc, GetDpiForMonitor>(export) })
    }))?;
    let (mut x, mut y) = (0, 0);
    // SAFETY: a monitor handle from MonitorFromRect, MDT_EFFECTIVE_DPI (0), and out-pointers to
    // locals.
    let hr = unsafe { get(monitor, 0, &raw mut x, &raw mut y) };
    (hr >= 0 && x != 0).then_some(x)
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GW_OWNER, GWL_EXSTYLE, GetClassNameW, GetWindow, GetWindowLongPtrW,
        IsWindow, IsWindowVisible, MA_ACTIVATE, MSG, PM_REMOVE, PeekMessageW, WM_LBUTTONDOWN,
        WM_PAINT,
    };

    use super::*;
    use crate::gui::{Compose, Frame, Handler, SizePx, WindowOptions};

    /// What a [`Popper`] heard, with the surface each was for.
    type Log = Rc<RefCell<Vec<(Surface, Event)>>>;

    /// Damages a surface whole at each size, hides a popup it's told to dismiss or close, and
    /// logs the surfaces it paints.
    struct Popper {
        log: Log,
        paints: Rc<RefCell<Vec<Surface>>>,
    }

    impl Handler for Popper {
        fn event(&mut self, event: Event, cx: &mut WindowCx<'_>) -> bool {
            self.log.borrow_mut().push((cx.surface(), event));
            match (cx.surface(), event) {
                (surface, Event::Resize(size)) => cx.invalidate_in(surface, whole(size)),
                (Surface::Popup(kind), Event::Dismiss | Event::Close) => cx.hide_popup(kind),
                _ => {}
            }
            true
        }

        fn paint(&mut self, frame: &mut Frame<'_>) {
            frame.pixels.fill(0xFF33_6699);
            self.paints.borrow_mut().push(frame.surface);
        }
    }

    fn whole(size: SizePx) -> RectPx {
        RectPx {
            x0: 0,
            y0: 0,
            x1: i32::try_from(size.w).unwrap_or(0),
            y1: i32::try_from(size.h).unwrap_or(0),
        }
    }

    fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> RectPx {
        RectPx { x0, y0, x1, y1 }
    }

    fn open() -> (Window, Log, Rc<RefCell<Vec<Surface>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let paints = Rc::new(RefCell::new(Vec::new()));
        let handler = Popper {
            log: Rc::clone(&log),
            paints: Rc::clone(&paints),
        };
        let options = WindowOptions {
            title: "dpro-win popup test",
            size: (320, 200),
            min_size: (100, 100),
            compose: Compose::Never,
        };
        let window = Window::new(&options, Box::new(handler)).expect("a window");
        (window, log, paints)
    }

    /// Runs `f` as the handler would, then does what it asked, as the window does once its
    /// handler returns.
    fn as_handler(window: &Window, f: impl FnOnce(&mut WindowCx<'_>)) {
        let hwnd = window.hwnd;
        // SAFETY: the test's window, on its thread.
        unsafe {
            with_state(hwnd, |state| {
                f(&mut WindowCx {
                    hwnd,
                    state,
                    surface: Surface::Main,
                });
                state.apply_popups(hwnd);
            })
        };
    }

    fn popup(window: &Window, kind: PopupKind) -> Option<HWND> {
        // SAFETY: the test's window, on its thread.
        unsafe { with_state(window.hwnd, |s| s.popups.hwnd(kind)) }.flatten()
    }

    fn visible(hwnd: HWND) -> bool {
        // SAFETY: no preconditions; a destroyed handle is not visible.
        unsafe { IsWindowVisible(hwnd) != 0 }
    }

    fn heard(log: &Log, surface: Surface, event: Event) -> usize {
        log.borrow()
            .iter()
            .filter(|(s, e)| *s == surface && *e == event)
            .count()
    }

    /// Takes every message waiting, the `WM_QUIT` a destroyed main window posts included.
    fn drain() {
        // SAFETY: plain data; all-zero is a valid value.
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: msg is a live local; each message is dispatched to its window.
        while unsafe { PeekMessageW(&raw mut msg, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            // SAFETY: msg is the message PeekMessageW just filled.
            unsafe { DispatchMessageW(&raw const msg) };
        }
    }

    /// T2: a popup is a tool window of class `dismpro.popup` owned by the main window. It hears
    /// its presenter and size on its own surface, paints there, and is kept for the next show.
    /// Destroying the owner destroys it.
    #[test]
    fn a_popup_is_an_owned_tool_window_kept_for_the_next_show() {
        let (window, log, paints) = open();
        let menu = Surface::Popup(PopupKind::Menu);
        as_handler(&window, |cx| {
            cx.show_popup(PopupKind::Menu, rect(100, 100, 300, 260), Backdrop::None);
            assert!(
                !cx.popup_shown(PopupKind::Menu),
                "only once the handler returns"
            );
        });
        let hwnd = popup(&window, PopupKind::Menu).expect("the menu's window");
        assert!(visible(hwnd));
        // SAFETY: hwnd is the test's live popup (every block below).
        let owner = unsafe { GetWindow(hwnd, GW_OWNER) };
        assert_eq!(owner, window.hwnd);
        let mut class = [0u16; 32];
        // SAFETY: as above; the buffer's length is passed.
        let len = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), 32) };
        let class = String::from_utf16_lossy(&class[..usize::try_from(len).unwrap_or(0)]);
        assert_eq!(class, "dismpro.popup");
        // SAFETY: as above.
        let ex_style = u32::try_from(unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) }).unwrap_or(0);
        assert_ne!(ex_style & WS_EX_TOOLWINDOW, 0, "no taskbar button");
        assert_eq!(ex_style & WS_EX_NOACTIVATE, 0, "a menu activates");
        assert_eq!(heard(&log, menu, Event::Presenter(Presenter::default())), 1);
        assert_eq!(
            heard(&log, menu, Event::Resize(SizePx { w: 200, h: 160 })),
            1,
            "{:?}",
            log.borrow()
        );
        // SAFETY: as above; the paint the handler's damage asked for.
        unsafe { SendMessageW(hwnd, WM_PAINT, 0, 0) };
        assert_eq!(*paints.borrow(), [menu]);
        // Hidden and shown again elsewhere: the same window, at its new size.
        as_handler(&window, |cx| cx.hide_popup(PopupKind::Menu));
        assert!(!visible(hwnd));
        as_handler(&window, |cx| {
            assert!(!cx.popup_shown(PopupKind::Menu));
            cx.show_popup(PopupKind::Menu, rect(40, 50, 140, 130), Backdrop::None);
        });
        assert_eq!(popup(&window, PopupKind::Menu), Some(hwnd));
        assert!(visible(hwnd));
        assert_eq!(heard(&log, menu, Event::Presenter(Presenter::default())), 1);
        assert_eq!(
            heard(&log, menu, Event::Resize(SizePx { w: 100, h: 80 })),
            1
        );
        // Hiding it never dismissed it: that was the handler's own doing.
        assert_eq!(heard(&log, menu, Event::Dismiss), 0, "{:?}", log.borrow());
        assert_eq!(Rc::strong_count(&log), 2);
        drop(window);
        // SAFETY: no preconditions on a handle that may be gone.
        assert_eq!(unsafe { IsWindow(hwnd) }, 0, "destroyed with its owner");
        assert_eq!(Rc::strong_count(&log), 1, "and the handler freed");
        drain();
    }

    /// T2: a tooltip passes clicks to what's beneath it and never takes the activation.
    #[test]
    fn a_tooltip_lets_clicks_through_and_never_activates() {
        let (window, _, _) = open();
        as_handler(&window, |cx| {
            cx.show_popup(PopupKind::Tooltip, rect(10, 10, 110, 34), Backdrop::None);
        });
        let tip = popup(&window, PopupKind::Tooltip).expect("the tooltip's window");
        assert!(visible(tip));
        // SAFETY: the test's live popup (every block below).
        let ex_style = u32::try_from(unsafe { GetWindowLongPtrW(tip, GWL_EXSTYLE) }).unwrap_or(0);
        assert_ne!(ex_style & WS_EX_NOACTIVATE, 0);
        // SAFETY: as above.
        let hit = unsafe { SendMessageW(tip, WM_NCHITTEST, 0, 0) };
        assert_eq!(hit, LRESULT::try_from(HTTRANSPARENT).unwrap_or(0));
        // SAFETY: as above.
        let activate = unsafe { SendMessageW(tip, WM_MOUSEACTIVATE, 0, 0) };
        assert_eq!(activate, LRESULT::try_from(MA_NOACTIVATE).unwrap_or(0));
        // SAFETY: no preconditions.
        assert_ne!(unsafe { GetActiveWindow() }, tip);
        drop(window);
        drain();
    }

    /// T2: a popup that loses the activation is dismissed, unless it's being hidden; a click on
    /// the owner while its menu is shown is eaten; the owner moving dismisses what's shown.
    #[test]
    fn light_dismiss_and_the_eaten_click() {
        let (window, log, _) = open();
        let menu = Surface::Popup(PopupKind::Menu);
        as_handler(&window, |cx| {
            cx.show_popup(PopupKind::Menu, rect(100, 100, 300, 260), Backdrop::None);
        });
        let hwnd = popup(&window, PopupKind::Menu).expect("the menu's window");
        // SAFETY: no preconditions. Printed, not checked: a test thread may not be in the
        // foreground, and Wine's null driver keeps the activation to itself.
        let active = unsafe { GetActiveWindow() } == hwnd;
        println!("the menu is active after its show: {active}");
        let button = LPARAM::try_from(WM_LBUTTONDOWN).unwrap_or(0) << 16;
        let lp = LPARAM::try_from(HTCLIENT).unwrap_or(0) | button;
        // SAFETY: the test's live windows, on its thread (every block below).
        let eaten = unsafe { SendMessageW(window.hwnd, WM_MOUSEACTIVATE, 0, lp) };
        assert_eq!(eaten, LRESULT::try_from(MA_ACTIVATEANDEAT).unwrap_or(0));
        // Another window took the activation (lp: none of ours).
        let before = heard(&log, menu, Event::Dismiss);
        // SAFETY: as above.
        unsafe { SendMessageW(hwnd, WM_ACTIVATE, 0, 0) };
        assert_eq!(heard(&log, menu, Event::Dismiss), before + 1);
        assert!(!visible(hwnd), "the handler hid it");
        // SAFETY: as above. Nothing is shown now, so the click goes through.
        let through = unsafe { SendMessageW(window.hwnd, WM_MOUSEACTIVATE, 0, lp) };
        assert_eq!(through, LRESULT::try_from(MA_ACTIVATE).unwrap_or(0));
        // Shown again, then losing the activation while being hidden: no second dismiss.
        as_handler(&window, |cx| {
            cx.show_popup(PopupKind::Menu, rect(100, 100, 300, 260), Backdrop::None);
        });
        let dismissed = heard(&log, menu, Event::Dismiss);
        // SAFETY: as above.
        unsafe {
            with_state(hwnd, |p| p.hiding.set(true));
            SendMessageW(hwnd, WM_ACTIVATE, 0, 0);
            with_state(hwnd, |p| p.hiding.set(false));
        };
        assert_eq!(heard(&log, menu, Event::Dismiss), dismissed);
        // The owner moved: what's shown is dismissed, what isn't hears nothing.
        // SAFETY: as above.
        unsafe { SendMessageW(window.hwnd, WM_MOVE, 0, 0) };
        assert_eq!(heard(&log, menu, Event::Dismiss), dismissed + 1);
        let tip = Surface::Popup(PopupKind::Tooltip);
        assert_eq!(heard(&log, tip, Event::Dismiss), 0);
        drop(window);
        drain();
    }

    /// T2: the release of a click the open menu ate reaches nothing either, so a right-click
    /// outside a menu asks for no other menu; the next click's release goes through.
    #[test]
    fn an_eaten_click_s_release_reaches_nothing() {
        let (window, log, _) = open();
        as_handler(&window, |cx| {
            cx.show_popup(PopupKind::Menu, rect(100, 100, 300, 260), Backdrop::None);
        });
        let activate = |button: u32| {
            let hit = LPARAM::try_from(HTCLIENT).unwrap_or(0);
            hit | (LPARAM::try_from(button).unwrap_or(0) << 16)
        };
        let at = (LPARAM::from(12_i16) << 16) | LPARAM::from(10_i16);
        let menus = |log: &Log| {
            log.borrow()
                .iter()
                .filter(|(s, e)| *s == Surface::Main && matches!(e, Event::ContextMenu(_)))
                .count()
        };
        let ups = |log: &Log| {
            log.borrow()
                .iter()
                .filter(|(s, e)| *s == Surface::Main && matches!(e, Event::PointerUp(_)))
                .count()
        };
        // SAFETY: the test's live window, on its thread (every block below).
        let eaten =
            unsafe { SendMessageW(window.hwnd, WM_MOUSEACTIVATE, 0, activate(WM_RBUTTONDOWN)) };
        assert_eq!(eaten, LRESULT::try_from(MA_ACTIVATEANDEAT).unwrap_or(0));
        // SAFETY: as above.
        unsafe { SendMessageW(window.hwnd, WM_RBUTTONUP, 0, at) };
        assert_eq!(menus(&log), 0, "{:?}", log.borrow());
        // The left button's release is eaten the same way.
        // SAFETY: as above.
        unsafe { SendMessageW(window.hwnd, WM_MOUSEACTIVATE, 0, activate(WM_LBUTTONDOWN)) };
        // SAFETY: as above.
        unsafe { SendMessageW(window.hwnd, WM_LBUTTONUP, 0, at) };
        assert_eq!(ups(&log), 0, "{:?}", log.borrow());
        // A release with nothing eaten before it: the default handling asks for a menu.
        // SAFETY: as above.
        unsafe { SendMessageW(window.hwnd, WM_RBUTTONUP, 0, at) };
        assert_eq!(menus(&log), 1, "{:?}", log.borrow());
        // An eaten press whose release went elsewhere doesn't eat the next click's.
        // SAFETY: as above.
        unsafe { SendMessageW(window.hwnd, WM_MOUSEACTIVATE, 0, activate(WM_RBUTTONDOWN)) };
        // SAFETY: as above.
        unsafe { SendMessageW(window.hwnd, WM_RBUTTONDOWN, 0, at) };
        // SAFETY: as above.
        unsafe { SendMessageW(window.hwnd, WM_RBUTTONUP, 0, at) };
        assert_eq!(menus(&log), 2, "{:?}", log.borrow());
        drop(window);
        drain();
    }

    /// T2: Windows hides an owner's popups as it minimizes and shows them again at its restore.
    /// One the handler hid meanwhile stays hidden; one it didn't comes back.
    #[test]
    fn a_popup_hidden_while_minimized_stays_hidden_at_the_restore() {
        use windows_sys::Win32::UI::WindowsAndMessaging::ShowOwnedPopups;
        let (window, _, _) = open();
        // A tooltip: it never activates, so being hidden doesn't dismiss it.
        as_handler(&window, |cx| {
            cx.show_popup(PopupKind::Tooltip, rect(10, 10, 110, 34), Backdrop::None);
        });
        let tip = popup(&window, PopupKind::Tooltip).expect("the tooltip's window");
        assert!(visible(tip));
        // What minimizing and restoring the owner do to its popups: SW_PARENTCLOSING, then
        // SW_PARENTOPENING for those it hid.
        // SAFETY: the test's live window, on its thread (every block below).
        unsafe { ShowOwnedPopups(window.hwnd, 0) };
        assert!(!visible(tip), "hidden with its owner");
        // SAFETY: as above.
        unsafe { ShowOwnedPopups(window.hwnd, 1) };
        assert!(visible(tip), "still shown, so back with its owner");
        // SAFETY: as above.
        unsafe { ShowOwnedPopups(window.hwnd, 0) };
        as_handler(&window, |cx| cx.hide_popup(PopupKind::Tooltip));
        // SAFETY: as above.
        unsafe { ShowOwnedPopups(window.hwnd, 1) };
        assert!(
            !visible(tip),
            "hidden by the handler meanwhile, so not back"
        );
        drop(window);
        drain();
    }

    /// T2: Alt+F4 in a popup (its `SC_CLOSE`) closes the main window, and never the popup's own.
    #[test]
    fn a_popup_s_close_is_its_owner_s() {
        let (window, log, _) = open();
        as_handler(&window, |cx| {
            cx.show_popup(PopupKind::List, rect(100, 100, 300, 260), Backdrop::None);
        });
        let hwnd = popup(&window, PopupKind::List).expect("the list's window");
        let close = WPARAM::try_from(SC_CLOSE).unwrap_or(0);
        // SAFETY: the test's live popup, on its thread.
        unsafe { SendMessageW(hwnd, WM_SYSCOMMAND, close, 0) };
        drain();
        // SAFETY: no preconditions.
        assert_ne!(unsafe { IsWindow(hwnd) }, 0, "the popup's window is kept");
        assert_eq!(
            heard(&log, Surface::Main, Event::Close),
            1,
            "{:?}",
            log.borrow()
        );
        assert_eq!(
            heard(&log, Surface::Popup(PopupKind::List), Event::Close),
            0
        );
        drop(window);
        drain();
    }

    /// T2: a popup whose window can't be made is dismissed, so the handler doesn't hold it open
    /// with nothing on screen; the next show makes it.
    #[test]
    fn a_popup_that_cannot_be_made_is_dismissed() {
        let (window, log, _) = open();
        let menu = Surface::Popup(PopupKind::Menu);
        let hwnd = window.hwnd;
        // SAFETY: the test's window, on its thread.
        unsafe {
            with_state(hwnd, |state| {
                // Its slot is borrowed, as if the show came back into a show: no window is made.
                let held = state.popups.windows.borrow();
                state.popups.queue(Op::Show {
                    kind: PopupKind::Menu,
                    rect: rect(100, 100, 300, 260),
                    backdrop: Backdrop::None,
                });
                state.apply_popups(hwnd);
                drop(held);
            })
        };
        assert_eq!(heard(&log, menu, Event::Dismiss), 1, "{:?}", log.borrow());
        assert_eq!(popup(&window, PopupKind::Menu), None);
        as_handler(&window, |cx| {
            cx.show_popup(PopupKind::Menu, rect(100, 100, 300, 260), Backdrop::None);
        });
        assert!(popup(&window, PopupKind::Menu).is_some_and(visible));
        drop(window);
        drain();
    }

    /// T2: a context request reaches the main surface from the keyboard (no point) and from
    /// inside the client area (a client point); one from outside it is the system's.
    #[test]
    fn context_requests_from_the_keyboard_or_the_client_area() {
        let (window, log, _) = open();
        let lparam = |p: POINT| {
            let word = |v: i32| v.cast_unsigned() & 0xFFFF;
            LPARAM::try_from((word(p.y) << 16) | word(p.x)).unwrap_or(0)
        };
        let screen = |x: i32, y: i32| {
            let mut p = POINT { x, y };
            // SAFETY: the test's live window, and a live local.
            unsafe { ClientToScreen(window.hwnd, &raw mut p) };
            p
        };
        let wp = window.hwnd.expose_provenance();
        // SAFETY: messages sent to the test's live window, on its thread (every block below).
        unsafe {
            SendMessageW(
                window.hwnd,
                WM_CONTEXTMENU,
                wp,
                lparam(POINT { x: -1, y: -1 }),
            )
        };
        assert_eq!(heard(&log, Surface::Main, Event::ContextMenu(None)), 1);
        // SAFETY: as above.
        unsafe { SendMessageW(window.hwnd, WM_CONTEXTMENU, wp, lparam(screen(10, 12))) };
        let inside = Event::ContextMenu(Some(PointPx { x: 10, y: 12 }));
        assert_eq!(heard(&log, Surface::Main, inside), 1, "{:?}", log.borrow());
        let before = log.borrow().len();
        // SAFETY: as above.
        unsafe { SendMessageW(window.hwnd, WM_CONTEXTMENU, wp, lparam(screen(-40, -40))) };
        assert_eq!(log.borrow().len(), before, "the title bar's");
        drop(window);
        drain();
    }

    /// T2 (TD-0009 A2.5, slice 5a): `WM_MOUSEWHEEL` and `WM_MOUSEHWHEEL` reach the surface under
    /// the pointer as [`Event::Wheel`] in its client pixels, whichever of them Windows sent them
    /// to: the window under the pointer, or with "Scroll inactive windows" off the focused one (an
    /// open list). Both are handled, so `DefWindowProc` never passes them on to an owner again.
    #[test]
    fn wheels_reach_the_surface_under_the_pointer() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{WM_MOUSEHWHEEL, WM_MOUSEWHEEL};

        let (window, log, _) = open();
        let lparam = |p: POINT| {
            let word = |v: i32| v.cast_unsigned() & 0xFFFF;
            LPARAM::try_from((word(p.y) << 16) | word(p.x)).unwrap_or(0)
        };
        // The delta is wParam's high word, signed; its low word is the buttons and keys held.
        let wparam = |delta: i16| WPARAM::from(delta.cast_unsigned()) << 16;
        let screen = |hwnd: HWND, x: i32, y: i32| {
            let mut p = POINT { x, y };
            // SAFETY: a live window of the test's, and a live local.
            unsafe { ClientToScreen(hwnd, &raw mut p) };
            p
        };
        let wheel = |delta: i32, horizontal: bool, x: i32, y: i32| Event::Wheel {
            delta,
            horizontal,
            at: PointPx { x, y },
        };
        let main = window.hwnd;
        let to = |hwnd: HWND, msg: u32, delta: i16, at: POINT| {
            // SAFETY: a message sent to one of the test's live windows, on their thread.
            unsafe { SendMessageW(hwnd, msg, wparam(delta), lparam(at)) }
        };
        assert_eq!(
            to(main, WM_MOUSEWHEEL, -120, screen(main, 10, 12)),
            0,
            "handled"
        );
        assert_eq!(
            heard(&log, Surface::Main, wheel(-120, false, 10, 12)),
            1,
            "{:?}",
            log.borrow()
        );
        assert_eq!(to(main, WM_MOUSEHWHEEL, 30, screen(main, 20, 30)), 0);
        assert_eq!(heard(&log, Surface::Main, wheel(30, true, 20, 30)), 1);

        // Over an open list, the list's; over the owner beside it, the owner's. The list covers
        // the owner's right side, clear of the owner's point below.
        let o = screen(main, 150, 40);
        as_handler(&window, |cx| {
            let at = rect(o.x, o.y, o.x + 150, o.y + 140);
            cx.show_popup(PopupKind::List, at, Backdrop::None);
        });
        let list = popup(&window, PopupKind::List).expect("the list's window");
        let in_list = Surface::Popup(PopupKind::List);
        to(list, WM_MOUSEWHEEL, 120, screen(list, 5, 6));
        assert_eq!(
            heard(&log, in_list, wheel(120, false, 5, 6)),
            1,
            "{:?}",
            log.borrow()
        );
        to(list, WM_MOUSEWHEEL, -240, screen(main, 15, 16));
        assert_eq!(
            heard(&log, Surface::Main, wheel(-240, false, 15, 16)),
            1,
            "{:?}",
            log.borrow()
        );
        assert_eq!(heard(&log, in_list, wheel(-240, false, 15, 16)), 0);

        // Outside both (the title bar, another window): nobody's.
        let before = log.borrow().len();
        to(main, WM_MOUSEWHEEL, 120, screen(main, -40, -40));
        let wheels = log.borrow()[before..]
            .iter()
            .filter(|(_, e)| matches!(e, Event::Wheel { .. }))
            .count();
        assert_eq!(wheels, 0);
        drop(window);
        drain();
    }

    /// The work area holds the point it was asked for, and the DPI is a real one.
    #[test]
    fn the_work_area_is_where_the_point_is() {
        let (work, dpi) = work_area(rect(10, 10, 11, 11), 96);
        println!("work area {work:?} at {dpi} DPI");
        assert!(work.x0 <= 10 && work.y0 <= 10 && work.x1 > 10 && work.y1 > 10);
        assert!(dpi >= 96);
    }
}
