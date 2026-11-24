//! Slides (TD-0009 A3.5 addition 11): a node's paint offset animated in from a window edge over
//! `motion.drawer`, on the animation clock.
//!
//! Layout runs once, at the node's final place, and hit testing and assistive technology see it
//! there throughout; only its paint moves, its subtree with it. Each frame damages where it
//! showed and where it shows, and the last frame asks for no other (U6). A node slides when it
//! first appears, once: [`Cx::slide_in`] on a node already laid out does nothing, so a view can
//! ask on every build.

use crate::{
    Cx, Motion, NodeId, Rect, RectPx, Ui,
    input::ease_out,
    tokens::{motion::DRAWER_MS, slide::FRAME_MS},
    tree::NOT_STARTED,
};

/// The window edge a node [slides in](Cx::slide_in) from: its start (left, in a left-to-right
/// window) or its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Edge {
    /// The window's start edge: left in a left-to-right window.
    Start,
    /// The window's end edge: right in a left-to-right window.
    End,
}

/// A slide running: its node, the edge it comes from, its clock and how far it has come.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Slide {
    node: NodeId,
    edge: Edge,
    /// When it started on the animation clock; [`NOT_STARTED`] until its first tick.
    start: u64,
    /// How far it has come, 0 to 1.
    t: f32,
}

impl Motion {
    /// The drawer's slide (doc 05 §4's 180 ms), or 0 with reduced motion: *Animation effects*
    /// turn every duration on or off together, so it follows the hover fade's.
    #[must_use]
    pub const fn drawer_ms(self) -> u16 {
        if self.hover_ms == 0 { 0 } else { DRAWER_MS }
    }
}

impl<M> Cx<'_, M> {
    /// Slides node `id` in from `edge` over [`Motion::drawer_ms`]: painted moved toward that edge,
    /// coming to where layout put it. Layout runs once, at the final place; each frame damages
    /// where it was and where it is, and nothing is scheduled after the last.
    ///
    /// Only a node that appeared in this build slides, and once: on a node already laid out, or
    /// with animations off, it does nothing.
    pub fn slide_in(&mut self, id: NodeId, edge: Edge) {
        let ui = &mut *self.ui;
        let Some(i) = ui.index(id) else {
            return;
        };
        if ui.motion.drawer_ms() == 0
            || !ui.nodes[i as usize].rect.is_empty()
            || ui.slides.iter().any(|s| s.node == id)
        {
            return;
        }
        ui.slides.push(Slide {
            node: id,
            edge,
            start: NOT_STARTED,
            t: 0.0,
        });
    }
}

impl<M> Ui<M> {
    /// Advances the running slides to `now_ms`, damaging where each node showed and where it
    /// shows now. A slide's clock starts a frame before its first tick, when the frame that
    /// showed its node was painted, so its first tick already moves it. One that comes to its
    /// end is dropped, and asks for no other frame.
    pub(crate) fn tick_slides(&mut self, now_ms: u64) {
        let duration = self.motion.drawer_ms();
        let mut slot = 0;
        while slot < self.slides.len() {
            let s = self.slides[slot];
            let Some(i) = self.index(s.node) else {
                self.slides.swap_remove(slot);
                continue;
            };
            let start = if s.start == NOT_STARTED {
                now_ms.saturating_sub(u64::from(FRAME_MS))
            } else {
                s.start
            };
            let elapsed = now_ms.saturating_sub(start).min(u64::from(duration));
            let t = if duration == 0 {
                1.0
            } else {
                f32::from(u16::try_from(elapsed).unwrap_or(duration)) / f32::from(duration)
            };
            let (from, to) = (self.slide_dx(i, s.edge, s.t), self.slide_dx(i, s.edge, t));
            if from != to {
                // Where it showed and where it shows: one band, as the two overlap or touch.
                let shown = self.window_clip(i).to_px(self.scale);
                let band = RectPx {
                    x0: shown.x0.saturating_add(from.min(to)),
                    x1: shown.x1.saturating_add(from.max(to)),
                    ..shown
                };
                let window = Rect::new(0.0, 0.0, self.viewport.w, self.viewport.h);
                self.damage.add(band.intersect(window.to_px(self.scale)));
            }
            if t >= 1.0 {
                self.slides.swap_remove(slot);
            } else {
                self.slides[slot] = Slide { start, t, ..s };
                slot += 1;
            }
        }
    }

    /// Node `i`'s paint offset along x, in physical pixels, while it slides in; else 0.
    pub(crate) fn slide_offset(&self, i: u32) -> i32 {
        for s in &self.slides {
            if s.node.index == i {
                return self.slide_dx(i, s.edge, s.t);
            }
        }
        0
    }

    /// Where node `i` paints `t` of the way in from `edge`: its offset from where layout put it,
    /// in physical pixels, easing out (doc 05 §4) from just past the window's edge to 0.
    fn slide_dx(&self, i: u32, edge: Edge, t: f32) -> i32 {
        let r = self.window_rect(i);
        let distance = match edge {
            Edge::End => self.viewport.w - r.x,
            Edge::Start => -r.right(),
        };
        px((distance * (1.0 - ease_out(t)) * self.scale).round())
    }

    /// Drops the slides of nodes just freed (`forget_freed`).
    pub(crate) fn forget_freed_slides(&mut self) {
        if !self.slides.is_empty() {
            let nodes = &self.nodes;
            self.slides.retain(|s| {
                let n = &nodes[s.node.index as usize];
                n.live && n.generation == s.node.generation
            });
        }
    }
}

/// `as` from a float saturates at the integer's bounds and maps NaN to 0, which is what a pixel
/// offset wants.
#[allow(
    clippy::cast_possible_truncation,
    reason = "float-to-int `as` saturates, and NaN becomes 0"
)]
fn px(v: f32) -> i32 {
    v as i32
}
