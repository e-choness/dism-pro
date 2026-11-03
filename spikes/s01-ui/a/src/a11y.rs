//! The UI Automation tree (ADR-0002 gate: names and roles for rows and buttons), built with
//! AccessKit from the same `State` the frame is drawn from. Rebuilt in full on every change: it's
//! about 60 nodes, since the list exposes only the rows on screen.

use accesskit::{
    Action, ActionHandler, ActionRequest, ActivationHandler, Node, NodeId, Rect, Role, TreeId,
    TreeInfo, TreeUpdate,
};
use scene::{
    Screen, State,
    data::{self, Nav, Rows},
    layout::{self, Rect as L},
};

pub const ROOT: NodeId = NodeId(1);
const PILL: NodeId = NodeId(2);
const SEARCH: NodeId = NodeId(3);
const ACTIVITY: NodeId = NodeId(4);
const TITLE: NodeId = NodeId(5);
const NAV: NodeId = NodeId(10);
const LIST: NodeId = NodeId(20);
const DRAWER: NodeId = NodeId(30);
pub const CANCEL: NodeId = NodeId(31);
pub const APPLY: NodeId = NodeId(32);
const CLI: NodeId = NodeId(33);
const NAV_BASE: u64 = 100;
const CARD_BASE: u64 = 200;
const RECENT_BASE: u64 = 250;
const CHANGE_BASE: u64 = 300;
pub const ROW_BASE: u64 = 1_000_000;

/// What a node id stands for, for actions coming back from assistive technology.
pub enum Target {
    Nav(usize),
    Row(usize),
    Cancel,
    Other,
}

pub fn target(id: NodeId) -> Target {
    match id.0 {
        n if n >= ROW_BASE => Target::Row((n - ROW_BASE) as usize),
        n if (NAV_BASE..NAV_BASE + data::NAV.len() as u64).contains(&n) => {
            Target::Nav((n - NAV_BASE) as usize)
        }
        _ if id == CANCEL => Target::Cancel,
        _ => Target::Other,
    }
}

fn node(role: Role, label: &str, r: L, scale: f32) -> Node {
    let mut n = Node::new(role);
    n.set_label(label);
    let s = f64::from(scale);
    n.set_bounds(Rect {
        x0: f64::from(r.x) * s,
        y0: f64::from(r.y) * s,
        x1: f64::from(r.right()) * s,
        y1: f64::from(r.bottom()) * s,
    });
    n
}

fn clickable(mut n: Node) -> Node {
    n.add_action(Action::Click);
    n
}

/// The full tree for a state. Bounds are physical client pixels.
pub fn tree(s: &State, rows: &Rows, scale: f32) -> TreeUpdate {
    let mut nodes = Vec::new();
    let mut top = vec![PILL, SEARCH, ACTIVITY, NAV, TITLE];

    nodes.push((
        PILL,
        clickable(node(Role::Button, data::TARGET, layout::PILL, scale)),
    ));
    nodes.push((
        SEARCH,
        node(Role::TextInput, data::SEARCH, layout::SEARCH, scale),
    ));
    nodes.push((
        ACTIVITY,
        clickable(node(Role::Button, data::ACTIVITY, layout::ACTIVITY, scale)),
    ));

    let mut nav = node(Role::List, "Navigation", layout::NAV, scale);
    for (i, e, r) in layout::nav_entries() {
        if let Nav::Item(label) = e {
            let id = NodeId(NAV_BASE + i as u64);
            let mut n = clickable(node(Role::ListItem, label, r, scale));
            n.set_selected(i == s.nav);
            nav.push_child(id);
            nodes.push((id, n));
        }
    }
    nodes.push((NAV, nav));
    let title = if s.screen == Screen::Shell {
        data::OVERVIEW_TITLE
    } else {
        data::APPS_TITLE
    };
    nodes.push((TITLE, node(Role::Heading, title, layout::PAGE_TITLE, scale)));

    match s.screen {
        Screen::Shell => {
            for (i, (r, (label, value))) in layout::cards().into_iter().zip(data::CARDS).enumerate()
            {
                let id = NodeId(CARD_BASE + i as u64);
                nodes.push((
                    id,
                    node(Role::Group, &format!("{label}: {value}"), r, scale),
                ));
                top.push(id);
            }
            for (i, (label, rev)) in data::RECENT.into_iter().enumerate() {
                let id = NodeId(RECENT_BASE + i as u64);
                nodes.push((
                    id,
                    node(
                        Role::ListItem,
                        &format!("{label}, {}", rev.label()),
                        layout::recent_row(i),
                        scale,
                    ),
                ));
                top.push(id);
            }
        }
        Screen::List | Screen::Drawer => {
            let mut list = node(Role::List, data::APPS_TITLE, layout::VIEWPORT, scale);
            for i in layout::visible_rows(s.scroll, rows.len()) {
                let Some(r) = layout::row(i, s.scroll).intersect(layout::VIEWPORT) else {
                    continue;
                };
                let id = NodeId(ROW_BASE + i as u64);
                let mut n = clickable(node(Role::ListItem, rows.cell(i, 0), r, scale));
                n.set_selected(s.selected == Some(i));
                list.push_child(id);
                nodes.push((id, n));
            }
            nodes.push((LIST, list));
            top.push(LIST);
        }
    }

    if s.screen == Screen::Drawer {
        let mut d = node(Role::Dialog, data::DRAWER_TITLE, layout::DRAWER, scale);
        for (i, (label, _, rev)) in data::CHANGES.into_iter().enumerate() {
            let id = NodeId(CHANGE_BASE + i as u64);
            nodes.push((
                id,
                node(
                    Role::ListItem,
                    &format!("{label}, {}", rev.label()),
                    layout::change(i),
                    scale,
                ),
            ));
            d.push_child(id);
        }
        nodes.push((
            CLI,
            node(Role::Label, data::DRAWER_CLI, layout::DRAWER_CLI, scale),
        ));
        nodes.push((
            CANCEL,
            clickable(node(
                Role::Button,
                data::DRAWER_CANCEL,
                layout::DRAWER_CANCEL,
                scale,
            )),
        ));
        nodes.push((
            APPLY,
            clickable(node(
                Role::Button,
                data::DRAWER_APPLY,
                layout::DRAWER_APPLY,
                scale,
            )),
        ));
        for id in [CLI, CANCEL, APPLY] {
            d.push_child(id);
        }
        nodes.push((DRAWER, d));
        top.push(DRAWER);
    }

    let mut root = node(
        Role::Window,
        "S-01 option A",
        L::new(0.0, 0.0, layout::WIDTH, layout::HEIGHT),
        scale,
    );
    root.set_children(top);
    nodes.push((ROOT, root));
    TreeUpdate {
        nodes,
        tree: Some(TreeInfo::new(ROOT)),
        tree_id: TreeId::ROOT,
        focus: ROOT,
    }
}

/// Answers UI Automation's first request with the tree of the current state.
pub struct Initial<'a> {
    pub state: &'a State,
    pub rows: &'a Rows,
    pub scale: f32,
}

impl ActivationHandler for Initial<'_> {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        Some(tree(self.state, self.rows, self.scale))
    }
}

/// Actions arrive on any thread: post them to the window, which applies them like a click.
pub struct Actions {
    pub hwnd: isize,
    pub message: u32,
}

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        if request.action == Action::Click {
            // SAFETY: posting to a window handle is safe even if the window is gone.
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(
                    self.hwnd as _,
                    self.message,
                    request.target_node.0 as usize,
                    0,
                );
            }
        }
    }
}
