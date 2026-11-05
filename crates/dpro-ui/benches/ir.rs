//! TD-0009 §7 U2, U3 and U12 as instruction counts: the deterministic gate (doc 04 §3).
//!
//! The same shell and session as `interact.rs`. Setups warm the Ui with one full session first, so
//! the counts are the steady state an open window lives in, and keep everything alive until the
//! benchmark returns (see `dpro-app/benches/ir.rs` on `malloc_consolidate`).
//! - `interact`: U2, frames end at layout and damage.
//! - `paint`: U3 without presenting, frames painted at 1000 × 700, scale 1, light theme; and the
//!   whole window repainted, as after a theme switch, at scales 1 and 1.5.
//! - `a11y`: U12 with a client listening, as in `a11y.rs`: the whole tree, and one frame's update
//!   for an unchanged event, another row selected, and a page switch each way.
//!
//! C1: `docker compose run --rm bench-gate`.

#![allow(missing_docs)]

#[path = "fixtures/shell.rs"]
mod shell;

use std::hint::black_box;

use dpro_ui::{Input, Point, Size, Theme, accesskit::TreeUpdate};
use gungraun::{
    Callgrind, LibraryBenchmarkConfig, library_benchmark, library_benchmark_group, main,
};
use shell::{Msg, Run, START, Shell, Step};

/// `run` after one full session, and the session.
fn warmed(mut run: Run) -> (Run, Vec<Step>) {
    let steps = run.session();
    for &s in &steps {
        run.step(s);
    }
    (run, steps)
}

fn warm() -> (Run, Vec<Step>) {
    warmed(Run::with(Shell::new()))
}

fn warm_with(step: Step) -> (Run, Step) {
    (warm().0, step)
}

fn warm_painted() -> (Run, Vec<Step>) {
    warmed(Run::painted(Shell::new(), 1.0, Theme::LIGHT))
}

fn warm_painted_with(step: Step) -> (Run, Step) {
    (warm_painted().0, step)
}

fn warm_painted_150() -> (Run, Vec<Step>) {
    warmed(Run::painted(Shell::new(), 1.5, Theme::DARK))
}

#[library_benchmark]
#[bench::scripted(setup = warm)]
fn session((mut run, steps): (Run, Vec<Step>)) -> (Run, Vec<Step>) {
    for &s in &steps {
        run.step(black_box(s));
    }
    (run, steps)
}

#[library_benchmark]
#[bench::unchanged(args = (Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 }))), setup = warm_with)]
#[bench::keystroke(args = (Step::Type('c')), setup = warm_with)]
#[bench::resize(args = (Step::Resize(Size { w: 1400.0, ..START })), setup = warm_with)]
fn event((mut run, step): (Run, Step)) -> Run {
    run.step(black_box(step));
    run
}

#[library_benchmark]
#[bench::shell(setup = Shell::new)]
fn first_frame(shell: Shell) -> Run {
    Run::with(black_box(shell))
}

library_benchmark_group!(name = interact, benchmarks = [session, event, first_frame]);

#[library_benchmark]
#[bench::scripted(setup = warm_painted)]
fn painted_session((mut run, steps): (Run, Vec<Step>)) -> (Run, Vec<Step>) {
    for &s in &steps {
        run.step(black_box(s));
    }
    (run, steps)
}

#[library_benchmark]
#[bench::unchanged(args = (Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 }))), setup = warm_painted_with)]
#[bench::keystroke(args = (Step::Type('c')), setup = warm_painted_with)]
#[bench::resize(args = (Step::Resize(Size { w: 1400.0, ..START })), setup = warm_painted_with)]
fn painted_event((mut run, step): (Run, Step)) -> Run {
    run.step(black_box(step));
    run
}

#[library_benchmark]
#[bench::shell(setup = Shell::new)]
fn painted_first_frame(shell: Shell) -> Run {
    Run::painted(black_box(shell), 1.0, Theme::LIGHT)
}

#[library_benchmark]
#[bench::light_100(setup = warm_painted)]
#[bench::dark_150(setup = warm_painted_150)]
fn repaint_all((mut run, steps): (Run, Vec<Step>)) -> (Run, Vec<Step>) {
    run.repaint_all();
    (run, steps)
}

library_benchmark_group!(
    name = paint,
    benchmarks = [
        painted_session,
        painted_event,
        painted_first_frame,
        repaint_all
    ]
);

/// An accessible run after one full session, with row 2 selected.
fn warm_accessible() -> Run {
    let mut run = Run::accessible(Shell::new());
    for s in run.session() {
        run.step(s);
    }
    run.step(Step::Update(Msg::Row(2)));
    run
}

/// [`warm_accessible`], then `before` if any, then `step` with its update left to take.
fn accessible_after(before: Option<Step>, step: Step) -> Run {
    let mut run = warm_accessible();
    if let Some(s) = before {
        run.step(s);
    }
    run.take_a11y = false;
    run.step(step);
    run
}

#[library_benchmark]
#[bench::shell(setup = warm_accessible)]
fn a11y_tree(mut run: Run) -> (TreeUpdate, Run) {
    (run.ui.a11y_tree(), run)
}

#[library_benchmark]
#[bench::unchanged(args = (None, Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 }))), setup = accessible_after)]
#[bench::row_selection(args = (None, Step::Update(Msg::Row(3))), setup = accessible_after)]
#[bench::page_away(args = (None, Step::Update(Msg::Page(1))), setup = accessible_after)]
#[bench::page_back(args = (Some(Step::Update(Msg::Page(1))), Step::Update(Msg::Page(0))), setup = accessible_after)]
fn a11y_update(mut run: Run) -> (Option<TreeUpdate>, Run) {
    (run.a11y_update(), run)
}

library_benchmark_group!(name = a11y, benchmarks = [a11y_tree, a11y_update]);

main!(
    // Instruction counts only: the cache simulation would slow every run for metrics nothing gates.
    config = LibraryBenchmarkConfig::default().tool(Callgrind::with_args(["--cache-sim=no"]));
    library_benchmark_groups = interact, paint, a11y
);
