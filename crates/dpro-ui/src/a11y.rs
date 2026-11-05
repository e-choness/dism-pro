//! The accessibility tree (TD-0009 §5.2): AccessKit nodes built from the retained tree, which
//! `accesskit_windows` serves to UI Automation.
//!
//! A client's first request gets the whole tree ([`Ui::a11y_tree`]). From then on each frame sends
//! only what changed ([`Ui::a11y_update`]): nodes whose text, check state, enabled or selected
//! state, bounds or children changed, plus the focus. Changes are tracked only while a client
//! listens, so without one the frame does no accessibility work (§7 U8).
//!
//! Each changed node is sent whole, as AccessKit requires. Its id joins the arena index and the
//! generation, so a node's id is stable for its lifetime and never names another node.
//!
//! Roles:
//! - The root is the window.
//! - A container is a group when it has a [name](crate::Element::name), and otherwise a generic
//!   container, which the client leaves out of the tree it exposes.
//! - A label is text; an empty one is hidden.
//! - A button is a button, or a list item (a selection item) when it's [selectable](
//!   crate::Element::selected).
//! - A check box is a toggle.
//!
//! Enabled buttons and check boxes take `Click` and `Focus`; disabled ones are marked so.

use accesskit::{
    Action, ActionRequest, Node, NodeId as AkId, Rect as AkRect, Role as AkRole, Toggled, TreeId,
    TreeInfo, TreeUpdate,
};
use dpro_app::table::Tri;

use crate::{
    tree::{NIL, NodeId, Role},
    ui::{ROOT, Ui},
};

/// What the next accessibility update sends.
#[derive(Debug)]
pub(crate) struct A11y {
    /// A client listens, so changes are tracked.
    active: bool,
    /// Nodes to send. Each was listed when its `a11y_dirty` flag was set; an index may be stale
    /// (freed since) or repeated (freed and reused), and is skipped then.
    changed: Vec<u32>,
    /// The focus last sent.
    focus: u32,
    /// Every node changed (the scale did): the next update sends the whole tree.
    pub(crate) all: bool,
}

impl A11y {
    pub(crate) fn new() -> Self {
        Self {
            active: false,
            changed: Vec::new(),
            focus: NIL,
            all: false,
        }
    }
}

impl<M> Ui<M> {
    /// The whole tree, for a client's first request (AccessKit's `ActivationHandler`). From then
    /// on, [`a11y_update`](Self::a11y_update) reports what changes.
    pub fn a11y_tree(&mut self) -> TreeUpdate {
        self.forget_changes();
        self.a11y.active = true;
        self.a11y.all = false;
        self.a11y.focus = self.focus;
        let mut nodes = Vec::with_capacity(self.len() + 1);
        let mut stack = vec![ROOT];
        while let Some(i) = stack.pop() {
            nodes.push((self.ak_id(i), self.ak_node(i)));
            stack.extend(self.children_of(i));
        }
        let mut tree = TreeInfo::new(self.ak_id(ROOT));
        tree.toolkit_name = Some(env!("CARGO_PKG_NAME").to_owned());
        tree.toolkit_version = Some(env!("CARGO_PKG_VERSION").to_owned());
        TreeUpdate {
            nodes,
            tree: Some(tree),
            tree_id: TreeId::ROOT,
            focus: self.ak_focus(),
        }
    }

    /// What changed since the last tree or update, for the client to apply; `None` when nothing
    /// it sees did, or when no client listens. Call it after [`layout`](Self::layout).
    pub fn a11y_update(&mut self) -> Option<TreeUpdate> {
        if !self.a11y.active {
            return None;
        }
        if self.a11y.all {
            return Some(self.a11y_tree());
        }
        let focus_moved = self.a11y.focus != self.focus;
        if self.a11y.changed.is_empty() && !focus_moved {
            return None;
        }
        let mut nodes = Vec::with_capacity(self.a11y.changed.len());
        for k in 0..self.a11y.changed.len() {
            let i = self.a11y.changed[k];
            let n = &mut self.nodes[i as usize];
            if !n.a11y_dirty {
                continue;
            }
            n.a11y_dirty = false;
            if n.live {
                nodes.push((self.ak_id(i), self.ak_node(i)));
            }
        }
        self.a11y.changed.clear();
        self.a11y.focus = self.focus;
        (!nodes.is_empty() || focus_moved).then(|| TreeUpdate {
            nodes,
            tree: None,
            tree_id: TreeId::ROOT,
            focus: self.ak_focus(),
        })
    }

