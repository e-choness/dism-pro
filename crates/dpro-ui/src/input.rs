//! Input, focus and the animation clock (TD-0009 §5.2).
//!
//! - **Pointer:** the hit test finds the topmost enabled button or check box under the pointer;
//!   children paint over parents and later siblings over earlier ones, and a node outside its
//!   parent is clipped away. A press captures: the node that took it gets the release, and a
//!   release over that node activates it. While captured, only that node shows hover.
//! - **Keyboard:** Tab and Shift+Tab move focus in tree order, which is visual order because views
//!   build in reading order, wrapping at the ends. Enter activates on the press; Space presses and
//!   activates on the release, as Windows buttons do. A focused field takes its editing keys and
//!   typed characters first (`field.rs`). Other keys are left to the caller.
//! - **Groups** (tabs, radio buttons, lists; TD-0009 §4.3) are one Tab stop each: Tab lands on the
//!   picked item, or the first (the last, going back). Inside, the arrow keys move focus (Home
//!   and End to the ends). In tabs and radio groups they wrap and pick the item they reach, as
//!   Windows does; in a list they move focus only, and Enter or Space opens.
//! - **Menus and dropdown lists** (TD-0009 A1.4): Up and Down move focus through a menu's items,
//!   wrapping, and through a list's options, not wrapping; Enter or Space invokes or picks.
//!   Hovering an item focuses it, and a release on an item invokes it, even if the press was on
//!   another. A disabled item can be focused, never invoked. In a popup with nothing focused yet
//!   (a menu opened by the pointer), the first arrow focuses its first item.
//! - **Dropdowns:** a click, Enter, Space, Alt+Down or F4 sends its message (the model opens the
//!   list); Up and Down, while it's closed, send its step.
//! - **Context requests** (a right-click, Shift+F10, the Menu key; `WM_CONTEXTMENU`): the nearest
//!   node at or above the target with [`context`](crate::Element::context) set is focused and
//!   picked if it's selectable, and the request is left for the host
//!   ([`Ui::take_context`]), which asks the model for its menu.
//! - **Holds** (TD-0009 §5.2, doc 05 §4): a hold button's message is sent once its press has
//!   lasted [`HOLD_MS`] on the frame clock, counted from the first tick after the press. The
//!   press is the pointer held on it, or Enter or Space held while it has focus. A pointer that
//!   slides off pauses it, back to the start; letting go, focus moving, the button going away
//!   or being disabled, or the window going inactive ends it. It never activates any other way
//!   but through UI Automation, whose Invoke sends its other message (§8).
//! - **Tables** (`table.rs`): a table row takes its own keys and clicks, which become the table's
//!   events: a press's modifiers and a double-click say which.
//! - **Fields** take a press as a caret position and a drag as a selection; they never activate,
//!   and show no hover highlight.
//! - **Animations:** hover fades over [`Motion::hover_ms`](crate::Motion) with an ease-out curve
//!   (doc 05 §4). A duration of 0 jumps, so reduced motion never asks for a frame. A hold is not
//!   an animation: it takes its time under reduced motion too.

use crate::{
    geom::Point,
    tokens::motion::HOLD_MS,
    tree::{Group, NIL, NOT_STARTED, NodeId, Role},
    ui::{Hold, ROOT, Ui},
};

/// An input event, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    PointerMove(Point),
    /// A press with no modifiers, not a double-click: [`Input::Press`] as most tests send it.
    PointerDown(Point),
    /// The primary button went down (`WM_LBUTTONDOWN`), with the Shift and Ctrl held (a table's
    /// Ctrl+click and Shift+click), and whether it's a double-click's second press.
    Press {
        at: Point,
        mods: Mods,
        double: bool,
    },
    PointerUp(Point),
    /// The pointer left the window (`WM_MOUSELEAVE`), or the window lost it mid-press
    /// (`WM_CAPTURECHANGED`: Windows sends no leave while a press holds the capture). A pointer
    /// press still held is cancelled, so no release or hold activates it, and a thumb drag ends.
    PointerLeave,
    /// The wheel turned at a point (`WM_MOUSEWHEEL`, `WM_MOUSEHWHEEL`): `delta` in 120ths of a
    /// notch, positive away from the user (up the content), or with `horizontal` to the right.
    Wheel {
        at: Point,
        delta: i32,
        horizontal: bool,
    },
    /// A context menu was asked for (`WM_CONTEXTMENU`): by the pointer at a point (a right-click),
    /// or by the keyboard (`None`: Shift+F10, the Menu key) for the focused node.
    ContextMenu(Option<Point>),
    Key(KeyEvent),
    /// A typed character (`WM_CHAR`, surrogate pairs joined), after its key's
    /// [`Input::Key`]. Control characters come too (Ctrl+A is U+0001); fields ignore them.
    Char(char),
}

/// A key going down or up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: KeyCode,
    pub down: bool,
    pub mods: Mods,
}

/// The keys the toolkit acts on; everything else is [`KeyCode::Other`] with its virtual-key code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCode {
    Tab,
    Enter,
    Space,
    Escape,
    Backspace,
    Delete,
    Insert,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    /// Opens or closes a dropdown's list.
    F4,
    Other(u16),
}

/// Modifier keys held during a key event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per modifier key, as Windows reports them"
)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub win: bool,
}

impl<M: Clone> Ui<M> {
    /// Handles `input`, pushing the messages of activated nodes to `out`. Returns whether the UI
    /// used it; a key it didn't use goes on to the caller's accelerators and then the system.
    pub fn dispatch(&mut self, input: Input, out: &mut Vec<M>) -> bool {
        self.pointer = match input {
            Input::PointerMove(p)
            | Input::PointerDown(p)
            | Input::PointerUp(p)
            | Input::Press { at: p, .. } => Some(p),
            Input::PointerLeave => None,
            Input::Key(_) | Input::Char(_) | Input::ContextMenu(_) | Input::Wheel { .. } => {
                self.pointer
            }
        };
        match input {
            Input::PointerMove(p) => {
                // A thumb being dragged takes the pointer (A2.4).
                if self.scroll_pointer(Some(p)) {
                    return true;
                }
                let hit = self.hit(p);
                let selecting = self.is_field(self.pressed);
                if selecting {
                    // The selection's end follows the pointer, even outside the field.
                    self.field_pointer(self.pressed, p, true);
                }
                let moved = self.pointer_over(hit);
                // Hovering a menu item focuses it, as in Windows menus.
                let item = self.pressed == NIL
                    && hit != self.focus
                    && self
                        .nodes
                        .get(hit as usize)
                        .is_some_and(|n| n.role == Role::MenuItem);
                if item {
                    self.hide_focus_ring();
                    self.focus_node(hit);
                }
                if !self.sides_empty() {
                    self.tip_pointer(Some(p));
                }
                moved || selecting || item
            }
            Input::PointerLeave => {
                // Mid-press, the window lost the pointer: nothing will release the press.
                self.cancel_pointer_press();
                self.scroll_pointer(None);
                self.tip_pointer(None);
                self.pointer_over(NIL)
            }
            Input::ContextMenu(at) => self.context_request(at, out),
            Input::PointerDown(p) => self.pointer_down(p, Mods::default(), false),
            Input::Press { at, mods, double } => self.pointer_down(at, mods, double),
            Input::PointerUp(p) => self.pointer_up(p, out),
            Input::Key(k) => {
                let before = self.focus;
                if k.down {
                    // Any key hides a tooltip.
                    self.tip_cancel(true);
                }
                let used = if let Some(i) = self.focused_field()
                    && let Some(used) = self.field_key(i, k, out)
                {
                    used
                } else {
                    self.key(k, out)
                };
                // Focus the keyboard moved shows its tip at once.
                if self.focus != before && self.focus_visible {
                    self.tip_focus(self.focus);
                }
                used
            }
            Input::Char(c) => self.field_char(c, out),
            Input::Wheel {
                at,
                delta,
                horizontal,
            } => {
                self.tip_cancel(true);
                let scrolled = self.wheel(at, delta, horizontal);
                // The content moved under the pointer: the hover follows it, but for a press's
                // or a drag's.
                if scrolled && self.pressed == NIL && self.drag.is_none() {
                    let hit = self.hit(at);
                    self.pointer_over(hit);
                }
                scrolled
            }
        }
    }

