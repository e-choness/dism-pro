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
    field::TextEdit,
    pool::capacity_for,
    text::TextStyle,
    tree::{Element, Extra, NIL, NodeId, Role, rare},
    ui::{HoldSlot, ROOT, Side, Ui},
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
                if el.role.interactive() {
                    ui.stop_changed(i);
                }
                i
            } else {
                // Moved: its old place is damaged, and layout finds its new one.
                ui.damage_view(found);
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
        let picked = n.selected != el.selected;
        let mut seen = state || picked;
        // A field's text never changes its size: the view sets its width, and it's a line high.
        let field = n.field;
        let (mut field_text, mut field_style) = (false, false);
        if n.style != el.style {
            n.style = el.style;
            layout = true;
        }
        // Windows' text size multiplies every style here, so measuring, caret stops, painting and
        // the accessibility tree all see one size. At 100 % the product is exact.
        let text_style = TextStyle {
            size: el.text_style.size * self.text_scale,
            ..el.text_style
        };
        if n.text_style != text_style {
            n.text_style = text_style;
            layout = true;
            field_style = true;
        }
        if n.text != el.text {
            n.text.clear();
            if n.text.capacity() < el.text.len() {
                n.text.reserve_exact(capacity_for(el.text.len()));
            }
            n.text.push_str(el.text);
            if field == NIL {
                layout = true;
            } else {
                field_text = true;
            }
            seen = true;
        }
        // A radio button shows whether it's picked.
        let mut looks = state || n.look != el.look || (picked && n.role == Role::Radio);
        n.look = el.look;
        n.check = el.check;
        n.selected = el.selected;
        n.msg = el.msg;
        let special = n.special;
        // Popup state, now or last frame: few nodes have any, so the rest skip it here.
        let rare = n.rare | el.rare;
        if n.enabled != el.enabled {
            n.enabled = el.enabled;
            if !el.enabled {
                self.lose_input(i);
            }
            if self.nodes[i as usize].role.interactive() {
                self.stop_changed(i);
            }
        }
        if rare != 0 {
            seen |= self.update_rare(i, el.rare, el.tip, el.extra);
        }
        match el.extra {
            // Most nodes: nothing more.
            Extra::None if !special => {}
            // Only a field describes one, and every field has its slot.
            Extra::Field {
                label,
                placeholder,
                on_edit,
            } if field != NIL => {
                seen |= self.update_field(field, label, placeholder, on_edit);
                if field_text {
                    self.field_text_set(i);
                } else if field_style {
                    // The same text in another size: new caret stops, the caret where it was.
                    self.fields[field as usize].stops_valid = false;
                }
            }
            extra => {
                let (s, l) = self.update_extra(i, extra);
                seen |= s;
                looks |= l;
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

    /// Copies a field's name, placeholder and edit handler into its slot `f`. Returns whether a
    /// client sees a change; a new placeholder shows while the field is empty.
    fn update_field(
        &mut self,
        f: u32,
        name: &str,
        placeholder: &str,
        on_edit: Option<fn(TextEdit) -> M>,
    ) -> bool {
        let slot = &mut self.fields[f as usize];
        // Becoming read-only, or editable, is seen.
        let mut seen = slot.on_edit.is_some() != on_edit.is_some();
        slot.on_edit = on_edit;
        if slot.name != name {
            slot.name.clear();
            slot.name.push_str(name);
            seen = true;
        }
        if slot.placeholder != placeholder {
            slot.placeholder.clear();
            slot.placeholder.push_str(placeholder);
            let i = slot.node;
            if self.nodes[i as usize].text.is_empty() {
                self.damage_node(i);
            }
            seen = true;
        }
        seen
    }

    /// Copies a group, a progress bar's value or a hold button's Invoke message into node `i`,
    /// clearing what it had of the others. Returns whether a client sees a change, and whether
    /// it shows.
    fn update_extra(&mut self, i: u32, extra: Extra<'_, M>) -> (bool, bool) {
        let (group, value, invoke) = match extra {
            Extra::Group(g) => (Some(g), 0.0, None),
            Extra::Progress(v) => (None, v, None),
            Extra::Hold(m) => (None, 0.0, Some(m)),
            Extra::None | Extra::Field { .. } | Extra::Combo { .. } => (None, 0.0, None),
        };
        let n = &mut self.nodes[i as usize];
        // The group sets a client's role (a tab list, a radio group, a list).
        let regrouped = n.group != group;
        // A progress bar's fill shows, and a client reads it as its value.
        let revalued = n.value.to_bits() != value.to_bits();
        n.group = group;
        n.value = value;
        n.special = group.is_some() || invoke.is_some() || value.to_bits() != 0;
        if n.hold || invoke.is_some() {
            self.update_hold(i, invoke);
        }
        (regrouped || revalued, revalued)
    }

    /// Node `i`'s popup state from its element's [`rare`] flags `r`, tooltip `tip` and `extra`:
    /// whether it takes context requests, whether what it opens is open, and its side slot.
    /// Returns whether a client sees a change.
    #[inline(never)]
    fn update_rare(&mut self, i: u32, r: u8, tip: &str, extra: Extra<'_, M>) -> bool {
        let n = &mut self.nodes[i as usize];
        let scrolled = (n.rare | r) & rare::SCROLL != 0;
        n.rare = r;
        n.context = r & rare::CONTEXT != 0;
        // Whether its popup is open: a client sees it (ExpandCollapse).
        let expanded = rare::expanded(r);
        let mut seen = n.expanded != expanded;
        n.expanded = expanded;
        let (label, step) = match extra {
            Extra::Combo { label, step } => (label, step),
            _ => ("", None),
        };
        if n.side != NIL || !tip.is_empty() || !label.is_empty() || step.is_some() {
            seen |= self.update_side(i, tip, label, step);
        }
        if scrolled {
            seen |= self.update_scroll(i, r);
        }
        seen
    }

    /// Keeps node `i`'s tooltip, and a dropdown's name and step, in its side slot: a node that
    /// gained one gets a slot, and one left with none gives it back (and its tooltip goes).
    /// Returns whether a client sees a change: the tip is its description, the name its name.
    fn update_side(&mut self, i: u32, tip: &str, label: &str, step: Option<fn(bool) -> M>) -> bool {
        let mut s = self.nodes[i as usize].side;
        if tip.is_empty() && label.is_empty() && step.is_none() {
            if s == NIL {
                return false;
            }
            self.nodes[i as usize].side = NIL;
            self.release_side(s);
            self.tip_forget(i);
            return true;
        }
        let mut seen = false;
        if s == NIL {
            s = self.alloc_side(i);
            self.nodes[i as usize].side = s;
            seen = true;
        }
        let side = &mut self.sides[s as usize];
        side.step = step;
        if side.tip != tip {
            side.tip.clear();
            side.tip.push_str(tip);
            seen = true;
        }
        if side.label != label {
            side.label.clear();
            side.label.push_str(label);
            seen = true;
        }
        if tip.is_empty() {
            self.tip_forget(i);
        }
        seen
    }

    /// A side slot for node `i`, a freed one if there is one.
    fn alloc_side(&mut self, i: u32) -> u32 {
        if let Some(s) = self.free_sides.pop() {
            self.sides[s as usize].node = i;
            s
        } else {
            self.sides.push(Side {
                node: i,
                tip: String::new(),
                label: String::new(),
                step: None,
            });
            u32::try_from(self.sides.len() - 1).unwrap_or(NIL)
        }
    }

    /// Frees side slot `s`, keeping its buffers for the next.
    pub(crate) fn release_side(&mut self, s: u32) {
        if let Some(side) = self.sides.get_mut(s as usize) {
            side.node = NIL;
            side.tip.clear();
            side.label.clear();
            side.step = None;
            self.free_sides.push(s);
        }
    }

    /// Keeps hold button `i`'s Invoke message in the hold table: a button that became a hold
    /// button gets a slot, and one that stopped being one loses it, with its hold if one runs.
    fn update_hold(&mut self, i: u32, invoke: Option<fn() -> M>) {
        let slot = self.holds.iter().position(|s| s.node == i);
        match (slot, invoke) {
            (Some(k), Some(invoke)) => self.holds[k].invoke = invoke,
            (None, Some(invoke)) => {
                self.holds.push(HoldSlot { node: i, invoke });
                self.nodes[i as usize].hold = true;
            }
            (_, None) => {
                self.nodes[i as usize].hold = false;
                self.forget_hold(i);
            }
        }
    }

    /// A node that became disabled lets go of focus, hover, presses and holds.
    fn lose_input(&mut self, i: u32) {
        if self.hold.node == i {
            self.end_hold();
        }
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
