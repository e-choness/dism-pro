//! The host's popups (TD-0009 A1.5): the model's menu or dropdown list, and the toolkit's
//! tooltip, each in its own window with a [`Ui`] of its own, because each window serves its own
//! UI Automation tree.
//!
//! - **After every event** the host compares the model's [`popup`](super::Model::popup) with
//!   what shows. A new one opens at its anchor (the node that asked for it, or the point of a
//!   right-click), laid out at its monitor's DPI, sized to its content and placed below the
//!   anchor. One whose content changed size is placed again. One the model closed hides, and one
//!   whose anchor went away is closed (A1.8).
//! - **Closing is the model's.** Esc, a dismiss (a click outside, another window taking the
//!   activation, the main window moving), F4 or Alt+Up in a list, UI Automation's Collapse on the
//!   dropdown, and an item's own message all send the model the popup's
//!   [`on_close`](crate::Popup::on_close). An item's message comes after it, as a Windows menu
//!   closes before its command arrives, so a message that opens another popup keeps it open.
//!   Alt+F4 isn't a popup's: the popup window passes it to the main window, which closes.
//! - **Focus.** Opened from the keyboard, a menu focuses its first item with the focus ring.
//!   Opened by the pointer, it focuses nothing until a key or the pointer moves. A list focuses
//!   its picked option. Closing gives the activation back to the main window, whose focus never
//!   left the anchor.
//! - **The tooltip** shows the active surface's tip ([`Ui::tooltip`]) above its node.
//! - **The palette and a dialog** (slice 6c, A3.5 addition 13) are the model's popup too, placed
//!   against the main window's client area (centered, [`palette::TOP`] below its top, or on its
//!   center) at their width, less where the window is narrower, and placed again whenever it
//!   moves or resizes. Both take the activation and focus their first control (a dialog's
//!   `initial` button); the page shows the scrim beneath them. Their messages run as they come:
//!   they close what they close themselves. The palette is light-dismissed; a dialog isn't.
//! - **The toast** (A3.5 (7)) is the model's [`toast`](super::Model::toast), at the client area's
//!   bottom end. It never takes the activation, so the keyboard stays where it was: F6 gives it
//!   the keyboard and F6 or Esc hands it back. Its `Ui` sends the toast's Dismiss for Esc and,
//!   once ended, after Windows' message duration (one wake). Both wait for a minimized window's
//!   restore.
//! - **Accelerators.** A key the page doesn't use goes to the model's
//!   [`accelerator`](super::Model::accelerator) (Ctrl+K opens the palette).
//! - **Look.** A popup is glass (DWM's transient backdrop, acrylic, and the glass theme) when the
//!   main window is and the popup is composed; otherwise it's solid.

use dpro_win::gui::{self, Backdrop, Event, PointPx, Presenter, SizePx, Surface, WindowCx};

use super::{Host, Model, key_code, logical, logical_size, mods_of, scale_of, theme_of};
use crate::{
    Element, Input, Key, KeyCode, KeyEvent, Motion, Placement, Point, PopupKind, Rect, RectPx,
    Size, TextSystem, Theme, Ui, place,
    tokens::{dialog, palette, popup as tokens, toast},
};

/// F6: into a shown toast from the main window, and back (A3.5 (7)).
const VK_F6: u16 = 0x75;

/// A popup window's UI, and where the window is.
pub(super) struct Layer<M> {
    pub(super) ui: Ui<M>,
    /// Whether its window shows.
    pub(super) shown: bool,
    /// Where its window shows, in screen pixels.
    pub(super) rect: gui::RectPx,
    /// What it's placed against, and the work area it's kept inside, in screen pixels.
    anchor: gui::RectPx,
    work: gui::RectPx,
    /// Its window's presenter, once the window has said.
    pub(super) presenter: Presenter,
    /// The backdrop its window was last shown over: a theme change while it shows asks again.
    backdrop: Backdrop,
}