    fn pointer_down(&mut self, p: Point, mods: Mods, double: bool) -> bool {
        self.press = crate::table::Press {
            at: p,
            mods,
            double,
        };
        // A press on a scroll bar's strip drags its thumb or pages (A2.4).
        if self.scroll_press(p) {
            self.tip_cancel(true);
            return true;
        }
        let hit = self.hit(p);
        self.pointer_over(hit);
        self.tip_cancel(true);
        self.hide_focus_ring();
        if hit == NIL {
            return false;
        }
        self.pressed = hit;
        // Focus from the pointer scrolls nothing: the node stays under the pointer.
        self.focus_node(hit);
        if self.is_field(hit) {
            self.field_pointer(hit, p, false);
        } else {
            self.set_pressed(hit, true);
            if self.nodes[hit as usize].hold {
                self.start_hold(hit, None);
            }
        }
        true
    }

    fn pointer_up(&mut self, p: Point, out: &mut Vec<M>) -> bool {
        if self.scroll_release() {
            // What's under the pointer now: the drag held the bar and the hover as they were.
            self.scroll_pointer(Some(p));
            let hit = self.hit(p);
            self.pointer_over(hit);
            return true;
        }
        let target = self.pressed;
        if target == NIL {
            return false;
        }
        self.pressed = NIL;
        self.set_pressed(target, false);
        let held = self.hold.node == target && self.hold.key.is_none();
        if held {
            self.end_hold();
        }
        let hit = self.hit(p);
        self.pointer_over(hit);
        // A release on a menu item invokes it, even if the press began on another.
        let item = |i: u32| {
            self.nodes
                .get(i as usize)
                .is_some_and(|n| n.role == Role::MenuItem)
        };
        let to = if item(target) && item(hit) {
            hit
        } else {
            target
        };
        if hit == to && !held && !self.is_field(to) {
            if self.nodes[to as usize].role == Role::Row {
                // A table's row: its click is the table's.
                self.row_click(to, out);
            } else {
                self.activate(to, out);
            }
        }
        true
    }

    fn key(&mut self, k: KeyEvent, out: &mut Vec<M>) -> bool {
        // A focused table row takes its keys first (A2.5).
        if let Some(used) = self.table_key(k, out) {
            return used;
        }
        // What nothing else used may scroll the viewport holding the focus (A2.4).
        self.control_key(k, out) || self.scroll_key(k)
    }

    /// Enter and Space press the focused node: anything focused but a viewport (one with no
    /// tab stop inside, A2.4), which has nothing to press.
    fn focus_presses(&self) -> bool {
        self.focus != NIL && self.nodes[self.focus as usize].scroll == NIL
    }

    fn control_key(&mut self, k: KeyEvent, out: &mut Vec<M>) -> bool {
        let holds = self.focus != NIL && self.nodes[self.focus as usize].hold;
        match (k.key, k.down) {
            (KeyCode::Tab, true) => {
                let to = if k.mods.shift {
                    self.focusable_before(self.focus)
                } else {
                    self.focusable_after(self.focus)
                };
                self.show_focus_ring();
                if to != NIL {
                    self.move_focus(to);
                    if self.is_field(to) {
                        self.select_all(to);
                    }
                }
                true
            }
            // A hold button: the key held is the press. Its repeats change nothing.
            (KeyCode::Enter | KeyCode::Space, true) if holds => {
                self.show_focus_ring();
                if self.hold.node != self.focus {
                    self.set_pressed(self.focus, true);
                    self.start_hold(self.focus, Some(k.key));
                }
                true
            }
            // Its release ends the hold, and the press with it.
            (KeyCode::Enter | KeyCode::Space, false) if self.hold.key == Some(k.key) => {
                self.end_hold();
                true
            }
            // A focused viewport has nothing to press: the keys go on, unused.
            (KeyCode::Enter, true) if self.focus_presses() => {
                self.show_focus_ring();
                self.activate(self.focus, out);
                true
            }
            (KeyCode::Space, true) if self.focus_presses() => {
                self.show_focus_ring();
                self.key_pressed = self.focus;
                self.set_pressed(self.focus, true);
                true
            }
            (KeyCode::Space, false) if self.key_pressed != NIL => {
                let target = self.key_pressed;
                self.key_pressed = NIL;
                self.set_pressed(target, false);
                if target == self.focus {
                    self.activate(target, out);
                }
                true
            }
            // A dropdown opens on Alt+Down and F4, as on a click.
            (KeyCode::Down, true) if k.mods.alt && self.focused(Role::ComboBox) => {
                self.show_focus_ring();
                self.activate(self.focus, out);
                true
            }
            // Plain F4 only: Alt+F4 goes on to the system and closes the window.
            (KeyCode::F4, true)
                if !(k.mods.alt || k.mods.ctrl || k.mods.win) && self.focused(Role::ComboBox) =>
            {
                self.show_focus_ring();
                self.activate(self.focus, out);
                true
            }
            // Up and Down change a closed dropdown's value, without opening it.
            (KeyCode::Up | KeyCode::Down, true)
                if !(k.mods.ctrl || k.mods.alt || k.mods.win) && self.focused(Role::ComboBox) =>
            {
                self.show_focus_ring();
                let n = &self.nodes[self.focus as usize];
                if n.interactive()
                    && n.expanded != Some(true)
                    && let Some(step) = self.sides.get(n.side as usize).and_then(|s| s.step)
                {
                    out.push(step(k.key == KeyCode::Down));
                }
                true
            }
            // A popup with nothing focused yet (opened by the pointer): an arrow focuses its
            // first item.
            (KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End, true)
                if self.focus == NIL && self.in_popup() =>
            {
                self.focus_first(true);
                true
            }
            (
                KeyCode::Left
                | KeyCode::Right
                | KeyCode::Up
                | KeyCode::Down
                | KeyCode::Home
                | KeyCode::End,
                true,
            ) if self.focus != NIL && !(k.mods.ctrl || k.mods.alt || k.mods.win) => {
                self.rove(k.key, out)
            }
            _ => false,
        }
    }

