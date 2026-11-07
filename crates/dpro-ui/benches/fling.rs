//! TD-0009 §7 U1: frame times of S-01's fling script on a 100k-row, 8-column table, at scales 1
//! and 1.5 (A2.7). Each frame is built, laid out and painted whole, on the software path.
//!
//! It runs the script once to warm up, then times every frame of a second run, prints p50, p95
//! and p99, and fails if p99 is over 16.6 ms (60 Hz; headless C1). Times are advisory on shared
//! runners; the instruction counts in `ir.rs` (`fling::frames`) are the gate.

#![allow(missing_docs)]

#[path = "fixtures/fling.rs"]
mod fling;

use std::{
    process::ExitCode,
    time::{Duration, Instant},
};

use fling::Fling;

/// U1's budget: a frame at 60 Hz.
const BUDGET: Duration = Duration::from_micros(16_600);

fn main() -> ExitCode {
    let mut over = false;
    for scale in [1.0, 1.5] {
        let mut f = Fling::new(scale);
        let script = f.script().frames;
        for &offset in &script {
            f.frame(offset);
        }
        let mut times: Vec<Duration> = script
            .iter()
            .map(|&offset| {
                let t = Instant::now();
                f.frame(offset);
                t.elapsed()
            })
            .collect();
        times.sort_unstable();
        let at = |p: usize| times[(times.len() * p / 100).min(times.len() - 1)];
        let (p50, p95, p99) = (at(50), at(95), at(99));
        println!(
            "fling at {scale}: {} frames, p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms (budget {:.1} ms)",
            times.len(),
            ms(p50),
            ms(p95),
            ms(p99),
            ms(BUDGET),
        );
        over |= p99 > BUDGET;
    }
    if over {
        eprintln!("U1: p99 over budget");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1_000.0
}