    /// No client listens any more (AccessKit's `DeactivationHandler`): changes aren't tracked
    /// until the next [`a11y_tree`](Self::a11y_tree).
    pub fn a11y_deactivate(&mut self) {
        self.forget_changes();
        self.a11y.active = false;
    }

    /// The node behind an AccessKit id, if it's still live.
    #[must_use]
    pub fn a11y_node(&self, id: AkId) -> Option<NodeId> {
        self.ak_index(id).and_then(|i| self.id(i))
    }

    /// Lists node `i` for the next update, while a client listens.
    pub(crate) fn a11y_touch(&mut self, i: u32) {
        if !self.a11y.active {
            return;
        }
        let n = &mut self.nodes[i as usize];
        if !n.a11y_dirty {
            n.a11y_dirty = true;
            self.a11y.changed.push(i);
        }
    }

    fn forget_changes(&mut self) {
        for k in 0..self.a11y.changed.len() {
            let i = self.a11y.changed[k];
            self.nodes[i as usize].a11y_dirty = false;
        }
        self.a11y.changed.clear();
    }

    fn ak_id(&self, i: u32) -> AkId {
        AkId((u64::from(self.nodes[i as usize].generation) << 32) | u64::from(i))
    }

    fn ak_index(&self, id: AkId) -> Option<u32> {
        let (index, generation) = split(id);
        self.index(NodeId { index, generation })
    }

    fn ak_focus(&self) -> AkId {
        self.ak_id(if self.focus == NIL { ROOT } else { self.focus })
    }

    /// Node `i` as the client sees it. Bounds are in physical pixels from the client area's
    /// top-left, snapped as painted.
    fn ak_node(&self, i: u32) -> Node {
        let n = &self.nodes[i as usize];
        let role = if i == ROOT {
            AkRole::Window
        } else {
            match n.role {
                Role::Container if n.text.is_empty() => AkRole::GenericContainer,
                Role::Container => AkRole::Group,
                Role::Label => AkRole::Label,
                Role::Button if n.selected.is_some() => AkRole::ListItem,
                Role::Button => AkRole::Button,
                Role::CheckBox => AkRole::CheckBox,
            }
        };
        let mut node = Node::new(role);
        let r = n.rect.snap(self.scale);
        node.set_bounds(AkRect {
            x0: f64::from(r.x0),
            y0: f64::from(r.y0),
            x1: f64::from(r.x1),
            y1: f64::from(r.y1),
        });
        if n.first != NIL {
            node.set_children(
                self.children_of(i)
                    .map(|c| self.ak_id(c))
                    .collect::<Vec<_>>(),
            );
        }
        match (role, n.text.is_empty()) {
            // A label's text is its value, which clients read as its name.
            (AkRole::Label, false) => node.set_value(n.text.as_str()),
            (AkRole::Label, true) => node.set_hidden(),
            (_, false) => node.set_label(n.text.as_str()),
            (_, true) => {}
        }
        if n.role.interactive() {
            if n.enabled {
                node.add_action(Action::Click);
                node.add_action(Action::Focus);
            } else {
                node.set_disabled();
            }
        }
        if n.role == Role::CheckBox {
            node.set_toggled(match n.check {
                Tri::Off => Toggled::False,
                Tri::On => Toggled::True,
                Tri::Mixed => Toggled::Mixed,
            });
        }
        if let Some(selected) = n.selected {
            node.set_selected(selected);
        }
        node
    }
}

