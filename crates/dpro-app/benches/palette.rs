//! TD-0009 §7 U10: rank 1,000 palette entries (commands, targets, settings pages) per keystroke.
//!
//! Budget ≤ 1 ms: a share of U2's 2 ms per-event UI-thread budget (doc 04 §5).
//! C1: `docker compose run --rm bench -p dpro-app --bench palette`. Advisory timings; the gate is
//! the instruction count in `ir.rs`.

#![allow(missing_docs)]

#[path = "fixtures/palette.rs"]
mod fixtures;

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use dpro_app::palette::{Score, rank};

fn palette(c: &mut Criterion) {
    let entries = fixtures::entries();
    let mut out: Vec<(u32, Score)> = Vec::with_capacity(entries.len());
    let mut g = c.benchmark_group("palette_rank_1000");
    for q in fixtures::QUERIES {
        g.bench_with_input(BenchmarkId::from_parameter(format!("{q:?}")), q, |b, q| {
            b.iter(|| rank(black_box(q), &entries, &mut out));
        });
    }
    g.finish();
}

criterion_group!(benches, palette);
criterion_main!(benches);
