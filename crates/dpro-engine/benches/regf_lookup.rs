//! ADR-0008: the offline identity lookups on a synthetic 40 MiB hive, the size of a real
//! `SOFTWARE` (budget: a lookup stays under 1 ms once the file is in memory; `offreg.dll`
//! baseline on G1: 239 ms including its load). Also the base block check, and a full walk as
//! the cost of a TD-0014 scan.
//! C1: `docker compose run --rm bench -- regf_lookup`.

#![allow(missing_docs)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use dpro_engine::regf::{Key, Regf, RegfError};

#[allow(dead_code, reason = "the tests use the rest of the kit")]
#[path = "../src/regf/testkit.rs"]
mod testkit;

fn walk(key: Key<'_>) -> Result<usize, RegfError> {
    let mut seen = key.values()?.len();
    for sub in key.subkeys()? {
        seen += 1 + walk(sub)?;
    }
    Ok(seen)
}

fn regf_lookup(c: &mut Criterion) {
    let image = testkit::software_like(40 << 20);
    let mut group = c.benchmark_group("regf_lookup");
    group.sample_size(20);
    group.bench_function("parse_40mb", |b| {
        b.iter_batched(
            || image.clone(),
            |image| Regf::parse(black_box(image)).map(|h| h.is_dirty()),
            criterion::BatchSize::LargeInput,
        );
    });
    let Ok(hive) = Regf::parse(image) else {
        return;
    };
    group.bench_function("current_build_40mb", |b| {
        b.iter(|| {
            let key = hive.key(black_box(r"Microsoft\Windows NT\CurrentVersion"));
            let value = key.ok().flatten()?.value("CurrentBuild").ok().flatten()?;
            value.data().ok().map(|data| data.len())
        });
    });
    group.bench_function("last_vendor_key_40mb", |b| {
        b.iter(|| {
            hive.key(black_box(r"Vendor0199\Key000000"))
                .map(|k| k.is_some())
        });
    });
    group.bench_function("full_walk_40mb", |b| {
        b.iter(|| hive.root().and_then(walk));
    });
    group.finish();
}

criterion_group!(benches, regf_lookup);
criterion_main!(benches);
