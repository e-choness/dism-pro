//! TD-0009 §7 U2 and U3 on a shell page of 387 to 488 nodes (`fixtures/shell.rs`):
//! - `interact`, U2: UI-thread work per input event without raster: dispatch → update → view →
//!   reconcile → layout → damage. Budget: p99 ≤ 2 ms per event (doc 04 §5).
//! - `paint`, U3 without presenting: the same events with the damage painted into a 1000 × 700
//!   canvas, and whole-window repaints. Budget: input to visual ≤ 50 ms (doc 04 §2); presenting is
//!   measured on G1.
//!
//! C1: `docker compose run --rm bench -p dpro-ui --bench interact`. Advisory timings; the gate is
//! the instruction count in `ir.rs`.

#![allow(missing_docs)]

#[path = "fixtures/shell.rs"]
mod shell;

use std::hint::black_box;

use criterion::{
    BatchSize, BenchmarkGroup, Criterion, Throughput, criterion_group, criterion_main,
    measurement::WallTime,
};
use dpro_ui::{Input, Point, Size, Theme};
use shell::{Run, START, Shell, Step};

/// The session, an unchanged event, a filter keystroke and a resize, on `run`.
fn events(g: &mut BenchmarkGroup<'_, WallTime>, run: &mut Run) {
    let steps = run.session();

    // The scripted session, reported per event.
    g.throughput(Throughput::Elements(
        u64::try_from(steps.len()).unwrap_or(u64::MAX),
    ));
    g.bench_function("session_per_event", |b| {
        b.iter(|| {
            for &s in &steps {
                run.step(black_box(s));
            }
        });
    });

    g.throughput(Throughput::Elements(1));
    // Nothing changes: the floor every event pays (view + reconcile compares).
    let outside = Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 }));
    g.bench_function("event_unchanged", |b| {
        b.iter(|| run.step(black_box(outside)));
    });

    // A keystroke refilters the table: rows move, appear and disappear. Typed, then cleared.
    g.throughput(Throughput::Elements(2));
    g.bench_function("filter_keystroke", |b| {
        b.iter(|| {
            run.step(Step::Type('c'));
            run.step(Step::ClearFilter);
        });
    });
    let wide = Step::Resize(Size { w: 1400.0, ..START });
    let narrow = Step::Resize(START);
    g.bench_function("resize", |b| {
        b.iter(|| {
            run.step(wide);
            run.step(narrow);
        });
    });
}

fn interact(c: &mut Criterion) {
    let mut g = c.benchmark_group("interact");
    events(&mut g, &mut Run::with(Shell::new()));

    // Startup's share: a new Ui, the first build and layout of every node.
    g.throughput(Throughput::Elements(1));
    g.bench_function("first_frame", |b| {
        b.iter_batched(
            Shell::new,
            |s| black_box(Run::with(s)),
            BatchSize::LargeInput,
        );
    });
    g.finish();
}

fn paint(c: &mut Criterion) {
    let mut g = c.benchmark_group("paint");
    events(&mut g, &mut Run::painted(Shell::new(), 1.0, Theme::LIGHT));

    g.throughput(Throughput::Elements(1));
    g.bench_function("first_frame", |b| {
        b.iter_batched(
            Shell::new,
            |s| black_box(Run::painted(s, 1.0, Theme::LIGHT)),
            BatchSize::LargeInput,
        );
    });
    // The whole window, as after a theme switch: 1000 × 700 pixels, then 1500 × 1050.
    for (name, scale) in [("repaint_all_100", 1.0), ("repaint_all_150", 1.5)] {
        let mut run = Run::painted(Shell::new(), scale, Theme::LIGHT);
        g.bench_function(name, |b| b.iter(|| run.repaint_all()));
    }
    g.finish();
}

criterion_group!(benches, interact, paint);
criterion_main!(benches);
