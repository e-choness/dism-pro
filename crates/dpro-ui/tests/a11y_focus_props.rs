//! Focus restoration over generated rebuilds (TD-0009 A4.4, A4.5, A4.8, A4.9; slice 7c): A4.8's
//! `ui_state` fuzz operations, remove and restore, as a property test until that target exists.
//!
//! A case draws a tree of buttons, lines and panels, then runs steps on it: rebuilds that drop
//! nodes, bring them back, disable buttons or reorder children; focus set on a button; memos
//! taken and restored; the return stack pushed and popped. After each step:
//! - focus is on a node that's there, and on a button: an enabled one, or a disabled one where
//!   it parked;
//! - a focused node a rebuild kept keeps the focus, parked if it was disabled;
//! - a focused node a rebuild took gives the focus to an enabled button whenever one is left,
//!   never to the window. When the rebuild only removed nodes (no reorder, nothing new), to the
//!   one A4.4's successor rule names, found here by walking the drawn tree;
//! - a memo, or a pop of the return stack, finds its node by its keys; if it went, focus falls
//!   back as A4.5 says, and a pop keeps a focus the successor rule already moved.

#![allow(missing_docs)]

use dpro_ui::{Cx, Element, FakeText, Key, Motion, NodeId, Size, Ui};
use proptest::prelude::*;

/// What a drawn node is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Button,
    Line,
    Panel,
}

/// A drawn node: its kind, parent, children (indexes into [`Tree::nodes`]) and key among its
/// siblings.
#[derive(Clone, Debug)]
struct Node {
    kind: Kind,
    parent: usize,
    children: Vec<usize>,
    key: u64,
}

/// The drawn tree. Node 0 is the window's root, always there.
#[derive(Clone, Debug)]
struct Tree {
    nodes: Vec<Node>,
}

/// The tree's shape as proptest draws it.
#[derive(Clone, Debug)]
enum Shape {
    Button,
    Line,
    Panel(Vec<Shape>),
}

impl Tree {
    fn new(top: &[Shape]) -> Self {
        let mut tree = Self {
            nodes: vec![Node {
                kind: Kind::Panel,
                parent: 0,
                children: Vec::new(),
                key: 0,
            }],
        };
        tree.add(0, top);
        tree
    }

    fn add(&mut self, parent: usize, shapes: &[Shape]) {
        for (key, shape) in (0u64..).zip(shapes) {
            let i = self.nodes.len();
            let kind = match shape {
                Shape::Button => Kind::Button,
                Shape::Line => Kind::Line,
                Shape::Panel(_) => Kind::Panel,
            };
            self.nodes.push(Node {
                kind,
                parent,
                children: Vec::new(),
                key,
            });
            self.nodes[parent].children.push(i);
            if let Shape::Panel(inner) = shape {
                self.add(i, inner);
            }
        }
    }

    /// Node `i`'s keys from the root, as `Ui::find` takes them.
    fn path(&self, mut i: usize) -> Vec<Key> {
        let mut keys = Vec::new();
        while i != 0 {
            keys.push(Key::index(self.nodes[i].key));
            i = self.nodes[i].parent;
        }
        keys.reverse();
        keys
    }
}

/// What a build shows of the tree: each node shown or not, each button enabled or not, and each
/// node's children in the order built.
#[derive(Clone, Debug)]
struct View {
    shown: Vec<bool>,
    enabled: Vec<bool>,
    order: Vec<Vec<usize>>,
}

/// How a rebuild changes the view.
#[derive(Clone, Copy, Debug)]
enum Mode {
    /// Some shown nodes go, and some buttons are enabled or disabled; nothing comes back and
    /// nothing moves, so the successor rule's choice is checked exactly.
    Remove,
    /// Everything drawn anew, in the tree's order.
    Free,
    /// Everything drawn anew, and every node's children shuffled.
    Reorder,
}

#[derive(Clone, Copy, Debug)]
enum Step {
    Rebuild {
        seed: u64,
        mode: Mode,
    },
    /// Focus on the stop at this place, counted around.
    Focus(u16),
    Push,
    Pop,
    /// Takes a memo of the focus.
    Take,
    /// Restores the memo last taken.
    Restore,
}

/// `SplitMix64`: a view's draws, from a step's seed.
struct Rng(u64);

