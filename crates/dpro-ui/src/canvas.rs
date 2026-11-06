//! The software canvas (TD-0009 §5.2): premultiplied BGRA pixels, the layout of a GDI DIB section
//! and of DXGI's `B8G8R8A8`, so the presenter copies damage straight out of it.
//!
//! The UI draws only rectangles, rounded rectangles, rings (borders and focus rings) and 8-bit
//! coverage masks (glyphs and icons, from the text system), always as source-over. These few
//! shapes are rasterized here directly:
//! - Edges snap to the physical pixel grid, so fills and hairlines are crisp at every scale. Only
//!   rounded corners are anti-aliased, by each pixel center's distance to the corner's circle.
//! - The arithmetic is integer except that distance, so results are the same on every machine
//!   (T1 compares pixels exactly).
//! - Drawing never allocates (§7 U8), and every draw is clipped to the clip rectangle.

use crate::{
    geom::RectPx,
    theme::{Color, div255},
};

// Pixels are `0xAARRGGBB` words, which are B, G, R, A bytes on a little-endian machine: every
// Windows target is one.
const _: () = assert!(cfg!(target_endian = "little"));

/// A window-sized pixel buffer to paint into: premultiplied BGRA, one `u32` per pixel, rows top
/// to bottom without padding. The pixels are borrowed: a DIB section's memory, or a `Vec<u32>`.
#[derive(Debug)]
pub struct Canvas<'a> {
    px: &'a mut [u32],
    width: u32,
    height: u32,
    clip: RectPx,
}

