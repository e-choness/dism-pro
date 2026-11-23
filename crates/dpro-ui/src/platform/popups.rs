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
//! - **Look.** A popup is glass (DWM's transient backdrop, acrylic, and the glass theme) when the
//!   main window is and the popup is composed; otherwise it's solid.

use dpro_win::gui::{self, Backdrop, Event, PointPx, Presenter, SizePx, Surface, WindowCx};

use super::{Host, Model, key_code, logical, logical_size, mods_of, scale_of, theme_of};
use crate::{
    Element, Input, Key, KeyCode, KeyEvent, Motion, Placement, Point, PopupKind, Rect, RectPx,
    Size, TextSystem, Theme, Ui, place, tokens::popup as tokens,
};

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

    /// Its window was hidden: a press (an item held while Esc closed it), the hover, the focus
    /// and its tip go, and so does damage nothing will paint.
    fn hidden(&mut self) {
        self.shown = false;
        self.ui.cancel_press();
        let mut none = Vec::new();
        self.ui.dispatch(Input::PointerLeave, &mut none);
        self.ui.set_focus(None);
        self.ui.set_active(false);
        self.ui.clear_damage();
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

/// The popup windows, in [`Host`]'s `layers`: a menu, a dropdown's list and the tooltip.
pub(super) const KINDS: [gui::PopupKind; 3] = [
    gui::PopupKind::Menu,
    gui::PopupKind::List,
    gui::PopupKind::Tooltip,
];

/// `kind`'s place in [`KINDS`].
pub(super) const fn slot(kind: gui::PopupKind) -> usize {
    match kind {
        gui::PopupKind::Menu => 0,
        gui::PopupKind::List => 1,
        gui::PopupKind::Tooltip => 2,
    }
}

/// The window a popup of the toolkit's `kind` shows in.
const fn window_kind(kind: PopupKind) -> gui::PopupKind {
    match kind {
        PopupKind::Menu => gui::PopupKind::Menu,
        PopupKind::List => gui::PopupKind::List,
        PopupKind::Tooltip => gui::PopupKind::Tooltip,
    }
}

/// Whether a key down in a popup of `kind` closes it: Esc, and F4 or Alt+Up in a list (the
/// dropdown's own keys, as Windows' combo boxes). Not Alt+F4: that goes on to the system.
fn closes(kind: gui::PopupKind, key: KeyCode, alt: bool) -> bool {
    key == KeyCode::Escape
        || (kind == gui::PopupKind::List
            && ((key == KeyCode::F4 && !alt) || (alt && key == KeyCode::Up)))
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
        self.item_messages();
        used
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

    /// Opens, rebuilds, moves or hides the menu or list window to match the model's popup.
    /// Call it after the main view is built and laid out.
    pub(super) fn sync_popup(&mut self, cx: &mut WindowCx<'_>) {
        let want = self.model.popup().filter(|p| p.kind != PopupKind::Tooltip);
        let kind = want.as_ref().map(|p| window_kind(p.kind));
        if let Some(open) = self.open
            && Some(open) != kind
        {
            cx.hide_popup(open);
            self.layers[slot(open)].hidden();
            self.open = None;
        }
        let (Some(kind), Some(anchor)) = (kind, self.ui.popup_anchor()) else {
            return;
        };
        let opening = self.open.is_none();
        let backdrop = self.popup_backdrop();
        let main_scale = self.ui.scale();
        let text_scale = self.ui.text_scale();
        let motion = self.ui.motion();
        let keyboard = self.ui.focus_visible();
        let layer = &mut self.layers[slot(kind)];
        if opening {
            let anchor = main_screen(cx, anchor, main_scale);
            let (work, dpi) = cx.work_area(anchor);
            layer.at(anchor, work, dpi, text_scale, motion);
        }
        let model = &self.model;
        layer.ui.build(|c| model.popup_view(c));
        let rect = layer.fit(self.text.as_mut(), Placement::Below, tokens::MAX_HEIGHT);
        layer.ui.layout(self.text.as_mut());
        if opening {
            // A list shows where its value is; a menu opened by the pointer waits for a move.
            // After layout, so a list taller than its window scrolls to the option from where
            // this open placed it, and before the window shows, so a client sees it focused.
            if keyboard || kind == gui::PopupKind::List {
                layer.ui.focus_first(keyboard);
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

    /// Shows the active surface's tip in the tooltip window, above its node, or hides the window
    /// when no tip shows. Call it after the tooltip timers ran.
    pub(super) fn sync_tip(&mut self, cx: &mut WindowCx<'_>) {
        let backdrop = self.popup_backdrop();
        let scale = self.ui.scale();
        let text_scale = self.ui.text_scale();
        let motion = self.ui.motion();
        let [menu, list, tip] = &mut self.layers;
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
        self.tip_for = Some((surface, id));
        self.tip_text.clear();
        self.tip_text.push_str(text);
        let (work, dpi) = cx.work_area(anchor);
        tip.at(anchor, work, dpi, text_scale, motion);
        let text = self.tip_text.as_str();
        tip.ui.build(|c| {
            c.add(Element::tooltip_text(Key::name("tip"), text));
        });
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