impl Rng {
    fn draw(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.draw() % 100 < percent
    }

    /// A draw below `n`, which isn't 0.
    fn below(&mut self, n: usize) -> usize {
        let n64 = u64::try_from(n).unwrap_or(u64::MAX);
        usize::try_from(self.draw() % n64).unwrap_or(0)
    }
}

impl View {
    fn all(tree: &Tree) -> Self {
        Self {
            shown: vec![true; tree.nodes.len()],
            enabled: vec![true; tree.nodes.len()],
            order: tree.nodes.iter().map(|n| n.children.clone()).collect(),
        }
    }

    fn next(&self, tree: &Tree, seed: u64, mode: Mode) -> Self {
        let mut rng = Rng(seed);
        let mut view = self.clone();
        for i in 1..tree.nodes.len() {
            match mode {
                Mode::Remove => {
                    view.shown[i] = view.shown[i] && rng.chance(80);
                    if rng.chance(10) {
                        view.enabled[i] = !view.enabled[i];
                    }
                }
                Mode::Free | Mode::Reorder => {
                    view.shown[i] = rng.chance(75);
                    view.enabled[i] = rng.chance(85);
                }
            }
        }
        match mode {
            Mode::Remove => {}
            Mode::Free => view.order = Self::all(tree).order,
            Mode::Reorder => {
                for children in &mut view.order {
                    // Fisher-Yates.
                    for k in (1..children.len()).rev() {
                        let pick = rng.below(k + 1);
                        children.swap(k, pick);
                    }
                }
            }
        }
        view
    }

    /// Node `i` and every ancestor are shown.
    fn present(&self, tree: &Tree, mut i: usize) -> bool {
        while i != 0 {
            if !self.shown[i] {
                return false;
            }
            i = tree.nodes[i].parent;
        }
        true
    }

    /// Node `i` is a Tab stop: an enabled button that's there.
    fn stop(&self, tree: &Tree, i: usize) -> bool {
        tree.nodes[i].kind == Kind::Button && self.enabled[i] && self.present(tree, i)
    }

    /// The stops in `i`'s subtree, `i` included, in Tab's order (buttons are leaves).
    fn stops_in(&self, tree: &Tree, i: usize, out: &mut Vec<usize>) {
        if !self.present(tree, i) {
            return;
        }
        if self.stop(tree, i) {
            out.push(i);
        }
        for &c in &self.order[i] {
            self.stops_in(tree, c, out);
        }
    }

    fn first_stop_in(&self, tree: &Tree, i: usize) -> Option<usize> {
        let mut out = Vec::new();
        self.stops_in(tree, i, &mut out);
        out.first().copied()
    }

    fn last_stop_in(&self, tree: &Tree, i: usize) -> Option<usize> {
        let mut out = Vec::new();
        self.stops_in(tree, i, &mut out);
        out.last().copied()
    }

    /// A4.4's successor of focused node `f`, which this view no longer has: the first stop after
    /// its place among its siblings' subtrees, else the last before it, else the same one level
    /// up. Its place is that of its highest ancestor that went (itself, if its parent is there).
    fn successor(&self, tree: &Tree, f: usize) -> Option<usize> {
        let mut t = f;
        while !self.present(tree, tree.nodes[t].parent) {
            t = tree.nodes[t].parent;
        }
        loop {
            let p = tree.nodes[t].parent;
            let siblings = &self.order[p];
            let k = siblings.iter().position(|&s| s == t)?;
            let after = siblings[k + 1..]
                .iter()
                .find_map(|&s| self.first_stop_in(tree, s));
            if after.is_some() {
                return after;
            }
            let before = siblings[..k]
                .iter()
                .rev()
                .find_map(|&s| self.last_stop_in(tree, s));
            if before.is_some() || p == 0 {
                return before;
            }
            t = p;
        }
    }

    /// Where A4.5's fallback puts focus for a memo of node `t`, gone: the nearest ancestor still
    /// there (the root excepted) with a stop, its first; else the window's first.
    fn fallback(&self, tree: &Tree, t: usize) -> Option<usize> {
        let mut a = tree.nodes[t].parent;
        while a != 0 && !self.present(tree, a) {
            a = tree.nodes[a].parent;
        }
        while a != 0 {
            if let Some(s) = self.first_stop_in(tree, a) {
                return Some(s);
            }
            a = tree.nodes[a].parent;
        }
        self.first_stop_in(tree, 0)
    }

