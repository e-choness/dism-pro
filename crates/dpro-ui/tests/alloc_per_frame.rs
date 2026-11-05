//! TD-0009 §7 U8: no allocation per event once the window is warm (doc 04 §5), counted exactly.
//!
//! The scripted session of `benches/interact.rs` runs twice to warm up: the arena, the free list
//! and every buffer reach their working size, and the drawer's nodes have been created once. A
//! third session must then neither allocate nor reallocate, for the app model as well as the Ui.

#![allow(missing_docs)]

#[path = "../benches/fixtures/shell.rs"]
mod shell;

use std::alloc::System;

use shell::{Run, Shell};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[test]
fn a_warm_session_allocates_nothing() {
    let mut run = Run::with(Shell::new());
    let steps = run.session();
    for _ in 0..2 {
        for &s in &steps {
            run.step(s);
        }
    }
    let region = Region::new(GLOBAL);
    for &s in &steps {
        run.step(s);
    }
    let stats = region.change();
    assert_eq!(
        (stats.allocations, stats.reallocations),
        (0, 0),
        "{} events: {stats:?}",
        steps.len()
    );
}
