//! TD-0009 §7 U2 and U3 as instruction counts: the deterministic gate (doc 04 §3).
//!
//! The same shell and session as `interact.rs`. Setups warm the Ui with one full session first, so
//! the counts are the steady state an open window lives in, and keep everything alive until the
//! benchmark returns (see `dpro-app/benches/ir.rs` on `malloc_consolidate`).
//! - `interact`: U2, frames end at layout and damage.
//! - `paint`: U3 without presenting, frames painted at 1000 × 700, scale 1, light theme; and the
//!   whole window repainted, as after a theme switch, at scales 1 and 1.5.
//!
//! C1: `docker compose run --rm bench-gate`.

#![allow(missing_docs)]

#[path = "fixtures/shell.rs"]
mod shell;

use std::hint::black_box;

use dpro_ui::{Input, Point, Size, Theme};
use gungraun::{
    Callgrind, LibraryBenchmarkConfig, library_benchmark, library_benchmark_group, main,
};
use shell::{Run, START, Shell, Step};

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

main!(
    // Instruction counts only: the cache simulation would slow every run for metrics nothing gates.
    config = LibraryBenchmarkConfig::default().tool(Callgrind::with_args(["--cache-sim=no"]));
    library_benchmark_groups = interact, paint
);
