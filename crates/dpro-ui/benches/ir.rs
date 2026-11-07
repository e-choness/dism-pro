//! TD-0009 §7 U2, U3 and U12 as instruction counts: the deterministic gate (doc 04 §3).
//!
//! The same shell and session as `interact.rs`. Setups warm the Ui with one full session first, so
//! the counts are the steady state an open window lives in, and keep everything alive until the
//! benchmark returns (see `dpro-app/benches/ir.rs` on `malloc_consolidate`).
//! - `interact`: U2, frames end at layout and damage. Text input (§11 step 3) in the filter field:
//!   a keystroke, the caret one character left and one word left, a word deleted, and pastes of a
//!   `MAX_PATH`-long path and of 64 KiB (the most a field holds, the worst case). The standard
//!   controls (§11 step 4): Right on the focused tab (the next tab is picked and the table
//!   refilters), one frame of a hold on Hold to apply, and a progress report. Windows' text size
//!   switched from 100 to 225 % (every text node measured again, the whole page laid out), and the
//!   first frame at 100 and at 225 %. Popups (TD-0009 A1.7): Shift+F10 on a row (it's picked and
//!   its menu built and laid out), Down in the open menu, and a pending tooltip coming due.
//! - `paint`: U3 without presenting, frames painted at 1000 × 700, scale 1, light theme; and the
//!   whole window repainted, as after a theme switch, at scales 1 and 1.5 and with the text at
//!   225 %. With a keystroke and a caret blink in the field, a hold's frame, a progress report and
//!   Shift+F10 on a row, the menu's window included.
//! - `popup`: a 6-item menu's `Ui` built, laid out and painted whole, as its window first shows.
//! - `a11y`: U12 with a client listening, as in `a11y.rs`: the whole tree (the shell's, and an open
//!   menu's), and one frame's update for an unchanged event, another row selected, a page switch
//!   each way, and a progress report.
//! - `fling` (U1): the first 60 frames of S-01's fling script on a 100k-row, 8-column table, and
//!   30 of its thumb drag, each built, laid out and painted whole (TD-0009 A2.7). The table's
//!   events join `interact`: Down on the last row in view (which scrolls), Space checking the
//!   focused row, a header's sort (750 rows, inline), and a far row realized for a client with
//!   its update made. `a11y` adds the 100k-row table's first tree.
//!
//! C1: `docker compose run --rm bench-gate`.

#![allow(missing_docs)]

#[path = "fixtures/fling.rs"]
mod fling_table;
#[path = "fixtures/shell.rs"]
mod shell;

use std::hint::black_box;

use dpro_ui::{
    Canvas, ColumnId, FakeText, Input, KeyCode, Mods, Motion, Point, Size, TableEvent, Theme, Ui,
    accesskit::TreeUpdate, tokens,
};
use fling_table::Fling;
use gungraun::{
    Callgrind, LibraryBenchmarkConfig, library_benchmark, library_benchmark_group, main,
};
use shell::{
    CTRL, FRAME_MS, Msg, PATH, Run, START, Shell, Step, key, long_text, popup_key, popup_view,
};

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

/// Warm, row 4 focused, then `step` (TD-0009 A1.7).
fn row_with(step: Step) -> (Run, Step) {
    (warm().0.row_focused(), step)
}

fn row_painted_with(step: Step) -> (Run, Step) {
    (warm_painted().0.row_focused(), step)
}

/// Warm, row 4's menu open from the keyboard, then `step`.
fn menu_with(step: Step) -> (Run, Step) {
    (warm().0.menu_open(), step)
}

/// Warm, the pointer resting on the drawer button with its tip pending, then `step`.
fn tip_with(step: Step) -> (Run, Step) {
    (warm().0.tip_pending(), step)
}

/// Warm, the pointer resting on the table, then a wheel notch of `delta` there (A2, slice 5a).
fn table_with(delta: i32) -> (Run, Step) {
    let run = warm().0.over_table();
    let step = run.table_wheel(delta);
    (run, step)
}

fn table_painted_with(delta: i32) -> (Run, Step) {
    let run = warm_painted().0.over_table();
    let step = run.table_wheel(delta);
    (run, step)
}

/// Warm, the table's thumb pressed, then the pointer `dy` lower: one frame of a thumb drag.
fn thumb_with(dy: f32) -> (Run, Step) {
    let run = warm().0.thumb_grabbed();
    let step = run.thumb_moved(dy);
    (run, step)
}

/// Warm, the last row wholly in view current and focused, then `step` (A2, slice 5b1).
fn bottom_row_with(step: Step) -> (Run, Step) {
    (warm().0.bottom_row_current(), step)
}

