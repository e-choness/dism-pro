//! Popups (TD-0009 amendment A1): menus, a dropdown's list and tooltips, each in a window of its
//! own above the main one, so it can pass the main window's edges and show DWM's acrylic.
//!
//! - **Who owns what.** The view-model owns whether a menu or a list is open ([`Popup`]); the host
//!   opens, moves and closes the window to match, and builds its [`Ui`](crate::Ui) from the
//!   model's popup view. Tooltips belong to the toolkit ([`Ui::tooltip`](crate::Ui::tooltip)):
//!   hover and focus timing on nodes given [tooltip text](crate::Element::tooltip).
//! - **Closing.** Esc, a click outside, another window taking focus, the main window moving, and
//!   an item's message all close a menu or a list. The host sends the model the popup's
//!   [`on_close`](Popup::on_close) then, so the model's state and the window never disagree.
//! - **Where it goes.** [`place`] puts it below or above its anchor, flipped when there's no room,
//!   then kept inside the monitor's work area: pure arithmetic on screen pixels, T1-tested.

use crate::{
    NodeId,
    geom::{Point, Rect, RectPx, Size},
    text::TextSystem,
    tree::{Group, NIL},
    ui::{ROOT, Ui},
};

/// What kind of window a popup is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PopupKind {
    /// A context menu or a menu button's menu: it takes keyboard focus.
    Menu,
    /// A dropdown's list of options: it takes keyboard focus.
    List,
    /// A tooltip: never focused, never clicked.
    Tooltip,
}

/// A menu or a list the view-model has open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Popup<M> {
    /// [`PopupKind::Menu`] or [`PopupKind::List`]; tooltips are the toolkit's.
    pub kind: PopupKind,
    /// What the host sends when the popup closes without the model closing it: Esc, a click
    /// outside, another window taking focus, the main window moving or resizing, and right after
    /// an item's own message (an item invoked, an option picked). The model closes it then.
    pub on_close: M,
}

/// Which side of its anchor a popup prefers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Below, starting at the anchor's left edge (menus, the dropdown's list).
    Below,
    /// Above, starting at the anchor's left edge (tooltips).
    Above,
}

/// Where a popup `w` × `h` physical pixels goes for `anchor`, `gap` pixels from it, inside
/// `work` (the anchor's monitor's work area): all in screen pixels.
///
/// - **Vertically** on the `placement` side if it fits there, else on the other side if it fits
///   there, else on the side with more room. Then it's moved inside the work area.
/// - **Horizontally** from the anchor's left edge, or, if that overflows the work area's right,
///   ending at the anchor's right edge (a zero-width anchor, a point, so opens leftward). Then it's
///   moved inside the work area.
/// - A popup larger than the work area keeps its size and starts at the work area's top left.
#[must_use]
pub fn place(
    anchor: RectPx,
    w: i32,
    h: i32,
    work: RectPx,
    placement: Placement,
    gap: i32,
) -> RectPx {
    let (w, h, gap) = (w.max(0), h.max(0), gap.max(0));
    let below = anchor.y1.saturating_add(gap);
    let above = anchor.y0.saturating_sub(gap).saturating_sub(h);
    let fits_below = below.saturating_add(h) <= work.y1;
    let fits_above = above >= work.y0;
    let room_below = work.y1.saturating_sub(below);
    let room_above = anchor.y0.saturating_sub(gap).saturating_sub(work.y0);
    // The preferred side if it fits, else the other if that fits, else the roomier.
    let roomier_below = room_below >= room_above;
    let go_below = match placement {
        Placement::Below => fits_below || (!fits_above && roomier_below),
        Placement::Above => !fits_above && (fits_below || roomier_below),
    };
    let y = if go_below { below } else { above };
    let x = if anchor.x0.saturating_add(w) <= work.x1 {
        anchor.x0
    } else {
        anchor.x1.saturating_sub(w)
    };
    let (x, y) = (
        inside(x, w, work.x0, work.x1),
        inside(y, h, work.y0, work.y1),
    );
    RectPx {
        x0: x,
        y0: y,
        x1: x.saturating_add(w),
        y1: y.saturating_add(h),
    }
}

/// `start` moved so that `[start, start + len)` lies within `[lo, hi)`; `lo` if it can't.
fn inside(start: i32, len: i32, lo: i32, hi: i32) -> i32 {
    start.min(hi.saturating_sub(len)).max(lo)
}

impl<M> Ui<M> {
    /// The node a context request found since the last call, if one did and it's still there: the
    /// host then asks the model for that node's context menu, anchored at
    /// [`popup_anchor`](Self::popup_anchor).
    pub fn take_context(&mut self) -> Option<NodeId> {
        let i = std::mem::replace(&mut self.context, NIL);
        self.id(i)
    }