    /// Whether the focused node has `role`.
    fn focused(&self, role: Role) -> bool {
        self.nodes
            .get(self.focus as usize)
            .is_some_and(|n| n.role == role)
    }

    /// Sends node `i`'s message, if it takes input and isn't a hold button, whose message only
    /// a whole hold sends. A node that opens a popup becomes the popup's anchor.
    pub(crate) fn activate(&mut self, i: u32, out: &mut Vec<M>) {
        let n = &self.nodes[i as usize];
        if n.interactive()
            && !n.hold
            && let Some(msg) = &n.msg
        {
            out.push(msg.clone());
            if n.expanded.is_some() {
                self.anchor = Some(self.window_rect(i));
                self.anchor_node = i;
            }
        }
    }

    /// UI Automation's Invoke (or Toggle, or Select) on node `i`: what a click does, except on a
    /// hold button, which sends its Invoke message instead (the typed confirmation, TD-0009 §8).
    pub(crate) fn invoke(&mut self, i: u32, out: &mut Vec<M>) {
        if !self.nodes[i as usize].hold {
            self.activate(i, out);
        } else if self.nodes[i as usize].interactive()
            && let Some(slot) = self.holds.iter().find(|s| s.node == i)
        {
            out.push((slot.invoke)());
        }
    }

    /// An arrow, Home or End on the focused node: inside a group, focus moves to the item it
    /// names, and in tabs and radio groups that item is picked. Returns whether it was used: an
    /// arrow along a list's axis is, even at its end, so it doesn't reach the caller.
    fn rove(&mut self, key: KeyCode, out: &mut Vec<M>) -> bool {
        let parent = self.nodes[self.focus as usize].parent;
        let Some((group, axis)) = self
            .nodes
            .get(parent as usize)
            .and_then(|p| p.group.map(|g| (g, p.style.axis)))
        else {
            return false;
        };
        // A table's rows move its current row instead (`table.rs`).
        if group == Group::Rows {
            return false;
        }
        let row = axis == crate::Axis::Row;
        let step = match key {
            KeyCode::Home => Rove::First,
            KeyCode::End => Rove::Last,
            KeyCode::Left | KeyCode::Up if group == Group::Radios => Rove::Prev,
            KeyCode::Right | KeyCode::Down if group == Group::Radios => Rove::Next,
            KeyCode::Left if row => Rove::Prev,
            KeyCode::Right if row => Rove::Next,
            KeyCode::Up if !row => Rove::Prev,
            KeyCode::Down if !row => Rove::Next,
            _ => return false,
        };
        // Tabs, radio buttons and menus wrap; lists and a dropdown's options stop at their ends.
        let wrap = matches!(group, Group::Tabs | Group::Radios | Group::Menu);
        let to = self.rove_target(parent, self.focus, step, wrap);
        self.show_focus_ring();
        if to != NIL && to != self.focus {
            self.move_focus(to);
            // Only tabs and radio buttons pick the item they reach.
            if matches!(group, Group::Tabs | Group::Radios) {
                self.activate(to, out);
            }
        }
        true
    }
}

/// Where an arrow key, Home or End moves in a group.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rove {
    First,
    Last,
    Prev,
    Next,
}

impl<M: Clone> Ui<M> {
    /// Advances animations and the hold to `now_ms` on the monotonic clock, damaging what they
    /// change. An animation or a hold started since the last tick starts now. A hold that comes
    /// to its end pushes its button's message to `out`.
    pub fn tick(&mut self, now_ms: u64, out: &mut Vec<M>) {
        self.tick_hold(now_ms, out);
        self.tick_animations(now_ms);
    }

    fn tick_hold(&mut self, now_ms: u64, out: &mut Vec<M>) {
        if !self.hold_running() {
            return;
        }
        let i = self.hold.node;
        let start = *self.hold.start.get_or_insert(now_ms);
        let total = u64::from(HOLD_MS);
        let elapsed = now_ms.saturating_sub(start).min(total);
        self.hold.t = f32::from(u16::try_from(elapsed).unwrap_or(HOLD_MS)) / f32::from(HOLD_MS);
        self.damage_node(i);
        if elapsed >= total {
            self.hold.done = true;
            let n = &self.nodes[i as usize];
            if n.interactive()
                && let Some(msg) = &n.msg
            {
                out.push(msg.clone());
            }
        }
    }
}

impl<M> Ui<M> {
    /// Moves focus to `id`, as UI Automation's `SetFocus` does; `None` clears it. A node that
    /// can't take focus is ignored.
    pub fn set_focus(&mut self, id: Option<NodeId>) {
        match id.and_then(|id| self.index(id)) {
            Some(i) if self.nodes[i as usize].interactive() || self.lone_viewport(i) => {
                self.move_focus(i);
            }
            Some(_) => {}
            None => self.move_focus(NIL),
        }
    }

    /// Focus last moved by keyboard, so the focus ring shows (doc 05 §6).
    #[must_use]
    pub fn focus_visible(&self) -> bool {
        self.focus_visible && self.focus != NIL
    }

    fn tick_animations(&mut self, now_ms: u64) {
        let duration = self.motion.hover_ms;
        let mut k = 0;
        while k < self.anims.len() {
            let Some(i) = self.index(self.anims[k]) else {
                self.anims.swap_remove(k);
                continue;
            };
            let n = &mut self.nodes[i as usize];
            if n.anim_start == NOT_STARTED {
                n.anim_start = now_ms;
            }
            let start = n.anim_start;
            let elapsed = now_ms.saturating_sub(start).min(u64::from(duration));
            let progress = if duration == 0 {
                1.0
            } else {
                f32::from(u16::try_from(elapsed).unwrap_or(duration)) / f32::from(duration)
            };
            let target = if n.hovered { 1.0 } else { 0.0 };
            n.hover_t = n.anim_from + (target - n.anim_from) * ease_out(progress);
            let done = progress >= 1.0;
            if done {
                n.animating = false;
                n.anim_start = NOT_STARTED;
                self.anims.swap_remove(k);
            } else {
                k += 1;
            }
            self.damage_node(i);
        }
    }

    /// Whether an animation or a hold is running, so the window should keep asking for frames.
    #[must_use]
    pub fn wants_frame(&self) -> bool {
        !self.anims.is_empty() || self.hold_running()
    }

    /// How far the running hold has come, 0 to 1; `None` when no hold is running (none is
    /// pressed, or the one pressed has already come to its end).
    #[must_use]
    pub fn hold_progress(&self) -> Option<f32> {
        self.hold_running().then_some(self.hold.t)
    }

    /// A hold is pressed and hasn't come to its end.
    fn hold_running(&self) -> bool {
        self.hold.node != NIL && !self.hold.done && self.nodes[self.hold.node as usize].pressed
    }

    /// Hold button `i` was pressed, by the pointer or by `key`: its hold counts from the next
    /// tick. A hold on it already running goes on.
    fn start_hold(&mut self, i: u32, key: Option<KeyCode>) {
        if self.hold.node == i {
            return;
        }
        self.end_hold();
        self.hold = Hold {
            node: i,
            key,
            ..Hold::NONE
        };
    }