/// Warm with a client listening, then `step`: its frame makes the update (A2.5's realizer).
fn accessible_with(step: Step) -> (Run, Step) {
    (warm_accessible(), step)
}

/// Warm, Publisher's divider pressed, then the pointer `dx` right: one frame of a divider drag,
/// Name frozen and the shared columns right of it re-flowing (A2, slice 5b2).
fn divider_with(dx: f32) -> (Run, Step) {
    let run = warm().0.divider_grabbed(1);
    let step = run.divider_moved(1, dx);
    (run, step)
}

fn divider_painted_with(dx: f32) -> (Run, Step) {
    let run = warm_painted().0.divider_grabbed(1);
    let step = run.divider_moved(1, dx);
    (run, step)
}

/// Warm, Publisher's header focused, then `step`.
fn header_with(step: Step) -> (Run, Step) {
    (warm().0.header_focused(1), step)
}

fn warm_painted_150() -> (Run, Vec<Step>) {
    warmed(Run::painted(Shell::new(), 1.5, Theme::DARK))
}

/// Warm and painted, with the text size at 225 % (Windows' largest) from the first frame.
fn warm_painted_225() -> (Run, Vec<Step>) {
    warmed(Run::painted_text_scaled(Shell::new(), Theme::LIGHT, 2.25))
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
#[bench::text_scale(args = (Step::TextScale(2.25)), setup = warm_with)]
#[bench::context_menu(args = (Step::Input(Input::ContextMenu(None))), setup = row_with)]
#[bench::menu_arrow(args = (popup_key(KeyCode::Down, true, Mods::default())), setup = menu_with)]
#[bench::tooltip_due(args = (Step::Tick(u64::from(tokens::popup::TIP_DELAY_MS))), setup = tip_with)]
#[bench::wheel_notch(args = (-120), setup = table_with)]
#[bench::thumb_drag(args = (40.0), setup = thumb_with)]
#[bench::row_down(args = (key(KeyCode::Down, true, Mods::default())), setup = bottom_row_with)]
#[bench::check_space(args = (key(KeyCode::Space, true, Mods::default())), setup = row_with)]
#[bench::header_sort(args = (Step::Update(Msg::Table(TableEvent::Sort(ColumnId(0))))), setup = warm_with)]
#[bench::realize(args = (Step::Realize(700)), setup = accessible_with)]
#[bench::column_drag(args = (30.0), setup = divider_with)]
#[bench::column_fit(args = (Step::Update(Msg::Table(TableEvent::Fit(Some(ColumnId(1)))))), setup = warm_with)]
#[bench::column_menu(args = (Step::Input(Input::ContextMenu(None))), setup = header_with)]
#[bench::formats_change(args = (Step::Decimal(",")), setup = warm_with)]
fn event((mut run, step): (Run, Step)) -> Run {
    run.step(black_box(step));
    run
}

#[library_benchmark]
#[bench::shell(args = (1.0), setup = shell_at)]
#[bench::text_225(args = (2.25), setup = shell_at)]
fn first_frame((shell, text_scale): (Shell, f32)) -> Run {
    Run::text_scaled(black_box(shell), black_box(text_scale))
}

/// A new shell, to open with the text size at `text_scale`.
fn shell_at(text_scale: f32) -> (Shell, f32) {
    (Shell::new(), text_scale)
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
#[bench::context_menu(args = (Step::Input(Input::ContextMenu(None))), setup = row_painted_with)]
#[bench::wheel_notch(args = (-120), setup = table_painted_with)]
#[bench::column_drag(args = (30.0), setup = divider_painted_with)]
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
#[bench::text_225(setup = warm_painted_225)]
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

/// A shell with row 4's menu open: what its window shows.
fn menu_shell() -> Shell {
    let mut shell = Shell::new();
    shell.update(Msg::RowMenu(4));
    shell
}

// A popup window's first show (U3 without presenting): its `Ui` made, the menu built, its first
// item focused (opened from the keyboard), sized to its content, laid out, and painted whole.
#[library_benchmark]
#[bench::menu(setup = menu_shell)]
fn open(shell: Shell) -> (Ui<Msg>, Vec<u32>, Shell) {
    let motion = Motion {
        hover_ms: tokens::motion::HOVER_MS,
    };
    let mut ui = Ui::new(Size { w: 0.0, h: 0.0 }, 1.0, motion);
    ui.build(|cx| popup_view(black_box(&shell), cx));
    ui.focus_first(true);
    let size = ui.natural_size(&mut FakeText, START.w);
    ui.set_viewport(size);
    ui.layout(&mut FakeText);
    let (w, h) = ui.pixel_size();
    let mut pixels = vec![0; usize::try_from(u64::from(w) * u64::from(h)).unwrap_or(0)];
    if let Some(mut canvas) = Canvas::new(&mut pixels, w, h) {
        ui.paint(&mut canvas, &mut FakeText, &Theme::LIGHT);
    }
    (ui, pixels, shell)
}

library_benchmark_group!(name = popup, benchmarks = [open]);

/// The fling's table at scale 1, its first frame painted, and the frames counted: the script's
/// first 60 (its first fling) and 30 of its 90-frame thumb drag.
fn fling_frames() -> (Box<Fling>, Vec<f32>) {
    let f = Fling::new(1.0);
    let script = f.script();
    let mut frames: Vec<f32> = script.frames.iter().take(60).copied().collect();
    frames.extend(script.frames.iter().skip(script.drag).take(30));
    (Box::new(f), frames)
}

#[library_benchmark]
#[bench::frames(setup = fling_frames)]
fn fling((mut f, frames): (Box<Fling>, Vec<f32>)) -> (Box<Fling>, Vec<f32>) {
    for &offset in &frames {
        f.frame(black_box(offset));
    }
    (f, frames)
}

library_benchmark_group!(name = table, benchmarks = [fling]);

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
///
/// The U12 benches take the run boxed. They hand it back so it's dropped outside the count, and
/// moving a whole `Run` out was most of a small update's count (112 of `unchanged`'s 173 Ir).
fn accessible_after(before: Option<Step>, step: Step) -> Box<Run> {
    let mut run = warm_accessible();
    if let Some(s) = before {
        run.step(s);
    }
    run.take_a11y = false;
    run.step(step);
    Box::new(run)
}

/// [`warm_accessible`], the pointer on the table, then a wheel notch down it with its update left
/// to take: rows move, and the table's scroll offset changes (A2, slice 5a).
fn accessible_scroll() -> Box<Run> {
    let mut run = warm_accessible().over_table();
    let step = run.table_wheel(-120);
    run.take_a11y = false;
    run.step(step);
    Box::new(run)
}

/// The fling's 100k-row table, for its first tree.
fn table_tree() -> Box<Fling> {
    Box::new(Fling::new(1.0))
}

#[library_benchmark]
#[bench::table(setup = table_tree)]
fn a11y_table_tree(mut f: Box<Fling>) -> (TreeUpdate, Box<Fling>) {
    (f.a11y_tree(), f)
}

/// [`warm_accessible`], for the shell's whole tree.
fn shell_tree() -> (Box<Run>, bool) {
    (Box::new(warm_accessible()), false)
}

/// [`warm_accessible`] with row 4's menu open, for the menu window's whole tree.
fn menu_tree() -> (Box<Run>, bool) {
    (Box::new(warm_accessible().menu_open()), true)
}

#[library_benchmark]
#[bench::shell(setup = shell_tree)]
#[bench::menu(setup = menu_tree)]
fn a11y_tree((mut run, popup): (Box<Run>, bool)) -> (TreeUpdate, Box<Run>) {
    let tree = if popup {
        run.popup.a11y_tree()
    } else {
        run.ui.a11y_tree()
    };
    (tree, run)
}

#[library_benchmark]
#[bench::unchanged(args = (None, Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 }))), setup = accessible_after)]
#[bench::row_selection(args = (None, Step::Update(Msg::Row(5))), setup = accessible_after)]
#[bench::page_away(args = (None, Step::Update(Msg::Page(1))), setup = accessible_after)]
#[bench::page_back(args = (Some(Step::Update(Msg::Page(1))), Step::Update(Msg::Page(0))), setup = accessible_after)]
#[bench::progress(args = (Some(Step::Update(Msg::Drawer)), Step::Update(Msg::Progress(41))), setup = accessible_after)]
#[bench::scroll(setup = accessible_scroll)]
#[bench::column_hide(args = (None, Step::Update(Msg::Table(TableEvent::Show(ColumnId(2), false)))), setup = accessible_after)]
fn a11y_update(mut run: Box<Run>) -> (Option<TreeUpdate>, Box<Run>) {
    (run.a11y_update(), run)
}

library_benchmark_group!(
    name = a11y,
    benchmarks = [a11y_tree, a11y_table_tree, a11y_update]
);

main!(
    // Instruction counts only: the cache simulation would slow every run for metrics nothing gates.
    config = LibraryBenchmarkConfig::default().tool(Callgrind::with_args(["--cache-sim=no"]));
    library_benchmark_groups = interact, paint, popup, table, a11y
);
