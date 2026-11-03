//! In-app acrylic on the software path (ADR-0007: the preview drawer). The frame under the drawer is
//! downsampled ¼, blurred with three box passes (≈ a Gaussian of σ 40 logical px, doc 05 "blur
//! 40"), upsampled bilinearly and tinted. Recomputed only when the content beneath changes; the
//! caller decides that (doc 04: 0 ms when nothing beneath changes).

use scene::{layout::Rect, tokens::Rgb};

use crate::canvas::Canvas;

/// Blurs and tints `r` (logical) of the canvas in place; `alpha` is the tint's opacity.
pub fn apply(c: &mut Canvas, r: Rect, tint: Rgb, alpha: u8) {
    let p = c.px(r);
    let (cw, ch) = (c.width() as i32, c.height() as i32);
    let (x0, y0, x1, y1) = (
        p.x0.clamp(0, cw),
        p.y0.clamp(0, ch),
        p.x1.clamp(0, cw),
        p.y1.clamp(0, ch),
    );
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    if w == 0 || h == 0 {
        return;
    }
    let stride = c.width() as usize;
    let (sw, sh) = (w.div_ceil(4), h.div_ceil(4));
    let mut small = vec![[0.0_f32; 4]; sw * sh];

    // ¼ downsample: average each 4×4 block (premultiplied, so transparent pixels weigh nothing).
    let data = c.pix.data();
    for sy in 0..sh {
        for sx in 0..sw {
            let mut acc = [0.0_f32; 4];
            let mut n = 0.0;
            for y in (sy * 4)..((sy * 4 + 4).min(h)) {
                let row = (y0 as usize + y) * stride;
                for x in (sx * 4)..((sx * 4 + 4).min(w)) {
                    let i = (row + x0 as usize + x) * 4;
                    for k in 0..4 {
                        acc[k] += f32::from(data[i + k]);
                    }
                    n += 1.0;
                }
            }
            small[sy * sw + sx] = acc.map(|v| v / n);
        }
    }

    // σ 40 logical px → 10 × scale at ¼; three box passes of width √(4σ² + 1).
    let sigma = 10.0 * c.scale;
    let radius = (((4.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0)
        .round()
        .max(1.0) as usize;
    let mut tmp = small.clone();
    for _ in 0..3 {
        box_pass(&small, &mut tmp, sw, sh, radius, true);
        box_pass(&tmp, &mut small, sw, sh, radius, false);
    }

    // Upsample (bilinear) and tint: out = blurred × (1 − a) + tint × a, premultiplied.
    let a = f32::from(alpha) / 255.0;
    let t = [
        f32::from(tint.0) * a,
        f32::from(tint.1) * a,
        f32::from(tint.2) * a,
        255.0 * a,
    ];
    let data = c.pix.data_mut();
    for y in 0..h {
        let fy = ((y as f32 + 0.5) / 4.0 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (yi, ty) = (fy.floor() as usize, fy.fract());
        let yj = (yi + 1).min(sh - 1);
        let row = (y0 as usize + y) * stride;
        for x in 0..w {
            let fx = ((x as f32 + 0.5) / 4.0 - 0.5).clamp(0.0, (sw - 1) as f32);
            let (xi, tx) = (fx.floor() as usize, fx.fract());
            let xj = (xi + 1).min(sw - 1);
            let (q00, q10, q01, q11) = (
                small[yi * sw + xi],
                small[yi * sw + xj],
                small[yj * sw + xi],
                small[yj * sw + xj],
            );
            let i = (row + x0 as usize + x) * 4;
            for k in 0..4 {
                let top = q00[k] + (q10[k] - q00[k]) * tx;
                let bottom = q01[k] + (q11[k] - q01[k]) * tx;
                let v = (top + (bottom - top) * ty) * (1.0 - a) + t[k];
                data[i + k] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// One box-blur pass (running sum, edges clamped), horizontal or vertical.
fn box_pass(
    src: &[[f32; 4]],
    dst: &mut [[f32; 4]],
    w: usize,
    h: usize,
    r: usize,
    horizontal: bool,
) {
    let (lines, len) = if horizontal { (h, w) } else { (w, h) };
    let at = |line: usize, i: usize| {
        if horizontal {
            line * w + i
        } else {
            i * w + line
        }
    };
    let norm = 1.0 / (2 * r + 1) as f32;
    for line in 0..lines {
        let mut sum = [0.0_f32; 4];
        for j in 0..=2 * r {
            let s = src[at(line, j.saturating_sub(r).min(len - 1))];
            for k in 0..4 {
                sum[k] += s[k];
            }
        }
        for i in 0..len {
            dst[at(line, i)] = sum.map(|v| v * norm);
            let out = src[at(line, i.saturating_sub(r))];
            let inn = src[at(line, (i + r + 1).min(len - 1))];
            for k in 0..4 {
                sum[k] += inn[k] - out[k];
            }
        }
    }
}
