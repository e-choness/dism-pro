//! TD-0001 §7.4: journal append + flush for 100k changes with a fake fsync cost model.
//! Budget: ≤ 500 syncs per 100k changes; journaling ≤ 2 % over a raw rename loop is a
//! ratio gate measured on G1 (`xtask perf quarantine-100k`), not here.
//! C1: `docker compose run --rm bench -- journal_append`.

#![allow(missing_docs, clippy::cast_possible_truncation)]

use std::{
    hint::black_box,
    io,
    time::{Duration, Instant},
};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use dpro_core::{
    cancel::CancelToken,
    id::{JobId, TargetId},
    plan::{Change, ChangeKind, OsBuild, Reversibility},
    units::{Bytes, UnixMillis},
    OsError,
};
use dpro_engine::{
    job::{self, Backend, MonotonicClock, RunConfig},
    journal::{Begin, JournalSink, JournalWriter},
};

/// Buffers bytes; each sync spins for `sync_cost` to model `FlushFileBuffers`.
struct CostedSink {
    bytes: Vec<u8>,
    sync_cost: Duration,
}

impl JournalSink for CostedSink {
    fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn sync(&mut self) -> io::Result<()> {
        let start = Instant::now();
        while start.elapsed() < self.sync_cost {
            std::hint::spin_loop();
        }
        Ok(())
    }
}

/// Applies nothing: isolates the engine's own cost.
struct NullBackend;

impl Backend for NullBackend {
    fn capture_inverse(&mut self, change: &Change<String>) -> Result<Option<ChangeKind<String>>, OsError> {
        Ok(match &change.kind {
            ChangeKind::FileQuarantine { path, .. } => {
                Some(ChangeKind::FileRestore { quarantined: format!("$q\\{path}"), path: path.clone() })
            }
            _ => None,
        })
    }

    fn apply(&mut self, _change: &Change<String>) -> Result<(), OsError> {
        Ok(())
    }
}

fn changes(n: usize) -> Vec<Change<String>> {
    (0..n)
        .map(|i| Change {
            kind: ChangeKind::FileQuarantine {
                path: format!(r"C:\Users\someone\AppData\Local\Temp\{:04}\pkg\{i:08}\cache.tmp", i % 1000),
                bytes: Bytes(4096),
                unique_bytes: Bytes(4096),
            },
            reversibility: Reversibility::ReversibleUntil { until: UnixMillis(u64::MAX) },
            reboot_required: false,
        })
        .collect()
}

fn begin() -> Begin {
    Begin {
        job: JobId::from_parts(UnixMillis(1), 1),
        target: TargetId([1; 16]),
        os_build: OsBuild { major: 10, minor: 0, build: 26100, revision: 0 },
        created: UnixMillis(1),
    }
}

fn journal_append(c: &mut Criterion) {
    let n = 100_000;
    let plan = changes(n);
    let mut group = c.benchmark_group("journal_append");
    group.throughput(Throughput::Elements(n as u64));
    group.sample_size(10);
    // 0 µs isolates CPU cost; 200 µs is a plausible SSD flush (to be replaced by the
    // FlushFileBuffers latency measured in W1/G1, TD-0001 §7.1).
    for sync_us in [0_u64, 200] {
        group.bench_with_input(BenchmarkId::new("100k_sync_us", sync_us), &sync_us, |b, &sync_us| {
            b.iter(|| {
                let sink = CostedSink { bytes: Vec::with_capacity(32 << 20), sync_cost: Duration::from_micros(sync_us) };
                let result = JournalWriter::create(sink, &begin()).and_then(|journal| {
                    job::run(
                        black_box(&plan),
                        &mut NullBackend,
                        journal,
                        &CancelToken::new(),
                        &MonotonicClock::new(),
                        RunConfig::default(),
                    )
                });
                black_box(result.map(|(summary, _)| summary.syncs))
            });
        });
    }
    group.finish();
}

criterion_group!(benches, journal_append);
criterion_main!(benches);