impl<'a> Canvas<'a> {
    /// A canvas over `px`, which must hold exactly `width × height` pixels.
    #[must_use]
    pub fn new(px: &'a mut [u32], width: u32, height: u32) -> Option<Self> {
        let len = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
        let (w, h) = (i32::try_from(width).ok()?, i32::try_from(height).ok()?);
        (px.len() == len).then_some(Self {
            px,
            width,
            height,
            clip: RectPx {
                x0: 0,
                y0: 0,
                x1: w,
                y1: h,
            },
        })
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn pixels(&self) -> &[u32] {
        self.px
    }

    /// The whole canvas.
    #[must_use]
    pub fn bounds(&self) -> RectPx {
        RectPx {
            x0: 0,
            y0: 0,
            x1: i32::try_from(self.width).unwrap_or(i32::MAX),
            y1: i32::try_from(self.height).unwrap_or(i32::MAX),
        }
    }

    /// Every later draw is clipped to `r`, within the canvas.
    pub fn set_clip(&mut self, r: RectPx) {
        self.clip = r.intersect(self.bounds());
    }

    #[must_use]
    pub fn clip(&self) -> RectPx {
        self.clip
    }

    /// Sets `rect` to `color`, replacing what's there instead of blending over it: the window's
    /// background, which under glass is clear, so the backdrop shows through (ADR-0007).
    pub fn clear(&mut self, rect: RectPx, color: Color) {
        let area = rect.intersect(self.clip);
        if area.is_empty() {
            return;
        }
        let px = premultiplied(color);
        for y in area.y0..area.y1 {
            let row = self.row(y);
            row[idx(area.x0)..idx(area.x1)].fill(px);
        }
    }

    /// Fills `rect`.
    pub fn fill(&mut self, rect: RectPx, color: Color) {
        let area = rect.intersect(self.clip);
        if area.is_empty() || color.a == 0 {
            return;
        }
        for y in area.y0..area.y1 {
            let row = self.row(y);
            span(&mut row[idx(area.x0)..idx(area.x1)], color);
        }
    }

    /// Fills `rect` with its corners rounded to `radius` physical pixels.
    pub fn rrect(&mut self, rect: RectPx, radius: f32, color: Color) {
        let area = rect.intersect(self.clip);
        if area.is_empty() || color.a == 0 {
            return;
        }
        let shape = Shape::new(rect, radius);
        for y in area.y0..area.y1 {
            let corner = shape.corner_row(y);
            let row = self.row(y);
            if !corner {
                span(&mut row[idx(area.x0)..idx(area.x1)], color);
                continue;
            }
            // Corner rows: per-pixel coverage in the corner columns, a plain span between them.
            let (left, right) = (rect.x0 + shape.k, rect.x1 - shape.k);
            for x in area.x0..area.x1.min(left) {
                blend_px(&mut row[idx(x)], color, shape.coverage(x, y));
            }
            let (mid0, mid1) = (area.x0.max(left), area.x1.min(right));
            if mid0 < mid1 {
                span(&mut row[idx(mid0)..idx(mid1)], color);
            }
            for x in area.x0.max(right).max(mid0)..area.x1 {
                blend_px(&mut row[idx(x)], color, shape.coverage(x, y));
            }
        }
    }

    /// A ring `width` physical pixels wide inside the edge of `rect`, whose outer corners are
    /// rounded to `radius` (the inner ones to `radius - width`): a border, or a focus ring.
    pub fn ring(&mut self, rect: RectPx, radius: f32, width: i32, color: Color) {
        let width = width.max(1);
        let inner = RectPx {
            x0: rect.x0 + width,
            y0: rect.y0 + width,
            x1: rect.x1 - width,
            y1: rect.y1 - width,
        };
        if inner.is_empty() {
            self.rrect(rect, radius, color);
            return;
        }
        let area = rect.intersect(self.clip);
        if area.is_empty() || color.a == 0 {
            return;
        }
        let outer = Shape::new(rect, radius);
        let hole = Shape::new(inner, radius - f(width));
        for y in area.y0..area.y1 {
            if y < inner.y0 || y >= inner.y1 {
                // Above or below the hole: the outer shape's row.
                let corner = outer.corner_row(y);
                let row = self.row(y);
                for x in area.x0..area.x1 {
                    let cover = if corner { outer.coverage(x, y) } else { 255 };
                    blend_px(&mut row[idx(x)], color, cover);
                }
                continue;
            }
            // Beside the hole: outer coverage minus the hole's, in the two side bands.
            let (left, right) = (inner.x0 + hole.k, inner.x1 - hole.k);
            let row = self.row(y);
            for x in (area.x0..area.x1.min(left)).chain(area.x0.max(right).max(left)..area.x1) {
                let cut = if inner.contains_px(x, y) {
                    hole.coverage(x, y)
                } else {
                    0
                };
                blend_px(
                    &mut row[idx(x)],
                    color,
                    outer.coverage(x, y).saturating_sub(cut),
                );
            }
        }
    }

    /// Blends an 8-bit coverage mask in `color`: `data` holds rows of `width` values, with its
    /// top-left at `(left, top)`.
    pub fn mask(&mut self, left: i32, top: i32, width: u32, data: &[u8], color: Color) {
        if width == 0 || color.a == 0 {
            return;
        }
        let stride = idx_u(width);
        let target = RectPx {
            x0: left,
            y0: top,
            x1: left.saturating_add(i32::try_from(width).unwrap_or(i32::MAX)),
            y1: top.saturating_add(i32::try_from(data.len() / stride).unwrap_or(i32::MAX)),
        };
        let area = target.intersect(self.clip);
        if area.is_empty() {
            return;
        }
        for y in area.y0..area.y1 {
            let src = &data[idx(y - top) * stride..];
            let row = self.row(y);
            for x in area.x0..area.x1 {
                let cover = u32::from(src[idx(x - left)]);
                if cover != 0 {
                    blend_px(&mut row[idx(x)], color, cover);
                }
            }
        }
    }

    /// Row `y`, which the caller has clipped to the canvas.
    fn row(&mut self, y: i32) -> &mut [u32] {
        let w = idx_u(self.width);
        let start = idx(y) * w;
        &mut self.px[start..start + w]
    }
}

/// A rounded rectangle in pixels, with the band `k` wide at each edge where corners may cut it.
struct Shape {
    r: RectPx,
    radius: f32,
    /// `ceil(radius)`: rows and columns within `k` of an edge may be partly covered.
    k: i32,
}

impl Shape {
    fn new(r: RectPx, radius: f32) -> Self {
        let half = f((r.x1 - r.x0).min(r.y1 - r.y0)) / 2.0;
        let radius = radius.clamp(0.0, half.max(0.0));
        Self {
            r,
            radius,
            k: ceil(radius),
        }
    }

    fn corner_row(&self, y: i32) -> bool {
        y < self.r.y0 + self.k || y >= self.r.y1 - self.k
    }

