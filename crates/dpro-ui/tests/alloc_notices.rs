//! TD-0009 A4.7 U22: queuing a notice allocates nothing once warm (doc 04 §5), counted exactly.
//!
//! With a client listening, the shell's drawer job crosses a step: the announcer speaks, the shell
//! writes the notice's text into its buffer, the Ui copies it into its queue, sanitized and
//! capped, and the platform drains it. The frame around it is U8's (`alloc_per_frame.rs`), which
//! runs without a client: a client's own tree updates allocate the nodes they send. So this counts
//! the notice's path alone, after two rounds of it have warmed every buffer.
//!
//! One test only: the counting allocator is global, and `cargo test` runs tests on threads.

#![allow(missing_docs)]

#[path = "../benches/fixtures/shell.rs"]
mod shell;

use std::alloc::System;

use dpro_app::announce::JOB_GAP_MS;
use shell::{Msg, Run, Shell, Step};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

/// A progress report crossing a step at `at` ms, its notice queued and drained as the platform
/// does, then the job back at 0, a new phase, so the next round speaks again. Returns the bytes
/// heard.
fn round(run: &mut Run, at: u64) -> usize {
    let mut heard = 0;
    run.shell.update(Msg::Progress(41));
    let _ = run.shell.job_wake(at);
    let ui = &mut run.ui;
    run.shell.notices(|n| ui.notify(n));
    run.ui.take_notices(|n| heard += n.text.len());
    run.shell.update(Msg::Progress(0));
    let _ = run.shell.job_wake(at + 1);
    heard
}

#[test]
fn queuing_a_notice_allocates_nothing_once_warm() {
    let mut run = Run::accessible(Shell::new());
    for s in run.session() {
        run.step(s);
    }
    // The drawer opens: its job starts now.
    run.step(Step::Update(Msg::Drawer));
    let start = run.now();
    for k in 1..=2 {
        assert!(round(&mut run, start + k * JOB_GAP_MS) > 0, "warm-up {k}");
    }
    let region = Region::new(GLOBAL);
    let heard = round(&mut run, start + 3 * JOB_GAP_MS);
    let stats = region.change();
    assert!(heard > 0, "the measured round spoke");
    assert_eq!(
        (stats.allocations, stats.reallocations),
        (0, 0),
        "{stats:?}"
    );
}
