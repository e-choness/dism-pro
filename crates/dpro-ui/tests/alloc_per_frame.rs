//! TD-0009 §7 U8: no allocation per frame once the window is warm (doc 04 §5), counted exactly.
//!
//! The scripted session of `benches/interact.rs` runs twice to warm up: the arena, the free list,
//! the canvas and every buffer reach their working size, and the drawer's nodes have been created
//! once. A third session must then neither allocate nor reallocate, for the app model as well as
//! the Ui, through every stage of a frame: dispatch, update, view, reconcile, layout, damage and
//! painting. That includes text input (§11 step 3): typing into the filter field, whose edits
//! reach the view-model as messages, selecting all, deleting, and the caret's timer. Only a paste
//! allocates (its text, once), and the session has none.
//!
//! One test only: the counting allocator is global, and `cargo test` runs tests on threads.

#![allow(missing_docs)]

#[path = "../benches/fixtures/shell.rs"]
mod shell;

use std::alloc::System;

use dpro_ui::Theme;
use shell::{Run, Shell};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[test]
fn a_warm_session_allocates_nothing() {
    let mut run = Run::painted(Shell::new(), 1.0, Theme::LIGHT);
    let steps = run.session();
    for _ in 0..2 {
        for s in &steps {
            run.step(s.clone());
        }
    }
    let region = Region::new(GLOBAL);
    for s in &steps {
        run.step(s.clone());
    }
    let stats = region.change();
    assert_eq!(
        (stats.allocations, stats.reallocations),
        (0, 0),
        "{} events: {stats:?}",
        steps.len()
    );
}
