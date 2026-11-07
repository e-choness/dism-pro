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
//!   container, which the client leaves out of the tree it exposes. A [group](crate::Group) of
//!   tabs is a tab list, of radio buttons a radio group, and a list a list.
//! - A label is text; an empty one is hidden.
//! - A button is a button, or a list item (a selection item) when it's [selectable](
//!   crate::Element::selected).
//! - A check box is a toggle.
//! - A tab is a tab item and a radio button a radio button: selection items, picked or not.
//! - A progress bar is a progress bar, a read-only range value from 0 to 100.
//! - A field is a text input (UI Automation's Edit, with the Value pattern), named by its
//!   [name](crate::Element::name). Its text is its value; its placeholder is help text while it's
//!   empty; and it's read-only when it is.
//! - Popups (TD-0009 A1.4): a [menu](crate::Element::menu) is a menu of menu items; a dropdown's
//!   [options](crate::Element::options) are a list box of options (selection items); a separator
//!   is a separator; a tooltip window's text is a tooltip.
//! - A [dropdown](crate::Element::combo) is a combo box: named by its name, its value its text,
//!   expanded or collapsed (`ExpandCollapse`). Any node that [opens a popup](
//!   crate::Element::expanded) says whether it's open.
//! - A [data table](crate::Element::table) (TD-0009 A2.5) is a grid with its row and column
//!   counts, which gives its rows' scroll offsets and takes their scroll actions. Its header is a
//!   row group of column headers, each with its column index, a sortable one taking `Click` and
//!   giving its order. Its rows' viewport is a generic container, which the client leaves out,
//!   so the rows are the grid's: each with its index, named by its first column, the current one
//!   selected. Cells have their row and column; the first text column's are row headers, and a
//!   check column's are named as its header is, toggled, taking `Click`. `Focus` on a row makes
//!   it current.
//! - A node's [tooltip](crate::Element::tooltip) is its description (UI Automation's
//!   `FullDescription`), so a screen reader reads it on focus whether or not it shows.
//!
//! Enabled buttons, check boxes, tabs, radio buttons and menu items take `Click` (Invoke, Toggle or
//! Select) and `Focus`, except that a hold button's `Click` sends its Invoke message: a client
//! can't hold (TD-0009 §8). A dropdown takes `Expand` (its message) and `Focus`; `Collapse` is the
//! host's, which owns the open list. Enabled fields take `Focus`, and editable ones `SetValue`,
//! which replaces the text as a paste would. Disabled nodes are marked so; a disabled menu item
//! still takes `Focus`.
//!
//! Scroll viewports (TD-0009 A2.4) say where they're scrolled, in physical pixels from 0 to the
//! content less the view, and clip their children. Their content's bounds are where layout put
//! it, and the viewport's transform moves it by the offset, so a scroll sends the viewport
//! alone. As AccessKit applies a node's transform to its own bounds too, a viewport's bounds are
//! moved the other way first. A viewport takes `ScrollUp` and `ScrollDown` (along y) or
//! `ScrollLeft` and `ScrollRight` (along x), by a line, or a page with no unit as UI
//! Automation's large increment, and `SetScrollOffset`; one with no tab stop inside takes
//! `Focus`. Every node in a viewport takes `ScrollIntoView`, which scrolls it wholly into view
//! and moves no focus.

use accesskit::{
    Action, ActionData, ActionRequest, Affine, Node, NodeId as AkId, Rect as AkRect,
    Role as AkRole, ScrollUnit, SortDirection, Toggled, TreeId, TreeInfo, TreeUpdate,
};
use dpro_app::table::{Order, Tri};

