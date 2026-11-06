//! [`Ui`]: the retained tree in an arena, and its frame state (TD-0009 §5.1, §5.2).
//!
//! A frame is `build` (reconcile the view's description, `build.rs`), then `layout` (`layout.rs`).
//! Input goes through `dispatch`, and animations through `tick` (`input.rs`). Damage collects in
//! physical pixels until the presenter takes it.

use crate::{
    a11y::A11y,
    damage::Damage,
    field::{Caret, ClipboardRequest, Field},
    geom::{Point, Rect, RectPx, Size},
    layout::Shown,
    pool::TextPool,
    scroll::{Drag, Scroll},
    tooltip::Tip,
    tree::{Key, NIL, Node, NodeId, NodeInfo, Role},
};

/// Motion settings, from the theme's motion tokens (doc 05 §4). Durations of 0 turn animation off
/// (reduced motion), so no frame is ever requested for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Motion {
    /// The hover highlight's fade, in milliseconds.
    pub hover_ms: u16,
}

impl Motion {
    /// The motion tokens' durations.
    pub const STANDARD: Self = Self {
        hover_ms: crate::tokens::motion::HOVER_MS,
    };
    /// Reduced motion: every duration 0.
    pub const REDUCED: Self = Self { hover_ms: 0 };

    /// [`STANDARD`](Self::STANDARD) while Windows' *Animation effects* are on, else
    /// [`REDUCED`](Self::REDUCED) (TD-0009 §5.2).
    #[must_use]
    pub const fn system(animations: bool) -> Self {
        if animations {
            Self::STANDARD
        } else {
            Self::REDUCED
        }
    }
}

/// The UI of one window: a retained tree of nodes reconciled from views each frame.
///
/// `M` is the view's message type: input on a node yields the message the view gave it.
#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent window states (active, focus shown, caret on, bars hidden)"
)]
pub struct Ui<M> {
    pub(crate) nodes: Vec<Node<M>>,
    pub(crate) free: Vec<u32>,
    pub(crate) viewport: Size,
    pub(crate) scale: f32,
    /// Windows' text size, 1 to [`MAX_TEXT_SCALE`]: every text style's size is multiplied by it
    /// (TD-0009 §12 item 7).
    pub(crate) text_scale: f32,
    pub(crate) motion: Motion,
    pub(crate) damage: Damage,
    pub(crate) focus: u32,
    pub(crate) hover: u32,
    /// The node the pointer pressed, which gets the release (capture).
    pub(crate) pressed: u32,
    /// The node Space went down on.
    pub(crate) key_pressed: u32,
    /// Focus moved by keyboard, so the focus ring shows (doc 05 §6).
    pub(crate) focus_visible: bool,
    /// Where the pointer is, while it's over the window.
    pub(crate) pointer: Option<Point>,
    /// Nodes with a running animation.
    pub(crate) anims: Vec<NodeId>,
    /// Text buffers of freed nodes, for new ones.
    pub(crate) texts: TextPool,
    /// What the next accessibility update sends.
    pub(crate) a11y: A11y,
    /// The state of field nodes, by their `field` slot; freed slots keep their buffers.
    pub(crate) fields: Vec<Field<M>>,
    pub(crate) free_fields: Vec<u32>,
    /// Windows' caret settings.
    pub(crate) caret: Caret,
    /// The caret is in its on phase (it shows only where a field has focus).
    pub(crate) caret_on: bool,
    /// When the caret's blink started: at the first caret timer after it was last used.
    pub(crate) caret_epoch: Option<u64>,
    /// The window has keyboard focus.
    pub(crate) active: bool,
    /// What the last input asked of the clipboard.
    pub(crate) clipboard: Option<ClipboardRequest>,
    /// Hold buttons' Invoke messages, one per node with `hold` set.
    pub(crate) holds: Vec<HoldSlot<M>>,
    /// The hold running or spent, or none (`input.rs`).
    pub(crate) hold: Hold,
    /// What few nodes have (tooltips, a dropdown's name and step), by their `side` slot; freed
    /// slots keep their buffers.
    pub(crate) sides: Vec<Side<M>>,
    pub(crate) free_sides: Vec<u32>,
    /// The tooltip (`tooltip.rs`).
    pub(crate) tip: Tip,
    /// Where the last popup was asked for, in logical pixels: a context request's point (or its
    /// node, from the keyboard), or the node that opens a popup, activated (`popup.rs`).
    pub(crate) anchor: Option<Rect>,
    /// The node [`anchor`](Self::anchor) is for: when it goes, so does the anchor, and the host
    /// closes the popup that was anchored there (A1.8).
    pub(crate) anchor_node: u32,
    /// The node a context request found since [`take_context`](Self::take_context); [`NIL`] if
    /// none did.
    pub(crate) context: u32,
    /// Scroll viewports' state, by their `scroll` slot; freed slots are reused (`scroll.rs`).
    pub(crate) scrolls: Vec<Scroll>,
    pub(crate) free_scrolls: Vec<u32>,
    /// Windows' wheel characters and lines a notch, by axis (x, y).
    pub(crate) wheel: [u32; 2],
    /// "Automatically hide scroll bars".
    pub(crate) auto_hide_bars: bool,
    /// A scroll bar's thumb being dragged.
    pub(crate) drag: Option<Drag>,
    /// The bar whose strip the pointer is on: its viewport's slot and axis.
    pub(crate) bar_hot: Option<(u32, usize)>,
    /// While layout runs, where the subtree it's arranging shows (`layout.rs`).
    pub(crate) shown: Shown,
}