impl<M: Clone> Layer<M> {
    pub(super) fn new() -> Self {
        Self {
            ui: Ui::new(Size { w: 0.0, h: 0.0 }, 1.0, Motion::STANDARD),
            shown: false,
            rect: gui::RectPx::default(),
            anchor: gui::RectPx::default(),
            work: gui::RectPx::default(),
            presenter: Presenter::default(),
            backdrop: Backdrop::None,
        }
    }

    /// It's placed against `anchor`, inside `work`, laid out at `dpi` with the main window's
    /// text size and motion.
    fn at(
        &mut self,
        anchor: gui::RectPx,
        work: gui::RectPx,
        dpi: u32,
        text_scale: f32,
        motion: Motion,
    ) {
        self.anchor = anchor;
        self.work = work;
        self.ui.set_scale(scale_of(dpi));
        self.ui.set_text_scale(text_scale);
        self.ui.set_motion(motion);
    }

    /// The rectangle to show the window at: the size of what was built, at most `max_h` of the
    /// work area's height, on the `placement` side of the anchor. The viewport follows.
    fn fit(&mut self, text: &mut dyn TextSystem, placement: Placement, max_h: f32) -> gui::RectPx {
        let s = self.ui.scale();
        let work = ui_px(self.work);
        let work_w = work.x1.saturating_sub(work.x0);
        let work_h = work.y1.saturating_sub(work.y0);
        let size = self.ui.natural_size(text, logical(work_w, s));
        let h = size.h.min(logical(work_h, s) * max_h);
        let px = Rect::new(0.0, 0.0, size.w, h).to_px(s);
        let gap = Rect::new(0.0, 0.0, tokens::GAP, 0.0).to_px(s).x1;
        let r = place(ui_px(self.anchor), px.x1, px.y1, work, placement, gap);
        self.ui.set_viewport(Size {
            w: logical(r.x1.saturating_sub(r.x0), s),
            h: logical(r.y1.saturating_sub(r.y0), s),
        });
        gui_px(r)
    }

    /// The rectangle to show an overlay at (the palette, a dialog, the toast), placed by
    /// `placement` against the anchor, the main window's client area: `width` wide, less where
    /// the client area is narrower than it and two `margin`s, and as tall as what was built, at
    /// most the client area's height less `reserve`. The viewport follows.
    fn fit_over(
        &mut self,
        text: &mut dyn TextSystem,
        width: f32,
        margin: f32,
        reserve: f32,
        placement: Placement,
    ) -> gui::RectPx {
        let s = self.ui.scale();
        let client = ui_px(self.anchor);
        let client_w = logical(client.x1.saturating_sub(client.x0), s);
        let client_h = logical(client.y1.saturating_sub(client.y0), s);
        let w = width.min(client_w - 2.0 * margin).max(1.0);
        let size = self.ui.natural_size(text, w);
        let h = size.h.min(client_h - reserve).max(1.0);
        let px = Rect::new(0.0, 0.0, w, h).to_px(s);
        let r = place(client, px.x1, px.y1, ui_px(self.work), placement, 0);
        self.ui.set_viewport(Size {
            w: logical(r.x1.saturating_sub(r.x0), s),
            h: logical(r.y1.saturating_sub(r.y0), s),
        });
        gui_px(r)
    }

    /// Its window was hidden: a press (an item held while Esc closed it), the hover, the focus
    /// and its tip go, and so does damage nothing will paint. So does what it showed: the window
    /// is kept, but its next open builds new nodes, so a client hears each open and each close
    /// (TD-0009 A4.5, the adapter's PATCHES.md 12 and 13: `MenuOpened` and `MenuClosed`,
    /// `ToolTipOpened` and `ToolTipClosed`).
    fn hidden(&mut self) {
        self.shown = false;
        self.ui.cancel_press();
        let mut none = Vec::new();
        self.ui.dispatch(Input::PointerLeave, &mut none);
        self.ui.set_focus(None);
        self.ui.set_active(false);
        self.ui.build(|_| {});
        self.ui.clear_damage();
    }

    /// Builds the tooltip window's one node, with `text`. A tip for another node than the one
    /// showing is a new node, so a client hears the old tip close and the new one open (the
    /// adapter's PATCHES.md 13); new text for the same node is a name change.
    fn build_tip(&mut self, text: &str, another: bool) {
        if another {
            self.ui.build(|_| {});
        }
        self.ui.build(|c| {
            c.add(Element::tooltip_text(Key::name("tip"), text));
        });
    }