    /// Where the last popup was asked for, in this window's logical pixels: the point of a
    /// right-click (an empty rectangle), the node a keyboard context request was for, or the
    /// dropdown or menu button last activated.
    #[must_use]
    pub fn popup_anchor(&self) -> Option<Rect> {
        self.anchor
    }

    /// The natural size of what the view built (the root's first child, a popup's content) with
    /// `max_w` logical pixels of width available: what a popup window is sized to before it's
    /// placed. Call it after [`build`](Self::build).
    pub fn natural_size(&mut self, text: &mut dyn TextSystem, max_w: f32) -> Size {
        let first = self.nodes[ROOT as usize].first;
        if first == NIL {
            return Size::ZERO;
        }
        self.measure(first, max_w, text)
    }

    /// Focuses the first Tab stop (a menu's first item, a list's picked option), with the focus
    /// ring if `visible` (opened by the keyboard): a popup that just opened.
    pub fn focus_first(&mut self, visible: bool) {
        let to = self.tab_from(NIL, true);
        if to != NIL {
            self.move_focus(to);
        }
        if visible {
            self.show_focus_ring();
        } else {
            self.hide_focus_ring();
        }
    }

    /// Whether this is a popup's Ui: its content is a menu or a dropdown's options.
    pub(crate) fn in_popup(&self) -> bool {
        let first = self.nodes[ROOT as usize].first;
        self.nodes
            .get(first as usize)
            .is_some_and(|n| matches!(n.group, Some(Group::Menu | Group::Options)))
    }

    /// Whether no node has a side slot (no tooltips): the pointer then needs no tooltip hit test.
    pub(crate) fn sides_empty(&self) -> bool {
        self.sides.len() == self.free_sides.len()
    }

    /// A context request, at a point or (`None`) for the focused node: the nearest node at or
    /// above it marked [`context`](crate::Element::context) is focused, and picked if it's
    /// selectable, and becomes the anchor; the host learns of it from
    /// [`take_context`](Self::take_context). Returns whether a node took it.
    pub(crate) fn context_request(&mut self, at: Option<Point>, out: &mut Vec<M>) -> bool
    where
        M: Clone,
    {
        let start = match at {
            Some(p) => self.deepest(p),
            None => self.focus,
        };
        let mut i = start;
        while i != NIL && i != ROOT && !self.nodes[i as usize].context {
            i = self.nodes[i as usize].parent;
        }
        if i == NIL || i == ROOT {
            return false;
        }
        self.tip_cancel(true);
        if at.is_some() {
            self.hide_focus_ring();
        } else {
            self.show_focus_ring();
        }
        // A table's header asks the view for its column menu (A2.4).
        if self.is_table_header(i) {
            return self.column_menu_request(i, start, at, out);
        }
        if self.nodes[i as usize].interactive() {
            // From the keyboard, the node comes into view; under the pointer, it's there.
            if at.is_some() {
                self.focus_node(i);
            } else {
                self.move_focus(i);
            }
            if self.nodes[i as usize].selected == Some(false) {
                self.activate(i, out);
            }
        }
        self.anchor_node = i;
        self.anchor = Some(match at {
            Some(p) => Rect::new(p.x, p.y, 0.0, 0.0),
            None => self.window_rect(i),
        });
        self.context = i;
        true
    }

    /// The deepest node under `p`, interactive or not; the root if none.
    pub(crate) fn deepest(&self, p: Point) -> u32 {
        match self.walk_at(p, |_| {}).0 {
            NIL => ROOT,
            at => at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: RectPx = RectPx {
        x0: 0,
        y0: 0,
        x1: 1000,
        y1: 800,
    };

    fn r(x0: i32, y0: i32, x1: i32, y1: i32) -> RectPx {
        RectPx { x0, y0, x1, y1 }
    }

    #[test]
    fn a_popup_that_fits_goes_on_its_side() {
        let anchor = r(100, 100, 200, 130);
        assert_eq!(
            place(anchor, 150, 200, WORK, Placement::Below, 4),
            r(100, 134, 250, 334)
        );
        assert_eq!(
            place(anchor, 150, 50, WORK, Placement::Above, 4),
            r(100, 46, 250, 96)
        );
    }

    #[test]
    fn inside_is_lo_when_too_long() {
        assert_eq!(inside(50, 200, 0, 100), 0);
        assert_eq!(inside(-20, 10, 0, 100), 0);
        assert_eq!(inside(95, 10, 0, 100), 90);
    }
}
