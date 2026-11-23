//! TD-0009 A3.7's U8 for slice 6a's composites: no allocation per frame once warm, counted exactly.
//!
//! The composites page's session (counts up, down to 0 and back, sizes arriving, a scan failing,
//! Details open and shut, the info bar gone and back, settings changed, a resize, the text size,
//! Tab through the page) runs twice to warm up, painted. A third run must neither allocate nor
//! reallocate, for the page's model as well as the Ui.
//!
//! One test only: the counting allocator is global, and `cargo test` runs tests on threads.

#![allow(missing_docs)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use std::alloc::System;

use composites::{Model, Run};
use dpro_ui::Theme;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[test]
fn a_warm_composites_session_allocates_nothing() {
    let mut run = Run::painted(Model::new(), Theme::LIGHT);
    let steps = Run::session();
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
