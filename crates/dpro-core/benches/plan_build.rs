//! TD-0001 §7.4: plan build for 10k / 100k file changes (budget ≤ 150 ms and ≤ 64 MiB at 100k,
//! sizing excluded). C1: `docker compose run --rm bench -- plan_build`.

#![allow(missing_docs, clippy::cast_possible_truncation)]

mod common;

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn plan_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("plan_build");
    for n in [10_000_usize, 100_000] {
        let paths: Vec<String> = (0..n).map(common::path).collect();
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &paths, |b, paths| {
            b.iter(|| black_box(common::file_plan(black_box(paths))));
        });
    }
    group.finish();
}

criterion_group!(benches, plan_build);
criterion_main!(benches);
