//! The physical-pixel region to repaint and present (TD-0009 §5.2): a few rectangles, merged as
//! they come in, so the presenter makes at most [`MAX_RECTS`] blits or uploads per frame.

use crate::geom::RectPx;

/// More rectangles cost more blits than they save pixels.
pub(crate) const MAX_RECTS: usize = 8;

#[derive(Clone, Debug)]
pub(crate) struct Damage {
    rects: Vec<RectPx>,
}

impl Damage {
    pub(crate) fn new() -> Self {
        Self {
            rects: Vec::with_capacity(MAX_RECTS),
        }
    }

    pub(crate) fn rects(&self) -> &[RectPx] {
        &self.rects
    }

    pub(crate) fn clear(&mut self) {
        self.rects.clear();
    }

    /// Adds `r`. Rectangles it covers are dropped, a rectangle covering it absorbs it, and at
    /// the limit it merges into the rectangle whose union grows the least.
    pub(crate) fn add(&mut self, r: RectPx) {
        if r.is_empty() || self.rects.iter().any(|d| d.contains(r)) {
            return;
        }
        self.rects.retain(|d| !r.contains(*d));
        if self.rects.len() < MAX_RECTS {
            self.rects.push(r);
            return;
        }
        let growth = |d: &RectPx| d.union(r).area() - d.area();
        if let Some(best) = self.rects.iter_mut().min_by_key(|d| growth(d)) {
            *best = best.union(r);
        }
        // The merged rectangle may now cover others.
        self.normalize();
    }

    fn normalize(&mut self) {
        let mut i = 0;
        while i < self.rects.len() {
            let r = self.rects[i];
            let before = self.rects.len();
            let mut k = 0;
            self.rects.retain(|d| {
                let keep = k == i || !r.contains(*d);
                k += 1;
                keep
            });
            if self.rects.len() == before {
                i += 1;
            } else {
                // Rectangles before `i` may have gone; start over (at most MAX_RECTS passes).
                i = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(x0: i32, y0: i32, x1: i32, y1: i32) -> RectPx {
        RectPx { x0, y0, x1, y1 }
    }

    #[test]
    fn covered_and_covering_rects_collapse() {
        let mut d = Damage::new();
        d.add(px(0, 0, 10, 10));
        d.add(px(2, 2, 5, 5));
        d.add(RectPx::default());
        assert_eq!(d.rects(), [px(0, 0, 10, 10)]);
        d.add(px(20, 0, 30, 10));
        d.add(px(-5, -5, 50, 50));
        assert_eq!(d.rects(), [px(-5, -5, 50, 50)]);
    }

    #[test]
    fn past_the_limit_rects_merge_where_it_costs_least() {
        let mut d = Damage::new();
        for i in 0..8 {
            d.add(px(i * 100, 0, i * 100 + 10, 10));
        }
        assert_eq!(d.rects().len(), MAX_RECTS);
        // Next to the last one: merging there grows the least.
        d.add(px(712, 0, 720, 10));
        assert_eq!(d.rects().len(), MAX_RECTS);
        assert!(d.rects().contains(&px(700, 0, 720, 10)));
        let total: i64 = d.rects().iter().map(RectPx::area).sum();
        assert_eq!(total, 7 * 100 + 200);
    }

    #[test]
    fn a_merge_drops_rects_it_now_covers() {
        let (a, b) = (px(0, 0, 10, 10), px(8, 4, 12, 5));
        let mut d = Damage::new();
        d.add(a);
        d.add(b);
        for i in 0..6 {
            d.add(px(1000 + i * 100, 0, 1010 + i * 100, 10));
        }
        // Neither `a` nor the new rect covers `b`, but their union does.
        d.add(px(9, 0, 13, 10));
        assert_eq!(d.rects().len(), 7);
        assert!(d.rects().contains(&px(0, 0, 13, 10)));
        assert!(!d.rects().contains(&b));
        d.clear();
        assert_eq!(d.rects(), [] as [RectPx; 0]);
    }
}
