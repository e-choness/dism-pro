//! TD-0009 A3.7's U8 for slice 6e's in-app picker: no allocation per steady frame once warm,
//! counted exactly.
//!
//! The picker opens on a folder of 10,000 subfolders and lists it: that allocates, in the model
//! (the entries) as it must. Then the steady frames (a click on a row, which takes the keys from
//! the path field, a move down, a page down, the end and back, a wheel notch, the pointer over the
//! rows and gone) run twice to warm up, painted. A third run must neither allocate nor reallocate,
//! for the page's model as well as the Ui.
//!
//! One test only: the counting allocator is global, and `cargo test` runs tests on threads.

#![allow(missing_docs)]

#[path = "../benches/fixtures/picker.rs"]
mod picker;

use std::alloc::System;

use dpro_ui::Theme;
use picker::{Model, Msg, Run, Step};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[test]
fn a_warm_listed_picker_allocates_nothing_per_frame() {
    let mut run = Run::painted(Model::new(), Theme::LIGHT);
    run.step(Step::Update(Msg::Open));
    run.list_all();
    run.settle();
    let steps = Run::steady();
    for _ in 0..2 {
        for s in &steps {
            run.step(s.clone());
        }
        run.settle();
    }
    let region = Region::new(GLOBAL);
    for s in &steps {
        run.step(s.clone());
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
