//! TD-0002 §7: render a 100k-row table (the largest lists: packages, files in a plan).
//! C1: `docker compose run --rm bench -- render_table`.

#![allow(missing_docs)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use dpro_cli_fmt::table::{Table, render};

fn render_table(c: &mut Criterion) {
    let mut t = Table::new(&[("Package".into(), false), ("State".into(), false), ("Size".into(), true)]);
    for i in 0..100_000 {
        t.push(vec![
            format!("Package_for_RollupFix~31bf3856ad364e35~amd64~~26100.{i}.1.10"),
            "Installed".into(),
            format!("{}.{} MB", i % 900, i % 10),
        ]);
    }
    c.bench_function("render_table_100k", |b| b.iter(|| black_box(render(black_box(&t), 160))));
}

criterion_group!(benches, render_table);
criterion_main!(benches);
