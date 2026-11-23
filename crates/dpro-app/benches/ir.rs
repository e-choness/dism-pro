//! TD-0009 §7 U10 and U11 as instruction counts: the deterministic gate (doc 04 §3).
//!
//! gungraun runs each benchmark once under Callgrind and counts the instructions of the benchmark
//! function alone. Building the fixture (`setup`) and dropping what the function returns aren't
//! counted. The inputs are the criterion benches' (`palette.rs`, `table.rs`), whose timings stay
//! advisory.
//!
//! Setups keep everything they build alive, and benchmarks return it, so nothing is freed before
//! the count starts. Otherwise the benchmark's first large allocation pays for glibc merging the
//! freed chunks (`malloc_consolidate`): 21.9 M of the 25.4 M instructions first counted for
//! `apply`, whose setup had dropped the 100k rows.
//!
//! A refresh (TD-0009 A2.5, written before the code) is the new data's key map, built off the UI
//! thread, then installed on it: the checks, the current row and the Shift anchor carried over by
//! key, the view kept in its order.
//!
//! C1: `docker compose run --rm bench-gate` fails when a count is more than 5 % over
//! `benches/baseline.json`; `bench-gate -- --update` rewrites that file from the run.

#![allow(missing_docs)]

#[path = "fixtures/apps.rs"]
mod apps;
#[path = "fixtures/palette.rs"]
mod commands;

use std::hint::black_box;

use apps::{Apps, NAME, ROWS, SEARCHED, SIZE};
use dpro_app::{
    palette::{PaletteEntry, Score, rank},
    table::{
        Cell, ColumnId, FilterIndex, Order, Pick, Query, RowKey, RowMap, RowSource, SortKeys,
        TableModel,
    },
};
use gungraun::{
    Callgrind, LibraryBenchmarkConfig, library_benchmark, library_benchmark_group, main,
};

/// U10: the palette and one query, with the result buffer the palette keeps between keystrokes.
struct Palette {
    entries: Vec<PaletteEntry>,
    out: Vec<(u32, Score)>,
    query: &'static str,
}

fn palette(query: &'static str) -> Palette {
    // Keeps these benches on the criterion bench's queries.
    assert!(
        commands::QUERIES.contains(&query),
        "{query:?} isn't in fixtures/palette.rs QUERIES"
    );
    let entries = commands::entries();
    Palette {
        out: Vec::with_capacity(entries.len()),
        entries,
        query,
    }
}

#[library_benchmark]
#[bench::empty(args = (""), setup = palette)]
#[bench::r(args = ("r"), setup = palette)]
#[bench::re(args = ("re"), setup = palette)]
#[bench::rem(args = ("rem"), setup = palette)]
#[bench::remove(args = ("remove"), setup = palette)]
#[bench::remove_p(args = ("remove p"), setup = palette)]
#[bench::initials(args = ("rp"), setup = palette)]
#[bench::keyword(args = ("servicing"), setup = palette)]
#[bench::no_match(args = ("zqx"), setup = palette)]
fn keystroke(mut p: Palette) -> Palette {
    rank(black_box(p.query), &p.entries, &mut p.out);
    p
}

fn apps(col: ColumnId, order: Order) -> (Apps, ColumnId, Order) {
    (Apps::new(ROWS), col, order)
}

// U11 off the UI thread: the first click on a header builds the column's keys, then sorts.
#[library_benchmark]
#[bench::text(args = (NAME, Order::Ascending), setup = apps)]
#[bench::size(args = (SIZE, Order::Descending), setup = apps)]
fn sort_first((apps, col, order): (Apps, ColumnId, Order)) -> (Apps, SortKeys, Vec<u32>) {
    let keys = SortKeys::build(black_box(&apps), col);
    let rows = keys.order(order);
    (apps, keys, rows)
}

fn name_keys() -> (Apps, SortKeys) {
    let apps = Apps::new(ROWS);
    let keys = SortKeys::build(&apps, NAME);
    (apps, keys)
}

// A later click on the same header reuses the keys.
#[library_benchmark]
#[bench::text(setup = name_keys)]
fn sort_again((apps, keys): (Apps, SortKeys)) -> (Apps, SortKeys, Vec<u32>) {
    let rows = black_box(&keys).order(Order::Descending);
    (apps, keys, rows)
}

fn all_apps() -> Apps {
    Apps::new(ROWS)
}

// Built once per data load, like the sort keys.
#[library_benchmark]
#[bench::three_columns(setup = all_apps)]
fn filter_index(apps: Apps) -> (Apps, FilterIndex) {
    let index = FilterIndex::build(black_box(&apps), &SEARCHED);
    (apps, index)
}

/// U11 filtering: a fresh query scans every row; a narrower one rechecks the previous matches.
struct Filter {
    index: FilterIndex,
    query: Query,
    previous: Option<(Query, Vec<u32>)>,
    out: Vec<u32>,
    _apps: Apps,
}

