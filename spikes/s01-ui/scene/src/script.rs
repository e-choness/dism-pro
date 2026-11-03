//! The 100k-list "        assert!(frames.iter().any(|f| f.scroll == max), "reaches the end");ling script" (ADR-0002 gate: p99 frame ≤ 16.6 ms on the software path): the
//! list offset for each frame at 60 Hz, replayed identically by both prototypes.

use crate::layout::{self, ROW_H};

/// One frame of the script.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub scroll: f32,
}

/// Per-frame velocity decay of a fling (≈ 0.95 per 16.7 ms, close to Windows' inertia).
const FRICTION: f32 = 0.95;
const FPS: f32 = 60.0;

struct Builder {
    out: Vec<Frame>,
    max: f32,
}

impl Builder {
    fn go(&mut self, s: f32) -> f32 {
        let s = s.clamp(0.0, self.max);
        self.out.push(Frame { scroll: s });
        s
    }

    fn fling(&mut self, start: f32, v0: f32) -> f32 {
        let (mut s, mut v) = (start, v0);
        while v.abs() > 20.0 {
            s = self.go(s + v / FPS);
            v *= FRICTION;
        }
        s
    }
}

/// Flings at increasing speed, a fling back, a scrollbar drag through the whole list (every frame
/// shows new rows), a fling from the end, Home, End, and page-downs. Deterministic.
#[must_use]
pub fn fling(rows: usize) -> Vec<Frame> {
    let mut b = Builder {
        out: Vec::new(),
        max: layout::max_scroll(rows),
    };
    let mut s = 0.0;
    for v0 in [3_000.0, 6_000.0, 12_000.0] {
        s = b.fling(s, v0);
    }
    s = b.fling(s, -8_000.0);
    // Drag the scrollbar thumb from here to the end in 1.5 s.
    let (from, max) = (s, b.max);
    for f in 1..=90 {
        s = b.go(from + (max - from) * f as f32 / 90.0);
    }
    b.fling(s, -12_000.0);
    b.go(0.0); // Home
    b.go(max); // End
    let mut s = b.go(0.0); // Home
    for _ in 0..30 {
        s = b.go(s + layout::VIEWPORT.h - ROW_H); // Page Down
    }
    b.out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::ROWS;

    #[test]
    #[allow(clippy::float_cmp, reason = "the script clamps to exactly max")]
    fn the_script_is_long_enough_and_stays_in_range() {
        let frames = fling(ROWS);
        assert!(frames.len() >= 300, "{} frames", frames.len());
        let max = layout::max_scroll(ROWS);
        assert!(frames.iter().all(|f| (0.0..=max).contains(&f.scroll)));
        assert!(frames.iter().any(|f| f.scroll == max), "reaches the end");
        assert_eq!(frames, fling(ROWS), "deterministic");
    }
}
