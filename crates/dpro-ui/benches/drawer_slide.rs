//! TD-0009 A3.7's U16: frame times of the drawer's open, slide and close, at scales 1 and 1.5.
//! Each frame is built, laid out and painted, on the software path, on `fixtures/drawer.rs`'s page.
//!
//! It warms the page with its session, then times every frame of 20 opens and closes: the frame
//! that opens the drawer, each frame of its slide, and the frame that closes it. It prints p50,
//! p95 and p99, and fails if p99 is over 16.6 ms (U1: 60 Hz; headless C1). Times are advisory on
//! shared runners; the instruction counts in `ir.rs` (`composite_drawer::*`) are the gate, and
//! `tests/composites_drawer.rs` pins the frame and layout counts.

#![allow(missing_docs)]

#[path = "fixtures/drawer.rs"]
mod drawer;

use std::{
    process::ExitCode,
    time::{Duration, Instant},
};

use drawer::{FRAME_MS, Model, Msg, Run, Step};

/// U1's budget: a frame at 60 Hz.
const BUDGET: Duration = Duration::from_micros(16_600);
/// Opens and closes timed at each scale.
const CYCLES: usize = 20;

fn main() -> ExitCode {
    let mut over = false;
    for scale in [1.0, 1.5] {
        let mut run = Run::painted_at(Model::new(), scale).warmed();
        let mut times = Vec::with_capacity(CYCLES * 16);
        for _ in 0..CYCLES {
            times.push(timed(&mut run, Step::Update(Msg::Open)));
            while run.ui.wants_frame() {
                times.push(timed(&mut run, Step::Tick(FRAME_MS)));
            }
            times.push(timed(&mut run, Step::Update(Msg::Close)));
        }
        times.sort_unstable();
        let at = |p: usize| times[(times.len() * p / 100).min(times.len() - 1)];
        let (p50, p95, p99) = (at(50), at(95), at(99));
        eprintln!(
            "drawer slide at {scale}: {} frames, p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms (budget {:.1} ms)",
            times.len(),
            ms(p50),
            ms(p95),
            ms(p99),
            ms(BUDGET),
        );
        over |= p99 > BUDGET;
    }
    if over {
        eprintln!("U16: p99 over budget");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// How long `step` takes, through the whole frame.
fn timed(run: &mut Run, step: Step) -> Duration {
    let t = Instant::now();
    run.step(step);
    t.elapsed()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1_000.0
}