    /// `r`, in this popup's logical pixels, in screen pixels.
    fn screen(&self, r: Rect) -> gui::RectPx {
        let px = r.to_px(self.ui.scale());
        gui::RectPx {
            x0: self.rect.x0.saturating_add(px.x0),
            y0: self.rect.y0.saturating_add(px.y0),
            x1: self.rect.x0.saturating_add(px.x1),
            y1: self.rect.y0.saturating_add(px.y1),
        }
    }
}

/// The popup windows, in [`Host`]'s `layers`: a menu, a dropdown's list, the tooltip, the
/// palette, the toast and a dialog.
pub(super) const KINDS: [gui::PopupKind; 6] = [
    gui::PopupKind::Menu,
    gui::PopupKind::List,
    gui::PopupKind::Tooltip,
    gui::PopupKind::Palette,
    gui::PopupKind::Toast,
    gui::PopupKind::Dialog,
];

/// `kind`'s place in [`KINDS`].
pub(super) const fn slot(kind: gui::PopupKind) -> usize {
    match kind {
        gui::PopupKind::Menu => 0,
        gui::PopupKind::List => 1,
        gui::PopupKind::Tooltip => 2,
        gui::PopupKind::Palette => 3,
        gui::PopupKind::Toast => 4,
        gui::PopupKind::Dialog => 5,
    }
}

/// The window a popup of the toolkit's `kind` shows in.
const fn window_kind(kind: PopupKind) -> gui::PopupKind {
    match kind {
        PopupKind::Menu => gui::PopupKind::Menu,
        PopupKind::List => gui::PopupKind::List,
        PopupKind::Tooltip => gui::PopupKind::Tooltip,
        PopupKind::Palette => gui::PopupKind::Palette,
        PopupKind::Toast => gui::PopupKind::Toast,
        PopupKind::Dialog => gui::PopupKind::Dialog,
    }
}

/// Whether a popup of `kind` is placed against the main window's client area, not a node.
const fn over_owner(kind: gui::PopupKind) -> bool {
    matches!(kind, gui::PopupKind::Palette | gui::PopupKind::Dialog)
}

/// Whether a key down in a popup of `kind` closes it: Esc, and F4 or Alt+Up in a list (the
/// dropdown's own keys, as Windows' combo boxes). Not Alt+F4: that goes on to the system. Not
/// in the toast, whose `Ui` takes Esc (its Dismiss).
fn closes(kind: gui::PopupKind, key: KeyCode, alt: bool) -> bool {
    kind != gui::PopupKind::Toast
        && (key == KeyCode::Escape
            || (kind == gui::PopupKind::List
                && ((key == KeyCode::F4 && !alt) || (alt && key == KeyCode::Up))))
}

/// Whether `event` in the popup window of `kind` hands the keyboard back to the main window: F6
/// in the toast (A3.5 (7)).
pub(super) fn hands_back(kind: gui::PopupKind, event: &Event) -> bool {
    kind == gui::PopupKind::Toast
        && matches!(event, Event::Key { vk, down: true, mods } if *vk == VK_F6 && !mods.ctrl && !mods.alt)
}

/// `v` logical pixels at `scale`, in physical pixels.
fn px(v: f32, scale: f32) -> i32 {
    Rect::new(0.0, 0.0, v, 0.0).to_px(scale).x1
}

/// The main window's client area, in screen pixels: what the palette, a dialog and the toast
/// are placed against.
fn client_screen(cx: &WindowCx<'_>) -> gui::RectPx {
    let size = cx.size();
    let a = cx.client_to_screen(PointPx { x: 0, y: 0 });
    gui::RectPx {
        x0: a.x,
        y0: a.y,
        x1: a
            .x
            .saturating_add(i32::try_from(size.w).unwrap_or(i32::MAX)),
        y1: a
            .y
            .saturating_add(i32::try_from(size.h).unwrap_or(i32::MAX)),
    }
}

