//! TD-0009 A3.7's U8 for slice 6d's drawer: no allocation per frame once warm, counted exactly.
//!
//! The drawer page's session (open and its slide, Tab through it, a change checked, the body
//! scrolled, Esc, a sheet open and closed, the window back, open and closed again) runs twice to
//! warm up, painted. A third run must neither allocate nor reallocate, for the page's model as
//! well as the Ui: the slide's and the scope's tables keep their room.
//!
//! One test only: the counting allocator is global, and `cargo test` runs tests on threads.

#![allow(missing_docs)]

#[path = "../benches/fixtures/drawer.rs"]
mod drawer;

use std::alloc::System;

use dpro_ui::Theme;
use drawer::{Model, Run};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[test]
fn a_warm_drawer_session_allocates_nothing() {
    let mut run = Run::painted(Model::new(), Theme::LIGHT);
    let steps = Run::session();
    for _ in 0..2 {
        for s in &steps {
            run.step(*s);
        }
        run.settle();
    }
    let region = Region::new(GLOBAL);
    for s in &steps {
        run.step(*s);
    }
    run.settle();
    let stats = region.change();
    assert_eq!(
        (stats.allocations, stats.reallocations),
        (0, 0),
        "{} events: {stats:?}",
        steps.len()
    );
}
