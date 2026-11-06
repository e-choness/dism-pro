//! Scroll viewports (TD-0009 A2, slice 5a).
//!
//! - **State.** A viewport's offset is whole physical pixels, in `i64`: never `f32`, which would
//!   blur text at fractional offsets and can't count past 2^24 px. It lives in a side table, like
//!   fields, reached from the few nodes with the [`rare`] scroll flags.
//! - **Coordinates.** Layout places a viewport's content as if it weren't scrolled, so a scroll
//!   lays out nothing: painting, hit testing, damage and [`Ui::info`] move the content by the
//!   offset, and the accessibility tree gives the viewport a transform, so a scroll sends that one
//!   node.
//! - **The wheel** scrolls the innermost viewport under the pointer that can move that way, else
//!   the next one out. A notch is Windows' wheel lines of the viewport's line (its text style's
//!   line height), or a page when Windows says so (`WHEEL_PAGESCROLL`, [`u32::MAX`] lines); a
//!   smaller delta scrolls its share at once, and what's left of a pixel waits for the next. The
//!   horizontal wheel scrolls Windows' wheel characters, each half the body size.
//! - **Keys.** Page Up, Page Down, Ctrl+Home and Ctrl+End scroll the innermost viewport holding
//!   the focus, when the focused node doesn't use the key; a page is the viewport less a line. A
//!   viewport with no tab stop inside is one itself (WCAG 2.1.1), and its arrows, Home and End
//!   scroll it.
//! - **Into view.** Focus moving to a node scrolls each viewport holding it so the node is wholly
//!   in view (its focus ring is drawn inside it), or its start is if it's bigger than the view.
//! - **The bar** (A2.10 item 3), where the content overflows: an indicator, widened into a bar
//!   with its track while the pointer is on its strip, and always a bar with "Automatically hide
//!   scroll bars" off. Dragging the thumb scrolls in proportion; a press elsewhere on the strip
//!   moves a page toward it. The bar is no tab stop and not in the accessibility tree: the wheel,
//!   the keys and a client's scroll actions do all it does.
//! - **No timers.** A scroll is done when its event is (no smooth scrolling, no fading), so a
//!   settled viewport asks for no frames or wakes (U6).

use crate::{
    geom::{Point, Rect, Size},
    input::{KeyCode, KeyEvent},
    text::TextSystem,
    theme::tokens::scroll as bar,
    tree::{NIL, rare},
    ui::{ROOT, Ui},
};

/// Which ways a [scroll viewport](crate::Element::scroll) scrolls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axes {
    X,
    Y,
    Both,
}

impl Axes {
    /// The node's [`rare`] flags for these axes.
    pub(crate) const fn flags(self) -> u8 {
        match self {
            Self::X => rare::SCROLL_X,
            Self::Y => rare::SCROLL_Y,
            Self::Both => rare::SCROLL,
        }
    }
}

/// Where a scroll viewport is, for tests, probes and the host: [`NodeInfo::scroll`](
/// crate::NodeInfo::scroll).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollInfo {
    /// How far the content is scrolled, in logical pixels: [`offset_px`](Self::offset_px) over the
    /// scale.
    pub offset: Point,
    /// The offset in whole physical pixels, x then y.
    pub offset_px: [i64; 2],
    /// The content's size, its padding included, in logical pixels.
    pub content: Size,
    /// What a wheel line scrolls, in logical pixels of whole physical pixels: `w` a character
    /// (half the body size), `h` a line (its text style's line height).
    pub line: Size,
    /// The bars, where the content overflows, in window coordinates.
    pub bar_x: Option<ScrollBar>,
    pub bar_y: Option<ScrollBar>,
}

/// A viewport's scroll bar, in window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollBar {
    /// The strip along the viewport's edge that's the bar's: the pointer on it expands the bar, a
    /// press on it pages, and the thumb drags along it.
    pub track: Rect,
    /// The thumb, as painted.
    pub thumb: Rect,
    /// A bar with its track, not the indicator.
    pub expanded: bool,
}

