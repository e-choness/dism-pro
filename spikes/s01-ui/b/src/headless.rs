//! Headless gates (C1 and G1): pixel snapshots of the three screens, and the 100k-list fling
//! script on the software renderer. A `MinimalSoftwareWindow` platform, no OS window.

use std::{
    error::Error,
    fs,
    io::Write as _,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};

use scene::{Screen, State, check, data::Rows, script, stats::Summary, tokens::Theme};
use slint::{
    ComponentHandle, PhysicalSize,
    platform::{
        Platform, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType},
    },
};

use crate::{App, model};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct Headless(Rc<MinimalSoftwareWindow>);

impl Platform for Headless {
    fn create_window_adapter(
        &self,
    ) -> std::result::Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

pub struct Renderer {
    window: Rc<MinimalSoftwareWindow>,
    pub app: App,
    buf: Vec<PremultipliedRgbaColor>,
    w: u32,
    h: u32,
}

impl Renderer {
    pub fn new(rows: Rc<Rows>) -> Result<Self> {
        // NewBuffer: every frame is drawn in full, like a frame the list scrolled under.
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(Headless(window.clone())))?;
        let app = App::new()?;
        model::fill(&app, rows);
        app.show()?;
        Ok(Self {
            window,
            app,
            buf: Vec::new(),
            w: 0,
            h: 0,
        })
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.window.dispatch_event(WindowEvent::ScaleFactorChanged {
            scale_factor: scale,
        });
        self.w = (scene::layout::WIDTH * scale).round() as u32;
        self.h = (scene::layout::HEIGHT * scale).round() as u32;
        self.window.set_size(PhysicalSize::new(self.w, self.h));
        self.buf = vec![PremultipliedRgbaColor::default(); (self.w * self.h) as usize];
    }

    /// Lays out and rasterizes one full frame.
    pub fn frame(&mut self) -> bool {
        slint::platform::update_timers_and_animations();
        self.window.request_redraw();
        let (buf, w) = (&mut self.buf, self.w as usize);
        self.window.draw_if_needed(|r| {
            r.render(buf, w);
        })
    }

    /// The last frame as straight-alpha RGBA8.
    pub fn rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.buf.len() * 4);
        for p in &self.buf {
            let un = |c: u8| {
                if p.alpha == 0 {
                    0
                } else {
                    (u32::from(c) * 255 / u32::from(p.alpha)) as u8
                }
            };
            out.extend_from_slice(&[un(p.red), un(p.green), un(p.blue), p.alpha]);
        }
        out
    }
}

/// Renders every screen × theme × scale, writes the PNGs, and checks each. Returns the failures.
pub fn snapshot(dir: &Path, scales: &[f32]) -> Result<usize> {
    fs::create_dir_all(dir)?;
    let rows = Rc::new(Rows::generate(scene::data::ROWS));
    let mut r = Renderer::new(rows.clone())?;
    let mut failures = 0;
    for &scale in scales {
        r.set_scale(scale);
        for theme in Theme::ALL {
            for screen in Screen::ALL {
                let state = State::snapshot(screen, theme);
                model::apply(&r.app, &state, false);
                let drawn = r.frame();
                let rgba = r.rgba();
                let name = format!("b-{}-{}-{scale}", screen.name(), theme.name());
                let mut f = fs::File::create(dir.join(format!("{name}.png")))?;
                scene::png::write(&mut f, r.w, r.h, &rgba)?;
                let img = check::Image {
                    w: r.w,
                    h: r.h,
                    scale,
                    rgba: &rgba,
                };
                let out = check::verify(&state, &rows, img);
                let bad: Vec<_> = out.iter().filter(|o| !o.ok).collect();
                println!(
                    "snapshot {name}: {}/{} checks pass{}",
                    out.len() - bad.len(),
                    out.len(),
                    if drawn { "" } else { " (NOT DRAWN)" }
                );
                for o in &bad {
                    println!("  FAIL {}: {}", o.name, o.detail);
                }
                failures += bad.len() + usize::from(!drawn);
            }
        }
    }
    Ok(failures)
}

/// Replays the fling script on the List screen and reports frame times per scale.
pub fn bench(scales: &[f32], out: Option<&Path>) -> Result<Vec<(f32, Summary)>> {
    let rows = Rc::new(Rows::generate(scene::data::ROWS));
    let frames = script::fling(rows.len());
    let mut r = Renderer::new(rows)?;
    let mut results = Vec::new();
    let mut log = match out {
        Some(p) => Some(fs::File::create(p)?),
        None => None,
    };
    for &scale in scales {
        r.set_scale(scale);
        let state = State::new(Screen::List, Theme::Light);
        model::apply(&r.app, &state, false);
        let t = Instant::now();
        r.frame();
        let first = t.elapsed();
        // Two warm-up passes; the third is the one reported.
        let mut times = Vec::with_capacity(frames.len());
        for _ in 0..3 {
            times.clear();
            for f in &frames {
                let t = Instant::now();
                model::set_scroll(&r.app, f.scroll);
                r.frame();
                times.push(t.elapsed());
            }
        }
        let s = Summary::of(&times).ok_or("no frames")?;
        println!(
            "bench b scale {scale}: first frame {:.2} ms; fling {s}",
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
