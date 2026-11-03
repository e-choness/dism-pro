//! The software canvas: tiny-skia for shapes, plus coverage-mask blits for glyphs. Every path in
//! option A draws here (WinPE, GDI, DirectComposition); only presentation differs.
//!
//! Coordinates are logical pixels; the canvas scales them and snaps edges to the physical pixel
//! grid, so hairlines and fills stay crisp at every DPI.

use scene::{layout::Rect, tokens::Rgb};
use tiny_skia::{FillRule, Mask, Paint, PathBuilder, Pixmap, Transform};

/// An integer rectangle in physical pixels (half-open).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Px {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Px {
    fn intersect(self, o: Self) -> Option<Self> {
        let r = Self {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
        };
        (r.x1 > r.x0 && r.y1 > r.y0).then_some(r)
    }

    fn contains(self, o: Self) -> bool {
        o.x0 >= self.x0 && o.y0 >= self.y0 && o.x1 <= self.x1 && o.y1 <= self.y1
    }
}

/// An 8-bit coverage mask, `w × h`, with its top-left at physical `x, y`.
#[derive(Clone, Copy)]
pub struct Coverage<'a> {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub data: &'a [u8],
}

pub struct Canvas {
    pub pix: Pixmap,
    pub scale: f32,
    clip: Option<Px>,
    /// One mask per clip rectangle in use (the list viewport), rebuilt when the canvas resizes.
    masks: Vec<(Px, Mask)>,
}

impl Canvas {
    pub fn new(w: u32, h: u32, scale: f32) -> Option<Self> {
        Some(Self {
            pix: Pixmap::new(w.max(1), h.max(1))?,
            scale,
            clip: None,
            masks: Vec::new(),
        })
    }

    pub fn width(&self) -> u32 {
        self.pix.width()
    }

    pub fn height(&self) -> u32 {
        self.pix.height()
    }

    /// Snaps a logical rectangle to physical pixels.
    pub fn px(&self, r: Rect) -> Px {
        let s = self.scale;
        Px {
            x0: (r.x * s).round() as i32,
            y0: (r.y * s).round() as i32,
            x1: (r.right() * s).round() as i32,
            y1: (r.bottom() * s).round() as i32,
        }
    }

    fn bounds(&self) -> Px {
        Px {
            x0: 0,
            y0: 0,
            x1: self.pix.width() as i32,
            y1: self.pix.height() as i32,
        }
    }

    /// Clips every later draw to `r` (logical), or lifts the clip.
    pub fn set_clip(&mut self, r: Option<Rect>) {
        self.clip = r.map(|r| self.px(r));
    }

    /// Fills the whole canvas, alpha included (`alpha` 0 leaves it transparent for Mica).
    pub fn clear(&mut self, c: Rgb, alpha: u8) {
        self.pix
            .fill(tiny_skia::Color::from_rgba8(c.0, c.1, c.2, alpha));
    }