/// A viewport's state, in the Ui's scroll table.
#[derive(Debug)]
pub(crate) struct Scroll {
    pub(crate) node: u32,
    /// Whether it scrolls along x, along y.
    pub(crate) axes: [bool; 2],
    /// Physical pixels, x then y; at most the content less the view.
    pub(crate) offset: [i64; 2],
    /// The content's size, from layout (logical).
    pub(crate) content: Size,
    /// A character's width and a line's height, from layout (logical).
    pub(crate) line: [f32; 2],
    /// Wheel travel not yet a whole pixel, per axis, in 120ths of a pixel.
    pub(crate) rest: [i64; 2],
    /// The pointer is on the bar's strip, per axis.
    pub(crate) hot: [bool; 2],
}

/// A thumb being dragged: the press's capture.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Drag {
    pub(crate) slot: u32,
    pub(crate) axis: usize,
    /// Where along the thumb it was grabbed, from the thumb's start (logical).
    pub(crate) grab: f32,
}

/// What a press on a bar's strip is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Thumb,
    Before,
    After,
}

/// `rect`'s start and length along `axis` (0 is x).
fn along(r: Rect, axis: usize) -> (f32, f32) {
    if axis == 0 { (r.x, r.w) } else { (r.y, r.h) }
}

fn coord(p: Point, axis: usize) -> f32 {
    if axis == 0 { p.x } else { p.y }
}

/// A logical length as whole physical pixels.
#[allow(
    clippy::cast_possible_truncation,
    reason = "content is far below 2^63 px; `as` saturates anyway"
)]
fn to_px(v: f32, scale: f32) -> i64 {
    (f64::from(v) * f64::from(scale)).round() as i64
}

/// Physical pixels as a logical length.
#[allow(
    clippy::cast_precision_loss,
    reason = "an offset is exact in f64 below 2^53 px"
)]
#[allow(
    clippy::cast_possible_truncation,
    reason = "a logical length; f32 keeps it to its 24 bits"
)]
fn to_logical(px: i64, scale: f32) -> f32 {
    (px as f64 / f64::from(scale)) as f32
}

impl<M> Ui<M> {
    /// Windows' wheel settings: lines a notch scrolls vertically (`SPI_GETWHEELSCROLLLINES`;
    /// [`u32::MAX`] is a page, `WHEEL_PAGESCROLL`) and characters horizontally
    /// (`SPI_GETWHEELSCROLLCHARS`). Windows' defaults, 3 and 3, until the host sets them.
    pub fn set_wheel(&mut self, lines: u32, chars: u32) {
        self.wheel = [chars, lines];
    }

    /// Windows' "Automatically hide scroll bars" (Settings › Accessibility › Visual effects): on,
    /// a bar is an indicator until the pointer is on it; off, it's always a bar. The host turns it
    /// off in high contrast too. On until the host sets it.
    pub fn set_auto_hide_scroll_bars(&mut self, on: bool) {
        if self.auto_hide_bars != on {
            self.auto_hide_bars = on;
            for s in 0..self.scrolls.len() {
                let node = self.scrolls[s].node;
                if node != NIL && self.nodes[node as usize].scroll == slot(s) {
                    self.damage_strips(node);
                }
            }
        }
    }

    // ---- the side table ----------------------------------------------------------------------

    /// A scroll slot for node `i`, a freed one if there is one.
    pub(crate) fn alloc_scroll(&mut self, i: u32, axes: [bool; 2]) -> u32 {
        let fresh = Scroll {
            node: i,
            axes,
            offset: [0; 2],
            content: Size::ZERO,
            line: [0.0; 2],
            rest: [0; 2],
            hot: [false; 2],
        };
        if let Some(s) = self.free_scrolls.pop() {
            self.scrolls[s as usize] = fresh;
            s
        } else {
            self.scrolls.push(fresh);
            slot(self.scrolls.len() - 1)
        }
    }

    pub(crate) fn release_scroll(&mut self, s: u32) {
        self.scrolls[s as usize].node = NIL;
        if self.drag.is_some_and(|d| d.slot == s) {
            self.drag = None;
        }
        self.free_scrolls.push(s);
    }

