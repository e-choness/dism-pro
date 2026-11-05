//! TD-0009 §7 U11: a 100k-row table (S-01's list size). Sorting and filtering run off the UI thread
//! above 5,000 rows; installing their result runs on it.
//!
//! Budgets: sort ≤ 50 ms (first sort of a column, keys included), filter ≤ 10 ms per keystroke,
//! applying a result ≤ 2 ms (doc 04 §5's per-event UI-thread share).
//! C1: `docker compose run --rm bench -p dpro-app --bench table`. Advisory timings; the gate is
//! the instruction count in `ir.rs`.

#![allow(missing_docs)]

#[path = "fixtures/apps.rs"]
mod fixtures;

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use dpro_app::table::{FilterIndex, Order, Query, SortKeys, TableModel};
use fixtures::{Apps, NAME, ROWS, SEARCHED, SIZE};

fn table(c: &mut Criterion) {
    let apps = Apps::new(ROWS);
    let mut g = c.benchmark_group("table");

    // Off the UI thread: the first click on a header builds that column's keys, then sorts.
    g.bench_function("sort_text_first_100k", |b| {
        b.iter(|| black_box(SortKeys::build(&apps, NAME).order(Order::Ascending)));
    });
    let name_keys = SortKeys::build(&apps, NAME);
    g.bench_function("sort_text_again_100k", |b| {
        b.iter(|| black_box(name_keys.order(Order::Descending)));
    });
    g.bench_function("sort_size_first_100k", |b| {
        b.iter(|| black_box(SortKeys::build(&apps, SIZE).order(Order::Descending)));
    });

    // Built once per data load, like the sort keys.
    g.bench_function("filter_index_100k", |b| {
        b.iter(|| black_box(FilterIndex::build(&apps, &SEARCHED)));
    });
    let index = FilterIndex::build(&apps, &SEARCHED);
    let mut out = Vec::new();
    // The first keystroke scans every row; each later one narrows the previous result.
    let first = Query::new("c");
    g.bench_function("filter_first_keystroke_100k", |b| {
        b.iter(|| index.filter(black_box(&first), None, &mut out));
    });
    let two_terms = Query::new("contoso up");
    g.bench_function("filter_two_terms_100k", |b| {
        b.iter(|| index.filter(black_box(&two_terms), None, &mut out));
    });
    let mut previous = Vec::new();
    index.filter(&two_terms, None, &mut previous);
    let narrower = Query::new("contoso upd");
    g.bench_function("filter_narrow_100k", |b| {
        b.iter(|| {
            index.filter(
                black_box(&narrower),
                Some((&two_terms, previous.as_slice())),
                &mut out,
            );
        });
    });

    // On the UI thread: install a sort result and a filter result, recomposing the view.
    let order = name_keys.order(Order::Ascending);
    let mut matches = Vec::new();
    index.filter(&two_terms, None, &mut matches);
    let mut model = TableModel::new(ROWS);
    g.bench_function("apply_results_100k", |b| {
        b.iter_batched(
            || (order.clone(), matches.clone()),
            |(order, matches)| {
                let s = model.begin_sort();
                model.finish_sort(s, (NAME, Order::Ascending), order);
                let f = model.begin_filter();
                model.finish_filter(f, two_terms.clone(), matches);
                black_box(model.view().len())
            },
            BatchSize::LargeInput,
        );
    });
    g.finish();
}

criterion_group!(benches, table);
criterion_main!(benches);
