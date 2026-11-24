//! TD-0009 §7 U8 for grouped, two-line rows (A3.7's U18, slice 6b): no allocation per frame once
//! the window is warm, counted exactly.
//!
//! U18's table (`fixtures/fling.rs`: 100k two-line rows in 1,000 groups) plays part of S-01's
//! script twice to warm up: its first fling, 30 frames of its thumb drag, and Home, End and Home.
//! Rows and group headers enter and leave on every frame. Playing it a third time must neither
//! allocate nor reallocate, through every stage of a frame, painting included.
//!
//! Its own file: the counting allocator is global, and `cargo test` runs tests on threads.

#![allow(missing_docs)]

#[path = "../benches/fixtures/fling.rs"]
mod fling;

use std::alloc::System;

use fling::Grouped;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[test]
fn a_warm_grouped_fling_allocates_nothing() {
    let mut f = Grouped::new(1.0);
    let script = f.script();
    let max = script.frames.iter().copied().fold(0.0, f32::max);
    let mut frames: Vec<f32> = script.frames.iter().take(60).copied().collect();
    frames.extend(script.frames.iter().skip(script.drag).take(30));
    frames.extend([0.0, max, 0.0]);
    for _ in 0..2 {
        for &offset in &frames {
            f.frame(offset);
        }
    }
    let region = Region::new(GLOBAL);
    for &offset in &frames {
        f.frame(offset);
    }
    let stats = region.change();
    assert_eq!(
        (stats.allocations, stats.reallocations),
        (0, 0),
        "{} frames: {stats:?}",
        frames.len()
    );
}