/// `r`, in the main window's logical pixels at `scale`, in screen pixels.
fn main_screen(cx: &WindowCx<'_>, r: Rect, scale: f32) -> gui::RectPx {
    let px = r.to_px(scale);
    let a = cx.client_to_screen(PointPx { x: px.x0, y: px.y0 });
    let b = cx.client_to_screen(PointPx { x: px.x1, y: px.y1 });
    gui::RectPx {
        x0: a.x,
        y0: a.y,
        x1: b.x,
        y1: b.y,
    }
}

pub(super) const fn ui_px(r: gui::RectPx) -> RectPx {
    RectPx {
        x0: r.x0,
        y0: r.y0,
        x1: r.x1,
        y1: r.y1,
    }
}

pub(super) const fn gui_px(r: RectPx) -> gui::RectPx {
    gui::RectPx {
        x0: r.x0,
        y0: r.y0,
        x1: r.x1,
        y1: r.y1,
    }
}

impl<P: Model> Host<P> {
    /// Sends the model its popup's [`on_close`](crate::Popup::on_close), if it has one open.
    pub(super) fn close_popup(&mut self) {
        if let Some(p) = self.model.popup() {
            self.model.update(p.on_close);
        }
    }

    /// The backdrop a popup asks for: DWM's transient one (acrylic) when the main window is
    /// glass. A popup that isn't composed ignores it.
    pub(super) fn popup_backdrop(&self) -> Backdrop {
        if self.theme.is_glass() {
            Backdrop::Transient
        } else {
            Backdrop::None
        }
    }

    /// The theme the popup window of `kind` paints with: the main window's, unless that's glass
    /// and the popup isn't composed (no backdrop shows through it), which paints solid.
    pub(super) fn popup_theme(&self, kind: gui::PopupKind) -> Theme {
        if self.theme.is_glass() && !self.layers[slot(kind)].presenter.composed {
            self.env.map_or(Theme::LIGHT, |env| theme_of(&env))
        } else {
            self.theme
        }
    }

    /// An event for the popup window of `kind`. Returns whether it was used.
    pub(super) fn popup_event(&mut self, kind: gui::PopupKind, event: Event) -> bool {
        let layer = &mut self.layers[slot(kind)];
        let scale = layer.ui.scale();
        let at = |p: PointPx| Point {
            x: logical(p.x, scale),
            y: logical(p.y, scale),
        };
        let input = match event {
            Event::Resize(SizePx { w, h }) => {
                layer.ui.set_viewport(Size {
                    w: logical_size(w, scale),
                    h: logical_size(h, scale),
                });
                // The window's canvas is new and blank.
                layer.ui.repaint_all();
                return true;
            }
            Event::Presenter(presenter) => {
                // Composed or not, it may paint with another theme.
                layer.presenter = presenter;
                layer.ui.repaint_all();
                return true;
            }
            Event::Focus(active) => {
                layer.ui.set_active(active);
                return false;
            }
            // The main window moved or minimized under the tip, or its window failed: the tip
            // goes, and the frame hides its window. The pointer must reach another node for
            // the next.
            Event::Dismiss | Event::Close if kind == gui::PopupKind::Tooltip => {
                self.ui.tip_cancel(false);
                for layer in &mut self.layers {
                    layer.ui.tip_cancel(false);
                }
                return true;
            }
            // The toast and a dialog aren't light-dismissed: the main window moved or resized
            // under them, and the frame places them again.
            Event::Dismiss | Event::Close if kind == gui::PopupKind::Toast => return true,
            Event::Dismiss if kind == gui::PopupKind::Dialog => return true,
            Event::Dismiss | Event::Close => {
                self.close_popup();
                return true;
            }
            Event::PointerMove(p) => Input::PointerMove(at(p)),
            Event::PointerDown {
                at: p,
                mods,
                double,
            } => Input::Press {
                at: at(p),
                mods: mods_of(mods),
                double,
            },
            Event::PointerUp(p) => Input::PointerUp(at(p)),
            Event::PointerLeave => Input::PointerLeave,
            Event::Char(c) => Input::Char(c),
            Event::Wheel {
                delta,
                horizontal,
                at: p,
            } => Input::Wheel {
                at: at(p),
                delta,
                horizontal,
            },
            Event::Key { vk, down, mods } => {
                let key = key_code(vk);
                if down && closes(kind, key, mods.alt) {
                    self.close_popup();
                    return true;
                }
                Input::Key(KeyEvent {
                    key,
                    down,
                    mods: mods_of(mods),
                })
            }
            // The main window's alone: its DPI (a popup's is its monitor's), its settings, its
            // clock, and its context requests.
            Event::Dpi(_) | Event::Env(_) | Event::Wake | Event::Frame | Event::ContextMenu(_) => {
                return false;
            }
        };
        if kind == gui::PopupKind::Tooltip {
            return false;
        }
        let used = self.layers[slot(kind)].ui.dispatch(input, &mut self.msgs);
        self.popup_messages(kind);
        used
    }