use crate::{
    table::{TABLE_CHECK, sort_of},
    tree::{Group, NIL, NodeId, Role},
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

    /// Node `i`, in a viewport, may have become a tab stop or stopped being one (it was added,
    /// enabled or disabled, became a viewport or stopped, or goes with its subtree): the nearest
    /// viewport holding it may have become one with no stop inside, or stopped being one, and
    /// its `Focus` action goes to a client again. Nested viewports are stops themselves, so only
    /// the nearest is.
    pub(crate) fn stop_changed(&mut self, i: u32) {
        if !self.a11y.active || !self.nodes[i as usize].in_viewport {
            return;
        }
        let mut a = self.nodes[i as usize].parent;
        while a != NIL {
            if self.nodes[a as usize].scroll != NIL {
                self.a11y_touch(a);
                return;
            }
            a = self.nodes[a as usize].parent;
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

    /// Node `i`'s role, as the client sees it.
    fn ak_role(&self, i: u32) -> AkRole {
        if i == ROOT {
            return AkRole::Window;
        }
        let n = &self.nodes[i as usize];
        match n.role {
            // A table's header is a row group of its column headers.
            Role::Container if self.is_table_header(i) => AkRole::RowGroup,
            Role::Container => match n.group {
                // A table's rows' viewport: the client leaves it out, so its rows are the grid's.
                Some(Group::Rows) => AkRole::GenericContainer,
                Some(Group::Tabs) => AkRole::TabList,
                Some(Group::Radios) => AkRole::RadioGroup,
                Some(Group::List) => AkRole::List,
                Some(Group::Menu) => AkRole::Menu,
                Some(Group::Options) => AkRole::ListBox,
                None if n.text.is_empty() => AkRole::GenericContainer,
                None => AkRole::Group,
            },
            Role::Label => AkRole::Label,
            Role::Button if n.selected.is_some() => AkRole::ListItem,
            Role::Button => AkRole::Button,
            Role::CheckBox => AkRole::CheckBox,
            Role::Field => AkRole::TextInput,
            Role::Tab => AkRole::Tab,
            Role::Radio => AkRole::RadioButton,
            Role::Progress => AkRole::ProgressIndicator,
            Role::MenuItem if n.selected.is_some() => AkRole::ListBoxOption,
            Role::MenuItem if n.is_check_item() => AkRole::MenuItemCheckBox,
            Role::MenuItem => AkRole::MenuItem,
            Role::ComboBox => AkRole::ComboBox,
            Role::Separator => AkRole::Splitter,
            Role::Tooltip => AkRole::Tooltip,
            Role::Grid => AkRole::Grid,
            Role::Row => AkRole::Row,
            Role::Cell if self.is_row_header(i) => AkRole::RowHeader,
            Role::Cell => AkRole::Cell,
            Role::ColumnHeader => AkRole::ColumnHeader,
        }
    }

    /// Node `i` as the client sees it. Bounds are in physical pixels from the client area's
    /// top-left, snapped as painted.
    fn ak_node(&self, i: u32) -> Node {
        let n = &self.nodes[i as usize];
        let role = self.ak_role(i);
        let mut node = Node::new(role);
        node.set_bounds(self.ak_bounds(i));
        if n.scroll != NIL {
            self.ak_viewport(i, &mut node);
        }
        if matches!(
            n.role,
            Role::Grid | Role::Row | Role::Cell | Role::ColumnHeader
        ) {
            self.ak_table(i, &mut node);
        }
        if n.in_viewport {
            node.add_action(Action::ScrollIntoView);
        }
        if n.first != NIL {
            node.set_children(
                self.children_of(i)
                    .map(|c| self.ak_id(c))
                    .collect::<Vec<_>>(),
            );
        }
        match (role, n.text.is_empty()) {
            (AkRole::TextInput, _) => {
                let f = &self.fields[n.field as usize];
                node.set_value(n.text.as_str());
                if !f.name.is_empty() {
                    node.set_label(f.name.as_str());
                }
                if !f.placeholder.is_empty() {
                    // Clients show it only while the field is empty.
                    node.set_placeholder(f.placeholder.as_str());
                }
                if f.on_edit.is_none() {
                    node.set_read_only();
                } else if n.enabled {
                    node.add_action(Action::SetValue);
                }
            }
            // A label's text is its value, which clients read as its name.
            (AkRole::Label, false) => node.set_value(n.text.as_str()),
            (AkRole::Label, true) => node.set_hidden(),
            // A dropdown's text is its value; its name is in its side slot.
            (AkRole::ComboBox, _) => {
                node.set_value(n.text.as_str());
                if let Some(side) = self.sides.get(n.side as usize)
                    && !side.label.is_empty()
                {
                    node.set_label(side.label.as_str());
                }
            }
            (_, false) => node.set_label(n.text.as_str()),
            (_, true) => {}
        }
        if let Some(tip) = self.tip_text(i) {
            node.set_description(tip);
        }
        if let Some(expanded) = n.expanded {
            node.set_expanded(expanded);
            if expanded && n.enabled {
                node.add_action(Action::Collapse);
            }
        }
        if n.role.interactive() {
            if n.enabled {
                match n.role {
                    // A column header that doesn't sort only takes focus.
                    Role::Field => {}
                    Role::ColumnHeader if n.msg.is_none() => {}
                    Role::ComboBox => node.add_action(Action::Expand),
                    _ => node.add_action(Action::Click),
                }
                node.add_action(Action::Focus);
            } else {
                node.set_disabled();
                if n.role == Role::MenuItem {
                    node.add_action(Action::Focus);
                }
            }
        }
        if n.role == Role::CheckBox || n.is_check_item() {
            node.set_toggled(toggled(n.check));
        }
        match (n.role, n.selected) {
            // UI Automation reads a radio button's selection from its toggled state.
            (Role::Radio, Some(on)) => {
                node.set_toggled(if on { Toggled::True } else { Toggled::False });
            }
            (_, Some(selected)) => node.set_selected(selected),
            (_, None) => {}
        }
        if n.role == Role::Progress {
            // A percentage, to a tenth.
            node.set_numeric_value((f64::from(n.value) * 1_000.0).round() / 10.0);
            node.set_min_numeric_value(0.0);
            node.set_max_numeric_value(100.0);
        }
        node
    }

    /// Node `i`'s bounds: as painted, where layout put it. A viewport's transform moves its own
    /// bounds too, so they start where they'd be scrolled to 0.
    fn ak_bounds(&self, i: u32) -> AkRect {
        let n = &self.nodes[i as usize];
        let r = n.rect.snap(self.scale);
        let [ox, oy] = if n.scroll == NIL {
            [0.0; 2]
        } else {
            self.shift_px(n.scroll).map(px_f64)
        };
        AkRect {
            x0: f64::from(r.x0) + ox,
            y0: f64::from(r.y0) + oy,
            x1: f64::from(r.x1) + ox,
            y1: f64::from(r.y1) + oy,
        }
    }

    /// Viewport `i`'s scroll state and actions.
    fn ak_viewport(&self, i: u32, node: &mut Node) {
        let s = self.nodes[i as usize].scroll;
        let [dx, dy] = self.shift_px(s).map(px_f64);
        node.set_transform(Affine::translate((-dx, -dy)));
        node.set_clips_children();
        let sc = &self.scrolls[s as usize];
        // A table's header follows its rows, whose grid gives their scroll state.
        if sc.leader != NIL || sc.table != NIL {
            return;
        }
        self.ak_scroll_state(s, node);
        if self.lone_viewport(i) {
            node.add_action(Action::Focus);
        }
    }

    /// Slot `s`'s offsets, extent and scroll actions, on its viewport or its table's grid.
    pub(crate) fn ak_scroll_state(&self, s: u32, node: &mut Node) {
        let sc = &self.scrolls[s as usize];
        let [x, y] = sc.offset.map(px_f64);
        let [max_x, max_y] = self.max_px(s).map(px_f64);
        if sc.axes[0] {
            node.set_scroll_x(x);
            node.set_scroll_x_min(0.0);
            node.set_scroll_x_max(max_x);
            node.add_action(Action::ScrollLeft);
            node.add_action(Action::ScrollRight);
        }
        if sc.axes[1] {
            node.set_scroll_y(y);
            node.set_scroll_y_min(0.0);
            node.set_scroll_y_max(max_y);
            node.add_action(Action::ScrollUp);
            node.add_action(Action::ScrollDown);
        }
        node.add_action(Action::SetScrollOffset);
    }

    /// A table part's grid, row and column facts: the grid's counts and its rows' scroll state, a
    /// row's index, a cell's row and column (a check cell's name and state), a column header's
    /// column and sort.
    fn ak_table(&self, i: u32, node: &mut Node) {
        let n = &self.nodes[i as usize];
        match n.role {
            Role::Grid => {
                let Some(t) = self.table_of_grid(i) else {
                    return;
                };
                let st = &self.tables[t as usize];
                node.set_row_count(st.rows as usize);
                // AccessKit computes neither a set's size nor a member's place (A2.3).
                node.set_size_of_set(st.rows as usize);
                node.set_column_count(st.columns.len() + usize::from(st.checks));
                let s = self.grid_rows_slot(i);
                if s != NIL {
                    self.ak_scroll_state(s, node);
                }
            }
            Role::Row => {
                if let Some((_, at)) = self.row_place(i) {
                    node.set_row_index(at as usize);
                    node.set_position_in_set(at as usize);
                }
            }
            Role::Cell => {
                if let Some((t, at)) = self.row_place(n.parent) {
                    node.set_row_index(at as usize);
                    node.set_column_index(self.sibling_index(i));
                    if n.key == TABLE_CHECK {
                        let label = self.tables[t as usize].check_label.as_str();
                        if !label.is_empty() {
                            node.set_label(label);
                        }
                        if n.enabled {
                            node.set_toggled(toggled(n.check));
                            node.add_action(Action::Click);
                        }
                    }
                }
            }
            Role::ColumnHeader => {
                node.set_column_index(self.sibling_index(i));
                if n.key == TABLE_CHECK {
                    node.set_toggled(toggled(n.check));
                } else if let Some(order) = sort_of(n.value) {
                    node.set_sort_direction(match order {
                        Order::Ascending => SortDirection::Ascending,
                        Order::Descending => SortDirection::Descending,
                    });
                }
            }
            _ => {}
        }
    }

    /// A client's scroll request for node `i`; `None` if it isn't one.
    fn ak_scroll_action(&mut self, i: u32, request: &ActionRequest) -> Option<bool> {
        // A table's grid scrolls its rows; its header scrolls them too.
        let s = match self.nodes[i as usize].role {
            Role::Grid => self.grid_rows_slot(i),
            _ => self.nodes[i as usize].scroll,
        };
        let s = if s == NIL { s } else { self.leader_of(s) };
        let unit = match &request.data {
            Some(ActionData::ScrollUnit(ScrollUnit::Item)) => Some(false),
            // No unit is a page: UI Automation's large increment.
            Some(ActionData::ScrollUnit(ScrollUnit::Page)) | None => Some(true),
            _ => None,
        };
        let step = |axis: usize, dir: i64| unit.map(|page| (axis, dir, page));
        let by = match request.action {
            Action::ScrollIntoView => {
                return Some(self.nodes[i as usize].in_viewport && self.reveal(i));
            }
            Action::SetScrollOffset if s != NIL => {
                return Some(match &request.data {
                    Some(ActionData::SetScrollOffset(to)) => {
                        self.scroll_set(s, [px_i64(to.x), px_i64(to.y)])
                    }
                    _ => false,
                });
            }
            Action::ScrollUp if s != NIL => step(1, -1),
            Action::ScrollDown if s != NIL => step(1, 1),
            Action::ScrollLeft if s != NIL => step(0, -1),
            Action::ScrollRight if s != NIL => step(0, 1),
            _ => return None,
        };
        Some(by.is_some_and(|(axis, dir, page)| self.scroll_step(s, axis, dir, page)))
    }
}

/// A check state as a client reads it.
const fn toggled(check: Tri) -> Toggled {
    match check {
        Tri::Off => Toggled::False,
        Tri::On => Toggled::True,
        Tri::Mixed => Toggled::Mixed,
    }
}

/// A pixel offset for a client.
#[allow(
    clippy::cast_precision_loss,
    reason = "offsets are exact in f64 below 2^53 px"
)]
fn px_f64(v: i64) -> f64 {
    v as f64
}

