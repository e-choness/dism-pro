//! TD-0009 §5.2, §10: incremental accessibility updates keep a client's copy of the tree equal to
//! the whole tree.
//!
//! A client (`accesskit_windows`) applies each update to its copy: every node sent replaces the
//! one with its id, and nodes no longer reachable from the root are gone. The update must follow
//! AccessKit's rules: every node it sends is in the tree afterwards (the root, or a child of a
//! node in it), so a removed node is never sent, and a new node's parent is sent with it. After
//! every step of the scripted session, plus row selections and page switches, the copy must equal
//! the whole tree node for node, focus included.

#[path = "../benches/fixtures/shell.rs"]
mod shell;

use std::collections::{HashMap, HashSet};

use dpro_ui::{
    Input, KeyCode, KeyEvent, Mods, Point,
    accesskit::{Node, NodeId, TreeUpdate},
};
use shell::{Msg, Run, Shell, Step};

/// A client's copy of the tree.
struct Mirror {
    root: NodeId,
    nodes: HashMap<NodeId, Node>,
    focus: NodeId,
}

impl Mirror {
    fn new(full: TreeUpdate) -> Self {
        assert!(full.tree.is_some(), "a whole tree names its root");
        let root = full.tree.as_ref().map_or(NodeId(0), |tree| tree.root);
        let mut m = Self {
            root,
            nodes: HashMap::new(),
            focus: full.focus,
        };
        m.apply(full, "the first tree");
        m
    }

    fn apply(&mut self, update: TreeUpdate, step: &str) {
        if let Some(tree) = &update.tree {
            assert_eq!(tree.root.0, self.root.0, "{step}: the root changed");
        }
        let sent: Vec<NodeId> = update.nodes.iter().map(|(id, _)| *id).collect();
        let distinct: HashSet<u64> = sent.iter().map(|id| id.0).collect();
        assert_eq!(distinct.len(), sent.len(), "{step}: a node was sent twice");
        for (id, node) in update.nodes {
            self.nodes.insert(id, node);
        }
        self.prune();
        for id in sent {
            assert!(
                self.nodes.contains_key(&id),
                "{step}: node {} was sent but isn't in the tree",
                id.0
            );
        }
        assert!(
            self.nodes.contains_key(&update.focus),
            "{step}: focus {} isn't in the tree",
            update.focus.0
        );
        self.focus = update.focus;
    }

    /// Drops what the root no longer reaches, as the client does.
    fn prune(&mut self) {
        let mut reached = HashSet::new();
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            if reached.insert(id.0)
                && let Some(node) = self.nodes.get(&id)
            {
                stack.extend_from_slice(node.children());
            }
        }
        self.nodes.retain(|id, _| reached.contains(&id.0));
    }

    fn assert_equals(&self, full: &TreeUpdate, step: &str) {
        assert_eq!(self.focus.0, full.focus.0, "{step}: focus");
        assert_eq!(
            self.nodes.len(),
            full.nodes.len(),
            "{step}: the copy has {} nodes, the tree {}",
            self.nodes.len(),
            full.nodes.len()
        );
        for (id, node) in &full.nodes {
            assert!(
                self.nodes.get(id) == Some(node),
                "{step}: node {} differs from the tree's",
                id.0
            );
        }
    }
}

#[test]
fn incremental_updates_keep_a_copy_equal_to_the_whole_tree() {
    let mut run = Run::accessible(Shell::new());
    run.take_a11y = false;
    let mut mirror = Mirror::new(run.ui.a11y_tree());
    let tab = |down| {
        Step::Input(Input::Key(KeyEvent {
            key: KeyCode::Tab,
            down,
            mods: Mods::default(),
        }))
    };
    let mut steps = run.session();
    steps.extend([
        Step::Update(Msg::Row(2)),
        Step::Update(Msg::Row(3)),
        tab(true),
        tab(false),
        Step::Update(Msg::Page(1)),
        tab(true),
        tab(false),
        Step::Update(Msg::Drawer),
        Step::Update(Msg::Page(0)),
        Step::Type('x'),
        Step::Update(Msg::Drawer),
        Step::ClearFilter,
        Step::Tick(500),
    ]);
    let mut updates = 0;
    for (n, &s) in steps.iter().enumerate() {
        run.step(s);
        let step = format!("step {n} ({s:?})");
        if let Some(update) = run.a11y_update() {
            updates += 1;
            mirror.apply(update, &step);
        }
        // Nothing is pending, so the whole tree doesn't change what the next update reports.
        mirror.assert_equals(&run.ui.a11y_tree(), &step);
    }
    assert!(updates > steps.len() / 2, "only {updates} updates");
    // An event that changes nothing a client sees sends nothing.
    run.step(Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 })));
    assert!(run.a11y_update().is_none());
}