    /// Ends the hold, if one runs or is spent, without its message; a key's press ends with it.
    pub(crate) fn end_hold(&mut self) {
        let h = std::mem::replace(&mut self.hold, Hold::NONE);
        if h.node == NIL {
            return;
        }
        if h.key.is_some() {
            self.set_pressed(h.node, false);
        }
        self.damage_node(h.node);
    }

    /// Node `i` is no longer a hold button (or is gone): its Invoke message and its hold go.
    pub(crate) fn forget_hold(&mut self, i: u32) {
        if let Some(k) = self.holds.iter().position(|s| s.node == i) {
            self.holds.swap_remove(k);
        }
        if self.hold.node == i {
            self.end_hold();
        }
    }

    /// Where `step` moves from `from` among the interactive children of group `parent`, wrapping
    /// if `wrap`; [`NIL`] when there's nowhere to go.
    fn rove_target(&self, parent: u32, from: u32, step: Rove, wrap: bool) -> u32 {
        let p = &self.nodes[parent as usize];
        // A disabled menu item can be reached, as in Windows menus.
        let usable = |i: u32| i != NIL && self.nodes[i as usize].focusable();
        let walk = |mut at: u32, back: bool| {
            while at != NIL && !usable(at) {
                let n = &self.nodes[at as usize];
                at = if back { n.prev } else { n.next };
            }
            at
        };
        let sibling = |i: u32, back: bool| {
            let n = &self.nodes[i as usize];
            if back { n.prev } else { n.next }
        };
        match step {
            Rove::First => walk(p.first, false),
            Rove::Last => walk(p.last, true),
            Rove::Next | Rove::Prev => {
                let back = step == Rove::Prev;
                let to = walk(sibling(from, back), back);
                if to == NIL && wrap {
                    walk(if back { p.last } else { p.first }, back)
                } else {
                    to
                }
            }
        }
    }

    /// The parent of interactive node `i` when it's a group, so Tab treats its items as one
    /// stop; [`NIL`] otherwise.
    fn group_of(&self, i: u32) -> u32 {
        let parent = self.nodes[i as usize].parent;
        match self.nodes.get(parent as usize) {
            Some(p) if p.group.is_some() => parent,
            _ => NIL,
        }
    }

    /// Where Tab lands in group `g`: its picked item, else its first (`forward`) or last.
    fn group_entry(&self, g: u32, forward: bool) -> u32 {
        let mut c = self.nodes[g as usize].first;
        while c != NIL {
            let n = &self.nodes[c as usize];
            if n.interactive() && n.selected == Some(true) {
                return c;
            }
            c = n.next;
        }
        let step = if forward { Rove::First } else { Rove::Last };
        self.rove_target(g, NIL, step, false)
    }

    /// Whether Tab can land on `at`, coming from `from`, and where: an interactive node, or for
    /// one in a group its [entry](Self::group_entry), unless `from` is in that group already.
    fn tab_stop(&self, at: u32, from: u32, forward: bool) -> u32 {
        if !self.nodes[at as usize].interactive() {
            // A viewport with no tab stop inside is one (A2.4).
            return if self.nodes[at as usize].scroll != NIL && self.lone_viewport(at) {
                at
            } else {
                NIL
            };
        }
        let g = self.group_of(at);
        if g == NIL {
            at
        } else if from != NIL && self.nodes[from as usize].parent == g {
            NIL
        } else {
            self.group_entry(g, forward)
        }
    }

    /// The topmost enabled interactive node under `p`, or [`NIL`].
    /// Over a scroll bar, none: the bar is above the content (A2.4).
    pub(crate) fn hit(&self, p: Point) -> u32 {
        let mut found = NIL;
        let (_, bar) = self.walk_at(p, |at| {
            if self.nodes[at as usize].interactive() {
                found = at;
            }
        });
        if bar.is_some() { NIL } else { found }
    }

    /// The pointer is over `hit`. While a press is captured, only the captured node can be
    /// hovered, and it shows pressed only while the pointer is over it.
    pub(crate) fn pointer_over(&mut self, hit: u32) -> bool {
        let hit = if self.pressed == NIL || hit == self.pressed {
            hit
        } else {
            NIL
        };
        if self.pressed != NIL && !self.is_field(self.pressed) {
            self.set_pressed(self.pressed, hit == self.pressed);
        }
        self.set_hover(hit)
    }

    /// Hover moved to `to`. Returns whether it changed, which can change the pointer's shape.
    pub(crate) fn set_hover(&mut self, to: u32) -> bool {
        let from = self.hover;
        if from == to {
            return false;
        }
        self.hover = to;
        for (i, on) in [(from, false), (to, true)] {
            if i != NIL {
                self.nodes[i as usize].hovered = on;
                // A field shows hover only as the I-beam.
                if !self.is_field(i) {
                    self.animate_hover(i);
                }
            }
        }
        true
    }

    /// Whether `i` is a field node; false for [`NIL`].
    pub(crate) fn is_field(&self, i: u32) -> bool {
        self.nodes
            .get(i as usize)
            .is_some_and(|n| n.role == Role::Field)
    }

    fn animate_hover(&mut self, i: u32) {
        let duration = self.motion.hover_ms;
        let n = &mut self.nodes[i as usize];
        if duration == 0 {
            n.hover_t = if n.hovered { 1.0 } else { 0.0 };
        } else {
            // From wherever the highlight is now, for a full duration starting at the next tick.
            n.anim_from = n.hover_t;
            n.anim_start = NOT_STARTED;
            if !n.animating {
                n.animating = true;
                let id = NodeId {
                    index: i,
                    generation: n.generation,
                };
                self.anims.push(id);
            }
        }
        self.damage_node(i);
    }

    /// Cancels a press, by the pointer or by Space, so its release activates nothing, and ends a
    /// thumb drag: the window lost the pointer, went inactive, or hid (a popup closed by Esc
    /// while its item is held).
    pub fn cancel_press(&mut self) {
        let held = [
            std::mem::replace(&mut self.pressed, NIL),
            std::mem::replace(&mut self.key_pressed, NIL),
        ];
        for i in held {
            if i != NIL {
                self.set_pressed(i, false);
            }
        }
        self.end_hold();
        self.scroll_release();
    }

    /// The pointer's press only, the window having lost the pointer: a press held on a node (and
    /// its hold, which would otherwise count again as the bare pointer came back over it) or a
    /// thumb being dragged. A press by Space stays.
    fn cancel_pointer_press(&mut self) {
        self.scroll_release();
        let i = std::mem::replace(&mut self.pressed, NIL);
        if i == NIL {
            return;
        }
        self.set_pressed(i, false);
        if self.hold.node == i && self.hold.key.is_none() {
            self.end_hold();
        }
    }

    /// Shows node `i` pressed or not. A hold let go before its end (the pointer sliding off)
    /// goes back to its start, and counts again from the next tick it's pressed.
    fn set_pressed(&mut self, i: u32, on: bool) {
        let n = &mut self.nodes[i as usize];
        if n.pressed != on {
            n.pressed = on;
            if !on && self.hold.node == i && !self.hold.done {
                self.hold.start = None;
                self.hold.t = 0.0;
            }
            self.damage_node(i);
        }
    }

    /// Moves focus to `to` and scrolls it into view (A2.4): from the keyboard, a client, or the
    /// app.
    pub(crate) fn move_focus(&mut self, to: u32) {
        self.focus_node(to);
        self.reveal(to);
    }

