//! Geometry. Layout works in logical pixels (DIPs, `f32`); damage and presenting work in physical
//! pixels (`i32`) at the window's scale.

/// A position in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// A size in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    pub const ZERO: Self = Self { w: 0.0, h: 0.0 };
    pub const INFINITE: Self = Self {
        w: f32::INFINITY,
        h: f32::INFINITY,
    };
}

/// A rectangle in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    #[must_use]
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    #[must_use]
    pub fn center(&self) -> Point {
        Point {
            x: self.x + self.w / 2.0,
            y: self.y + self.h / 2.0,
        }
    }

    /// Half-open: a point on the right or bottom edge belongs to the neighbor.
    #[must_use]
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.right() && p.y < self.bottom()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    /// The part `other` shares, at its corner if they share nothing (empty).
    #[must_use]
    pub fn intersect(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let w = (self.right().min(other.right()) - x).max(0.0);
        let h = (self.bottom().min(other.bottom()) - y).max(0.0);
        Self { x, y, w, h }
    }

    /// Shrunk by `insets`, never below zero size.
    #[must_use]
    pub fn inset(&self, insets: Insets) -> Self {
        Self {
            x: self.x + insets.left,
            y: self.y + insets.top,
            w: (self.w - insets.horizontal()).max(0.0),
            h: (self.h - insets.vertical()).max(0.0),
        }
    }

    /// The physical pixels this rectangle touches at `scale`, rounded outward so that partly
    /// covered pixels are included.
    #[must_use]
    pub fn to_px(&self, scale: f32) -> RectPx {
        RectPx {
            x0: px((self.x * scale).floor()),
            y0: px((self.y * scale).floor()),
            x1: px((self.right() * scale).ceil()),
            y1: px((self.bottom() * scale).ceil()),
        }
    }

    /// The physical pixels painting fills for this rectangle at `scale`: each edge rounded to the
    /// nearest pixel boundary, so fills and hairlines are crisp. Always inside [`Rect::to_px`],
    /// which damage uses.
    #[must_use]
    pub fn snap(&self, scale: f32) -> RectPx {
        RectPx {
            x0: px((self.x * scale).round()),
            y0: px((self.y * scale).round()),
            x1: px((self.right() * scale).round()),
            y1: px((self.bottom() * scale).round()),
        }
    }
}

/// `as` from a float saturates at the integer's bounds and maps NaN to 0, which is what a pixel
/// coordinate wants.
#[allow(
    clippy::cast_possible_truncation,
    reason = "float-to-int `as` saturates, and NaN becomes 0"
)]
fn px(v: f32) -> i32 {
    v as i32
}

/// A rectangle in physical pixels, half-open: `[x0, x1) × [y0, y1)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RectPx {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl RectPx {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// The smallest rectangle holding both. An empty side is ignored.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Self {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// The overlap, possibly empty.
    #[must_use]
    pub fn intersect(self, other: Self) -> Self {
        Self {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }

    /// Whether `other` lies inside this rectangle. An empty `other` always does.
    #[must_use]
    pub fn contains(&self, other: Self) -> bool {
        other.is_empty()
            || (other.x0 >= self.x0
                && other.y0 >= self.y0
                && other.x1 <= self.x1
                && other.y1 <= self.y1)
    }

    /// Whether pixel `(x, y)` is inside.
    #[must_use]
    pub fn contains_px(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }

    /// Area in pixels; 0 when empty.
    #[must_use]
    pub fn area(&self) -> i64 {
        if self.is_empty() {
            return 0;
        }
        (i64::from(self.x1) - i64::from(self.x0)) * (i64::from(self.y1) - i64::from(self.y0))
    }
}

/// Space inside a box's edges, in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Insets {
    #[must_use]
    pub const fn all(v: f32) -> Self {
        Self {
            left: v,
            top: v,
            right: v,
            bottom: v,
        }
    }

    /// `x` left and right, `y` top and bottom.
    #[must_use]
    pub const fn xy(x: f32, y: f32) -> Self {
        Self {
            left: x,
            top: y,
            right: x,
            bottom: y,
        }
    }

    #[must_use]
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    #[must_use]
    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rects_round_outward_to_pixels() {
        let r = Rect::new(10.25, 5.5, 20.0, 10.0);
        assert_eq!(
            r.to_px(1.0),
            RectPx {
                x0: 10,
                y0: 5,
                x1: 31,
                y1: 16
            }
        );
        assert_eq!(
            r.to_px(1.5),
            RectPx {
                x0: 15,
                y0: 8,
                x1: 46,
                y1: 24
            }
        );
        let huge = Rect::new(0.0, 0.0, f32::INFINITY, f32::NAN);
        assert_eq!(huge.to_px(1.0).x1, i32::MAX);
        assert_eq!(huge.to_px(1.0).y1, 0);
    }

    #[test]
    fn snapping_rounds_edges_inside_the_damage() {
        let r = Rect::new(10.25, 5.5, 20.0, 10.0);
        assert_eq!(
            r.snap(1.5),
            RectPx {
                x0: 15,
                y0: 8,
                x1: 45,
                y1: 23
            }
        );
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            assert!(r.to_px(scale).contains(r.snap(scale)), "{scale}");
        }
        let px = RectPx {
            x0: 0,
            y0: 0,
            x1: 2,
            y1: 2,
        };
        assert!(px.contains_px(1, 0) && !px.contains_px(2, 0));
    }

    #[test]
    fn containment_is_half_open() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(r.contains(Point { x: 0.0, y: 9.99 }));
        assert!(!r.contains(Point { x: 10.0, y: 5.0 }));
        assert!(!r.contains(Point { x: 5.0, y: -0.01 }));
    }

    #[test]
    fn unions_ignore_empty_sides() {
        let a = RectPx {
            x0: 0,
            y0: 0,
            x1: 10,
            y1: 10,
        };
        let b = RectPx {
            x0: 20,
            y0: 5,
            x1: 30,
            y1: 6,
        };
        let empty = RectPx::default();
        assert_eq!(a.union(empty), a);
        assert_eq!(empty.union(b), b);
        assert_eq!(a.union(b).area(), 30 * 10);
        assert!(a.intersect(b).is_empty());
        assert!(a.union(b).contains(b) && a.contains(empty));
    }

    #[test]
    fn insets_never_make_negative_sizes() {
        let r = Rect::new(0.0, 0.0, 10.0, 4.0).inset(Insets::xy(3.0, 4.0));
        assert_eq!(r, Rect::new(3.0, 4.0, 4.0, 0.0));
        assert!(r.is_empty());
    }
}
