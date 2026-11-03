//! Headless gates: pixel snapshots of the three screens (solid mode) and the 100k-list fling
//! script, both on the software path with no window.

use std::{
    error::Error,
    fs,
    io::Write as _,
    path::Path,
    time::{Duration, Instant},
};

use scene::{Screen, State, check, data::Rows, script, stats::Summary, tokens::Theme};

use crate::{canvas::Canvas, draw, text::Text};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn canvas(scale: f32) -> Result<Canvas> {
    let (w, h) = (
        (scene::layout::WIDTH * scale).round() as u32,
        (scene::layout::HEIGHT * scale).round() as u32,
    );
    Canvas::new(w, h, scale).ok_or_else(|| "canvas allocation failed".into())
}

/// Renders every screen × theme × scale, writes the PNGs and checks each. Returns the failures.
pub fn snapshot(dir: &Path, scales: &[f32]) -> Result<usize> {
    fs::create_dir_all(dir)?;
    let rows = Rows::generate(scene::data::ROWS);
    let mut text = Text::new()?;
    let mut failures = 0;
    for &scale in scales {
        let mut c = canvas(scale)?;
        for theme in Theme::ALL {
            for screen in Screen::ALL {
                let state = State::snapshot(screen, theme);
                draw::frame(&mut c, &mut text, &state, &rows, false);
                let rgba = c.to_rgba();
                let name = format!("a-{}-{}-{scale}", screen.name(), theme.name());
                let mut f = fs::File::create(dir.join(format!("{name}.png")))?;
                scene::png::write(&mut f, c.width(), c.height(), &rgba)?;
                let img = check::Image {
                    w: c.width(),
                    h: c.height(),
                    scale,
                    rgba: &rgba,
                };
                let out = check::verify(&state, &rows, img);
                let bad: Vec<_> = out.iter().filter(|o| !o.ok).collect();
                println!(
                    "snapshot {name}: {}/{} checks pass",
                    out.len() - bad.len(),
                    out.len()
                );
                for o in &bad {
                    println!("  FAIL {}: {}", o.name, o.detail);
                }
                failures += bad.len();
            }
        }
    }
    Ok(failures)
}

/// Replays the fling script on the List screen; reports frame times per scale.
pub fn bench(scales: &[f32], out: Option<&Path>) -> Result<Vec<(f32, Summary)>> {
    let rows = Rows::generate(scene::data::ROWS);
    let frames = script::fling(rows.len());
    let mut text = Text::new()?;
    let mut log = match out {
        Some(p) => Some(fs::File::create(p)?),
        None => None,
    };
    let mut results = Vec::new();
    for &scale in scales {
        let mut c = canvas(scale)?;
        let mut state = State::new(Screen::List, Theme::Light);
        let t = Instant::now();
        draw::frame(&mut c, &mut text, &state, &rows, false);
        let first = t.elapsed();
        // Two warm-up passes; the third is the one reported.
        let mut times = Vec::with_capacity(frames.len());
        for _ in 0..3 {
            times.clear();
            for f in &frames {
                let t = Instant::now();
                state.scroll = f.scroll;
                draw::frame(&mut c, &mut text, &state, &rows, false);
                times.push(t.elapsed());
            }
        }
        let s = Summary::of(&times).ok_or("no frames")?;
        let (layouts, glyphs) = text.cached();
        println!(
            "bench a scale {scale}: first frame {:.2} ms; fling {s}; cache {layouts} layouts, {glyphs} glyphs",
            ms(first)
        );
        if let Some(f) = log.as_mut() {
            for (i, t) in times.iter().enumerate() {
                writeln!(f, "{scale}\t{i}\t{:.4}", ms(*t))?;
            }
        }
        results.push((scale, s));
    }
    Ok(results)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// The in-app acrylic gate (ADR-0002): one recomputation over a 460 × 900 px drawer region, on
/// option A's software path and with Direct2D on WARP, each against a plain fill of the region.
/// The content beneath is the List screen (its right edge, rows repeated to 900 px).
pub fn acrylic_bench(rounds: usize) -> Result<()> {
    const W: u32 = 460;
    const H: u32 = 900;
    let tint = scene::tokens::Rgb::hex(0x00F9_FAFD);
    let rows = Rows::generate(scene::data::ROWS);
    let mut text = Text::new()?;
    let mut c = canvas(1.0)?;
    draw::frame(
        &mut c,
        &mut text,
        &State::snapshot(Screen::List, Theme::Light),
        &rows,
        false,
    );
    let mut region = Canvas::new(W, H, 1.0).ok_or("canvas allocation failed")?;
    {
        let (cw, ch, w) = (c.width() as usize, c.height() as usize, W as usize);
        let src = c.pix.data();
        let dst = region.pix.data_mut();
        for y in 0..H as usize {
            let from = ((y % ch) * cw + cw - w) * 4;
            dst[y * w * 4..(y + 1) * w * 4].copy_from_slice(&src[from..from + w * 4]);
        }
    }
    let beneath = region.pix.clone();
    let r = scene::layout::Rect::new(0.0, 0.0, W as f32, H as f32);

    let (mut blur, mut fill) = (Vec::new(), Vec::new());
    for i in 0..rounds + 3 {
        region.pix.clone_from(&beneath);
        let t = Instant::now();
        crate::acrylic::apply(&mut region, r, tint, 189);
        let b = ms(t.elapsed());
        let t = Instant::now();
        region.rect(r, tint, 255);
        let f = ms(t.elapsed());
        if i >= 3 {
            blur.push(b);
            fill.push(f);
        }
    }
    let med = |v: &mut Vec<f64>| scene::stats::median(v).unwrap_or(f64::NAN);
    let (b, f) = (med(&mut blur), med(&mut fill));
    println!(
        "acrylic software {W}x{H}: {b:.3} ms; plain fill {f:.3} ms; ratio {:.2}; rounds {rounds}",
        b / f
    );

    region.pix.clone_from(&beneath);
    let mut bgra = vec![0; (W * H * 4) as usize];
    region.to_bgra(&mut bgra);
    match crate::gpu::acrylic_bench(&bgra, W, H, rounds) {
        Ok(t) => println!(
            "acrylic d2d-warp {W}x{H}: {:.3} ms; plain fill {:.3} ms; ratio {:.2}; upload {:.3} ms; rounds {rounds}",
            t.blur,
            t.fill,
            t.blur / t.fill,
            t.upload
        ),
        Err(e) => println!("acrylic d2d-warp: unavailable ({e})"),
    }
    Ok(())
}