    /// Where a memo of `t` puts focus: on it if it's there (a button, enabled or not), else
    /// [`fallback`](Self::fallback). `None` (no node) is the window's first stop.
    fn memo_target(&self, tree: &Tree, t: Option<usize>) -> Option<usize> {
        match t {
            Some(t) if self.present(tree, t) => Some(t),
            Some(t) => self.fallback(tree, t),
            None => self.first_stop_in(tree, 0),
        }
    }
}

fn build_children(cx: &mut Cx<'_, u8>, tree: &Tree, view: &View, i: usize) {
    for &c in &view.order[i] {
        if !view.shown[c] {
            continue;
        }
        let key = Key::index(tree.nodes[c].key);
        match tree.nodes[c].kind {
            Kind::Button => {
                cx.add(Element::button(key, "Button", 0).enabled(view.enabled[c]));
            }
            Kind::Line => {
                cx.add(Element::text(key, "A line"));
            }
            Kind::Panel => {
                cx.add_with(Element::stack(key), |cx| {
                    build_children(cx, tree, view, c);
                });
            }
        }
    }
}

fn build(ui: &mut Ui<u8>, tree: &Tree, view: &View) {
    ui.build(|cx| build_children(cx, tree, view, 0));
    ui.layout(&mut FakeText);
}

/// Node `i`'s id in `ui`, if it's there.
fn id_of(ui: &Ui<u8>, tree: &Tree, i: usize) -> Option<NodeId> {
    ui.find(&tree.path(i))
}

/// The drawn node `id` is, among those `view` shows.
fn node_of(ui: &Ui<u8>, tree: &Tree, view: &View, id: NodeId) -> Option<usize> {
    (1..tree.nodes.len()).find(|&i| view.present(tree, i) && id_of(ui, tree, i) == Some(id))
}

/// The drawn node focused; `None` with nothing focused or focus on a node not drawn.
fn focused(ui: &Ui<u8>, tree: &Tree, view: &View) -> Option<usize> {
    ui.focus().and_then(|id| node_of(ui, tree, view, id))
}

fn shape() -> impl Strategy<Value = Vec<Shape>> {
    let leaf = prop_oneof![3 => Just(Shape::Button), 1 => Just(Shape::Line)];
    let node = leaf.prop_recursive(3, 32, 4, |inner| {
        prop::collection::vec(inner, 0..5).prop_map(Shape::Panel)
    });
    prop::collection::vec(node, 1..7)
}

fn step() -> impl Strategy<Value = Step> {
    let mode = prop_oneof![
        4 => Just(Mode::Remove),
        2 => Just(Mode::Free),
        1 => Just(Mode::Reorder),
    ];
    prop_oneof![
        5 => (any::<u64>(), mode).prop_map(|(seed, mode)| Step::Rebuild { seed, mode }),
        4 => any::<u16>().prop_map(Step::Focus),
        1 => Just(Step::Push),
        1 => Just(Step::Pop),
        1 => Just(Step::Take),
        1 => Just(Step::Restore),
    ]
}

/// The state a case runs: the `Ui`, the view it shows, the memo last taken (with the node it
/// names), and the return stack as A4.5 says it behaves.
struct Case {
    ui: Ui<u8>,
    tree: Tree,
    view: View,
    memo: Option<(dpro_ui::FocusMemo, usize)>,
    returns: Vec<Option<usize>>,
}

impl Case {
    fn new(tree: Tree) -> Self {
        let view = View::all(&tree);
        let mut ui = Ui::new(Size { w: 600.0, h: 400.0 }, 1.0, Motion::default());
        ui.set_active(true);
        build(&mut ui, &tree, &view);
        Self {
            ui,
            tree,
            view,
            memo: None,
            returns: Vec::new(),
        }
    }

    fn focused(&self) -> Option<usize> {
        focused(&self.ui, &self.tree, &self.view)
    }

    fn id(&self, i: Option<usize>) -> Option<NodeId> {
        i.and_then(|i| id_of(&self.ui, &self.tree, i))
    }