    /// Moves focus to `to` where it is: from the pointer, which it stays under.
    pub(crate) fn focus_node(&mut self, to: u32) {
        let from = self.focus;
        if from == to {
            return;
        }
        if self.hold.node != NIL && self.hold.node == from {
            self.end_hold();
        }
        if self.key_pressed != NIL {
            let k = self.key_pressed;
            self.key_pressed = NIL;
            self.set_pressed(k, false);
        }
        self.focus = to;
        self.damage_node(from);
        self.damage_node(to);
        if self.is_field(to) {
            // The caret shows at once, and blinks from now.
            self.reset_caret();
        }
    }

    pub(crate) fn show_focus_ring(&mut self) {
        if !self.focus_visible {
            self.focus_visible = true;
            self.damage_node(self.focus);
        }
    }

    pub(crate) fn hide_focus_ring(&mut self) {
        if self.focus_visible {
            self.focus_visible = false;
            self.damage_node(self.focus);
        }
    }

    /// The next Tab stop after `from` in tree order, wrapping; [`NIL`] when there is none. A
    /// group is one stop (its [entry](Self::group_entry)).
    fn focusable_after(&self, from: u32) -> u32 {
        self.tab_from(from, true)
    }

    /// The next Tab stop before `from` in tree order, wrapping; [`NIL`] when there is none.
    fn focusable_before(&self, from: u32) -> u32 {
        self.tab_from(from, false)
    }

    pub(crate) fn tab_from(&self, from: u32, forward: bool) -> u32 {
        let start = if from == NIL { ROOT } else { from };
        let mut at = start;
        loop {
            at = if forward {
                self.next_in_order(at)
            } else {
                self.prev_in_order(at)
            };
            if at == start {
                // Around to where it started: nowhere else to go.
                return if self.nodes[at as usize].interactive() {
                    at
                } else {
                    NIL
                };
            }
            let stop = self.tab_stop(at, from, forward);
            if stop != NIL {
                return stop;
            }
        }
    }

    /// Pre-order successor, wrapping from the last node to the root.
    fn next_in_order(&self, i: u32) -> u32 {
        let n = &self.nodes[i as usize];
        if n.first != NIL {
            return n.first;
        }
        let mut at = i;
        while at != ROOT {
            let n = &self.nodes[at as usize];
            if n.next != NIL {
                return n.next;
            }
            at = n.parent;
        }
        ROOT
    }

    /// Pre-order predecessor, wrapping from the root to the last node.
    fn prev_in_order(&self, i: u32) -> u32 {
        let mut at = if i == ROOT {
            ROOT
        } else {
            let n = &self.nodes[i as usize];
            if n.prev == NIL {
                return n.parent;
            }
            n.prev
        };
        // The last node of `at`'s subtree.
        while self.nodes[at as usize].last != NIL {
            at = self.nodes[at as usize].last;
        }
        at
    }
}