    /// The messages popup `kind`'s UI sent: a menu's or a list's items close it first
    /// ([`item_messages`](Self::item_messages)); the palette's, a dialog's and the toast's run as
    /// they come, and close what they close themselves.
    pub(super) fn popup_messages(&mut self, kind: gui::PopupKind) {
        if matches!(kind, gui::PopupKind::Menu | gui::PopupKind::List) {
            self.item_messages();
        } else {
            for msg in self.msgs.drain(..) {
                self.model.update(msg);
            }
        }
    }

    /// A key in the main window: F6 gives a shown toast the keyboard (A3.5 (7)); otherwise the
    /// page takes it, and a key down it doesn't use goes to the model's accelerators.
    pub(super) fn main_key(
        &mut self,
        vk: u16,
        down: bool,
        mods: gui::Mods,
        cx: &mut WindowCx<'_>,
    ) -> bool {
        if down
            && vk == VK_F6
            && !mods.ctrl
            && !mods.alt
            && self.layers[slot(gui::PopupKind::Toast)].shown
        {
            // Its first control takes focus, with the ring, so a screen reader reads where it is.
            self.layers[slot(gui::PopupKind::Toast)]
                .ui
                .focus_first(true);
            cx.activate(Surface::Popup(gui::PopupKind::Toast));
            return true;
        }
        let key = KeyEvent {
            key: key_code(vk),
            down,
            mods: mods_of(mods),
        };
        if self.input(Input::Key(key)) {
            return true;
        }
        match down.then(|| self.model.accelerator(key)).flatten() {
            Some(msg) => {
                self.model.update(msg);
                true
            }
            None => false,
        }
    }

    /// Whether the scrim shows over the page: while the palette or a dialog is open (A3.5
    /// addition 13).
    pub(super) fn wants_scrim(&self) -> bool {
        self.model
            .popup()
            .is_some_and(|p| matches!(p.kind, PopupKind::Palette | PopupKind::Dialog))
    }

    /// The popup windows' next timer, in milliseconds from `now`: the open popup's caret blink
    /// (the palette's field), and an ended toast's hide (one wake, A3.5 (7)).
    pub(super) fn popup_timers(&mut self, now: u64) -> Option<u64> {
        let caret = self
            .open
            .and_then(|k| self.layers[slot(k)].ui.caret_timer(now));
        let toast = &mut self.layers[slot(gui::PopupKind::Toast)];
        let hide = if toast.shown {
            toast.ui.expiry_timer(now)
        } else {
            None
        };
        caret.into_iter().chain(hide).min()
    }

    /// The window's wake came: an ended toast whose wait is over sends its Dismiss.
    pub(super) fn wake_popups(&mut self, now: u64) {
        let toast = &mut self.layers[slot(gui::PopupKind::Toast)];
        if toast.shown {
            toast.ui.tick(now, &mut self.msgs);
            for msg in self.msgs.drain(..) {
                self.model.update(msg);
            }
        }
    }

    /// The messages an item of the open popup sent: the popup closes first (A1.4), then they
    /// run, as a Windows menu closes before its command arrives. So a message that opens another
    /// popup keeps it open.
    pub(super) fn item_messages(&mut self) {
        if self.msgs.is_empty() {
            return;
        }
        self.close_popup();
        for msg in self.msgs.drain(..) {
            self.model.update(msg);
        }
    }

