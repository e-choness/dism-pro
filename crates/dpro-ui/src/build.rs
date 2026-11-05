//! Reconciling (TD-0009 §5.2): a view describes this frame's children through [`Cx`], and each
//! description is matched by [`Key`](crate::Key) against the retained children in order.
//!
//! - The next retained child has the key: it's updated in place. A stable list costs one
//!   comparison per node.
//! - A later child has it: that node moves here (a reorder, or nodes before it went away).
//! - None has it: a node is created here, from the free list when possible.
//!
//! Children left unmatched when the view is done are removed. Text is copied only when it changed,
//! into the node's own buffer, so a warm window reconciles without allocating (§7 U8).

use crate::{
    pool::capacity_for,
    tree::{Element, NIL, NodeId},
    ui::{ROOT, Ui},
};

/// Builds one parent's children for this frame.
#[derive(Debug)]
pub struct Cx<'u, M> {
    ui: &'u mut Ui<M>,
    parent: u32,
    /// The first retained child not matched yet; everything before it is this frame's.
    next: u32,
}

impl<M> Ui<M> {
    /// Reconciles the root's children with `view`'s description of them.
    pub fn build(&mut self, view: impl FnOnce(&mut Cx<'_, M>)) {
        let next = self.nodes[ROOT as usize].first;
        let mut cx = Cx {
            ui: self,
            parent: ROOT,
            next,
        };
        view(&mut cx);
        cx.finish();
    }
}

impl<M> Cx<'_, M> {
    /// A node without children. Children it had last frame are removed.
    pub fn add(&mut self, el: Element<'_, M>) -> NodeId {
        let i = self.place(el);
        let mut c = self.ui.nodes[i as usize].first;
        while c != NIL {
            let next = self.ui.nodes[c as usize].next;
            self.ui.remove(c);
            c = next;
        }
        self.id(i)
    }

    /// A node whose children `children` describes.
    pub fn add_with(
        &mut self,
        el: Element<'_, M>,
        children: impl FnOnce(&mut Cx<'_, M>),
    ) -> NodeId {
        let i = self.place(el);
        let next = self.ui.nodes[i as usize].first;
        let mut cx = Cx {
            ui: &mut *self.ui,
            parent: i,
            next,
        };
        children(&mut cx);
        cx.finish();
        self.id(i)
    }

    fn id(&self, i: u32) -> NodeId {
        NodeId {
            index: i,
            generation: self.ui.nodes[i as usize].generation,
        }
    }

    /// Matches, moves or creates the node for `el` at the cursor, and updates it.
    #[inline]
    fn place(&mut self, el: Element<'_, M>) -> u32 {
        let ui = &mut *self.ui;
        let matches = |ui: &Ui<M>, c: u32| {
            let n = &ui.nodes[c as usize];
            n.key == el.key && n.role == el.role
        };
        let i = if self.next != NIL && matches(ui, self.next) {
            let i = self.next;
            self.next = ui.nodes[i as usize].next;
            i
        } else {
            let mut found = NIL;
            let mut c = self.next;
            while c != NIL {
                let after = ui.nodes[c as usize].next;
                if matches(ui, c) {
                    found = c;
                    break;
                }
                c = after;
            }
            if found == NIL {
                let i = ui.alloc(el.key, el.role, el.text.len());
                ui.link_before(self.parent, i, self.next);
                ui.mark_dirty(self.parent);
                ui.a11y_touch(i);
                ui.a11y_touch(self.parent);
                i
            } else {
                // Moved: its old place is damaged, and layout finds its new one.
                let rect = ui.nodes[found as usize].rect;
                ui.damage_rect(rect);
                ui.unlink(found);
                ui.link_before(self.parent, found, self.next);
                ui.mark_dirty(self.parent);
                ui.a11y_touch(self.parent);
                found
            }
        };
        ui.update(i, el);
        i
    }

    /// Removes the children no description matched.
    fn finish(self) {
        let mut c = self.next;
        while c != NIL {
            let next = self.ui.nodes[c as usize].next;
            self.ui.remove(c);
            c = next;
        }
    }
}

impl<M> Ui<M> {
    /// Copies what changed from `el` into node `i`: layout changes mark it dirty, looks (check
    /// state, enabled, [`Look`](crate::Look)) damage it, and what a client sees (text, check
    /// state, enabled, selected) goes into the next accessibility update.
    #[inline]
    fn update(&mut self, i: u32, el: Element<'_, M>) {
        let n = &mut self.nodes[i as usize];
        let mut layout = false;
        // Check state and enabled show, and a client sees them.
        let state = n.check != el.check || n.enabled != el.enabled;
        let mut seen = state || n.selected != el.selected;
        if n.style != el.style {
            n.style = el.style;
            layout = true;
        }
        if n.text_style != el.text_style {
            n.text_style = el.text_style;
            layout = true;
        }
        if n.text != el.text {
            n.text.clear();
            if n.text.capacity() < el.text.len() {
                n.text.reserve_exact(capacity_for(el.text.len()));
            }
            n.text.push_str(el.text);
            layout = true;
            seen = true;
        }
        let looks = state || n.look != el.look;
        n.look = el.look;
        n.check = el.check;
        n.selected = el.selected;
        n.msg = el.msg;
        if n.enabled != el.enabled {
            n.enabled = el.enabled;
            if !el.enabled {
                self.lose_input(i);
            }
        }
        if layout {
            self.mark_dirty(i);
        }
        if looks {
            // Where it is now; if layout moves it, layout damages where it goes.
            self.damage_node(i);
        }
        if seen {
            self.a11y_touch(i);
        }
    }

