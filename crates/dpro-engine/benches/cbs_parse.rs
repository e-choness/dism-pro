//! TD-0006 §7: CBS.log parsing vs a raw line read of the same bytes, in the same run
//! (ratio gate: parse ≤ 1.5× raw). Synthetic 20 MB log, ~1 finding per 1000 lines.
//! C1: `docker compose run --rm bench -- cbs_parse`.

#![allow(missing_docs)]

use std::{
    hint::black_box,
    io::{BufRead, BufReader},
};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use dpro_core::health::LogFile;
use dpro_engine::health::logs;

fn synthetic_log(bytes: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes + 256);
    let mut i = 0_u64;
    while out.len() < bytes {
        let line = if i % 1000 == 999 {
            format!("2026-10-01 12:{:02}:{:02}, Error                 CBS    Failed to resolve package [HRESULT = 0x800f081f - CBS_E_SOURCE_MISSING]\r\n", (i / 60) % 60, i % 60)
        } else {
            format!("2026-10-01 12:{:02}:{:02}, Info                  CBS    Exec: Processing package Package_for_RollupFix~31bf3856ad364e35~amd64~~26100.{i}.1.10\r\n", (i / 60) % 60, i % 60)
        };
        out.extend_from_slice(line.as_bytes());
        i += 1;
    }
    out
}

fn cbs_parse(c: &mut Criterion) {
    let log = synthetic_log(20 << 20);
    let mut group = c.benchmark_group("cbs_parse");
    group.throughput(Throughput::Bytes(log.len() as u64));
    group.sample_size(10);
    group.bench_function("raw_read_line_20mb", |b| {
        b.iter(|| {
            let mut r = BufReader::new(black_box(log.as_slice()));
            let mut line = String::new();
            let mut n = 0_u64;
            while r.read_line(&mut line).unwrap_or(0) > 0 {
                n += 1;
                line.clear();
            }
            black_box(n)
        });
    });
    group.bench_function("scan_20mb", |b| {
        b.iter(|| black_box(logs::scan(black_box(log.as_slice()), LogFile::Cbs).map(|f| f.len())));
    });
    group.finish();
}

criterion_group!(benches, cbs_parse);
criterion_main!(benches);
