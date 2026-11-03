//! TD-0013 §7: shaping a 100k-row list into one agent-sized page (`limit` 50, `cursor`, `fields`,
//! `filter`, `sort`, 32 KiB cap). C1: `docker compose run --rm bench -- shape_lists`.
//!
//! The baselines are the floors any shaping can't beat: serializing one 50-row page as is, one
//! pass of the filter over every row, and the unshaped tool that returns the whole list.

#![allow(missing_docs)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use dpro_mcp::shape::{ListArgs, MAX_RESULT_BYTES, Row, SortKey, page};
use serde::Serialize;

/// A row like `packages.v1`'s, the longest list the `core`/`maintain` tools return.
#[derive(Debug, Serialize)]
struct PackageRow {
    identity: String,
    state: &'static str,
    release_type: i32,
    installed: String,
    bytes: u64,
}

impl Row for PackageRow {
    const FIELDS: &'static [&'static str] =
        &["identity", "state", "release_type", "installed", "bytes"];
    const DEFAULT_FIELDS: &'static [&'static str] = &["identity", "state"];
    const SORTABLE: &'static [&'static str] = &["identity", "installed", "bytes"];

    fn filter_text(&self) -> &str {
        &self.identity
    }

    fn sort_key(&self, field: usize) -> SortKey<'_> {
        match field {
            0 => SortKey::Text(&self.identity),
            1 => SortKey::Text(&self.installed),
            _ => SortKey::Number(self.bytes),
        }
    }
}

fn rows() -> Vec<PackageRow> {
    (0..100_000_u32)
        .map(|i| PackageRow {
            identity: format!(
                "Package_for_RollupFix~31bf3856ad364e35~{}~~26100.{i}.1.10",
                if i % 3 == 0 { "arm64" } else { "amd64" }
            ),
            state: if i % 7 == 0 { "staged" } else { "installed" },
            release_type: i32::try_from(i % 12).unwrap_or(0),
            installed: format!("2026-{:02}-{:02}T10:00:00", i % 12 + 1, i % 28 + 1),
            bytes: u64::from(i) * 4096,
        })
        .collect()
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

fn baselines(c: &mut Criterion) {
    let rows = rows();
    let whole = serde_json::to_vec(&rows).unwrap_or_default();
    // One line of context for the report: what the unshaped tool would hand an agent.
    eprintln!(
        "unshaped list: {} bytes (~{} tokens at 4 bytes/token)",
        whole.len(),
        whole.len() / 4
    );
    c.bench_function("baseline_page_50", |b| {
        b.iter(|| black_box(serde_json::to_vec(black_box(&rows[..50]))));
    });
    c.bench_function("baseline_filter_scan_100k", |b| {
        b.iter(|| {
            black_box(&rows)
                .iter()
                .filter(|r| contains_ignore_ascii_case(&r.identity, "~arm64~"))
                .count()
        });
    });
    c.bench_function("baseline_unshaped_100k", |b| {
        b.iter(|| black_box(serde_json::to_vec(black_box(&rows))));
    });
}

fn shaped(c: &mut Criterion) {
    let rows = rows();
    let first = ListArgs::default();
    c.bench_function("shape_first_page_100k", |b| {
        b.iter(|| black_box(page(black_box(&rows), &first, MAX_RESULT_BYTES)));
    });
    let filtered = ListArgs {
        filter: Some("~arm64~".into()),
        sort: Some("-bytes".into()),
        fields: Some(vec!["identity".into(), "state".into()]),
        ..ListArgs::default()
    };
    c.bench_function("shape_filter_sort_project_100k", |b| {
        b.iter(|| black_box(page(black_box(&rows), &filtered, MAX_RESULT_BYTES)));
    });
    let by_name = ListArgs {
        sort: Some("-identity".into()),
        ..ListArgs::default()
    };
    c.bench_function("shape_sort_text_100k", |b| {
        b.iter(|| black_box(page(black_box(&rows), &by_name, MAX_RESULT_BYTES)));
    });
}

criterion_group!(benches, baselines, shaped);
criterion_main!(benches);