/// A node's tooltip, and a dropdown's accessible name and step: in a side table, so the nodes
/// without them (nearly all) carry one index.
#[derive(Debug)]
pub(crate) struct Side<M> {
    pub(crate) node: u32,
    pub(crate) tip: String,
    pub(crate) label: String,
    pub(crate) step: Option<fn(bool) -> M>,
}

/// A hold button's Invoke message.
#[derive(Debug)]
pub(crate) struct HoldSlot<M> {
    pub(crate) node: u32,
    pub(crate) invoke: fn() -> M,
}

/// The one hold a window can have: one pointer, one keyboard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hold {
    /// The hold button pressed, or [`NIL`].
    pub(crate) node: u32,
    /// The key holding it, if it's the keyboard's: only that key's release ends it.
    pub(crate) key: Option<crate::KeyCode>,
    /// When the press started counting: the first tick while it's pressed. A pointer that
    /// slides off the button and back starts it again.
    pub(crate) start: Option<u64>,
    /// How far it has come, 0 to 1, as painted.
    pub(crate) t: f32,
    /// It came to the end and sent its message; it does nothing more until let go.
    pub(crate) done: bool,
}

impl Hold {
    pub(crate) const NONE: Self = Self {
        node: NIL,
        key: None,
        start: None,
        t: 0.0,
        done: false,
    };
}

/// The root: index 0, never freed.
pub(crate) const ROOT: u32 = 0;

/// Windows' largest text size, 225 % (Settings › Accessibility › Text size).
pub const MAX_TEXT_SCALE: f32 = 2.25;

impl<M> Ui<M> {
    /// An empty window of `viewport` logical pixels at `scale` physical pixels per logical pixel.
    /// The whole window is damaged for its first paint. It's inactive, so a focused field shows
    /// no caret, until [`set_active`](Self::set_active) says it has keyboard focus.
    #[must_use]
    pub fn new(viewport: Size, scale: f32, motion: Motion) -> Self {
        let mut ui = Self {
            nodes: vec![Node::new(Key(0), Role::Container)],
            free: Vec::new(),
            viewport,
            scale,
            text_scale: 1.0,
            motion,
            damage: Damage::new(),
            focus: NIL,
            hover: NIL,
            pressed: NIL,
            key_pressed: NIL,
            focus_visible: false,
            pointer: None,
            anims: Vec::new(),
            texts: TextPool::default(),
            a11y: A11y::new(),
            fields: Vec::new(),
            free_fields: Vec::new(),
            caret: Caret::WINDOWS,
            caret_on: true,
            caret_epoch: None,
            active: false,
            clipboard: None,
            holds: Vec::new(),
            hold: Hold::NONE,
            sides: Vec::new(),
            free_sides: Vec::new(),
            tip: Tip::new(),
            anchor: None,
            anchor_node: NIL,
            context: NIL,
            scrolls: Vec::new(),
            free_scrolls: Vec::new(),
            wheel: [3, 3],
            auto_hide_bars: true,
            drag: None,
            bar_hot: None,
            shown: Shown::default(),
        };
        ui.damage_all();
        ui
    }

