//! TD-0009 §7 U12: the accessibility tree that AccessKit serves to UI Automation, on the shell
//! fixture with a client listening. Advisory timings (C1); `ir.rs` gates the same scenarios'
//! instruction counts.
//!
//! - `full_tree`: the whole tree, for the client's first request (387 nodes).
//! - `update_*`: one frame's incremental update, after the frame that made the change:
//!   - `unchanged`: an event that changes nothing a client sees;
//!   - `row_selection`: another row selected (two rows and nothing else);
//!   - `page_away` and `page_back`: the rail switching pages, which replaces the content.
//!
//! Budget: 0.5 ms per frame for the update (U12). Run: `docker compose run --rm bench -- -p
//! dpro-ui --bench a11y`.

#![allow(missing_docs)]

#[path = "fixtures/shell.rs"]
mod shell;

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use dpro_ui::{Input, Point};
use shell::{Msg, Run, Shell, Step};

/// An accessible run after one full session, with row 2 selected.
fn warm() -> Run {
    let mut run = Run::accessible(Shell::new());
    for s in run.session() {
        run.step(s);
    }
    run.step(Step::Update(Msg::Row(2)));
    run
}

/// [`warm`], then `before` if any, then `step` with its update left for the bench to take.
fn after(before: Option<Step>, step: Step) -> impl Fn() -> Run {
    move || {
        let mut run = warm();
        if let Some(s) = &before {
            run.step(s.clone());
        }
        run.take_a11y = false;
        run.step(step.clone());
        run
    }
}

fn a11y(c: &mut Criterion) {
    let mut g = c.benchmark_group("a11y");
    g.bench_function("full_tree", |b| {
        b.iter_batched(
            warm,
            |mut run| (run.ui.a11y_tree(), run),
            BatchSize::LargeInput,
        );
    });
    let away = Step::Update(Msg::Page(1));
    let cases = [
        (
            "update_unchanged",
            after(
                None,
                Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 })),
            ),
        ),
        (
            "update_row_selection",
            after(None, Step::Update(Msg::Row(3))),
        ),
        ("update_page_away", after(None, away.clone())),
        (
            "update_page_back",
            after(Some(away), Step::Update(Msg::Page(0))),
        ),
    ];
    for (name, setup) in cases {
        g.bench_function(name, |b| {
            b.iter_batched(
                &setup,
                |mut run| (black_box(run.a11y_update()), run),
                BatchSize::LargeInput,
            );
        });
    }
    g.finish();
}

criterion_group!(benches, a11y);
criterion_main!(benches);
