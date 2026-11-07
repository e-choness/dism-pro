//! TD-0009 §7 U8 for scrolling (A2.7, slice 5a): no allocation per frame once the window is warm,
//! counted exactly.
//!
//! The shell runs its scripted session twice to warm up, then scrolls its table back and forth
//! twice: wheel notches down and up (half notches among them), a thumb drag down and back, and
//! Ctrl+End and Ctrl+Home with row 4 focused. Doing all of it a third time must neither allocate
//! nor reallocate, through every stage of a frame, painting included.
//!
//! The table (slice 5b1) builds only the rows in view and half a view either side, so rows enter
//! and leave it on every step. On the focused row, Ctrl+End and Ctrl+Home move the current row to
//! the ends, which realizes the rows there; then Down and Up move it, and Space twice checks it
//! and clears it.
//!
//! The columns (slice 5b2): Publisher's divider dragged 120 px right in 12 frames and let go, a
//! fit of every column, which the table measures and sends back, then the columns reset.
//!
//! Its own file: the counting allocator is global, `cargo test` runs tests on threads, and
//! `alloc_per_frame.rs` holds one test.

#![allow(missing_docs)]

#[path = "../benches/fixtures/shell.rs"]
mod shell;

use std::alloc::System;

use dpro_ui::{Input, KeyCode, Mods, Point, ScrollBar, TableEvent, Theme};
use shell::{CTRL, Msg, Run, Shell, Step, key};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

/// The scroll script, from the table's top with the pointer resting on it and a row focused.
fn script(run: &Run, bar: ScrollBar) -> Vec<Step> {
    let mut steps = Vec::new();
    for delta in [-120, -120, -120, -60, -60, 120, 120, 120, 120] {
        steps.push(run.table_wheel(delta));
    }
    // The thumb, pressed at its center and dragged 300 px down in 30 frames, then back.
    let grab = bar.thumb.center();
    steps.push(Step::Input(Input::PointerMove(grab)));
    steps.push(Step::Input(Input::PointerDown(grab)));
    for k in (0..=30u8).chain((0..30u8).rev()) {
        let y = grab.y + 10.0 * f32::from(k);
        steps.push(Step::Input(Input::PointerMove(Point { y, ..grab })));
    }
    steps.push(Step::Input(Input::PointerUp(grab)));
    steps.push(Step::Input(Input::PointerMove(run.center(&Run::TABLE))));
    steps.push(key(KeyCode::End, true, CTRL));
    steps.push(key(KeyCode::End, false, CTRL));
    steps.push(key(KeyCode::Home, true, CTRL));
    steps.push(key(KeyCode::Home, false, CTRL));
    for k in [KeyCode::Down, KeyCode::Up, KeyCode::Space, KeyCode::Space] {
        steps.push(key(k, true, Mods::default()));
        steps.push(key(k, false, Mods::default()));
    }
    let at = run.divider(1);
    steps.push(Step::Input(Input::PointerMove(at)));
    steps.push(Step::Input(Input::PointerDown(at)));
    for k in 1..=12u8 {
        let x = at.x + 10.0 * f32::from(k);
        steps.push(Step::Input(Input::PointerMove(Point { x, ..at })));
    }
    steps.push(Step::Input(Input::PointerUp(Point {
        x: at.x + 120.0,
        ..at
    })));
    steps.push(Step::Update(Msg::Table(TableEvent::Fit(None))));
    steps.push(Step::Update(Msg::Table(TableEvent::ResetColumns)));
    steps.push(Step::Input(Input::PointerMove(run.center(&Run::TABLE))));
    steps
}

#[test]
fn a_warm_scroll_allocates_nothing() {
    let mut run = Run::painted(Shell::new(), 1.0, Theme::LIGHT);
    let session = run.session();
    for _ in 0..2 {
        for s in &session {
            run.step(s.clone());
        }
    }
    let mut run = run.over_table();
    let row = run.ui.find(&Run::row(4));
    run.ui.set_focus(row);
    let bar = run.table_bar().expect("the table overflows");
    let steps = script(&run, bar);
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