/// Doc 05's ease-out: fast, then settling (cubic).
fn ease_out(t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

#[cfg(test)]
mod tests {
    use crate::{
        Element, FakeText, Input, Key, KeyCode, KeyEvent, Mods, Motion, Point, RectPx, Size, Ui,
    };

    fn key(key: KeyCode, down: bool, shift: bool) -> Input {
        Input::Key(KeyEvent {
            key,
            down,
            mods: Mods {
                shift,
                ..Mods::default()
            },
        })
    }

    /// Three 100 × 20 buttons in a column, the middle one optionally disabled, then a label.
    fn ui(motion: Motion, middle: bool) -> Ui<u8> {
        let mut ui = Ui::new(Size { w: 100.0, h: 100.0 }, 1.0, motion);
        build(&mut ui, middle);
        ui
    }

    fn build(ui: &mut Ui<u8>, middle: bool) {
        let fixed = crate::Style {
            height: crate::Dim::Fixed(20.0),
            ..crate::Style::default()
        };
        ui.build(|cx| {
            cx.add(Element::button(Key(1), "a", 1).style(fixed));
            cx.add(Element::button(Key(2), "b", 2).style(fixed).enabled(middle));
            cx.add(Element::button(Key(3), "c", 3).style(fixed));
            cx.add(Element::text(Key(4), "label").style(fixed));
        });
        ui.layout(&mut FakeText);
    }

    fn at(y: f32) -> Point {
        Point { x: 50.0, y }
    }

    fn focused(ui: &Ui<u8>) -> Option<u64> {
        ui.focus().map(|id| ui.info(id).unwrap().key.0)
    }

    #[test]
    fn a_click_is_a_press_and_release_on_the_same_node() {
        let mut ui = ui(Motion::default(), true);
        let mut out = Vec::new();
        assert!(ui.dispatch(Input::PointerDown(at(30.0)), &mut out));
        assert!(ui.info(ui.find(&[Key(2)]).unwrap()).unwrap().pressed);
        assert!(ui.dispatch(Input::PointerUp(at(35.0)), &mut out));
        assert_eq!(out, [2]);
        assert_eq!(focused(&ui), Some(2));
        assert!(!ui.focus_visible());

        // Released elsewhere: captured, so nothing activates, and nothing else hovers meanwhile.
        out.clear();
        ui.dispatch(Input::PointerDown(at(10.0)), &mut out);
        ui.dispatch(Input::PointerMove(at(50.0)), &mut out);
        assert_eq!(ui.hover(), None);
        assert!(!ui.info(ui.find(&[Key(1)]).unwrap()).unwrap().pressed);
        ui.dispatch(Input::PointerUp(at(50.0)), &mut out);
        assert_eq!(out, [] as [u8; 0]);
        assert_eq!(ui.hover(), ui.find(&[Key(3)]));

        // Labels and empty space take no clicks.
        assert!(!ui.dispatch(Input::PointerDown(at(70.0)), &mut out));
        assert!(!ui.dispatch(Input::PointerUp(at(70.0)), &mut out));
        assert!(!ui.dispatch(Input::PointerDown(at(95.0)), &mut out));
        assert_eq!(out, [] as [u8; 0]);
    }

    #[test]
    fn disabled_nodes_take_no_input_and_lose_what_they_had() {
        let mut ui = ui(Motion::default(), true);
        let mut out = Vec::new();
        ui.dispatch(Input::PointerMove(at(30.0)), &mut out);
        ui.set_focus(ui.find(&[Key(2)]));
        assert_eq!(focused(&ui), Some(2));
        build(&mut ui, false);
        assert_eq!((ui.hover(), ui.focus()), (None, None));
        ui.dispatch(Input::PointerDown(at(30.0)), &mut out);
        ui.dispatch(Input::PointerUp(at(30.0)), &mut out);
        assert_eq!(out, [] as [u8; 0]);
        ui.set_focus(ui.find(&[Key(2)]));
        assert_eq!(ui.focus(), None);
    }

    #[test]
    fn tab_walks_focusable_nodes_in_order_and_wraps() {
        let mut ui = ui(Motion::default(), false);
        let mut out = Vec::new();
        let mut order = Vec::new();
        for _ in 0..3 {
            assert!(ui.dispatch(key(KeyCode::Tab, true, false), &mut out));
            assert!(!ui.dispatch(key(KeyCode::Tab, false, false), &mut out));
            order.push(focused(&ui).unwrap());
        }
        assert_eq!(order, [1, 3, 1]);
        assert!(ui.focus_visible());
        ui.dispatch(key(KeyCode::Tab, true, true), &mut out);
        assert_eq!(focused(&ui), Some(3));
        ui.dispatch(key(KeyCode::Tab, true, true), &mut out);
        assert_eq!(focused(&ui), Some(1));
        // From nothing, Shift+Tab starts at the end.
        ui.set_focus(None);
        ui.dispatch(key(KeyCode::Tab, true, true), &mut out);
        assert_eq!(focused(&ui), Some(3));
        assert_eq!(out, [] as [u8; 0]);
    }

    #[test]
    fn enter_activates_on_press_and_space_on_release() {
        let mut ui = ui(Motion::default(), true);
        let mut out = Vec::new();
        assert!(!ui.dispatch(key(KeyCode::Enter, true, false), &mut out));
        ui.dispatch(key(KeyCode::Tab, true, false), &mut out);
        assert!(ui.dispatch(key(KeyCode::Enter, true, false), &mut out));
        assert_eq!(out, [1]);
        assert!(ui.dispatch(key(KeyCode::Space, true, false), &mut out));
        assert_eq!(out, [1]);
        assert!(ui.dispatch(key(KeyCode::Space, false, false), &mut out));
        assert_eq!(out, [1, 1]);
        // Focus moving away mid-press cancels it.
        ui.dispatch(key(KeyCode::Space, true, false), &mut out);
        ui.dispatch(key(KeyCode::Tab, true, false), &mut out);
        assert!(!ui.dispatch(key(KeyCode::Space, false, false), &mut out));
        assert_eq!(out, [1, 1]);
        assert!(!ui.dispatch(key(KeyCode::Escape, true, false), &mut out));
    }

    #[test]
    fn hover_fades_on_the_clock_and_reduced_motion_jumps() {
        let mut ui = ui(Motion { hover_ms: 120 }, true);
        let mut out = Vec::new();
        let a = ui.find(&[Key(1)]).unwrap();
        ui.dispatch(Input::PointerMove(at(10.0)), &mut out);
        assert!(ui.wants_frame());
        ui.tick(1_000, &mut out);
        assert_eq!(ui.info(a).unwrap().hover, 0.0);
        ui.tick(1_060, &mut out);
        let half = ui.info(a).unwrap().hover;
        assert!(half > 0.5 && half < 1.0, "{half}");
        ui.clear_damage();
        ui.tick(1_200, &mut out);
        assert_eq!(ui.info(a).unwrap().hover, 1.0);
        assert!(!ui.wants_frame());
        assert_ne!(ui.damage(), [] as [RectPx; 0]);

        // Leaving mid-fade turns back from where the highlight is.
        ui.dispatch(Input::PointerLeave, &mut out);
        ui.tick(2_000, &mut out);
        ui.tick(2_030, &mut out);
        ui.dispatch(Input::PointerMove(at(10.0)), &mut out);
        let from = ui.info(a).unwrap().hover;
        ui.tick(2_031, &mut out);
        assert!(ui.info(a).unwrap().hover >= from);
        assert_eq!(ui.anims.len(), 1);

        let mut ui = self::ui(Motion { hover_ms: 0 }, true);
        ui.dispatch(Input::PointerMove(at(10.0)), &mut out);
        assert!(!ui.wants_frame());
        assert_eq!(ui.info(ui.find(&[Key(1)]).unwrap()).unwrap().hover, 1.0);
    }

    /// *Animation effects* turned off mid-fade: the fade ends on the next tick, and the frames
    /// stop.
    #[test]
    fn reduced_motion_mid_fade_ends_it_at_the_next_tick() {
        let mut ui = ui(Motion::STANDARD, true);
        let mut out = Vec::new();
        ui.dispatch(Input::PointerMove(at(10.0)), &mut out);
        ui.tick(1_000, &mut out);
        ui.tick(1_030, &mut out);
        let a = ui.find(&[Key(1)]).unwrap();
        assert!(ui.info(a).unwrap().hover < 1.0);
        assert!(ui.wants_frame());
        ui.set_motion(Motion::system(false));
        assert_eq!(ui.motion(), Motion::REDUCED);
        ui.tick(1_031, &mut out);
        assert_eq!(ui.info(a).unwrap().hover, 1.0);
        assert!(!ui.wants_frame());
    }

    #[test]
    fn hover_follows_nodes_that_move_under_a_still_pointer() {
        let mut ui = ui(Motion::default(), true);
        let mut out = Vec::new();
        ui.dispatch(Input::PointerMove(at(30.0)), &mut out);
        assert_eq!(ui.hover(), ui.find(&[Key(2)]));
        // Without the first button, the third moves up under the pointer.
        ui.build(|cx| {
            let fixed = crate::Style {
                height: crate::Dim::Fixed(20.0),
                ..crate::Style::default()
            };
            cx.add(Element::button(Key(2), "b", 2).style(fixed));
            cx.add(Element::button(Key(3), "c", 3).style(fixed));
        });
        ui.layout(&mut FakeText);
        assert_eq!(ui.hover(), ui.find(&[Key(3)]));
        ui.dispatch(Input::PointerLeave, &mut out);
        ui.build(|cx| {
            cx.add(Element::button(Key(3), "c", 3));
        });
        ui.layout(&mut FakeText);
        assert_eq!(ui.hover(), None);
    }

    #[test]
    fn removed_nodes_drop_focus_hover_and_animations() {
        let mut ui = ui(Motion { hover_ms: 120 }, true);
        let mut out = Vec::new();
        ui.dispatch(Input::PointerMove(at(10.0)), &mut out);
        ui.dispatch(key(KeyCode::Tab, true, false), &mut out);
        assert_eq!(focused(&ui), Some(1));
        ui.build(|cx| {
            cx.add(Element::button(Key(3), "c", 3));
        });
        assert_eq!((ui.hover(), ui.focus()), (None, None));
        assert!(!ui.wants_frame());
    }

    /// Three tabs (`tab` picked, messages 10–12), a button (20), three radio buttons (`radio`
    /// picked, 30–32) and a column list of three (the first picked, the last disabled; 40–42).
    fn groups(ui: &mut Ui<u8>, tab: u8, radio: u8) {
        ui.build(|cx| {
            cx.add_with(Element::tabs(Key(1)).name("Tabs"), |cx| {
                for t in 0..3 {
                    cx.add(Element::tab(Key::index(t.into()), "t", t == tab, 10 + t));
                }
            });
            cx.add(Element::button(Key(2), "b", 20));
            cx.add_with(Element::radios(Key(3)).name("Radios"), |cx| {
                for r in 0..3 {
                    cx.add(Element::radio(
                        Key::index(r.into()),
                        "r",
                        r == radio,
                        30 + r,
                    ));
                }
            });
            cx.add_with(Element::list(Key(4)).name("List"), |cx| {
                for k in 0..3 {
                    cx.add(
                        Element::button(Key::index(k.into()), "l", 40 + k)
                            .selected(k == 0)
                            .enabled(k != 2),
                    );
                }
            });
        });
        ui.layout(&mut FakeText);
    }

    fn item(ui: &Ui<u8>, group: u64, k: u64) -> Option<crate::NodeId> {
        ui.find(&[Key(group), Key::index(k)])
    }

    fn press(ui: &mut Ui<u8>, k: KeyCode, out: &mut Vec<u8>) -> bool {
        let used = ui.dispatch(key(k, true, false), out);
        ui.dispatch(key(k, false, false), out);
        used
    }

    #[test]
    fn a_group_is_one_tab_stop_landing_on_its_pick() {
        let mut ui = Ui::new(Size { w: 300.0, h: 300.0 }, 1.0, Motion::default());
        // No radio button picked.
        groups(&mut ui, 1, 9);
        let mut out = Vec::new();
        let mut stops = Vec::new();
        for _ in 0..5 {
            ui.dispatch(key(KeyCode::Tab, true, false), &mut out);
            stops.push(ui.focus());
        }
        let b = ui.find(&[Key(2)]);
        assert_eq!(
            stops,
            [
                item(&ui, 1, 1),
                b,
                item(&ui, 3, 0),
                item(&ui, 4, 0),
                item(&ui, 1, 1)
            ],
            "the picked tab, the button, the first radio button, the picked page, around"
        );
        let mut back = Vec::new();
        for _ in 0..3 {
            ui.dispatch(key(KeyCode::Tab, true, true), &mut out);
            back.push(ui.focus());
        }
        assert_eq!(
            back,
            [item(&ui, 4, 0), item(&ui, 3, 2), b],
            "going back: the last"
        );
        assert_eq!(out, [] as [u8; 0], "Tab picks nothing");
    }

    #[test]
    fn arrows_in_tabs_and_radio_groups_move_pick_and_wrap() {
        let mut ui = Ui::new(Size { w: 300.0, h: 300.0 }, 1.0, Motion::default());
        groups(&mut ui, 1, 0);
        let mut out = Vec::new();
        ui.set_focus(item(&ui, 1, 1));
        // A row of tabs: Left and Right, wrapping; Up and Down aren't theirs.
        for k in [
            KeyCode::Right,
            KeyCode::Right,
            KeyCode::Left,
            KeyCode::Home,
            KeyCode::End,
        ] {
            assert!(press(&mut ui, k, &mut out), "{k:?}");
        }
        assert_eq!(out, [12, 10, 12, 10, 12]);
        assert_eq!(ui.focus(), item(&ui, 1, 2));
        assert!(ui.focus_visible());
        out.clear();
        assert!(!press(&mut ui, KeyCode::Down, &mut out));
        let ctrl = Input::Key(KeyEvent {
            key: KeyCode::Right,
            down: true,
            mods: Mods {
                ctrl: true,
                ..Mods::default()
            },
        });
        assert!(!ui.dispatch(ctrl, &mut out), "Ctrl+Right is the caller's");
        assert_eq!(
            (out.as_slice(), ui.focus()),
            (&[] as &[u8], item(&ui, 1, 2))
        );

        // Radio buttons: every arrow.
        ui.set_focus(item(&ui, 3, 0));
        for k in [
            KeyCode::Down,
            KeyCode::Right,
            KeyCode::Down,
            KeyCode::Up,
            KeyCode::Left,
        ] {
            assert!(press(&mut ui, k, &mut out), "{k:?}");
        }
        assert_eq!(out, [31, 32, 30, 32, 31]);
        assert_eq!(ui.focus(), item(&ui, 3, 1));
    }

    #[test]
    fn arrows_in_a_list_move_focus_only_and_stop_at_its_ends() {
        let mut ui = Ui::new(Size { w: 300.0, h: 300.0 }, 1.0, Motion::default());
        groups(&mut ui, 0, 0);
        let mut out = Vec::new();
        ui.set_focus(item(&ui, 4, 0));
        assert!(press(&mut ui, KeyCode::Down, &mut out));
        assert_eq!(ui.focus(), item(&ui, 4, 1));
        // The next is disabled, and a list doesn't wrap: the arrow is used, and focus stays.
        assert!(press(&mut ui, KeyCode::Down, &mut out));
        assert_eq!(ui.focus(), item(&ui, 4, 1));
        assert!(press(&mut ui, KeyCode::Home, &mut out));
        assert_eq!(ui.focus(), item(&ui, 4, 0));
        assert!(press(&mut ui, KeyCode::Up, &mut out));
        assert_eq!(ui.focus(), item(&ui, 4, 0));
        assert!(press(&mut ui, KeyCode::End, &mut out));
        assert_eq!(ui.focus(), item(&ui, 4, 1));
        // A column: Left and Right aren't its.
        assert!(!press(&mut ui, KeyCode::Left, &mut out));
        assert_eq!(out, [] as [u8; 0], "moving picks nothing");
        press(&mut ui, KeyCode::Enter, &mut out);
        assert_eq!(out, [41], "Enter opens");
        // Outside any group, arrows are the caller's.
        ui.set_focus(ui.find(&[Key(2)]));
        assert!(!press(&mut ui, KeyCode::Down, &mut out));
    }

    /// A hold button (message 7, Invoke 8) over a button (9), each 100 × 20. `hold` is `None`
    /// to leave the hold button out, else whether it's enabled and whether it holds.
    fn hold_ui(ui: &mut Ui<u8>, hold: Option<(bool, bool)>) {
        let fixed = crate::Style {
            height: crate::Dim::Fixed(20.0),
            ..crate::Style::default()
        };
        ui.build(|cx| {
            if let Some((enabled, holds)) = hold {
                let b = Element::button(Key(1), "Hold", 7)
                    .style(fixed)
                    .enabled(enabled);
                cx.add(if holds { b.hold(|| 8) } else { b });
            }
            cx.add(Element::button(Key(2), "b", 9).style(fixed));
        });
        ui.layout(&mut FakeText);
    }

    fn holding() -> Ui<u8> {
        let mut ui = Ui::new(Size { w: 100.0, h: 100.0 }, 1.0, Motion::default());
        hold_ui(&mut ui, Some((true, true)));
        ui
    }

    #[test]
    fn a_hold_sends_once_after_hold_ms_on_the_frame_clock() {
        let mut ui = holding();
        let mut out = Vec::new();
        assert!(!ui.wants_frame());
        ui.dispatch(Input::PointerDown(at(10.0)), &mut out);
        assert_eq!((ui.hold_progress(), ui.wants_frame()), (Some(0.0), true));
        // It counts from the first tick.
        ui.tick(1_000, &mut out);
        ui.tick(1_750, &mut out);
        assert_eq!(ui.hold_progress(), Some(0.5));
        ui.tick(2_499, &mut out);
        assert_eq!(out, [] as [u8; 0]);
        ui.tick(2_500, &mut out);
        assert_eq!(out, [7]);
        // Spent: no more frames while it's still held, and no click on release.
        assert_eq!((ui.hold_progress(), ui.wants_frame()), (None, false));
        ui.tick(3_000, &mut out);
        ui.dispatch(Input::PointerUp(at(10.0)), &mut out);
        assert_eq!(out, [7]);

        // Let go early: nothing.
        ui.dispatch(Input::PointerDown(at(10.0)), &mut out);
        ui.tick(10_000, &mut out);
        ui.tick(11_499, &mut out);
        ui.dispatch(Input::PointerUp(at(10.0)), &mut out);
        assert_eq!(
            (out.as_slice(), ui.wants_frame()),
            (&[7_u8] as &[u8], false)
        );

        // Sliding off goes back to the start; back on, it counts again from the next tick.
        ui.dispatch(Input::PointerDown(at(10.0)), &mut out);
        ui.tick(20_000, &mut out);
        ui.tick(21_000, &mut out);
        ui.dispatch(Input::PointerMove(at(30.0)), &mut out);
        assert_eq!((ui.hold_progress(), ui.wants_frame()), (None, false));
        ui.tick(25_000, &mut out);
        ui.dispatch(Input::PointerMove(at(10.0)), &mut out);
        assert_eq!(ui.hold_progress(), Some(0.0));
        ui.tick(30_000, &mut out);
        ui.tick(31_499, &mut out);
        assert_eq!(out, [7]);
        ui.tick(31_500, &mut out);
        assert_eq!(out, [7, 7]);
    }

    /// Windows sends no leave while a press holds the capture: a leave mid-press means another
    /// window took the pointer, and the press is over. The bare pointer coming back over the
    /// button must not count the hold again, or a hold-to-confirm would confirm unheld.
    #[test]
    fn a_press_the_window_lost_is_cancelled_and_its_hold_never_completes() {
        let mut ui = holding();
        let mut out = Vec::new();
        ui.dispatch(Input::PointerDown(at(10.0)), &mut out);
        ui.tick(1_000, &mut out);
        ui.dispatch(Input::PointerLeave, &mut out);
        assert_eq!((ui.hold_progress(), ui.wants_frame()), (None, false));
        ui.dispatch(Input::PointerMove(at(10.0)), &mut out);
        assert_eq!(ui.hold_progress(), None);
        ui.tick(2_000, &mut out);
        ui.tick(10_000, &mut out);
        assert_eq!(out, [] as [u8; 0]);
        // The release of a press it never saw invokes nothing either.
        ui.dispatch(Input::PointerUp(at(10.0)), &mut out);
        assert_eq!(out, [] as [u8; 0]);
        // A press by Space, on the plain button, survives the pointer leaving.
        ui.set_focus(ui.find(&[Key(2)]));
        ui.dispatch(key(KeyCode::Space, true, false), &mut out);
        ui.dispatch(Input::PointerLeave, &mut out);
        ui.dispatch(key(KeyCode::Space, false, false), &mut out);
        assert_eq!(out, [9]);
    }

    #[test]
    fn reduced_motion_leaves_a_hold_its_time() {
        // A hold is a confirmation, not an animation: no shorter, and still drawn as it fills.
        let hold = u64::from(crate::tokens::motion::HOLD_MS);
        let mut ui = holding();
        ui.set_motion(Motion::REDUCED);
        let mut out = Vec::new();
        ui.dispatch(Input::PointerDown(at(10.0)), &mut out);
        ui.tick(0, &mut out);
        assert!(ui.wants_frame());
        ui.tick(hold / 2, &mut out);
        assert_eq!(ui.hold_progress(), Some(0.5));
        ui.tick(hold - 1, &mut out);
        assert_eq!(out, [] as [u8; 0]);
        ui.tick(hold, &mut out);
        assert_eq!(out, [7]);
    }

    #[test]
    fn a_held_key_holds_and_its_repeats_change_nothing() {
        let mut ui = holding();
        let mut out = Vec::new();
        ui.set_active(true);
        ui.dispatch(key(KeyCode::Tab, true, false), &mut out);
        // Enter on a hold button presses it; it doesn't send at once.
        assert!(ui.dispatch(key(KeyCode::Enter, true, false), &mut out));
        let hold = ui.find(&[Key(1)]).unwrap();
        assert!(ui.info(hold).unwrap().pressed);
        ui.tick(0, &mut out);
        ui.tick(1_000, &mut out);
        // Auto-repeat: the same hold goes on.
        ui.dispatch(key(KeyCode::Enter, true, false), &mut out);
        ui.tick(1_499, &mut out);
        assert_eq!(out, [] as [u8; 0]);
        ui.tick(1_500, &mut out);
        ui.dispatch(key(KeyCode::Enter, true, false), &mut out);
        assert!(ui.dispatch(key(KeyCode::Enter, false, false), &mut out));
        assert_eq!(out, [7]);
        assert!(!ui.info(hold).unwrap().pressed);

        // Space let go early sends nothing, on its release either.
        ui.dispatch(key(KeyCode::Space, true, false), &mut out);
        ui.tick(5_000, &mut out);
        ui.tick(6_000, &mut out);
        assert!(ui.dispatch(key(KeyCode::Space, false, false), &mut out));
        assert_eq!(out, [7]);

        // Focus moving ends it, and so does the window going inactive.
        ui.dispatch(key(KeyCode::Enter, true, false), &mut out);
        ui.tick(7_000, &mut out);
        ui.dispatch(key(KeyCode::Tab, true, false), &mut out);
        assert_eq!(
            (ui.hold_progress(), ui.info(hold).unwrap().pressed),
            (None, false)
        );
        ui.tick(9_000, &mut out);
        ui.dispatch(key(KeyCode::Enter, false, false), &mut out);
        ui.dispatch(key(KeyCode::Tab, true, true), &mut out);
        ui.dispatch(key(KeyCode::Enter, true, false), &mut out);
        ui.tick(10_000, &mut out);
        ui.set_active(false);
        assert_eq!((ui.hold_progress(), ui.wants_frame()), (None, false));
        ui.tick(12_000, &mut out);
        assert_eq!(out, [7]);
    }

    #[test]
    fn a_hold_ends_when_its_button_is_disabled_removed_or_plain() {
        let mut ui = holding();
        let mut out = Vec::new();
        ui.dispatch(Input::PointerDown(at(10.0)), &mut out);
        ui.tick(0, &mut out);
        hold_ui(&mut ui, Some((false, true)));
        assert_eq!((ui.hold_progress(), ui.wants_frame()), (None, false));
        ui.tick(2_000, &mut out);
        ui.dispatch(Input::PointerUp(at(10.0)), &mut out);

        hold_ui(&mut ui, Some((true, true)));
        ui.dispatch(Input::PointerDown(at(10.0)), &mut out);
        ui.tick(3_000, &mut out);
        hold_ui(&mut ui, Some((true, false)));
        assert_eq!((ui.hold_progress(), ui.holds.len()), (None, 0));
        ui.tick(5_000, &mut out);
        ui.dispatch(Input::PointerUp(at(10.0)), &mut out);
        assert_eq!(out, [7], "a plain button again: its click");
        out.clear();

        hold_ui(&mut ui, Some((true, true)));
        assert_eq!(ui.holds.len(), 1);
        ui.dispatch(Input::PointerDown(at(10.0)), &mut out);
        ui.tick(6_000, &mut out);
        hold_ui(&mut ui, None);
        assert_eq!((ui.hold_progress(), ui.holds.len()), (None, 0));
        ui.tick(8_000, &mut out);
        assert_eq!(out, [] as [u8; 0]);
    }
}