    /// The window's client size changed. The next `layout` lays out again.
    pub fn set_viewport(&mut self, viewport: Size) {
        if self.viewport != viewport {
            self.viewport = viewport;
            self.mark_dirty(ROOT);
        }
    }

    /// The window's scale changed (`WM_DPICHANGED`): every pixel is repainted, and every node's
    /// bounds go to assistive technology again.
    pub fn set_scale(&mut self, scale: f32) {
        if self.scale.to_bits() != scale.to_bits() {
            self.rescale_scrolls(self.scale, scale);
            self.scale = scale;
            self.damage_all();
            self.a11y.all = true;
        }
    }

    /// Windows' text size changed (Settings › Accessibility › Text size), as a factor: clamped to
    /// 1 to [`MAX_TEXT_SCALE`], and 1 if it isn't a number. The next [`build`](Self::build) gives
    /// every node its scaled text style, so the page is laid out again, and the whole window is
    /// repainted. The same size again changes nothing.
    pub fn set_text_scale(&mut self, text_scale: f32) {
        let text_scale = if text_scale.is_nan() {
            1.0
        } else {
            text_scale.clamp(1.0, MAX_TEXT_SCALE)
        };
        if self.text_scale.to_bits() != text_scale.to_bits() {
            self.text_scale = text_scale;
            self.damage_all();
        }
    }

    /// Windows' text size, as a factor.
    #[must_use]
    pub fn text_scale(&self) -> f32 {
        self.text_scale
    }

    /// The durations changed (*Animation effects* toggled). A running animation finishes on its
    /// next tick if its duration is now 0.
    pub fn set_motion(&mut self, motion: Motion) {
        self.motion = motion;
    }

    #[must_use]
    pub fn motion(&self) -> Motion {
        self.motion
    }

    #[must_use]
    pub fn viewport(&self) -> Size {
        self.viewport
    }

    #[must_use]
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// What to repaint and present, in physical pixels.
    #[must_use]
    pub fn damage(&self) -> &[RectPx] {
        self.damage.rects()
    }

    /// The presenter took the damage.
    pub fn clear_damage(&mut self) {
        self.damage.clear();
    }

    #[must_use]
    pub fn focus(&self) -> Option<NodeId> {
        self.id(self.focus)
    }

    #[must_use]
    pub fn hover(&self) -> Option<NodeId> {
        self.id(self.hover)
    }

    /// Live nodes, the root excluded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len() - self.free.len() - 1
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The node reached from the root through children with these keys.
    #[must_use]
    pub fn find(&self, path: &[Key]) -> Option<NodeId> {
        let mut at = ROOT;
        for &key in path {
            at = self
                .children_of(at)
                .find(|&c| self.nodes[c as usize].key == key)?;
        }
        self.id(at)
    }