    /// Opens, rebuilds, moves or hides the menu, list, palette or dialog window to match the
    /// model's popup. Call it after the main view is built and laid out.
    pub(super) fn sync_popup(&mut self, cx: &mut WindowCx<'_>) {
        let want = self
            .model
            .popup()
            .filter(|p| !matches!(p.kind, PopupKind::Tooltip | PopupKind::Toast));
        let kind = want.as_ref().map(|p| window_kind(p.kind));
        if let Some(open) = self.open
            && Some(open) != kind
        {
            cx.hide_popup(open);
            self.layers[slot(open)].hidden();
            self.open = None;
        }
        let Some(kind) = kind else {
            return;
        };
        let overlay = over_owner(kind);
        // A menu or a list stands by the node that asked for it, where it opened; the palette
        // and a dialog by the client area, wherever it goes, once it's on screen.
        let node = if overlay {
            None
        } else {
            let Some(node) = self.ui.popup_anchor() else {
                return;
            };
            Some(node)
        };
        let opening = self.open.is_none();
        let place = if overlay { !cx.minimized() } else { opening };
        if opening && !place {
            return;
        }
        let backdrop = self.popup_backdrop();
        let main_scale = self.ui.scale();
        let text_scale = self.ui.text_scale();
        let motion = self.ui.motion();
        let keyboard = self.ui.focus_visible();
        let layer = &mut self.layers[slot(kind)];
        if place {
            let anchor = node.map_or_else(|| client_screen(cx), |n| main_screen(cx, n, main_scale));
            let (work, dpi) = cx.work_area(anchor);
            layer.at(anchor, work, dpi, text_scale, motion);
        }
        let model = &self.model;
        layer.ui.build(|c| model.popup_view(c));
        let text = self.text.as_mut();
        let rect = match kind {
            gui::PopupKind::Palette => {
                let top = px(palette::TOP, layer.ui.scale());
                let reserve = palette::TOP + palette::MARGIN;
                let at = Placement::TopCenter(top);
                layer.fit_over(text, palette::WIDTH, palette::MARGIN, reserve, at)
            }
            gui::PopupKind::Dialog => {
                let reserve = 2.0 * dialog::MARGIN;
                let at = Placement::Center;
                layer.fit_over(text, dialog::WIDTH, dialog::MARGIN, reserve, at)
            }
            _ => layer.fit(text, Placement::Below, tokens::MAX_HEIGHT),
        };
        layer.ui.layout(self.text.as_mut());
        if opening {
            // A list shows where its value is; a menu opened by the pointer waits for a move.
            // After layout, so a list taller than its window scrolls to the option from where
            // this open placed it, and before the window shows, so a client sees it focused.
            // The palette's field and a dialog's initial button take focus, with the ring.
            if overlay || keyboard || kind == gui::PopupKind::List {
                layer.ui.focus_first(keyboard || overlay);
            } else {
                layer.ui.set_focus(None);
            }
        }
        // Shown again when it moves, and when the main window goes glass or solid under it.
        if opening || rect != layer.rect || backdrop != layer.backdrop {
            layer.rect = rect;
            layer.backdrop = backdrop;
            layer.shown = true;
            cx.show_popup(kind, rect, backdrop);
        }
        if opening {
            // The window may be one kept from an earlier open: none of its pixels are this one's.
            layer.ui.repaint_all();
        }
        self.open = Some(kind);
    }

