//! TD-0009 §7 U2, U3 and U12 as instruction counts: the deterministic gate (doc 04 §3).
//!
//! The same shell and session as `interact.rs`. Setups warm the Ui with one full session first, so
//! the counts are the steady state an open window lives in, and keep everything alive until the
//! benchmark returns (see `dpro-app/benches/ir.rs` on `malloc_consolidate`).
//! - `interact`: U2, frames end at layout and damage. Text input (§11 step 3) in the filter field:
//!   a keystroke, the caret one character left and one word left, a word deleted, and pastes of a
//!   `MAX_PATH`-long path and of 64 KiB (the most a field holds, the worst case). The standard
//!   controls (§11 step 4): Right on the focused tab (the next tab is picked and the table
//!   refilters), one frame of a hold on Hold to apply, and a progress report.
//! - `paint`: U3 without presenting, frames painted at 1000 × 700, scale 1, light theme; and the
//!   whole window repainted, as after a theme switch, at scales 1 and 1.5. With a keystroke and a
//!   caret blink in the field, a hold's frame and a progress report.
//! - `a11y`: U12 with a client listening, as in `a11y.rs`: the whole tree, and one frame's update
//!   for an unchanged event, another row selected, a page switch each way, and a progress report.
//!
//! C1: `docker compose run --rm bench-gate`.

#![allow(missing_docs)]

#[path = "fixtures/shell.rs"]
mod shell;

use std::hint::black_box;

use dpro_ui::{Input, KeyCode, Mods, Point, Size, Theme, accesskit::TreeUpdate};
use gungraun::{
    Callgrind, LibraryBenchmarkConfig, library_benchmark, library_benchmark_group, main,
};
use shell::{CTRL, FRAME_MS, Msg, PATH, Run, START, Shell, Step, key, long_text};

/// `run` after one full session, and the session.
fn warmed(mut run: Run) -> (Run, Vec<Step>) {
    let steps = run.session();
    for s in &steps {
        run.step(s.clone());
    }
    (run, steps)
}

fn warm() -> (Run, Vec<Step>) {
    warmed(Run::with(Shell::new()))
}

fn warm_with(step: Step) -> (Run, Step) {
    (warm().0, step)
}

/// `run`, then a click into the filter and `typed` typed into it, the caret at its end. Frames
/// tick at 60 Hz until the hover fades the click started have ended, as they have by the time
/// anyone types, so a measured step pays only for itself; the caret's blink restarted at the last
/// keystroke.
fn in_filter(mut run: Run, typed: &str) -> Run {
    for s in run.focus_filter() {
        run.step(s);
    }
    run.settle();
    for c in typed.chars() {
        run.step(Step::Type(c));
    }
    run
}

/// Warm, with "contoso up" in the focused filter, then `step`.
fn typed_with(step: Step) -> (Run, Step) {
    (in_filter(warm().0, "contoso up"), step)
}

/// Warm, the empty filter focused, then `step`.
fn focused_with(step: Step) -> (Run, Step) {
    (in_filter(warm().0, ""), step)
}

fn warm_painted() -> (Run, Vec<Step>) {
    warmed(Run::painted(Shell::new(), 1.0, Theme::LIGHT))
}

fn warm_painted_with(step: Step) -> (Run, Step) {
    (warm_painted().0, step)
}

fn focused_painted_with(step: Step) -> (Run, Step) {
    (in_filter(warm_painted().0, ""), step)
}

/// Warm and painted, "contoso" in the focused filter: the caret's next blink is one period away.
fn blinking_painted_with(step: Step) -> (Run, Step) {
    (in_filter(warm_painted().0, "contoso"), step)
}

/// Warm, the selected tab focused, then `step` (§11 step 4).
fn tabs_with(step: Step) -> (Run, Step) {
    (warm().0.in_tabs(), step)
}

/// Warm, Hold to apply pressed two frames ago, then `step`.
fn holding_with(step: Step) -> (Run, Step) {
    (warm().0.holding(), step)
}

fn holding_painted_with(step: Step) -> (Run, Step) {
    (warm_painted().0.holding(), step)
}

/// Warm, the drawer open, then `step`.
fn drawer_with(step: Step) -> (Run, Step) {
    (warm().0.drawer_open(), step)
}

fn drawer_painted_with(step: Step) -> (Run, Step) {
    (warm_painted().0.drawer_open(), step)
}

fn warm_painted_150() -> (Run, Vec<Step>) {
    warmed(Run::painted(Shell::new(), 1.5, Theme::DARK))
}

#[library_benchmark]
#[bench::scripted(setup = warm)]
fn session((mut run, steps): (Run, Vec<Step>)) -> (Run, Vec<Step>) {
    for s in &steps {
        run.step(black_box(s.clone()));
    }
    (run, steps)
}

#[library_benchmark]
#[bench::unchanged(args = (Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 }))), setup = warm_with)]
#[bench::keystroke(args = (Step::Type('c')), setup = focused_with)]
#[bench::resize(args = (Step::Resize(Size { w: 1400.0, ..START })), setup = warm_with)]
#[bench::caret_left(args = (key(KeyCode::Left, true, Mods::default())), setup = typed_with)]
#[bench::word_left(args = (key(KeyCode::Left, true, CTRL)), setup = typed_with)]
#[bench::word_delete(args = (key(KeyCode::Backspace, true, CTRL)), setup = typed_with)]
#[bench::paste_path(args = (Step::Paste(PATH)), setup = focused_with)]
#[bench::paste_64k(args = (Step::Paste(long_text())), setup = focused_with)]
#[bench::tab_arrow(args = (key(KeyCode::Right, true, Mods::default())), setup = tabs_with)]
#[bench::hold_tick(args = (Step::Tick(FRAME_MS)), setup = holding_with)]
#[bench::progress(args = (Step::Update(Msg::Progress(41))), setup = drawer_with)]
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
    for s in &steps {
        run.step(black_box(s.clone()));
    }
    (run, steps)
}

#[library_benchmark]
#[bench::unchanged(args = (Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 }))), setup = warm_painted_with)]
#[bench::keystroke(args = (Step::Type('c')), setup = focused_painted_with)]
#[bench::resize(args = (Step::Resize(Size { w: 1400.0, ..START })), setup = warm_painted_with)]
#[bench::caret_blink(args = (Step::Tick(u64::from(dpro_ui::Caret::WINDOWS.blink_ms))), setup = blinking_painted_with)]
#[bench::hold_tick(args = (Step::Tick(FRAME_MS)), setup = holding_painted_with)]
#[bench::progress(args = (Step::Update(Msg::Progress(41))), setup = drawer_painted_with)]
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
#[bench::row_selection(args = (None, Step::Update(Msg::Row(5))), setup = accessible_after)]
#[bench::page_away(args = (None, Step::Update(Msg::Page(1))), setup = accessible_after)]
#[bench::page_back(args = (Some(Step::Update(Msg::Page(1))), Step::Update(Msg::Page(0))), setup = accessible_after)]
#[bench::progress(args = (Some(Step::Update(Msg::Drawer)), Step::Update(Msg::Progress(41))), setup = accessible_after)]
fn a11y_update(mut run: Run) -> (Option<TreeUpdate>, Run) {
    (run.a11y_update(), run)
}

library_benchmark_group!(name = a11y, benchmarks = [a11y_tree, a11y_update]);

main!(
    // Instruction counts only: the cache simulation would slow every run for metrics nothing gates.
    config = LibraryBenchmarkConfig::default().tool(Callgrind::with_args(["--cache-sim=no"]));
    library_benchmark_groups = interact, paint, a11y
);
