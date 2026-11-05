//! TD-0009 §7 U2: UI-thread work per input event, without raster: dispatch → update → view →
//! reconcile → layout → damage, on a page of about 600 nodes.
//!
//! Budget: p99 ≤ 2 ms per event (doc 04 §5). C1: `docker compose run --rm bench -p dpro-ui
//! --bench interact`. Advisory timings; the gate is the instruction count in `ir.rs`.

#![allow(missing_docs)]

#[path = "fixtures/shell.rs"]
mod shell;

use std::hint::black_box;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use dpro_ui::{Input, Point, Size};
use shell::{Run, START, Shell, Step};

fn interact(c: &mut Criterion) {
    let mut g = c.benchmark_group("interact");
    let mut run = Run::with(Shell::new());
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
    g.bench_function("event_unchanged", |b| b.iter(|| run.step(black_box(outside))));

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

    // Startup's share: a new Ui, the first build and layout of every node.
    g.throughput(Throughput::Elements(1));
    g.bench_function("first_frame", |b| {
        b.iter_batched(Shell::new, |s| black_box(Run::with(s)), BatchSize::LargeInput);
    });
    g.finish();
}

criterion_group!(benches, interact);
criterion_main!(benches);