impl<M: Clone> Ui<M> {
    /// Carries out a client's request, which the platform posts to the UI thread: `Click`
    /// activates the node as a click does, pushing its message to `out`, and `Focus` moves focus
    /// to it and shows the focus ring. Returns whether it applied; requests for nodes gone since,
    /// nodes that take no input, or other actions do nothing.
    pub fn a11y_action(&mut self, request: &ActionRequest, out: &mut Vec<M>) -> bool {
        if request.target_tree != TreeId::ROOT {
            return false;
        }
        let Some(i) = self.ak_index(request.target_node) else {
            return false;
        };
        if !self.nodes[i as usize].interactive() {
            return false;
        }
        match request.action {
            Action::Click => {
                self.activate(i, out);
                true
            }
            Action::Focus => {
                self.move_focus(i);
                self.show_focus_ring();
                true
            }
            _ => false,
        }
    }
}

/// An AccessKit id's arena index and generation.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the id joins two u32 halves"
)]
fn split(id: AkId) -> (u32, u32) {
    (id.0 as u32, (id.0 >> 32) as u32)
}

#[cfg(test)]
mod tests {
    use accesskit::{Action, ActionRequest, NodeId as AkId, Role as AkRole, Toggled, TreeId};

    use crate::{Element, FakeText, Input, Key, KeyCode, KeyEvent, Mods, Motion, Size, Tri, Ui};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Msg {
        Go,
        Pick(u8),
        Flip,
    }

    struct Page {
        title: &'static str,
        picked: u8,
        rows: u8,
        on: bool,
        go: bool,
    }

    impl Page {
        fn new() -> Self {
            Self {
                title: "Programs",
                picked: 0,
                rows: 3,
                on: false,
                go: true,
            }
        }
    }

    fn frame(ui: &mut Ui<Msg>, p: &Page) {
        ui.build(|cx| {
            cx.add(Element::text(Key::name("heading"), p.title));
            cx.add(Element::text(Key::name("empty"), ""));
            cx.add(Element::button(Key::name("go"), "Go", Msg::Go).enabled(p.go));
            cx.add(Element::toggle(
                Key::name("flip"),
                "Flip",
                if p.on { Tri::On } else { Tri::Off },
                Msg::Flip,
            ));
            cx.add_with(Element::stack(Key::name("list")).name("Rows"), |cx| {
                for r in 0..p.rows {
                    cx.add(
                        Element::button(Key::index(u64::from(r)), "Row", Msg::Pick(r))
                            .selected(r == p.picked),
                    );
                }
            });
            cx.add_with(Element::stack(Key::name("plain")), |cx| {
                cx.add(Element::text(Key::name("note"), "Note"));
            });
        });
        ui.layout(&mut FakeText);
    }

    fn ui() -> (Ui<Msg>, Page) {
        let mut ui = Ui::new(Size { w: 200.0, h: 300.0 }, 1.5, Motion::default());
        let page = Page::new();
        frame(&mut ui, &page);
        (ui, page)
    }

    /// The AccessKit id of the node at `path`, which maps back to it.
    fn id(ui: &Ui<Msg>, path: &[Key]) -> AkId {
        let node = ui.find(path).unwrap();
        let ak = ui.ak_id(node.index);
        assert_eq!(ui.a11y_node(ak), Some(node));
        ak
    }

