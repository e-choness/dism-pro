//! Input, focus and the animation clock (TD-0009 §5.2).
//!
//! - **Pointer:** the hit test finds the topmost enabled button or check box under the pointer;
//!   children paint over parents and later siblings over earlier ones, and a node outside its
//!   parent is clipped away. A press captures: the node that took it gets the release, and a
//!   release over that node activates it. While captured, only that node shows hover.
//! - **Keyboard:** Tab and Shift+Tab move focus in tree order, which is visual order because views
//!   build in reading order, wrapping at the ends. Enter activates on the press; Space presses and
//!   activates on the release, as Windows buttons do. Other keys are left to the caller.
//! - **Animations:** hover fades over [`Motion::hover_ms`](crate::Motion) with an ease-out curve
//!   (doc 05 §4). A duration of 0 jumps, so reduced motion never asks for a frame.

use crate::{
    geom::Point,
    tree::{NIL, NodeId},
    ui::{ROOT, Ui},
};

/// An input event, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    PointerMove(Point),
    PointerDown(Point),
    PointerUp(Point),
    /// The pointer left the window (`WM_MOUSELEAVE`).
    PointerLeave,
    Key(KeyEvent),
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
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
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
            Input::PointerMove(p) | Input::PointerDown(p) | Input::PointerUp(p) => Some(p),
            Input::PointerLeave => None,
            Input::Key(_) => self.pointer,
        };
        match input {
            Input::PointerMove(p) => {
                let hit = self.hit(p);
                self.pointer_over(hit)
            }
            Input::PointerLeave => self.pointer_over(NIL),
            Input::PointerDown(p) => {
                let hit = self.hit(p);
                self.pointer_over(hit);
                self.hide_focus_ring();
                if hit == NIL {
                    return false;
                }
                self.pressed = hit;
                self.set_pressed(hit, true);
                self.move_focus(hit);
                true
            }
            Input::PointerUp(p) => {
                let target = self.pressed;
                if target == NIL {
                    return false;
                }
                self.pressed = NIL;
                self.set_pressed(target, false);
                let hit = self.hit(p);
                self.pointer_over(hit);
                if hit == target {
                    self.activate(target, out);
                }
                true
            }
            Input::Key(k) => self.key(k, out),
        }
    }

    fn key(&mut self, k: KeyEvent, out: &mut Vec<M>) -> bool {
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
                }
                true
            }
            (KeyCode::Enter, true) if self.focus != NIL => {
                self.show_focus_ring();
                self.activate(self.focus, out);
                true
            }
            (KeyCode::Space, true) if self.focus != NIL => {
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
            _ => false,
        }
    }

    fn activate(&mut self, i: u32, out: &mut Vec<M>) {
        let n = &self.nodes[i as usize];
        if n.interactive()
            && let Some(msg) = &n.msg
        {
            out.push(msg.clone());
        }
    }
}

impl<M> Ui<M> {
    /// Moves focus to `id`, as UI Automation's `SetFocus` does; `None` clears it. A node that
    /// can't take focus is ignored.
    pub fn set_focus(&mut self, id: Option<NodeId>) {
        match id.and_then(|id| self.index(id)) {
            Some(i) if self.nodes[i as usize].interactive() => self.move_focus(i),
            Some(_) => {}
            None => self.move_focus(NIL),
        }
    }

    /// Focus last moved by keyboard, so the focus ring shows (doc 05 §6).
    #[must_use]
    pub fn focus_visible(&self) -> bool {
        self.focus_visible && self.focus != NIL
    }

