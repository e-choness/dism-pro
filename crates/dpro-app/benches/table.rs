//! TD-0009 §7 U11: a 100k-row table (S-01's list size). Sorting and filtering run off the UI thread
//! above 5,000 rows; installing their result runs on it.
//!
//! Budgets: sort ≤ 50 ms (first sort of a column, keys included), filter ≤ 10 ms per keystroke,
//! applying a result ≤ 2 ms (doc 04 §5's per-event UI-thread share).
//! C1: `docker compose run --rm bench -p dpro-app --bench table`.

#![allow(missing_docs)]

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use dpro_app::table::{
    Cell, ColumnId, FilterIndex, Order, Query, RowKey, RowSource, SortKeys, TableModel,
};
use dpro_core::units::{Bytes, UnixMillis};

const ROWS: u32 = 100_000;
const NAME: ColumnId = ColumnId(0);
const PUBLISHER: ColumnId = ColumnId(1);
const VERSION: ColumnId = ColumnId(2);
const SIZE: ColumnId = ColumnId(3);
const INSTALLED: ColumnId = ColumnId(4);

/// Installed programs, with the Uninstaller's columns (artboard 08). Long shared prefixes
/// ("Contoso Updater Pro …") and embedded numbers are the expensive case for natural sorting.
struct Apps {
    name: Vec<String>,
    publisher: Vec<String>,
    version: Vec<String>,
}

impl Apps {
    fn new(n: u32) -> Self {
        const VENDORS: [&str; 8] = [
            "Contoso",
            "Fabrikam",
            "Northwind",
            "Tailspin",
            "Litware",
            "Adatum",
            "Woodgrove",
            "Proseware",
        ];
        const PRODUCTS: [&str; 8] = [
            "Updater Pro",
            "Toolbox",
            "Media Suite",
            "Backup Agent",
            "PDF Studio",
            "Driver Kit",
            "VPN Client",
            "Office Helper",
        ];
        let n = n as usize;
        let mut a = Self {
            name: Vec::with_capacity(n),
            publisher: Vec::with_capacity(n),
            version: Vec::with_capacity(n),
        };
        for i in 0..n {
            // Interleaved, so neither the name nor the size order follows the row order.
            let v = VENDORS[(i * 7) % 8];
            a.name.push(format!(
                "{v} {} {}",
                PRODUCTS[(i * 3) % 8],
                (i * 7919) % 100_000
            ));
            a.publisher.push(format!("{v} Corporation"));
            a.version
                .push(format!("{}.{}.{}", i % 13, i % 100, i % 997));
        }
        a
    }
}

impl RowSource for Apps {
    fn len(&self) -> u32 {
        u32::try_from(self.name.len()).unwrap_or(u32::MAX)
    }
    fn key(&self, row: u32) -> RowKey {
        RowKey(u64::from(row))
    }
    fn cell(&self, row: u32, col: ColumnId) -> Cell<'_> {
        let r = row as usize;
        match col {
            NAME => Cell::Text(&self.name[r]),
            PUBLISHER => Cell::Text(&self.publisher[r]),
            VERSION => Cell::Mono(&self.version[r]),
            SIZE => Cell::Size(Bytes(
                u64::from(row.wrapping_mul(2_654_435_761)) % 4_000_000_000,
            )),
            INSTALLED => Cell::Time(UnixMillis(1_600_000_000_000 + u64::from(row) * 60_000)),
            _ => Cell::Empty,
        }
    }
}

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
    let columns = [NAME, PUBLISHER, VERSION];
    g.bench_function("filter_index_100k", |b| {
        b.iter(|| black_box(FilterIndex::build(&apps, &columns)));
    });
    let index = FilterIndex::build(&apps, &columns);
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