    /// The children of `id`, in order.
    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let parent = self.index(id).unwrap_or(NIL);
        self.children_of(parent).filter_map(move |c| self.id(c))
    }

    /// The root's id, the parent of what the view builds.
    #[must_use]
    pub fn root(&self) -> NodeId {
        NodeId {
            index: ROOT,
            generation: self.nodes[ROOT as usize].generation,
        }
    }

    #[must_use]
    pub fn info(&self, id: NodeId) -> Option<NodeInfo<'_>> {
        let i = self.index(id)?;
        let n = &self.nodes[i as usize];
        Some(NodeInfo {
            role: n.role,
            key: n.key,
            rect: self.window_rect(i),
            text: &n.text,
            text_style: n.text_style,
            enabled: n.enabled,
            check: n.check,
            selected: n.selected,
            expanded: n.expanded,
            hovered: n.hovered,
            pressed: n.pressed,
            focused: self.focus == i,
            hover: n.hover_t,
            scroll: self.scroll_info(i),
        })
    }

    // ---- arena -------------------------------------------------------------------------------

    pub(crate) fn id(&self, index: u32) -> Option<NodeId> {
        let n = self.nodes.get(index as usize)?;
        n.live.then_some(NodeId {
            index,
            generation: n.generation,
        })
    }

    pub(crate) fn index(&self, id: NodeId) -> Option<u32> {
        let n = self.nodes.get(id.index as usize)?;
        (n.live && n.generation == id.generation).then_some(id.index)
    }

    pub(crate) fn children_of(&self, parent: u32) -> impl Iterator<Item = u32> + '_ {
        let first = self.nodes.get(parent as usize).map_or(NIL, |n| n.first);
        std::iter::successors((first != NIL).then_some(first), move |&c| {
            let next = self.nodes[c as usize].next;
            (next != NIL).then_some(next)
        })
    }

    /// A new detached node, from the free list when possible, with a spare text buffer that holds
    /// `text_len` bytes when there is one.
    pub(crate) fn alloc(&mut self, key: Key, role: Role, text_len: usize) -> u32 {
        let text = self.texts.take(text_len);
        let i = if let Some(i) = self.free.pop() {
            self.nodes[i as usize].reuse(key, role, text);
            i
        } else {
            let i = u32::try_from(self.nodes.len()).unwrap_or(NIL);
            let mut node = Node::new(key, role);
            node.text = text;
            self.nodes.push(node);
            i
        };
        if role == Role::Field {
            self.nodes[i as usize].field = self.alloc_field(i);
        }
        i
    }

    /// Links detached `child` under `parent`, before `before` (or last, for [`NIL`]).
    pub(crate) fn link_before(&mut self, parent: u32, child: u32, before: u32) {
        let prev = if before == NIL {
            self.nodes[parent as usize].last
        } else {
            self.nodes[before as usize].prev
        };
        let p = &self.nodes[parent as usize];
        let in_viewport = p.in_viewport || p.scroll != NIL;
        {
            let c = &mut self.nodes[child as usize];
            c.parent = parent;
            c.prev = prev;
            c.next = before;
            c.in_viewport = in_viewport;
        }
        if prev == NIL {
            self.nodes[parent as usize].first = child;
        } else {
            self.nodes[prev as usize].next = child;
        }
        if before == NIL {
            self.nodes[parent as usize].last = child;
        } else {
            self.nodes[before as usize].prev = child;
        }
    }

    pub(crate) fn unlink(&mut self, child: u32) {
        let (parent, prev, next) = {
            let c = &self.nodes[child as usize];
            (c.parent, c.prev, c.next)
        };
        if prev == NIL {
            self.nodes[parent as usize].first = next;
        } else {
            self.nodes[prev as usize].next = next;
        }
        if next == NIL {
            self.nodes[parent as usize].last = prev;
        } else {
            self.nodes[next as usize].prev = prev;
        }
        let c = &mut self.nodes[child as usize];
        c.parent = NIL;
        c.prev = NIL;
        c.next = NIL;
    }

    /// Unlinks and frees `child` with its subtree, damaging where it was.
    pub(crate) fn remove(&mut self, child: u32) {
        self.damage_view(child);
        let parent = self.nodes[child as usize].parent;
        self.unlink(child);
        self.free_subtree(child);
        self.forget_freed();
        self.mark_dirty(parent);
        self.a11y_touch(parent);
    }

    /// Drops what pointed into a subtree just freed (nothing has reused its slots yet). Once per
    /// removal rather than per freed node: a filter that removes every row frees ~400 nodes.
    fn forget_freed(&mut self) {
        let nodes = &self.nodes;
        let gone = |i: u32| i != NIL && !nodes[i as usize].live;
        for slot in [
            &mut self.focus,
            &mut self.hover,
            &mut self.pressed,
            &mut self.key_pressed,
            &mut self.context,
        ] {
            if gone(*slot) {
                *slot = NIL;
            }
        }
        if gone(self.anchor_node) {
            self.anchor = None;
            self.anchor_node = NIL;
        }
        self.tip_forget_freed();
    }

    fn free_subtree(&mut self, i: u32) {
        let mut c = self.nodes[i as usize].first;
        while c != NIL {
            let next = self.nodes[c as usize].next;
            self.free_subtree(c);
            c = next;
        }
        if self.nodes[i as usize].animating
            && let Some(k) = self.anims.iter().position(|a| a.index == i)
        {
            self.anims.swap_remove(k);
        }
        let field = std::mem::replace(&mut self.nodes[i as usize].field, NIL);
        if field != NIL {
            self.release_field(field);
        }
        if std::mem::replace(&mut self.nodes[i as usize].hold, false) {
            self.forget_hold(i);
        }
        // Only a node with popup state (a tooltip, a dropdown) has a side slot: the rest skip it.
        debug_assert!(
            self.nodes[i as usize].rare != 0 || self.nodes[i as usize].side == NIL,
            "node {i} has a side slot and no popup state"
        );
        if self.nodes[i as usize].rare != 0 {
            let side = std::mem::replace(&mut self.nodes[i as usize].side, NIL);
            if side != NIL {
                self.release_side(side);
            }
            let scroll = std::mem::replace(&mut self.nodes[i as usize].scroll, NIL);
            if scroll != NIL {
                self.release_scroll(scroll);
            }
        }
        let n = &mut self.nodes[i as usize];
        n.live = false;
        n.generation = n.generation.wrapping_add(1);
        n.msg = None;
        n.first = NIL;
        n.last = NIL;
        let text = std::mem::take(&mut n.text);
        self.texts.put(text);
        self.free.push(i);
    }

    /// `i` changed in a way layout must see: it and its ancestors are laid out again, and their
    /// cached measurements are dropped. Stops at an ancestor already marked, whose ancestors are.
    pub(crate) fn mark_dirty(&mut self, mut i: u32) {
        while i != NIL {
            let n = &mut self.nodes[i as usize];
            let was = n.dirty && !n.cache_valid;
            n.dirty = true;
            n.cache_valid = false;
            if was {
                break;
            }
            i = n.parent;
        }
    }

    // ---- damage ------------------------------------------------------------------------------

    /// Damages `rect` (logical), clipped to the window.
    pub(crate) fn damage_rect(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        let window = Rect::new(0.0, 0.0, self.viewport.w, self.viewport.h).to_px(self.scale);
        self.damage.add(rect.to_px(self.scale).intersect(window));
    }

    /// The node's look changed but not its layout (hover, press, focus, check state).
    /// Inside a scroll viewport, where it shows.
    pub(crate) fn damage_node(&mut self, i: u32) {
        if (i as usize) < self.nodes.len() {
            self.damage_view(i);
        }
    }

    pub(crate) fn damage_all(&mut self) {
        let all = Rect::new(0.0, 0.0, self.viewport.w, self.viewport.h);
        self.damage_rect(all);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui() -> Ui<()> {
        Ui::new(Size { w: 100.0, h: 50.0 }, 1.0, Motion::default())
    }

    #[test]
    fn a_new_window_is_damaged_whole() {
        let ui = ui();
        assert_eq!(
            ui.damage(),
            [RectPx {
                x0: 0,
                y0: 0,
                x1: 100,
                y1: 50
            }]
        );
        assert!(ui.is_empty());
    }

    #[test]
    fn links_keep_sibling_order_and_freed_ids_go_stale() {
        let mut ui = ui();
        let a = ui.alloc(Key(1), Role::Label, 0);
        let b = ui.alloc(Key(2), Role::Label, 0);
        let c = ui.alloc(Key(3), Role::Label, 0);
        ui.link_before(ROOT, a, NIL);
        ui.link_before(ROOT, c, NIL);
        ui.link_before(ROOT, b, c);
        assert_eq!(ui.children_of(ROOT).collect::<Vec<_>>(), [a, b, c]);
        let old = ui.id(b).unwrap();
        ui.remove(b);
        assert_eq!(ui.children_of(ROOT).collect::<Vec<_>>(), [a, c]);
        assert_eq!(ui.index(old), None);
        // The freed slot is reused under a new generation.
        let d = ui.alloc(Key(4), Role::Label, 0);
        assert_eq!(d, b);
        assert_ne!(ui.id(d), Some(old));
        assert_eq!(ui.len(), 3);
    }
}