    /// Shows, rebuilds, moves or hides the toast window to match the model's
    /// [`toast`](super::Model::toast), at the client area's bottom end, placed again wherever the
    /// main window goes. It shows without the activation. One opening while the main window is
    /// minimized waits for the restore. Call it after the main view is built and laid out.
    pub(super) fn sync_toast(&mut self, cx: &mut WindowCx<'_>) {
        let k = slot(gui::PopupKind::Toast);
        if !self.model.toast() {
            let layer = &mut self.layers[k];
            if layer.shown {
                cx.hide_popup(gui::PopupKind::Toast);
                // The next toast is a new one: its tree, and its hide's wait, start afresh.
                layer.hidden();
            }
            return;
        }
        let opening = !self.layers[k].shown;
        let minimized = cx.minimized();
        if opening && minimized {
            return;
        }
        let backdrop = self.popup_backdrop();
        let text_scale = self.ui.text_scale();
        let motion = self.ui.motion();
        let layer = &mut self.layers[k];
        if !minimized {
            let anchor = client_screen(cx);
            let (work, dpi) = cx.work_area(anchor);
            layer.at(anchor, work, dpi, text_scale, motion);
        }
        let model = &self.model;
        layer.ui.build(|c| model.toast_view(c));
        let inset = px(toast::INSET, layer.ui.scale());
        let at = Placement::BottomEnd(inset);
        let reserve = 2.0 * toast::INSET;
        let rect = layer.fit_over(self.text.as_mut(), toast::WIDTH, toast::INSET, reserve, at);
        layer.ui.layout(self.text.as_mut());
        if opening || rect != layer.rect || backdrop != layer.backdrop {
            layer.rect = rect;
            layer.backdrop = backdrop;
            layer.shown = true;
            cx.show_popup(gui::PopupKind::Toast, rect, backdrop);
        }
        if opening {
            // The window may be one kept from an earlier toast: none of its pixels are this one's.
            layer.ui.repaint_all();
        }
    }

    /// Shows the active surface's tip in the tooltip window, above its node, or hides the window
    /// when no tip shows. Call it after the tooltip timers ran.
    pub(super) fn sync_tip(&mut self, cx: &mut WindowCx<'_>) {
        let backdrop = self.popup_backdrop();
        let scale = self.ui.scale();
        let text_scale = self.ui.text_scale();
        let motion = self.ui.motion();
        let [menu, list, tip, ..] = &mut self.layers;
        // Only the active surface has a tip: a menu's, while it's open and focused, else the
        // main window's.
        let popup = match self.open {
            Some(gui::PopupKind::Menu) => Some((gui::PopupKind::Menu, &*menu)),
            Some(gui::PopupKind::List) => Some((gui::PopupKind::List, &*list)),
            _ => None,
        };
        let found = match popup.and_then(|(k, l)| l.ui.tooltip().map(|t| (k, l, t))) {
            Some((k, l, (id, text))) => {
                l.ui.info(id)
                    .map(|n| (Surface::Popup(k), id, text, l.screen(n.rect)))
            }
            None => self.ui.tooltip().and_then(|(id, text)| {
                self.ui
                    .info(id)
                    .map(|n| (Surface::Main, id, text, main_screen(cx, n.rect, scale)))
            }),
        };
        let Some((surface, id, text, anchor)) = found else {
            if self.tip_for.take().is_some() {
                cx.hide_popup(gui::PopupKind::Tooltip);
                tip.hidden();
            }
            return;
        };
        if self.tip_for == Some((surface, id)) && self.tip_text == text && tip.backdrop == backdrop
        {
            return;
        }
        let another = self.tip_for.replace((surface, id)) != Some((surface, id));
        self.tip_text.clear();
        self.tip_text.push_str(text);
        let (work, dpi) = cx.work_area(anchor);
        tip.at(anchor, work, dpi, text_scale, motion);
        tip.build_tip(self.tip_text.as_str(), another);
        let rect = tip.fit(self.text.as_mut(), Placement::Above, f32::INFINITY);
        tip.rect = rect;
        tip.backdrop = backdrop;
        tip.shown = true;
        cx.show_popup(gui::PopupKind::Tooltip, rect, backdrop);
        tip.ui.layout(self.text.as_mut());
        tip.ui.repaint_all();
    }