/// A client's offset as whole pixels (NaN is 0; `as` saturates).
#[allow(
    clippy::cast_possible_truncation,
    reason = "rounded first; `as` saturates and maps NaN to 0"
)]
fn px_i64(v: f64) -> i64 {
    v.round() as i64
}

impl<M: Clone> Ui<M> {
    /// Carries out a client's request, which the platform posts to the UI thread: `Click`
    /// activates the node as a click does, pushing its message to `out`, and `Focus` moves focus
    /// to it and shows the focus ring, and `SetValue` replaces an editable field's text, pushing
    /// its edit. Returns whether it applied; requests for nodes gone since, nodes that take no
    /// input, or other actions do nothing.
    pub fn a11y_action(&mut self, request: &ActionRequest, out: &mut Vec<M>) -> bool {
        if request.target_tree != TreeId::ROOT {
            return false;
        }
        let Some(i) = self.ak_index(request.target_node) else {
            return false;
        };
        if let Some(applied) = self.ak_scroll_action(i, request) {
            return applied;
        }
        if let Some(applied) = self.table_action(i, request.action, out) {
            return applied;
        }
        let n = &self.nodes[i as usize];
        // A disabled menu item takes focus only, as does a viewport that is a tab stop.
        let focusable = n.focusable() || self.lone_viewport(i);
        if !(n.interactive() || focusable && request.action == Action::Focus) {
            return false;
        }
        let field = n.role == Role::Field;
        let closed = n.expanded == Some(false);
        match (request.action, &request.data) {
            (Action::Click, _) if !field => {
                self.invoke(i, out);
                true
            }
            // Opens what it opens, as a click does; closing is the host's (it owns the popup).
            (Action::Expand, _) if closed => {
                self.invoke(i, out);
                true
            }
            (Action::SetValue, Some(ActionData::Value(value))) if field => {
                self.field_set_value(i, value, out)
            }
            (Action::Focus, _) => {
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
    fn id<M>(ui: &Ui<M>, path: &[Key]) -> AkId {
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

    /// TD-0009 §11 step 4: tabs, radio buttons, a list, a progress bar and a hold button, as a
    /// client sees and uses them.
    #[test]
    fn controls_have_their_roles_states_and_actions() {
        let mut ui: Ui<u8> = Ui::new(Size { w: 300.0, h: 400.0 }, 1.0, Motion::default());
        ui.build(|cx| {
            cx.add_with(Element::tabs(Key::name("tabs")).name("Kinds"), |cx| {
                cx.add(Element::tab(Key::index(0), "A", true, 10));
                cx.add(Element::tab(Key::index(1), "B", false, 11));
            });
            cx.add_with(Element::radios(Key::name("radios")).name("Theme"), |cx| {
                cx.add(Element::radio(Key::index(0), "Light", false, 20));
                cx.add(Element::radio(Key::index(1), "Dark", true, 21));
            });
            cx.add_with(Element::list(Key::name("rail")).name("Pages"), |cx| {
                cx.add(Element::button(Key::index(0), "Home", 30).selected(true));
            });
            cx.add(Element::progress(Key::name("bar"), 0.41).name("Applying"));
            cx.add(Element::button(Key::name("hold"), "Hold to apply", 40).hold(|| 41));
        });
        ui.layout(&mut FakeText);
        let tree = ui.a11y_tree();
        let node = |path: &[Key]| {
            let want = id(&ui, path);
            &tree.nodes.iter().find(|(id, _)| *id == want).unwrap().1
        };
        let tabs = node(&[Key::name("tabs")]);
        assert_eq!(
            (tabs.role(), tabs.label()),
            (AkRole::TabList, Some("Kinds"))
        );
        let a = node(&[Key::name("tabs"), Key::index(0)]);
        assert_eq!((a.role(), a.is_selected()), (AkRole::Tab, Some(true)));
        assert!(a.supports_action(Action::Click) && a.supports_action(Action::Focus));
        assert_eq!(
            node(&[Key::name("tabs"), Key::index(1)]).is_selected(),
            Some(false)
        );

        let radios = node(&[Key::name("radios")]);
        assert_eq!(
            (radios.role(), radios.label()),
            (AkRole::RadioGroup, Some("Theme"))
        );
        let dark = node(&[Key::name("radios"), Key::index(1)]);
        assert_eq!(
            (dark.role(), dark.toggled(), dark.is_selected()),
            (AkRole::RadioButton, Some(Toggled::True), None)
        );
        assert_eq!(
            node(&[Key::name("radios"), Key::index(0)]).toggled(),
            Some(Toggled::False)
        );

        let rail = node(&[Key::name("rail")]);
        assert_eq!((rail.role(), rail.label()), (AkRole::List, Some("Pages")));
        assert_eq!(
            node(&[Key::name("rail"), Key::index(0)]).role(),
            AkRole::ListItem
        );

        let bar = node(&[Key::name("bar")]);
        assert_eq!(
            (bar.role(), bar.label(), bar.numeric_value()),
            (AkRole::ProgressIndicator, Some("Applying"), Some(41.0))
        );
        assert_eq!(
            (bar.min_numeric_value(), bar.max_numeric_value()),
            (Some(0.0), Some(100.0))
        );
        assert!(!bar.supports_action(Action::Click) && !bar.supports_action(Action::SetValue));

        // Invoke on a hold button sends its other message; on the others, theirs.
        let request = |target_node| ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node,
            data: None,
        };
        let mut msgs = Vec::new();
        let paths: [&[Key]; 3] = [
            &[Key::name("hold")],
            &[Key::name("tabs"), Key::index(1)],
            &[Key::name("radios"), Key::index(0)],
        ];
        for path in paths {
            assert!(ui.a11y_action(&request(id(&ui, path)), &mut msgs));
        }
        assert!(!ui.a11y_action(&request(id(&ui, &[Key::name("bar")])), &mut msgs));
        assert_eq!(msgs, [41, 11, 20]);
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
