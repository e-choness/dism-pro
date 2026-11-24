//! Slides (TD-0009 A3.5 addition 11): a node's paint offset animated in from a window edge over
//! `motion.drawer`, on the animation clock.
//!
//! Slice 6d's MEASURE stub: [`Cx::slide_in`] does nothing yet, so a node shows where layout put
//! it at once, as with animations off.

use crate::{Cx, Motion, NodeId, tokens::motion::DRAWER_MS};

/// The window edge a node [slides in](Cx::slide_in) from: its start (left, in a left-to-right
/// window) or its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Edge {
    Start,
    End,
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
    pub fn slide_in(&mut self, id: NodeId, edge: Edge) {
        let _ = (id, edge);
    }
}
