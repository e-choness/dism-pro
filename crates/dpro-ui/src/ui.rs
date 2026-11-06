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
    pool::TextPool,
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
pub struct Ui<M> {
    pub(crate) nodes: Vec<Node<M>>,
    pub(crate) free: Vec<u32>,
    pub(crate) viewport: Size,
    pub(crate) scale: f32,
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
}

/// The root: index 0, never freed.
pub(crate) const ROOT: u32 = 0;

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
            self.scale = scale;
            self.damage_all();
            self.a11y.all = true;
        }
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
            rect: n.rect,
            text: &n.text,
            enabled: n.enabled,
            check: n.check,
            selected: n.selected,
            hovered: n.hovered,
            pressed: n.pressed,
            focused: self.focus == i,
            hover: n.hover_t,
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
        {
            let c = &mut self.nodes[child as usize];
            c.parent = parent;
            c.prev = prev;
            c.next = before;
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
        let rect = self.nodes[child as usize].rect;
        self.damage_rect(rect);
        let parent = self.nodes[child as usize].parent;
        self.unlink(child);
        self.free_subtree(child);
        self.mark_dirty(parent);
        self.a11y_touch(parent);
    }

    fn free_subtree(&mut self, i: u32) {
        let mut c = self.nodes[i as usize].first;
        while c != NIL {
            let next = self.nodes[c as usize].next;
            self.free_subtree(c);
            c = next;
        }
        for slot in [
            &mut self.focus,
            &mut self.hover,
            &mut self.pressed,
            &mut self.key_pressed,
        ] {
            if *slot == i {
                *slot = NIL;
            }
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
    pub(crate) fn damage_node(&mut self, i: u32) {
        if let Some(n) = self.nodes.get(i as usize) {
            let rect = n.rect;
            self.damage_rect(rect);
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