    /// Node `i`'s scroll flags are `r`: it gets a slot, gives its slot back, or keeps it with
    /// its axes updated. Returns whether a client sees a change.
    pub(crate) fn update_scroll(&mut self, i: u32, r: u8) -> bool {
        let axes = [r & rare::SCROLL_X != 0, r & rare::SCROLL_Y != 0];
        let s = self.nodes[i as usize].scroll;
        match (s == NIL, axes == [false; 2]) {
            (true, true) => false,
            (true, false) => {
                let s = self.alloc_scroll(i, axes);
                self.nodes[i as usize].scroll = s;
                self.mark_dirty(i);
                self.mark_viewports_below(i);
                true
            }
            (false, true) => {
                self.nodes[i as usize].scroll = NIL;
                self.release_scroll(s);
                self.mark_dirty(i);
                self.mark_viewports_below(i);
                true
            }
            (false, false) => {
                let sc = &mut self.scrolls[s as usize];
                if sc.axes == axes {
                    return false;
                }
                sc.axes = axes;
                for (k, on) in axes.into_iter().enumerate() {
                    if !on {
                        sc.offset[k] = 0;
                    }
                }
                self.mark_dirty(i);
                true
            }
        }
    }

    /// Node `i` became a viewport or stopped being one: its subtree's
    /// `in_viewport` follows. Each node's is set from its
    /// parent's, so a viewport further down keeps its own subtree's.
    fn mark_viewports_below(&mut self, i: u32) {
        let n = &self.nodes[i as usize];
        let held = n.in_viewport || n.scroll != NIL;
        let mut c = n.first;
        while c != NIL {
            self.nodes[c as usize].in_viewport = held;
            self.mark_viewports_below(c);
            c = self.nodes[c as usize].next;
        }
    }

    /// Whether any node is a viewport: without one, nothing below walks ancestors.
    pub(crate) fn any_scroll(&self) -> bool {
        self.scrolls.len() > self.free_scrolls.len()
    }

    // ---- geometry ----------------------------------------------------------------------------

    /// Slot `s`'s viewport in physical pixels, x then y, as it's painted.
    fn view_px(&self, s: u32) -> [i64; 2] {
        let r = self.nodes[self.scrolls[s as usize].node as usize]
            .rect
            .snap(self.scale);
        [i64::from(r.x1 - r.x0), i64::from(r.y1 - r.y0)]
    }

    /// The most slot `s` can scroll, per axis: the content less the view, 0 along an axis it
    /// doesn't scroll.
    pub(crate) fn max_px(&self, s: u32) -> [i64; 2] {
        let sc = &self.scrolls[s as usize];
        let view = self.view_px(s);
        let content = [
            to_px(sc.content.w, self.scale),
            to_px(sc.content.h, self.scale),
        ];
        [0, 1].map(|k| {
            if sc.axes[k] {
                (content[k] - view[k]).max(0)
            } else {
                0
            }
        })
    }

    /// A line (or character) of slot `s` along `axis`, in whole pixels, at least one.
    fn line_px(&self, s: u32, axis: usize) -> i64 {
        to_px(self.scrolls[s as usize].line[axis], self.scale).max(1)
    }

    /// A page of slot `s` along `axis`: the view less a line, so a line of context stays.
    fn page_px(&self, s: u32, axis: usize) -> i64 {
        let line = self.line_px(s, axis);
        (self.view_px(s)[axis] - line).max(line)
    }

    /// How far slot `s`'s content is moved, in logical pixels: minus its offset.
    pub(crate) fn content_shift(&self, s: u32) -> Point {
        let o = self.scrolls[s as usize].offset;
        Point {
            x: -to_logical(o[0], self.scale),
            y: -to_logical(o[1], self.scale),
        }
    }

    /// How far node `i` shows from where layout put it: the sum of its ancestor viewports'
    /// shifts (logical).
    pub(crate) fn shift_of(&self, i: u32) -> Point {
        let mut total = Point::default();
        if !self.nodes[i as usize].in_viewport {
            return total;
        }
        let mut up = self.nodes[i as usize].parent;
        while up != NIL {
            let slot = self.nodes[up as usize].scroll;
            if slot != NIL {
                let by = self.content_shift(slot);
                total.x += by.x;
                total.y += by.y;
            }
            up = self.nodes[up as usize].parent;
        }
        total
    }

