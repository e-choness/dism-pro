//! TD-0001 §7.4: plan JSON serialize / deserialize at 100k changes (budget ≤ 200 ms to
//! serialize). The journal's binary format is benchmarked with it in roadmap 2.2.
//! C1: `docker compose run --rm bench -- plan_serde`.

#![allow(missing_docs, clippy::cast_possible_truncation)]

mod common;

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use dpro_core::plan::{Plan, PlanDoc};

fn plan_serde(c: &mut Criterion) {
    let n = 100_000_usize;
    let paths: Vec<String> = (0..n).map(common::path).collect();
    let plan = common::file_plan(&paths);
    let Ok(doc) = plan.to_doc() else { return };
    let Ok(json) = serde_json::to_vec(&doc) else {
        return;
    };

    let mut group = c.benchmark_group("plan_serde");
    group.throughput(Throughput::Elements(n as u64));
    group.bench_function("to_json_100k", |b| {
        b.iter(|| black_box(plan.to_doc().map(|d| serde_json::to_vec(&d))));
    });
    group.bench_function("from_json_100k", |b| {
        b.iter(|| {
            let doc: Result<PlanDoc, _> = serde_json::from_slice(black_box(&json));
            black_box(doc.map(Plan::from_doc))
        });
    });
    group.finish();
}

criterion_group!(benches, plan_serde);
criterion_main!(benches);