    fn paint(c: Rgb, alpha: u8, aa: bool) -> Paint<'static> {
        let mut p = Paint::default();
        p.set_color_rgba8(c.0, c.1, c.2, alpha);
        p.anti_alias = aa;
        p
    }

    /// The effective area of a draw: the target clipped to the canvas and the clip rectangle.
    fn area(&self, target: Px) -> Option<Px> {
        let a = target.intersect(self.bounds())?;
        match self.clip {
            Some(c) => a.intersect(c),
            None => Some(a),
        }
    }

    /// A pixel-aligned rectangle fill (no mask needed: the clip is applied by intersection).
    pub fn rect(&mut self, r: Rect, c: Rgb, alpha: u8) {
        let p = self.px(r);
        self.fill_px(p, c, alpha);
    }

    pub fn fill_px(&mut self, p: Px, c: Rgb, alpha: u8) {
        let Some(a) = self.area(p) else { return };
        if let Some(r) =
            tiny_skia::Rect::from_ltrb(a.x0 as f32, a.y0 as f32, a.x1 as f32, a.y1 as f32)
        {
            self.pix.fill_rect(
                r,
                &Self::paint(c, alpha, false),
                Transform::identity(),
                None,
            );
        }
    }

    /// A one-physical-pixel horizontal line.
    pub fn hline(&mut self, x: f32, y: f32, w: f32, c: Rgb, alpha: u8) {
        let s = self.scale;
        let (x0, x1, y0) = (
            (x * s).round() as i32,
            ((x + w) * s).round() as i32,
            (y * s).round() as i32,
        );
        self.fill_px(
            Px {
                x0,
                y0,
                x1,
                y1: y0 + 1,
            },
            c,
            alpha,
        );
    }

    /// A one-physical-pixel vertical line.
    pub fn vline(&mut self, x: f32, y: f32, h: f32, c: Rgb, alpha: u8) {
        let s = self.scale;
        let (x0, y0, y1) = (
            (x * s).round() as i32,
            (y * s).round() as i32,
            ((y + h) * s).round() as i32,
        );
        self.fill_px(
            Px {
                x0,
                y0,
                x1: x0 + 1,
                y1,
            },
            c,
            alpha,
        );
    }

    /// A rounded rectangle; `radii` are top-left, top-right, bottom-right, bottom-left (logical).
    pub fn rrect(&mut self, r: Rect, radii: [f32; 4], c: Rgb, alpha: u8) {
        let p = self.px(r);
        self.rrect_px(p, radii.map(|v| v * self.scale), c, alpha);
    }

    fn rrect_px(&mut self, p: Px, radii: [f32; 4], c: Rgb, alpha: u8) {
        if self.area(p).is_none() {
            return;
        }
        let Some(path) = rounded(p, radii) else {
            return;
        };
        // tiny-skia clips to the pixmap itself; the mask is only for a clip rectangle that cuts p.
        let mask = match self.clip {
            Some(c) if !c.contains(p) => self.mask(),
            _ => None,
        };
        let paint = Self::paint(c, alpha, true);
        match mask {
            Some(i) => {
                let m = &self.masks[i].1;
                self.pix.fill_path(
                    &path,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    Some(m),
                );
            }
            None => self.pix.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            ),
        }
    }

    /// A rounded rectangle with a one-physical-pixel border: the border color, then the fill inset.
    pub fn bordered(&mut self, r: Rect, radii: [f32; 4], fill: (Rgb, u8), border: (Rgb, u8)) {
        let p = self.px(r);
        let radii = radii.map(|v| v * self.scale);
        self.rrect_px(p, radii, border.0, border.1);
        let inner = Px {
            x0: p.x0 + 1,
            y0: p.y0 + 1,
            x1: p.x1 - 1,
            y1: p.y1 - 1,
        };
        if border.1 == 255 || fill.1 == 255 {
            self.rrect_px(inner, radii.map(|v| (v - 1.0).max(0.0)), fill.0, fill.1);
        } else {
            // Translucent layers: punch the inner area out first so the border doesn't tint it.
            self.erase_px(inner, radii.map(|v| (v - 1.0).max(0.0)));
            self.rrect_px(inner, radii.map(|v| (v - 1.0).max(0.0)), fill.0, fill.1);
        }
    }

    fn erase_px(&mut self, p: Px, radii: [f32; 4]) {
        let Some(path) = rounded(p, radii) else {
            return;
        };
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 0);
        paint.blend_mode = tiny_skia::BlendMode::Source;
        paint.anti_alias = true;
        self.pix.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    /// The mask of the current clip rectangle, built once per size.
    fn mask(&mut self) -> Option<usize> {
        let clip = self.clip?;
        if let Some(i) = self.masks.iter().position(|(c, _)| *c == clip) {
            return Some(i);
        }
        let mut m = Mask::new(self.pix.width(), self.pix.height())?;
        let r = tiny_skia::Rect::from_ltrb(
            clip.x0 as f32,
            clip.y0 as f32,
            clip.x1 as f32,
            clip.y1 as f32,
        )?;
        m.fill_path(
            &PathBuilder::from_rect(r),
            FillRule::Winding,
            false,
            Transform::identity(),
        );
        self.masks.push((clip, m));
        Some(self.masks.len() - 1)
    }

    /// Blends an 8-bit coverage mask in `c` at `alpha`.
    pub fn blit(&mut self, m: &Coverage<'_>, c: Rgb, alpha: u8) {
        let Coverage {
            x,
            y,
            w,
            h,
            data: coverage,
        } = *m;
        let target = Px {
            x0: x,
            y0: y,
            x1: x + w as i32,
            y1: y + h as i32,
        };
        let Some(a) = self.area(target) else { return };
        let stride = self.pix.width() as usize;
        let data = self.pix.data_mut();
        let (cr, cg, cb, ca) = (
            u32::from(c.0),
            u32::from(c.1),
            u32::from(c.2),
            u32::from(alpha),
        );
        for py in a.y0..a.y1 {
            let src = &coverage[((py - y) as u32 * w) as usize..];
            let row = &mut data[(py as usize * stride) * 4..];
            for px in a.x0..a.x1 {
                let cov = u32::from(src[(px - x) as usize]);
                if cov == 0 {
                    continue;
                }
                let k = cov * ca / 255; // source alpha
                let inv = 255 - k;
                let d = &mut row[px as usize * 4..px as usize * 4 + 4];
                d[0] = ((cr * k + u32::from(d[0]) * inv + 127) / 255) as u8;
                d[1] = ((cg * k + u32::from(d[1]) * inv + 127) / 255) as u8;
                d[2] = ((cb * k + u32::from(d[2]) * inv + 127) / 255) as u8;
                d[3] = ((255 * k + u32::from(d[3]) * inv + 127) / 255) as u8;
            }
        }
    }

    /// Copies the pixels as premultiplied BGRA (GDI DIBs and DXGI's B8G8R8A8).
    pub fn to_bgra(&self, out: &mut [u8]) {
        let (out, _) = out.as_chunks_mut::<4>();
        let (src, _) = self.pix.data().as_chunks::<4>();
        for (d, s) in out.iter_mut().zip(src) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
    }

    /// Straight-alpha RGBA (for PNGs and the pixel checks).
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pix.data().len());
        for p in self.pix.pixels() {
            let c = p.demultiply();
            out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
        }
        out
    }
}

/// A rounded-rectangle path in physical pixels (cubic corners).
fn rounded(p: Px, radii: [f32; 4]) -> Option<tiny_skia::Path> {
    const K: f32 = 0.552_284_7; // cubic approximation of a quarter circle
    let (l, t, r, b) = (p.x0 as f32, p.y0 as f32, p.x1 as f32, p.y1 as f32);
    let max = ((r - l) / 2.0).min((b - t) / 2.0).max(0.0);
    let [tl, tr, br, bl] = radii.map(|v| v.clamp(0.0, max));
    let mut pb = PathBuilder::new();
    pb.move_to(l + tl, t);
    pb.line_to(r - tr, t);
    pb.cubic_to(r - tr + tr * K, t, r, t + tr - tr * K, r, t + tr);
    pb.line_to(r, b - br);
    pb.cubic_to(r, b - br + br * K, r - br + br * K, b, r - br, b);
    pb.line_to(l + bl, b);
    pb.cubic_to(l + bl - bl * K, b, l, b - bl + bl * K, l, b - bl);
    pb.line_to(l, t + tl);
    pb.cubic_to(l, t + tl - tl * K, l + tl - tl * K, t, l + tl, t);
    pb.close();
    pb.finish()
}