    /// Node `i`'s rectangle in the window, where it shows: unclipped.
    pub(crate) fn window_rect(&self, i: u32) -> Rect {
        moved(self.nodes[i as usize].rect, self.shift_of(i))
    }

    /// The part of node `i` that shows in the window: its rectangle there, clipped by every
    /// viewport holding it; empty if none of it shows.
    pub(crate) fn window_clip(&self, i: u32) -> Rect {
        let mut out = self.nodes[i as usize].rect;
        if !self.nodes[i as usize].in_viewport {
            return out;
        }
        // Outward, viewport by viewport: each moves what it holds by its offset into its own
        // parent's coordinates, where its rectangle is, and clips it there.
        let mut up = self.nodes[i as usize].parent;
        while up != NIL {
            let n = &self.nodes[up as usize];
            if n.scroll != NIL {
                out = moved(out, self.content_shift(n.scroll)).intersect(n.rect);
            }
            up = n.parent;
        }
        out
    }

    /// Damages what shows of node `i`. One not laid out yet shows nowhere, and walks nothing.
    pub(crate) fn damage_view(&mut self, i: u32) {
        if self.nodes[i as usize].rect.is_empty() {
            return;
        }
        let r = self.window_clip(i);
        self.damage_rect(r);
    }

    /// Slot `s`'s bar along `axis` in its viewport's own coordinates (where layout put the
    /// viewport): the strip and the thumb, and whether it's expanded. `None` where it doesn't
    /// scroll or the content fits.
    pub(crate) fn bar_local(&self, s: u32, axis: usize) -> Option<(Rect, Rect, bool)> {
        let sc = &self.scrolls[s as usize];
        let max = self.max_px(s);
        if max[axis] == 0 {
            return None;
        }
        let v = self.nodes[sc.node as usize].rect;
        // With both bars, each stops short of the other's strip.
        let corner = if max[1 - axis] > 0 { bar::STRIP } else { 0.0 };
        let track = if axis == 1 {
            Rect::new(
                v.right() - bar::STRIP,
                v.y,
                bar::STRIP,
                (v.h - corner).max(0.0),
            )
        } else {
            Rect::new(
                v.x,
                v.bottom() - bar::STRIP,
                (v.w - corner).max(0.0),
                bar::STRIP,
            )
        };
        let expanded = !self.auto_hide_bars
            || sc.hot[axis]
            || self.drag.is_some_and(|d| d.slot == s && d.axis == axis);
        let (start, len) = along(track, axis);
        let view = to_logical(self.view_px(s)[axis], self.scale);
        let content = if axis == 0 {
            sc.content.w
        } else {
            sc.content.h
        };
        let thumb_len = (len * view / content.max(1.0)).max(bar::MIN_THUMB).min(len);
        let travel = (len - thumb_len).max(0.0);
        #[allow(
            clippy::cast_precision_loss,
            reason = "a ratio of offsets, exact enough for a position"
        )]
        let t = sc.offset[axis] as f32 / max[axis] as f32;
        let at = start + travel * t;
        let thick = if expanded { bar::BAR } else { bar::INDICATOR };
        let thumb = if axis == 1 {
            Rect::new(track.right() - bar::INSET - thick, at, thick, thumb_len)
        } else {
            Rect::new(at, track.bottom() - bar::INSET - thick, thumb_len, thick)
        };
        Some((track, thumb, expanded))
    }

    /// Slot `s`'s bar along `axis` in the window.
    fn bar(&self, s: u32, axis: usize) -> Option<ScrollBar> {
        let (track, thumb, expanded) = self.bar_local(s, axis)?;
        let d = self.shift_of(self.scrolls[s as usize].node);
        let at = |r: Rect| Rect::new(r.x + d.x, r.y + d.y, r.w, r.h);
        Some(ScrollBar {
            track: at(track),
            thumb: at(thumb),
            expanded,
        })
    }

    /// Node `i`'s scroll state, if it's a viewport.
    pub(crate) fn scroll_info(&self, i: u32) -> Option<ScrollInfo> {
        let s = self.nodes[i as usize].scroll;
        if s == NIL {
            return None;
        }
        let sc = &self.scrolls[s as usize];
        let scale = self.scale;
        Some(ScrollInfo {
            offset: Point {
                x: to_logical(sc.offset[0], scale),
                y: to_logical(sc.offset[1], scale),
            },
            offset_px: sc.offset,
            content: sc.content,
            line: Size {
                w: to_logical(self.line_px(s, 0), scale),
                h: to_logical(self.line_px(s, 1), scale),
            },
            bar_x: self.bar(s, 0),
            bar_y: self.bar(s, 1),
        })
    }

    // ---- layout ------------------------------------------------------------------------------

    /// Layout found slot `s`'s content to be `content` (logical), its children laid out: the
    /// line sizes are measured, and an offset past the new end comes back to it.
    pub(crate) fn scroll_laid_out(&mut self, s: u32, content: Size, text: &mut dyn TextSystem) {
        let i = self.scrolls[s as usize].node;
        let style = self.nodes[i as usize].text_style;
        let line = [
            style.size / 2.0,
            text.measure("", &style, f32::INFINITY).height,
        ];
        let sc = &mut self.scrolls[s as usize];
        let seen = sc.content != content;
        sc.content = content;
        sc.line = line;
        let max = self.max_px(s);
        let sc = &mut self.scrolls[s as usize];
        let mut clamped = false;
        for (offset, max) in sc.offset.iter_mut().zip(max) {
            if *offset > max {
                *offset = max;
                clamped = true;
            }
        }
        if clamped {
            self.damage_view(i);
        } else if seen {
            // A bar comes, goes or changes with the content, over children that may not have.
            self.damage_strips(i);
        }
        if seen || clamped {
            self.a11y_touch(i);
        }
    }

    /// Damages what shows of viewport `i`'s bar strips, along its right and bottom edges.
    fn damage_strips(&mut self, i: u32) {
        let at = self.window_rect(i);
        let shows = self.window_clip(i);
        let right = Rect::new(at.right() - bar::STRIP, at.y, bar::STRIP, at.h);
        let bottom = Rect::new(at.x, at.bottom() - bar::STRIP, at.w, bar::STRIP);
        self.damage_rect(right.intersect(shows));
        self.damage_rect(bottom.intersect(shows));
    }

    /// The window's scale changed: offsets keep their place in the content.
    pub(crate) fn rescale_scrolls(&mut self, from: f32, to: f32) {
        for sc in &mut self.scrolls {
            for o in &mut sc.offset {
                *o = to_px(to_logical(*o, from), to);
            }
            sc.rest = [0; 2];
        }
    }

    // ---- scrolling ---------------------------------------------------------------------------

    /// Scrolls slot `s` along `axis` to `to` physical pixels, clamped. Returns whether it moved.
    pub(crate) fn scroll_to(&mut self, s: u32, axis: usize, to: i64) -> bool {
        let to = to.clamp(0, self.max_px(s)[axis]);
        let sc = &mut self.scrolls[s as usize];
        if sc.offset[axis] == to {
            return false;
        }
        sc.offset[axis] = to;
        let i = sc.node;
        self.damage_view(i);
        self.a11y_touch(i);
        true
    }

    fn scroll_by(&mut self, s: u32, axis: usize, by: i64) -> bool {
        let at = self.scrolls[s as usize].offset[axis];
        self.scroll_to(s, axis, at.saturating_add(by))
    }

    /// A client's step: slot `s` scrolls a line (or a `page`) along `axis`, toward `dir`.
    /// Returns whether it moved.
    pub(crate) fn scroll_step(&mut self, s: u32, axis: usize, dir: i64, page: bool) -> bool {
        if !self.scrolls[s as usize].axes[axis] {
            return false;
        }
        let by = if page {
            self.page_px(s, axis)
        } else {
            self.line_px(s, axis)
        };
        self.scroll_by(s, axis, by.saturating_mul(dir.signum()))
    }

    /// A client's offset: slot `s` scrolls to `to` physical pixels, each clamped. Returns
    /// whether it moved.
    pub(crate) fn scroll_set(&mut self, s: u32, to: [i64; 2]) -> bool {
        let x = self.scroll_to(s, 0, to[0]);
        let y = self.scroll_to(s, 1, to[1]);
        x || y
    }

    /// Whether slot `s` can scroll along `axis` toward `dir` (its sign).
    fn can_move(&self, s: u32, axis: usize, dir: i64) -> bool {
        let sc = &self.scrolls[s as usize];
        sc.axes[axis]
            && match dir.signum() {
                1 => sc.offset[axis] < self.max_px(s)[axis],
                -1 => sc.offset[axis] > 0,
                _ => false,
            }
    }

    /// A wheel turned at `p`: `delta` in 120ths of a notch, positive away from the user (or to
    /// the right, `horizontal`). Returns whether a viewport took it.
    pub(crate) fn wheel(&mut self, p: Point, delta: i32, horizontal: bool) -> bool {
        if delta == 0 || !self.any_scroll() {
            return false;
        }
        let axis = usize::from(!horizontal);
        // Away from the user is up the content; a tilt right is right.
        let travel = if horizontal {
            i64::from(delta)
        } else {
            -i64::from(delta)
        };
        let (mut a, _) = self.walk_at(p, |_| {});
        while a != NIL {
            let s = self.nodes[a as usize].scroll;
            if s != NIL && self.can_move(s, axis, travel) {
                self.wheel_on(s, axis, travel);
                return true;
            }
            a = self.nodes[a as usize].parent;
        }
        false
    }

    fn wheel_on(&mut self, s: u32, axis: usize, travel: i64) {
        let lines = self.wheel[axis];
        let notch = if lines == u32::MAX {
            self.page_px(s, axis)
        } else {
            i64::from(lines).saturating_mul(self.line_px(s, axis))
        };
        let sc = &mut self.scrolls[s as usize];
        // A turn the other way drops what was left of the last.
        if sc.rest[axis].signum() * travel.signum() < 0 {
            sc.rest[axis] = 0;
        }
        let total = sc.rest[axis].saturating_add(travel.saturating_mul(notch));
        sc.rest[axis] = total % 120;
        let by = total / 120;
        if by != 0 && !self.scroll_by(s, axis, by) {
            self.scrolls[s as usize].rest[axis] = 0;
        }
        if !self.can_move(s, axis, travel) {
            self.scrolls[s as usize].rest[axis] = 0;
        }
    }

    /// The innermost viewport holding node `i` (`i` itself included if `inclusive`), as its slot.
    pub(crate) fn viewport_of(&self, i: u32, inclusive: bool) -> u32 {
        let n = &self.nodes[i as usize];
        if inclusive && n.scroll != NIL {
            return n.scroll;
        }
        if !n.in_viewport {
            return NIL;
        }
        let mut a = if inclusive {
            i
        } else {
            self.nodes[i as usize].parent
        };
        while a != NIL {
            let s = self.nodes[a as usize].scroll;
            if s != NIL {
                return s;
            }
            a = self.nodes[a as usize].parent;
        }
        NIL
    }

    /// Whether node `i` is a viewport with no tab stop inside, which makes it one itself.
    pub(crate) fn lone_viewport(&self, i: u32) -> bool {
        let n = &self.nodes[i as usize];
        n.scroll != NIL && n.enabled && !self.has_stop_below(i)
    }

    fn has_stop_below(&self, i: u32) -> bool {
        let mut c = self.nodes[i as usize].first;
        while c != NIL {
            let n = &self.nodes[c as usize];
            if n.interactive() || n.scroll != NIL || self.has_stop_below(c) {
                return true;
            }
            c = n.next;
        }
        false
    }

    /// The scroll keys, after the focused node and its group didn't use `k`: Page Up, Page Down,
    /// Ctrl+Home and Ctrl+End in the innermost viewport holding the focus; and in a focused
    /// viewport, its arrows, Home and End too. Returns whether a viewport took it.
    pub(crate) fn scroll_key(&mut self, k: KeyEvent) -> bool {
        if !k.down || self.focus == NIL || !self.any_scroll() || k.mods.alt || k.mods.win {
            return false;
        }
        let lone = self.nodes[self.focus as usize].scroll != NIL;
        let s = self.viewport_of(self.focus, true);
        if s == NIL {
            return false;
        }
        let y = usize::from(self.scrolls[s as usize].axes[1]);
        let ctrl = k.mods.ctrl;
        let end = i64::MAX / 2;
        let (axis, by) = match k.key {
            KeyCode::PageDown if !ctrl => (y, self.page_px(s, y)),
            KeyCode::PageUp if !ctrl => (y, -self.page_px(s, y)),
            KeyCode::End if ctrl || lone => (y, end),
            KeyCode::Home if ctrl || lone => (y, -end),
            KeyCode::Down if lone && !ctrl => (1, self.line_px(s, 1)),
            KeyCode::Up if lone && !ctrl => (1, -self.line_px(s, 1)),
            KeyCode::Right if lone && !ctrl => (0, self.line_px(s, 0)),
            KeyCode::Left if lone && !ctrl => (0, -self.line_px(s, 0)),
            _ => return false,
        };
        if !self.scrolls[s as usize].axes[axis] {
            return false;
        }
        self.show_focus_ring();
        self.scroll_by(s, axis, by);
        true
    }

    /// Scrolls every viewport holding node `i` so it's wholly in view, or its start is if it's
    /// bigger than the view. Returns whether it's in a viewport.
    pub(crate) fn reveal(&mut self, i: u32) -> bool {
        if i == NIL || !self.nodes[i as usize].in_viewport {
            return false;
        }
        // The node where it shows in each viewport's content, viewport by viewport outward.
        let mut target = self.nodes[i as usize].rect.snap(self.scale);
        let mut found = false;
        let mut a = self.nodes[i as usize].parent;
        while a != NIL {
            let s = self.nodes[a as usize].scroll;
            if s != NIL {
                found = true;
                let view = self.nodes[a as usize].rect.snap(self.scale);
                let sc = &self.scrolls[s as usize];
                let mut to = sc.offset;
                for (k, (t0, t1, v0, v1)) in [
                    (target.x0, target.x1, view.x0, view.x1),
                    (target.y0, target.y1, view.y0, view.y1),
                ]
                .into_iter()
                .enumerate()
                {
                    if !sc.axes[k] {
                        continue;
                    }
                    let (t0, t1, v0, v1) =
                        (i64::from(t0), i64::from(t1), i64::from(v0), i64::from(v1));
                    // Shown from `o`: [v0 + o, v1 + o) of the content.
                    let o = to[k];
                    if t1 - t0 > v1 - v0 || t0 < v0 + o {
                        to[k] = t0 - v0;
                    } else if t1 > v1 + o {
                        to[k] = t1 - v1;
                    }
                }
                self.scroll_to(s, 0, to[0]);
                self.scroll_to(s, 1, to[1]);
                let off = self.scrolls[s as usize].offset;
                // Where it now shows in this viewport's parent's content.
                let (dx, dy) = (px_i32(off[0]), px_i32(off[1]));
                target = crate::geom::RectPx {
                    x0: target.x0 - dx,
                    y0: target.y0 - dy,
                    x1: target.x1 - dx,
                    y1: target.y1 - dy,
                };
            }
            a = self.nodes[a as usize].parent;
        }
        found
    }

    // ---- the pointer -------------------------------------------------------------------------

    /// Walks from the root to the deepest node under `p` (window coordinates), calling `visit`
    /// on each, and moving `p` into each viewport's content. It stops at a viewport whose bar
    /// strip holds `p`. Returns that deepest node ([`NIL`] if `p` is outside the window) and the
    /// bar it stopped on.
    pub(crate) fn walk_at(
        &self,
        p: Point,
        mut visit: impl FnMut(u32),
    ) -> (u32, Option<(u32, usize)>) {
        let mut p = p;
        if !self.nodes[ROOT as usize].rect.contains(p) {
            return (NIL, None);
        }
        let mut at = ROOT;
        loop {
            visit(at);
            let n = &self.nodes[at as usize];
            if n.scroll != NIL {
                for axis in [1, 0] {
                    if let Some((track, _, _)) = self.bar_local(n.scroll, axis)
                        && track.contains(p)
                    {
                        return (at, Some((n.scroll, axis)));
                    }
                }
                let c = self.content_shift(n.scroll);
                p = Point {
                    x: p.x - c.x,
                    y: p.y - c.y,
                };
            }
            let mut c = n.last;
            while c != NIL && !self.nodes[c as usize].rect.contains(p) {
                c = self.nodes[c as usize].prev;
            }
            if c == NIL {
                return (at, None);
            }
            at = c;
        }
    }

    /// The pointer moved to `p`: a thumb being dragged follows it, and the bar whose strip it's
    /// on expands. Returns whether a drag took it.
    pub(crate) fn scroll_pointer(&mut self, p: Option<Point>) -> bool {
        if !self.any_scroll() {
            return false;
        }
        if let (Some(d), Some(p)) = (self.drag, p) {
            self.drag_to(d, p);
            return true;
        }
        let on = p.and_then(|p| self.walk_at(p, |_| {}).1);
        if on != self.bar_hot {
            for (s, axis) in [self.bar_hot, on].into_iter().flatten() {
                if self.scrolls[s as usize].node != NIL {
                    let hot = on == Some((s, axis));
                    self.scrolls[s as usize].hot[axis] = hot;
                    let node = self.scrolls[s as usize].node;
                    self.damage_strips(node);
                }
            }
            self.bar_hot = on;
        }
        false
    }

    /// A press at `p`: on a bar's thumb it starts a drag; elsewhere on its strip it moves a page
    /// toward `p`. Returns whether a bar took it.
    pub(crate) fn scroll_press(&mut self, p: Point) -> bool {
        if !self.any_scroll() {
            return false;
        }
        let Some((s, axis)) = self.walk_at(p, |_| {}).1 else {
            return false;
        };
        let Some(thumb) = self.bar(s, axis).map(|b| b.thumb) else {
            return false;
        };
        let (start, len) = along(thumb, axis);
        let at = coord(p, axis);
        let part = if at < start {
            Part::Before
        } else if at >= start + len {
            Part::After
        } else {
            Part::Thumb
        };
        match part {
            Part::Thumb => {
                self.drag = Some(Drag {
                    slot: s,
                    axis,
                    grab: at - start,
                });
                // A drag holds the bar expanded.
                let node = self.scrolls[s as usize].node;
                self.damage_strips(node);
            }
            Part::Before => {
                let page = self.page_px(s, axis);
                self.scroll_by(s, axis, -page);
            }
            Part::After => {
                let page = self.page_px(s, axis);
                self.scroll_by(s, axis, page);
            }
        }
        true
    }

    /// The press ended: a drag ends with it. Returns whether there was one.
    pub(crate) fn scroll_release(&mut self) -> bool {
        let Some(d) = self.drag.take() else {
            return false;
        };
        let node = self.scrolls[d.slot as usize].node;
        if node != NIL {
            self.damage_strips(node);
        }
        true
    }

    fn drag_to(&mut self, d: Drag, p: Point) {
        let Some(b) = self.bar(d.slot, d.axis) else {
            return;
        };
        let (start, len) = along(b.track, d.axis);
        let (_, thumb_len) = along(b.thumb, d.axis);
        let travel = len - thumb_len;
        if travel <= 0.0 {
            return;
        }
        let t = ((coord(p, d.axis) - d.grab - start) / travel).clamp(0.0, 1.0);
        let max = self.max_px(d.slot)[d.axis];
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "a fraction of the travel, rounded to a pixel"
        )]
        let to = (f64::from(t) * max as f64).round() as i64;
        self.scroll_to(d.slot, d.axis, to);
    }
}

/// `r` moved by `by`.
fn moved(r: Rect, by: Point) -> Rect {
    Rect::new(r.x + by.x, r.y + by.y, r.w, r.h)
}

/// A slot index as a `u32`.
fn slot(s: usize) -> u32 {
    u32::try_from(s).unwrap_or(NIL)
}

/// An offset as pixel coordinates; offsets past `i32` are clamped (a reveal past 2^31 px is
/// off the grid anyway).
fn px_i32(v: i64) -> i32 {
    i32::try_from(v).unwrap_or(if v < 0 { i32::MIN } else { i32::MAX })
}