    /// Advances animations to `now_ms` on the monotonic clock, damaging what they change. An
    /// animation started since the last tick starts now.
    pub fn tick(&mut self, now_ms: u64) {
        let duration = self.motion.hover_ms;
        let mut k = 0;
        while k < self.anims.len() {
            let Some(i) = self.index(self.anims[k]) else {
                self.anims.swap_remove(k);
                continue;
            };
            let n = &mut self.nodes[i as usize];
            let start = *n.anim_start.get_or_insert(now_ms);
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
                n.anim_start = None;
                self.anims.swap_remove(k);
            } else {
                k += 1;
            }
            self.damage_node(i);
        }
    }

    /// Whether an animation is running, so the window should keep asking for frames.
    #[must_use]
    pub fn wants_frame(&self) -> bool {
        !self.anims.is_empty()
    }

    /// The topmost enabled interactive node under `p`, or [`NIL`].
    pub(crate) fn hit(&self, p: Point) -> u32 {
        let mut found = NIL;
        let mut at = ROOT;
        if !self.nodes[ROOT as usize].rect.contains(p) {
            return NIL;
        }
        loop {
            if self.nodes[at as usize].interactive() {
                found = at;
            }
            let mut c = self.nodes[at as usize].last;
            while c != NIL && !self.nodes[c as usize].rect.contains(p) {
                c = self.nodes[c as usize].prev;
            }
            if c == NIL {
                return found;
            }
            at = c;
        }
    }

    /// The pointer is over `hit`. While a press is captured, only the captured node can be
    /// hovered, and it shows pressed only while the pointer is over it.
    pub(crate) fn pointer_over(&mut self, hit: u32) -> bool {
        let hit = if self.pressed == NIL || hit == self.pressed {
            hit
        } else {
            NIL
        };
        if self.pressed != NIL {
            self.set_pressed(self.pressed, hit == self.pressed);
        }
        self.set_hover(hit)
    }

    pub(crate) fn set_hover(&mut self, to: u32) -> bool {
        let from = self.hover;
        if from == to {
            return false;
        }
        self.hover = to;
        for (i, on) in [(from, false), (to, true)] {
            if i != NIL {
                self.nodes[i as usize].hovered = on;
                self.animate_hover(i);
            }
        }
        true
    }

    fn animate_hover(&mut self, i: u32) {
        let duration = self.motion.hover_ms;
        let n = &mut self.nodes[i as usize];
        if duration == 0 {
            n.hover_t = if n.hovered { 1.0 } else { 0.0 };
        } else {
            // From wherever the highlight is now, for a full duration starting at the next tick.
            n.anim_from = n.hover_t;
            n.anim_start = None;
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

    fn set_pressed(&mut self, i: u32, on: bool) {
        let n = &mut self.nodes[i as usize];
        if n.pressed != on {
            n.pressed = on;
            self.damage_node(i);
        }
    }

    fn move_focus(&mut self, to: u32) {
        let from = self.focus;
        if from == to {
            return;
        }
        if self.key_pressed != NIL {
            let k = self.key_pressed;
            self.key_pressed = NIL;
            self.set_pressed(k, false);
        }
        self.focus = to;
        self.damage_node(from);
        self.damage_node(to);
    }

    fn show_focus_ring(&mut self) {
        if !self.focus_visible {
            self.focus_visible = true;
            self.damage_node(self.focus);
        }
    }

    fn hide_focus_ring(&mut self) {
        if self.focus_visible {
            self.focus_visible = false;
            self.damage_node(self.focus);
        }
    }

    /// The first focusable node after `from` in tree order, wrapping; [`NIL`] when none is.
    fn focusable_after(&self, from: u32) -> u32 {
        let start = if from == NIL { ROOT } else { from };
        let mut at = start;
        loop {
            at = self.next_in_order(at);
            if at == start {
                return if self.nodes[at as usize].interactive() {
                    at
                } else {
                    NIL
                };
            }
            if self.nodes[at as usize].interactive() {
                return at;
            }
        }
    }

    /// The first focusable node before `from` in tree order, wrapping; [`NIL`] when none is.
    fn focusable_before(&self, from: u32) -> u32 {
        let start = if from == NIL { ROOT } else { from };
        let mut at = start;
        loop {
            at = self.prev_in_order(at);
            if at == start {
                return if self.nodes[at as usize].interactive() {
                    at
                } else {
                    NIL
                };
            }
            if self.nodes[at as usize].interactive() {
                return at;
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
        ui.tick(1_000);
        assert_eq!(ui.info(a).unwrap().hover, 0.0);
        ui.tick(1_060);
        let half = ui.info(a).unwrap().hover;
        assert!(half > 0.5 && half < 1.0, "{half}");
        ui.clear_damage();
        ui.tick(1_200);
        assert_eq!(ui.info(a).unwrap().hover, 1.0);
        assert!(!ui.wants_frame());
        assert_ne!(ui.damage(), [] as [RectPx; 0]);

        // Leaving mid-fade turns back from where the highlight is.
        ui.dispatch(Input::PointerLeave, &mut out);
        ui.tick(2_000);
        ui.tick(2_030);
        ui.dispatch(Input::PointerMove(at(10.0)), &mut out);
        let from = ui.info(a).unwrap().hover;
        ui.tick(2_031);
        assert!(ui.info(a).unwrap().hover >= from);
        assert_eq!(ui.anims.len(), 1);

        let mut ui = self::ui(Motion { hover_ms: 0 }, true);
        ui.dispatch(Input::PointerMove(at(10.0)), &mut out);
        assert!(!ui.wants_frame());
        assert_eq!(ui.info(ui.find(&[Key(1)]).unwrap()).unwrap().hover, 1.0);
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
}