    fn run(&mut self, step: Step) -> Result<(), TestCaseError> {
        match step {
            Step::Rebuild { seed, mode } => self.rebuild(seed, mode)?,
            Step::Focus(at) => {
                let mut stops = Vec::new();
                self.view.stops_in(&self.tree, 0, &mut stops);
                if !stops.is_empty() {
                    let pick = stops[usize::from(at) % stops.len()];
                    self.ui.set_focus(self.id(Some(pick)));
                    prop_assert_eq!(self.focused(), Some(pick));
                }
            }
            Step::Push => {
                let f = self.focused();
                self.ui.push_focus_return();
                if self.returns.len() == 4 {
                    self.returns.remove(0);
                }
                self.returns.push(f);
            }
            Step::Pop => {
                let before = self.ui.focus();
                let got = self.ui.pop_focus_return();
                match self.returns.pop() {
                    None => {
                        prop_assert_eq!(got, None, "the stack is empty");
                        prop_assert_eq!(self.ui.focus(), before);
                    }
                    Some(entry) => {
                        let back = entry.is_some_and(|t| self.view.present(&self.tree, t));
                        if !back && before.is_some() {
                            // Its node went: a focus already on a node stays (A4.4).
                            prop_assert_eq!(self.ui.focus(), before, "popped {:?}", entry);
                        } else {
                            let want = self.view.memo_target(&self.tree, entry);
                            prop_assert_eq!(self.focused(), want, "popped {:?}", entry);
                        }
                        prop_assert_eq!(got, self.ui.focus());
                    }
                }
            }
            Step::Take => {
                let memo = self.ui.focus_memo();
                let f = self.focused();
                prop_assert_eq!(memo.is_some(), f.is_some());
                if let (Some(memo), Some(f)) = (memo, f) {
                    let path = self.tree.path(f);
                    prop_assert_eq!(memo.keys(), path.as_slice());
                    self.memo = Some((memo, f));
                }
            }
            Step::Restore => {
                if let Some((memo, t)) = self.memo {
                    let before = self.ui.focus();
                    let got = self.ui.restore_focus(&memo);
                    let want = self.view.memo_target(&self.tree, Some(t));
                    if want.is_some() {
                        prop_assert_eq!(self.focused(), want);
                        prop_assert_eq!(got, self.ui.focus());
                    } else {
                        // Nothing to focus: it stays as it was.
                        prop_assert_eq!(got, None);
                        prop_assert_eq!(self.ui.focus(), before);
                    }
                }
            }
        }
        // Focus is on a button that's there, enabled or parked.
        if let Some(id) = self.ui.focus() {
            prop_assert!(self.ui.info(id).is_some(), "focus on a node that went");
            let f = self.focused();
            prop_assert!(
                f.is_some_and(|f| self.tree.nodes[f].kind == Kind::Button),
                "focus on {:?}, not a button",
                f
            );
        }
        Ok(())
    }

    fn rebuild(&mut self, seed: u64, mode: Mode) -> Result<(), TestCaseError> {
        let before = self.ui.focus();
        let was = self.focused();
        let view = self.view.next(&self.tree, seed, mode);
        build(&mut self.ui, &self.tree, &view);
        self.view = view;
        let Some(f) = was else {
            prop_assert_eq!(self.ui.focus(), None, "nothing focused stays so");
            return Ok(());
        };
        if self.view.present(&self.tree, f) {
            prop_assert_eq!(self.ui.focus(), before, "a kept node keeps the focus");
            return Ok(());
        }
        let now = self.focused();
        if self.view.first_stop_in(&self.tree, 0).is_none() {
            prop_assert_eq!(self.ui.focus(), None, "nothing left to focus");
            return Ok(());
        }
        prop_assert!(
            now.is_some_and(|n| self.view.stop(&self.tree, n)),
            "the focus fell to the window or off a stop: {:?}",
            now
        );
        if matches!(mode, Mode::Remove) {
            prop_assert_eq!(now, self.view.successor(&self.tree, f), "from {}", f);
        }
        Ok(())
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn focus_survives_generated_rebuilds_memos_and_returns(
        top in shape(),
        steps in prop::collection::vec(step(), 1..40),
    ) {
        let mut case = Case::new(Tree::new(&top));
        for s in steps {
            case.run(s)?;
        }
    }
}