    /// Hands each shown popup's damage to its window; a hidden one's is dropped.
    pub(super) fn popup_damage(&mut self, cx: &mut WindowCx<'_>) {
        for (kind, layer) in KINDS.into_iter().zip(&mut self.layers) {
            if layer.shown {
                for r in layer.ui.damage() {
                    cx.invalidate_in(Surface::Popup(kind), gui_px(*r));
                }
            } else {
                layer.ui.clear_damage();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(vk: u16, ctrl: bool) -> Event {
        Event::Key {
            vk,
            down: true,
            mods: gui::Mods {
                ctrl,
                ..gui::Mods::default()
            },
        }
    }

    /// F6 in the toast, and only there, hands the keyboard back; Esc there is the toast's own
    /// (its Dismiss), not a popup's close (A3.5 (7)).
    #[test]
    fn f6_hands_the_toast_back_and_esc_is_its_own() {
        let toast = gui::PopupKind::Toast;
        assert!(hands_back(toast, &key(VK_F6, false)));
        assert!(!hands_back(toast, &key(VK_F6, true)), "Ctrl+F6 isn't it");
        assert!(!hands_back(gui::PopupKind::Palette, &key(VK_F6, false)));
        assert!(!closes(toast, KeyCode::Escape, false));
        assert!(closes(gui::PopupKind::Palette, KeyCode::Escape, false));
        assert!(closes(gui::PopupKind::Dialog, KeyCode::Escape, false));
    }

    /// Each window kind has its own layer, and the toolkit's kinds show in their own windows.
    #[test]
    fn every_kind_has_its_slot_and_window() {
        for (i, kind) in KINDS.into_iter().enumerate() {
            assert_eq!(slot(kind), i);
        }
        assert_eq!(window_kind(PopupKind::Palette), gui::PopupKind::Palette);
        assert_eq!(window_kind(PopupKind::Toast), gui::PopupKind::Toast);
        assert_eq!(window_kind(PopupKind::Dialog), gui::PopupKind::Dialog);
        assert!(over_owner(gui::PopupKind::Dialog) && !over_owner(gui::PopupKind::Toast));
    }

    /// A menu with one item, as a model's popup view builds it.
    fn menu(cx: &mut crate::Cx<'_, ()>) {
        cx.add_with(Element::menu(Key::name("m")), |cx| {
            cx.add(Element::menu_item(Key::name("i"), "item", ()));
        });
    }

    /// A hidden popup drops what it showed, so its next open is new nodes: a client hears the
    /// menu go as the window hides, and a new one come at the next open (TD-0009 A4.5; the
    /// adapter's PATCHES.md 12 and 13 raise the pair).
    #[test]
    fn a_hidden_popup_opens_again_as_new_nodes() {
        let mut layer = Layer::<()>::new();
        layer.ui.build(menu);
        let tree = layer.ui.a11y_tree();
        let root = tree.tree.expect("a whole tree").root;
        let first = layer.ui.find(&[Key::name("m")]).expect("the menu");
        layer.shown = true;
        layer.hidden();
        assert_eq!(
            layer.ui.find(&[Key::name("m")]),
            None,
            "hidden, it holds nothing"
        );
        let closed = layer.ui.a11y_update().expect("the menu goes");
        let (_, window) = closed
            .nodes
            .iter()
            .find(|(id, _)| *id == root)
            .expect("the window, its children changed");
        assert_eq!(window.children(), []);
        layer.ui.build(menu);
        let again = layer.ui.find(&[Key::name("m")]).expect("the menu again");
        assert_ne!(first, again, "a new node, with an id of its own");
        let opened = layer.ui.a11y_update().expect("a menu comes");
        let menus = opened
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == accesskit::Role::Menu)
            .count();
        assert_eq!(menus, 1);
    }

    /// The tooltip window's node is new for a tip of another node, and the same for new text of
    /// the same one (the adapter's PATCHES.md 13: `ToolTipClosed`, then `ToolTipOpened`).
    #[test]
    fn a_tip_for_another_node_is_a_new_node() {
        let mut tip = Layer::<()>::new();
        let node = |l: &Layer<()>| l.ui.find(&[Key::name("tip")]).expect("the tip");
        tip.build_tip("Every change before it applies", true);
        let first = node(&tip);
        tip.build_tip("Every change, before it applies", false);
        assert_eq!(node(&tip), first, "new text, the same node");
        tip.build_tip("Hold until the bar fills to apply", true);
        assert_ne!(node(&tip), first, "another tip, another node");
    }
}