    /// A node that became disabled lets go of focus, hover and presses.
    fn lose_input(&mut self, i: u32) {
        if self.focus == i {
            self.focus = NIL;
        }
        if self.pressed == i {
            self.pressed = NIL;
        }
        if self.key_pressed == i {
            self.key_pressed = NIL;
        }
        if self.hover == i {
            self.set_hover(NIL);
        }
        self.nodes[i as usize].pressed = false;
    }
}

#[cfg(test)]
mod tests {
    use crate::{Element, Key, Motion, Size, Ui};

    fn ui() -> Ui<u8> {
        Ui::new(Size { w: 100.0, h: 100.0 }, 1.0, Motion::default())
    }

    fn keys(ui: &Ui<u8>) -> Vec<u64> {
        ui.children(ui.root())
            .map(|id| ui.info(id).unwrap().key.0)
            .collect()
    }

    fn list(ui: &mut Ui<u8>, items: &[u64]) {
        ui.build(|cx| {
            for &k in items {
                cx.add(Element::text(Key(k), "x"));
            }
        });
    }

    #[test]
    fn stable_lists_keep_their_nodes() {
        let mut ui = ui();
        list(&mut ui, &[1, 2, 3]);
        let ids: Vec<_> = ui.children(ui.root()).collect();
        list(&mut ui, &[1, 2, 3]);
        assert_eq!(ui.children(ui.root()).collect::<Vec<_>>(), ids);
    }

    #[test]
    fn reorders_move_nodes_and_keep_their_identity() {
        let mut ui = ui();
        list(&mut ui, &[1, 2, 3, 4]);
        let id = |ui: &Ui<u8>, k| ui.find(&[Key(k)]).unwrap();
        let three = id(&ui, 3);
        list(&mut ui, &[3, 1, 4, 2]);
        assert_eq!(keys(&ui), [3, 1, 4, 2]);
        assert_eq!(id(&ui, 3), three);
        list(&mut ui, &[2, 4, 1, 3]);
        assert_eq!(keys(&ui), [2, 4, 1, 3]);
        assert_eq!(ui.len(), 4);
    }

    #[test]
    fn insertions_and_removals_anywhere() {
        let mut ui = ui();
        list(&mut ui, &[1, 2, 3]);
        let two = ui.find(&[Key(2)]).unwrap();
        list(&mut ui, &[0, 2, 5]);
        assert_eq!(keys(&ui), [0, 2, 5]);
        assert_eq!(ui.find(&[Key(2)]), Some(two));
        assert_eq!(ui.len(), 3);
        list(&mut ui, &[]);
        assert!(ui.is_empty());
        assert_eq!(ui.info(two), None);
    }

    #[test]
    fn a_role_change_makes_a_new_node() {
        let mut ui = ui();
        list(&mut ui, &[1]);
        let label = ui.find(&[Key(1)]).unwrap();
        ui.build(|cx| {
            cx.add(Element::button(Key(1), "x", 7));
        });
        let button = ui.find(&[Key(1)]).unwrap();
        assert_ne!(label, button);
        assert_eq!(ui.len(), 1);
    }

    #[test]
    fn children_are_reconciled_per_parent_and_dropped_by_add() {
        let mut ui = ui();
        ui.build(|cx| {
            cx.add_with(Element::stack(Key(1)), |cx| {
                cx.add(Element::text(Key(10), "a"));
                cx.add(Element::text(Key(11), "b"));
            });
        });
        assert_eq!(ui.len(), 3);
        let a = ui.find(&[Key(1), Key(10)]).unwrap();
        assert_eq!(ui.info(a).unwrap().text, "a");
        ui.build(|cx| {
            cx.add(Element::stack(Key(1)));
        });
        assert_eq!(ui.len(), 1);
        assert_eq!(ui.info(a), None);
    }

    #[test]
    fn changed_text_is_copied_and_marks_layout() {
        let mut ui = ui();
        ui.build(|cx| {
            cx.add(Element::text(Key(1), "before"));
        });
        let t = ui.find(&[Key(1)]).unwrap();
        ui.nodes[t.index as usize].dirty = false;
        ui.nodes[0].dirty = false;
        ui.build(|cx| {
            cx.add(Element::text(Key(1), "after"));
        });
        assert_eq!(ui.info(t).unwrap().text, "after");
        assert!(ui.nodes[t.index as usize].dirty && ui.nodes[0].dirty);
    }
}