    fn sent(update: &accesskit::TreeUpdate) -> Vec<u64> {
        let mut ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn the_whole_tree_has_roles_names_states_and_bounds() {
        let (mut ui, _) = ui();
        let tree = ui.a11y_tree();
        assert_eq!(tree.tree.as_ref().unwrap().root, id(&ui, &[]));
        assert_eq!(
            tree.tree.as_ref().unwrap().toolkit_name.as_deref(),
            Some("dpro-ui")
        );
        assert_eq!(tree.focus, id(&ui, &[]), "no focus: the root");
        // The root and its 6 children, 3 rows and the plain container's note.
        assert_eq!(tree.nodes.len(), 11);
        let node = |path: &[Key]| {
            let want = id(&ui, path);
            &tree.nodes.iter().find(|(id, _)| *id == want).unwrap().1
        };
        let root = node(&[]);
        assert_eq!(root.role(), AkRole::Window);
        assert_eq!(root.children().len(), 6);
        let bounds = root.bounds().unwrap();
        assert_eq!((bounds.x1, bounds.y1), (300.0, 450.0), "physical pixels");

        let heading = node(&[Key::name("heading")]);
        assert_eq!(heading.role(), AkRole::Label);
        assert_eq!(heading.value(), Some("Programs"));
        assert!(!heading.is_hidden());
        assert!(node(&[Key::name("empty")]).is_hidden());

        let go = node(&[Key::name("go")]);
        assert_eq!(go.role(), AkRole::Button);
        assert_eq!(go.label(), Some("Go"));
        assert!(go.supports_action(Action::Click) && go.supports_action(Action::Focus));
        assert!(!go.is_disabled());

        let flip = node(&[Key::name("flip")]);
        assert_eq!(flip.role(), AkRole::CheckBox);
        assert_eq!(flip.toggled(), Some(Toggled::False));

        let list = node(&[Key::name("list")]);
        assert_eq!((list.role(), list.label()), (AkRole::Group, Some("Rows")));
        let row0 = node(&[Key::name("list"), Key::index(0)]);
        assert_eq!(row0.role(), AkRole::ListItem);
        assert_eq!(row0.is_selected(), Some(true));
        assert_eq!(
            node(&[Key::name("list"), Key::index(1)]).is_selected(),
            Some(false)
        );
        assert_eq!(node(&[Key::name("plain")]).role(), AkRole::GenericContainer);
    }

    #[test]
    fn updates_send_only_what_changed() {
        let (mut ui, mut page) = ui();
        drop(ui.a11y_tree());
        // Nothing changed.
        frame(&mut ui, &page);
        assert!(ui.a11y_update().is_none());

        // Another row picked: those two rows, nothing else.
        page.picked = 2;
        frame(&mut ui, &page);
        let u = ui.a11y_update().unwrap();
        let mut want = vec![
            id(&ui, &[Key::name("list"), Key::index(0)]).0,
            id(&ui, &[Key::name("list"), Key::index(2)]).0,
        ];
        want.sort_unstable();
        assert_eq!(sent(&u), want);
        assert!(u.tree.is_none());

        // Disabled, toggled, renamed: each node alone.
        page.go = false;
        frame(&mut ui, &page);
        let u = ui.a11y_update().unwrap();
        assert_eq!(sent(&u), [id(&ui, &[Key::name("go")]).0]);
        assert!(u.nodes[0].1.is_disabled());
        assert!(!u.nodes[0].1.supports_action(Action::Click));
        page.on = true;
        frame(&mut ui, &page);
        let u = ui.a11y_update().unwrap();
        assert_eq!(u.nodes[0].1.toggled(), Some(Toggled::True));

        // A new row: it, and its parent with the new child list. Then a removed one: the parent.
        page.rows = 4;
        frame(&mut ui, &page);
        let u = ui.a11y_update().unwrap();
        let list = id(&ui, &[Key::name("list")]);
        let row3 = id(&ui, &[Key::name("list"), Key::index(3)]);
        // The list grew, so what follows it moved too: "plain" and its note, with new bounds.
        let mut want = vec![
            list.0,
            row3.0,
            id(&ui, &[Key::name("plain")]).0,
            id(&ui, &[Key::name("plain"), Key::name("note")]).0,
        ];
        want.sort_unstable();
        assert_eq!(sent(&u), want);
        page.rows = 3;
        frame(&mut ui, &page);
        let u = ui.a11y_update().unwrap();
        let gone = ui.find(&[Key::name("list"), Key::index(3)]);
        assert!(gone.is_none());
        let list_node = &u.nodes.iter().find(|(id, _)| *id == list).unwrap().1;
        assert_eq!(list_node.children().len(), 3);
    }

    #[test]
    fn focus_goes_with_every_update_and_a_move_alone_is_one() {
        let (mut ui, page) = ui();
        drop(ui.a11y_tree());
        let tab = Input::Key(KeyEvent {
            key: KeyCode::Tab,
            down: true,
            mods: Mods::default(),
        });
        ui.dispatch(tab, &mut Vec::new());
        frame(&mut ui, &page);
        let u = ui.a11y_update().unwrap();
        assert_eq!(u.nodes.len(), 0, "focus alone");
        assert_eq!(u.focus, id(&ui, &[Key::name("go")]));
        assert!(ui.a11y_update().is_none());
    }

    #[test]
    fn without_a_client_nothing_is_tracked() {
        let (mut ui, mut page) = ui();
        page.picked = 1;
        frame(&mut ui, &page);
        assert!(ui.a11y_update().is_none());
        assert_eq!(ui.a11y.changed.len(), 0);
        drop(ui.a11y_tree());
        ui.a11y_deactivate();
        page.picked = 2;
        frame(&mut ui, &page);
        assert!(ui.a11y_update().is_none());
        assert_eq!(ui.a11y.changed.len(), 0);
    }

    #[test]
    fn a_scale_change_sends_the_whole_tree() {
        let (mut ui, page) = ui();
        drop(ui.a11y_tree());
        ui.set_scale(2.0);
        frame(&mut ui, &page);
        let u = ui.a11y_update().unwrap();
        assert!(u.tree.is_some());
        assert_eq!(u.nodes.len(), 11);
        let root = &u.nodes.iter().find(|(id, _)| *id == u.focus).unwrap().1;
        assert_eq!(root.bounds().unwrap().x1, 400.0);
    }

    #[test]
    fn actions_click_and_focus_live_interactive_nodes() {
        let (mut ui, mut page) = ui();
        drop(ui.a11y_tree());
        let request = |action, target_node| ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node,
            data: None,
        };
        let mut msgs = Vec::new();
        let row1 = id(&ui, &[Key::name("list"), Key::index(1)]);
        assert!(ui.a11y_action(&request(Action::Click, row1), &mut msgs));
        assert_eq!(msgs, [Msg::Pick(1)]);
        let flip = id(&ui, &[Key::name("flip")]);
        assert!(ui.a11y_action(&request(Action::Focus, flip), &mut msgs));
        assert_eq!(ui.focus(), ui.find(&[Key::name("flip")]));
        assert!(ui.focus_visible());

        // Labels take no actions, disabled nodes neither, and stale ids name nothing.
        let heading = id(&ui, &[Key::name("heading")]);
        assert!(!ui.a11y_action(&request(Action::Click, heading), &mut msgs));
        page.go = false;
        frame(&mut ui, &page);
        let go = id(&ui, &[Key::name("go")]);
        assert!(!ui.a11y_action(&request(Action::Click, go), &mut msgs));
        let row3 = {
            page.rows = 4;
            frame(&mut ui, &page);
            id(&ui, &[Key::name("list"), Key::index(3)])
        };
        page.rows = 3;
        frame(&mut ui, &page);
        assert_eq!(ui.a11y_node(row3), None);
        assert!(!ui.a11y_action(&request(Action::Click, row3), &mut msgs));
        assert_eq!(msgs, [Msg::Pick(1)]);
    }

    #[test]
    fn ids_name_one_node_for_its_lifetime() {
        let (mut ui, mut page) = ui();
        let first = id(&ui, &[Key::name("list"), Key::index(2)]);
        page.rows = 2;
        frame(&mut ui, &page);
        page.rows = 3;
        frame(&mut ui, &page);
        // The slot is reused under a new generation, so the new row has a new id.
        let again = id(&ui, &[Key::name("list"), Key::index(2)]);
        assert_ne!(first, again);
        assert_eq!(ui.a11y_node(first), None);
    }
}