    /// Coverage (0–255) of pixel `(x, y)` inside `r`: full away from the corners, and in a corner
    /// `radius - d + ½` of the pixel, where `d` is its center's distance to the corner's center.
    fn coverage(&self, x: i32, y: i32) -> u32 {
        let (cx, cy) = (f(x) + 0.5, f(y) + 0.5);
        let rad = self.radius;
        let ox = if cx < f(self.r.x0) + rad {
            f(self.r.x0) + rad
        } else if cx > f(self.r.x1) - rad {
            f(self.r.x1) - rad
        } else {
            return 255;
        };
        let oy = if cy < f(self.r.y0) + rad {
            f(self.r.y0) + rad
        } else if cy > f(self.r.y1) - rad {
            f(self.r.y1) - rad
        } else {
            return 255;
        };
        let d = (cx - ox).hypot(cy - oy);
        unit_to_u8((rad - d + 0.5).clamp(0.0, 1.0))
    }
}

/// Fills a run of pixels with `c` at full coverage.
fn span(run: &mut [u32], c: Color) {
    if c.a == 255 {
        run.fill(opaque(c));
    } else {
        for p in run {
            blend_px(p, c, 255);
        }
    }
}

/// An opaque color as a pixel.
fn opaque(c: Color) -> u32 {
    0xFF00_0000 | u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
}

/// Any color as a pixel: its channels scaled by its alpha.
fn premultiplied(c: Color) -> u32 {
    let a = u32::from(c.a);
    a << 24
        | div255(u32::from(c.r) * a) << 16
        | div255(u32::from(c.g) * a) << 8
        | div255(u32::from(c.b) * a)
}

/// Source-over of `c` at coverage `k` (0–255) onto the premultiplied pixel `d`.
fn blend_px(d: &mut u32, c: Color, k: u32) {
    let sa = div255(u32::from(c.a) * k);
    if sa == 0 {
        return;
    }
    if sa == 255 {
        *d = opaque(c);
        return;
    }
    let inv = 255 - sa;
    let dst = *d;
    let ch =
        |src: u8, shift: u32| div255(u32::from(src) * sa) + div255(((dst >> shift) & 0xFF) * inv);
    *d = ((sa + div255((dst >> 24) * inv)) << 24)
        | (ch(c.r, 16) << 16)
        | (ch(c.g, 8) << 8)
        | ch(c.b, 0);
}

/// A canvas coordinate as an index; callers have clipped it to the canvas, so it isn't negative.
fn idx(v: i32) -> usize {
    usize::try_from(v).unwrap_or(0)
}

fn idx_u(v: u32) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

/// A pixel coordinate as `f32`: exact up to 2²⁴, far beyond any window.
#[allow(
    clippy::cast_precision_loss,
    reason = "pixel coordinates stay far below 2^24"
)]
fn f(v: i32) -> f32 {
    v as f32
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "radii are small and finite; `as` saturates anyway"
)]
fn ceil(v: f32) -> i32 {
    v.ceil() as i32
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "`v` is clamped to [0, 1]"
)]
fn unit_to_u8(v: f32) -> u32 {
    (v * 255.0).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };

    fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> RectPx {
        RectPx { x0, y0, x1, y1 }
    }

    fn canvas(px: &mut Vec<u32>, w: u32, h: u32) -> Canvas<'_> {
        px.clear();
        px.resize(idx_u(w * h), 0);
        Canvas::new(px, w, h).unwrap()
    }

    fn at(c: &Canvas<'_>, x: u32, y: u32) -> u32 {
        c.pixels()[idx_u(y * c.width() + x)]
    }

    #[test]
    fn the_buffer_must_match_the_size() {
        let mut px = vec![0u32; 12];
        assert!(Canvas::new(&mut px, 4, 3).is_some());
        assert!(Canvas::new(&mut px, 4, 4).is_none());
        assert!(Canvas::new(&mut px, 0, 0).is_none());
        assert!(Canvas::new(&mut [], 0, 0).is_some());
    }

    #[test]
    fn fills_are_premultiplied_bgra_and_clipped() {
        let mut px = Vec::new();
        let mut c = canvas(&mut px, 4, 4);
        c.fill(rect(-5, -5, 2, 2), RED);
        assert_eq!(at(&c, 1, 1), 0xFFFF_0000);
        assert_eq!(at(&c, 2, 1), 0);
        // Half-transparent white over transparent black: premultiplied 0x80.
        c.set_clip(rect(3, 3, 9, 9));
        c.fill(rect(0, 0, 4, 4), Color::hex(0xFF_FFFF).alpha(128));
        assert_eq!(at(&c, 3, 3), 0x8080_8080);
        assert_eq!(at(&c, 2, 2), 0);
        // Over opaque red: (255 + 0) / 2 etc.
        c.set_clip(c.bounds());
        c.fill(rect(0, 0, 1, 1), Color::hex(0xFF_FFFF).alpha(128));
        assert_eq!(at(&c, 0, 0), 0xFFFF_8080);
    }

    /// The background under glass: set, premultiplied, whatever was there; clipped like a fill.
    #[test]
    fn clearing_replaces_pixels_instead_of_blending() {
        let mut px = Vec::new();
        let mut c = canvas(&mut px, 4, 4);
        c.fill(c.bounds(), RED);
        c.set_clip(rect(0, 0, 2, 4));
        c.clear(c.bounds(), Color::hex(0xFF_FFFF).alpha(0));
        assert_eq!((at(&c, 1, 3), at(&c, 2, 3)), (0, 0xFFFF_0000));
        c.clear(rect(0, 0, 1, 1), Color::hex(0xFF_FFFF).alpha(128));
        assert_eq!(at(&c, 0, 0), 0x8080_8080);
        c.clear(rect(0, 0, 1, 1), RED);
        assert_eq!(at(&c, 0, 0), 0xFFFF_0000);
    }

    #[test]
    fn rounded_corners_are_antialiased_and_symmetric() {
        let mut px = Vec::new();
        let mut c = canvas(&mut px, 20, 20);
        c.rrect(rect(2, 2, 18, 18), 6.0, RED);
        // The corner pixel is outside the circle; the middle of each edge is covered.
        assert_eq!(at(&c, 2, 2), 0);
        assert_eq!(at(&c, 10, 2), 0xFFFF_0000);
        assert_eq!(at(&c, 2, 10), 0xFFFF_0000);
        assert_eq!(at(&c, 10, 10), 0xFFFF_0000);
        let partial = at(&c, 3, 4);
        assert!(partial >> 24 > 0 && partial >> 24 < 255, "{partial:08x}");
        // Mirror images match.
        for y in 0..20 {
            for x in 0..20 {
                let p = at(&c, x, y);
                assert_eq!(p, at(&c, 19 - x, y), "({x}, {y})");
                assert_eq!(p, at(&c, x, 19 - y), "({x}, {y})");
                assert_eq!(p, at(&c, y, x), "({x}, {y})");
            }
        }
        // Nothing outside the rectangle.
        assert!((0..20).all(|i| at(&c, i, 0) == 0 && at(&c, 0, i) == 0 && at(&c, i, 19) == 0));
    }

    #[test]
    fn a_radius_larger_than_half_the_side_makes_a_pill() {
        let mut px = Vec::new();
        let mut c = canvas(&mut px, 30, 10);
        c.rrect(rect(0, 0, 30, 10), 100.0, RED);
        assert_eq!(at(&c, 15, 0), 0xFFFF_0000);
        assert_eq!(at(&c, 0, 0), 0);
        assert_eq!(at(&c, 29, 9), 0);
    }

    #[test]
    fn a_ring_leaves_its_hole_alone() {
        let mut px = Vec::new();
        let mut c = canvas(&mut px, 20, 20);
        c.fill(c.bounds(), Color::hex(0x00_00FF));
        c.ring(rect(2, 2, 18, 18), 0.0, 2, RED);
        assert_eq!(at(&c, 2, 2), 0xFFFF_0000);
        assert_eq!(at(&c, 3, 10), 0xFFFF_0000);
        assert_eq!(at(&c, 4, 4), 0xFF00_00FF, "the hole");
        assert_eq!(at(&c, 1, 1), 0xFF00_00FF, "outside");
        // Rounded: still symmetric, the hole untouched.
        let mut px2 = Vec::new();
        let mut c = canvas(&mut px2, 20, 20);
        c.ring(rect(2, 2, 18, 18), 6.0, 1, RED);
        assert_eq!(at(&c, 10, 10), 0);
        assert_eq!(at(&c, 10, 2), 0xFFFF_0000);
        assert_eq!(at(&c, 10, 3), 0);
        for y in 0..20 {
            for x in 0..20 {
                assert_eq!(at(&c, x, y), at(&c, 19 - x, 19 - y), "({x}, {y})");
            }
        }
        // A ring too thick for a hole is the filled shape.
        let mut px3 = Vec::new();
        let mut c = canvas(&mut px3, 6, 6);
        c.ring(rect(0, 0, 6, 6), 0.0, 3, RED);
        assert!(c.pixels().iter().all(|&p| p == 0xFFFF_0000));
    }

    #[test]
    fn masks_blend_by_coverage_and_clip() {
        let mut px = Vec::new();
        let mut c = canvas(&mut px, 4, 4);
        let m = [255, 0, 128, 255];
        c.set_clip(rect(0, 0, 4, 3));
        c.mask(-1, 2, 2, &m, RED);
        // Column -1 is clipped; (0, 2) has 0 coverage; row 3 is outside the clip.
        assert_eq!(at(&c, 0, 2), 0);
        assert_eq!(at(&c, 0, 3), 0);
        c.mask(1, 1, 2, &m, RED);
        assert_eq!(at(&c, 1, 1), 0xFFFF_0000);
        assert_eq!(at(&c, 2, 1), 0);
        assert_eq!(at(&c, 1, 2), 0x8080_0000);
    }
}