fn filter(query: &str, previous: Option<&str>) -> Filter {
    let apps = Apps::new(ROWS);
    let index = FilterIndex::build(&apps, &SEARCHED);
    let previous = previous.map(|q| {
        let q = Query::new(q);
        let mut rows = Vec::with_capacity(ROWS as usize);
        index.filter(&q, None, &mut rows);
        (q, rows)
    });
    Filter {
        index,
        query: Query::new(query),
        previous,
        out: Vec::with_capacity(ROWS as usize),
        _apps: apps,
    }
}

#[library_benchmark]
#[bench::first_keystroke(args = ("c", None), setup = filter)]
#[bench::two_terms(args = ("contoso up", None), setup = filter)]
#[bench::narrow(args = ("contoso upd", Some("contoso up")), setup = filter)]
fn filter_rows(mut f: Filter) -> Filter {
    let previous = f.previous.as_ref().map(|(q, rows)| (q, rows.as_slice()));
    f.index.filter(black_box(&f.query), previous, &mut f.out);
    f
}

/// U11 on the UI thread: install a sort result and a filter result, recomposing the view.
struct Results {
    model: TableModel,
    order: Vec<u32>,
    query: Query,
    matches: Vec<u32>,
    fixture: (Apps, SortKeys, FilterIndex),
}

fn results() -> Results {
    let apps = Apps::new(ROWS);
    let keys = SortKeys::build(&apps, NAME);
    let index = FilterIndex::build(&apps, &SEARCHED);
    let query = Query::new("contoso up");
    let mut matches = Vec::with_capacity(ROWS as usize);
    index.filter(&query, None, &mut matches);
    Results {
        model: TableModel::new(ROWS),
        order: keys.order(Order::Ascending),
        query,
        matches,
        fixture: (apps, keys, index),
    }
}

#[library_benchmark]
#[bench::sort_and_filter(setup = results)]
fn apply(r: Results) -> (TableModel, (Apps, SortKeys, FilterIndex)) {
    let Results {
        mut model,
        order,
        query,
        matches,
        fixture,
    } = r;
    let s = model.begin_sort();
    model.finish_sort(s, (NAME, Order::Ascending), black_box(order));
    let f = model.begin_filter();
    model.finish_filter(f, query, black_box(matches));
    (model, fixture)
}

/// The U11 table refreshed (TD-0009 A2.5): the same 100k programs, 500 gone from the top, 500 new
/// at the end, so every other row moves 500 places. Row `r` has key `r + 500`.
struct Refreshed(u32);

impl RowSource for Refreshed {
    fn len(&self) -> u32 {
        self.0
    }
    fn key(&self, row: u32) -> RowKey {
        RowKey(u64::from(row) + 500)
    }
    fn cell(&self, _row: u32, _col: ColumnId) -> Cell<'_> {
        Cell::Empty
    }
}

/// The model of [`Apps`] with every tenth row checked and row 50,000 current, sorted by name,
/// and the refreshed data's key map, built off the UI thread.
struct Refresh {
    model: TableModel,
    map: RowMap,
    fixture: (Apps, Refreshed),
}

fn refresh() -> Refresh {
    let apps = Apps::new(ROWS);
    let mut model = TableModel::with_keys(RowMap::build(&apps));
    let s = model.begin_sort();
    model.finish_sort(
        s,
        (NAME, Order::Ascending),
        SortKeys::build(&apps, NAME).order(Order::Ascending),
    );
    for at in (0..ROWS).step_by(10) {
        model.pick(at, Pick::Toggle);
    }
    model.set_current(ROWS / 2);
    let new = Refreshed(ROWS);
    Refresh {
        model,
        map: RowMap::build(&new),
        fixture: (apps, new),
    }
}

fn refreshed() -> Refreshed {
    Refreshed(ROWS)
}

// Off the UI thread: the new data's keys, as a map from key to row.
#[library_benchmark]
#[bench::keys_100k(setup = refreshed)]
fn refresh_map(new: Refreshed) -> (RowMap, Refreshed) {
    (RowMap::build(black_box(&new)), new)
}

// On the UI thread: the checks, the current row and the anchor carried over by key, and the view
// kept in its order with the new rows after it.
#[library_benchmark]
#[bench::checks_10k(setup = refresh)]
fn refresh_apply(r: Refresh) -> (TableModel, (Apps, Refreshed)) {
    let Refresh {
        mut model,
        map,
        fixture,
    } = r;
    let g = model.begin_refresh();
    model.finish_refresh(g, black_box(map));
    (model, fixture)
}

library_benchmark_group!(name = palette_1000, benchmarks = keystroke);
library_benchmark_group!(
    name = table_100k,
    benchmarks = [
        sort_first,
        sort_again,
        filter_index,
        filter_rows,
        apply,
        refresh_map,
        refresh_apply
    ]
);

main!(
    // Instruction counts only: the cache simulation would slow every run for metrics nothing gates.
    config = LibraryBenchmarkConfig::default().tool(Callgrind::with_args(["--cache-sim=no"]));
    library_benchmark_groups = palette_1000, table_100k
);
