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
//!   whole window repainted, as after a theme switch, at scales 1 and 1.5, with the text at
//!   225 %, and in high contrast (slice 7d's audit). With a keystroke and a caret blink in the field, a hold's frame, a progress report and
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
//! - `composite` (TD-0009 A3.7, slice 6a): the composites page of `fixtures/composites.rs`,
//!   warmed by its session; frames end at layout and damage. The batch bar's count going from 3
//!   to 4 (`batch_count`), a scan card's size arriving (`scan_size`), and the window narrowed so
//!   the summary cards wrap two to a line (`cards_wrap_resize`). `a11y` adds the page's whole
//!   tree (`a11y_tree.composites`). A3.7 names them `composite::batch_count` and so on; the bench
//!   ids after the dot say which case.
//! - `grouped` (TD-0009 A3.7 U18, slice 6b): the fling's table with its rows two lines each, in
//!   1,000 groups of 100 under their header rows. The same 90 frames as `table::fling.frames`,
//!   painted (`fling.frames`); and, frames ending at layout and damage, Down on a group's last
//!   row, which passes its next group's header and scrolls (`event.row_down`), and a far row
//!   realized for a client by its grid row, headers counted, with its update made
//!   (`event.realize`).
//! - `composite_drawer` (TD-0009 A3.7's U16, slice 6d): the drawer page of `fixtures/drawer.rs`,
//!   warmed by its session. "Review changes" opening the drawer as a pane (`drawer_open`), and
//!   the 6th frame of its slide (`drawer_frame`), each at layout and damage and painted.
//! - Slice 6c (A3.7 U14, U15, U17): a character typed in the palette, painted
//!   (`composite::palette_keystroke`), Down in it (`palette_arrow`), and a job's progress with
//!   its toast and card both showing it (`toast_progress`); the palette's and the dialog's
//!   windows' first show (`popup::open.palette`, `.dialog`); the scrim turned on over the shell,
//!   painted (`paint::painted_event.palette_scrim`); and the palette's and the toast's windows'
//!   updates (`a11y::a11y_window_update.palette_arrow`, `.toast_line`).
//! - `notice` (TD-0009 A4.7, slice 7b), in a group of its own: a progress report that queues a
//!   notice, with a client listening (`notice_event.progress_step`, A4.7's `interact::event.notice`),
//!   and the update after a job toast's live line turns to an assertive failure
//!   (`live_toast.failed`, A4.7's `a11y_update.live_toast`; 6c's real toast since slice 7c).
//! - Slice 7c (TD-0009 A4.7 U21): Enter on the drawer's Cancel, focus going back to "Review
//!   changes" by the memo its scope took (`interact::event.drawer_close`); a summary card's
//!   category cleaned while its Review has focus, which goes to the next card's
//!   (`event.remove_focused`); and the drawer's close with a client listening
//!   (`a11y::a11y_update.focus_restore`).
//!
//! C1: `docker compose run --rm bench-gate`.

#![allow(missing_docs)]

#[path = "fixtures/drawer.rs"]
mod drawer_page;

#[path = "fixtures/composites.rs"]
mod composites;
#[path = "fixtures/fling.rs"]
mod fling_table;
#[path = "fixtures/shell.rs"]
mod shell;

use std::hint::black_box;

use dpro_ui::{
    Canvas, ColumnId, FakeText, Input, KeyCode, Mods, Motion, Point, Size, TableEvent, Theme, Ui,
    accesskit::TreeUpdate, tokens,
};
use dpro_ui::{
    Cx, Key,
    composite::{Toast, ToastTone},
};
use fling_table::Fling;
use gungraun::{
    Callgrind, LibraryBenchmarkConfig, library_benchmark, library_benchmark_group, main,
};
use shell::{
    CTRL, FAR_NAME, FRAME_MS, Msg, PATH, Run, START, Shell, Step, key, long_text, popup_key,
    popup_view,
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

/// Warm, the drawer opened from "Review changes" by the keyboard and its Cancel focused, then
/// `step` (TD-0009 A4.7, slice 7c).
fn drawer_return_with(step: Step) -> (Run, Step) {
    (warm().0.drawer_from_keyboard(), step)
}

/// Warm, on a summary page with card 3's Review focused, then `step` (slice 7c).
fn card_with(step: Step) -> (Run, Step) {
    (warm().0.card_focused(3), step)
}

fn warm_painted_150() -> (Run, Vec<Step>) {
    warmed(Run::painted(Shell::new(), 1.5, Theme::DARK))
}

/// Warm and painted, with the text size at 225 % (Windows' largest) from the first frame.
fn warm_painted_225() -> (Run, Vec<Step>) {
    warmed(Run::painted_text_scaled(Shell::new(), Theme::LIGHT, 2.25))
}

/// A dark contrast theme's system colors, as Night sky's are: all high contrast paints with.
const CONTRAST: dpro_ui::SysColors = {
    use dpro_ui::Color;
    dpro_ui::SysColors {
        window: Color::hex(0x00_0000),
        window_text: Color::hex(0xFF_FFFF),
        highlight: Color::hex(0x8E_E3F0),
        highlight_text: Color::hex(0x26_3B50),
        hot_light: Color::hex(0x75_E9FC),
        gray_text: Color::hex(0xA6_A6A6),
        btn_face: Color::hex(0x00_0000),
        btn_text: Color::hex(0xFF_FFFF),
    }
};

/// Warm and painted in high contrast at scale 1 (TD-0009 A4.5's audit, slice 7d).
fn warm_painted_contrast() -> (Run, Vec<Step>) {
    warmed(Run::painted(
        Shell::new(),
        1.0,
        Theme::high_contrast(CONTRAST),
    ))
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
#[bench::find_row(args = (Step::Find(FAR_NAME)), setup = accessible_with)]
#[bench::column_drag(args = (30.0), setup = divider_with)]
#[bench::column_fit(args = (Step::Update(Msg::Table(TableEvent::Fit(Some(ColumnId(1)))))), setup = warm_with)]
#[bench::column_menu(args = (Step::Input(Input::ContextMenu(None))), setup = header_with)]
#[bench::formats_change(args = (Step::Decimal(",")), setup = warm_with)]
#[bench::drawer_close(args = (key(KeyCode::Enter, true, Mods::default())), setup = drawer_return_with)]
#[bench::remove_focused(args = (Step::Update(Msg::Cleaned(3))), setup = card_with)]
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
#[bench::palette_scrim(args = (Step::Scrim(true)), setup = warm_painted_with)]
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
#[bench::contrast_100(setup = warm_painted_contrast)]
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

/// What a popup window shows as it first opens: row 4's menu, or slice 6c's palette or dialog
/// over the composites page (A3.7).
enum Opening {
    Menu(Shell),
    Palette(Box<composites::Model>),
    Dialog(Box<composites::Model>),
}

/// The popup's `Ui`, its pixels and what it showed, handed back so dropping them isn't counted.
#[allow(dead_code, reason = "held only so dropping them isn't counted")]
enum Opened {
    Menu(Ui<Msg>, Vec<u32>, Shell),
    Overlay(Ui<composites::Msg>, Vec<u32>, Box<composites::Model>),
}

fn menu_opening() -> Opening {
    Opening::Menu(menu_shell())
}

/// The composites page's model with the palette open.
fn palette_opening() -> Opening {
    let mut m = composites::Model::new();
    m.update(composites::Msg::PaletteOpen);
    Opening::Palette(Box::new(m))
}

/// The composites page's model with the dialog open.
fn dialog_opening() -> Opening {
    let mut m = composites::Model::new();
    m.update(composites::Msg::DialogOpen);
    Opening::Dialog(Box::new(m))
}

/// A popup's `Ui`, empty.
fn popup_ui<M>() -> Ui<M> {
    let motion = Motion {
        hover_ms: tokens::motion::HOVER_MS,
    };
    Ui::new(Size { w: 0.0, h: 0.0 }, 1.0, motion)
}

/// What the view built, its first Tab stop focused (opened from the keyboard), sized to its
/// content: `w` wide if given (the palette's and the dialog's width), else its own width in at
/// most the window's; laid out and painted whole.
fn first_show<M>(ui: &mut Ui<M>, w: Option<f32>) -> Vec<u32> {
    ui.focus_first(true);
    let size = ui.natural_size(&mut FakeText, w.unwrap_or(START.w));
    ui.set_viewport(w.map_or(size, |w| Size { w, h: size.h }));
    ui.layout(&mut FakeText);
    let (w, h) = ui.pixel_size();
    let mut pixels = vec![0; usize::try_from(u64::from(w) * u64::from(h)).unwrap_or(0)];
    if let Some(mut canvas) = Canvas::new(&mut pixels, w, h) {
        ui.paint(&mut canvas, &mut FakeText, &Theme::LIGHT);
    }
    pixels
}

// A popup window's first show (U3 without presenting; U15 for the palette): its `Ui` made, the
// view built, its first Tab stop focused (a menu's first item, the palette's field, the dialog's
// initial button), sized to its content, laid out, and painted whole.
#[library_benchmark]
#[bench::menu(setup = menu_opening)]
#[bench::palette(setup = palette_opening)]
#[bench::dialog(setup = dialog_opening)]
fn open(opening: Opening) -> Opened {
    match opening {
        Opening::Menu(shell) => {
            let mut ui = popup_ui();
            ui.build(|cx| popup_view(black_box(&shell), cx));
            let pixels = first_show(&mut ui, None);
            Opened::Menu(ui, pixels, shell)
        }
        Opening::Palette(m) => {
            let mut ui = popup_ui();
            ui.build(|cx| composites::palette_view(black_box(&m), cx));
            let pixels = first_show(&mut ui, Some(tokens::palette::WIDTH));
            Opened::Overlay(ui, pixels, m)
        }
        Opening::Dialog(m) => {
            let mut ui = popup_ui();
            ui.build(composites::dialog_view);
            let pixels = first_show(&mut ui, Some(tokens::dialog::WIDTH));
            Opened::Overlay(ui, pixels, black_box(m))
        }
    }
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

/// [`warm_accessible`], the drawer opened from "Review changes" by the keyboard and its Cancel
/// focused; then Enter on Cancel with its update left to take: the drawer goes, and focus goes back
/// to "Review changes" (TD-0009 A4.4, A4.7; slice 7c).
fn accessible_restore() -> Box<Run> {
    let mut run = warm_accessible().drawer_from_keyboard();
    run.take_a11y = false;
    run.step(key(KeyCode::Enter, true, Mods::default()));
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

/// A whole tree to serve: the shell's, its open menu's, or the composites page's.
enum Tree {
    Shell(Box<Run>),
    Menu(Box<Run>),
    Composites(Box<composites::Run>),
}

/// [`warm_accessible`], for the shell's whole tree.
fn shell_tree() -> Tree {
    Tree::Shell(Box::new(warm_accessible()))
}

/// [`warm_accessible`] with row 4's menu open, for the menu window's whole tree.
fn menu_tree() -> Tree {
    Tree::Menu(Box::new(warm_accessible().menu_open()))
}

/// The composites page after its session, a client listening, for its whole tree (A3.7).
fn composites_tree() -> Tree {
    Tree::Composites(Box::new(
        composites::Run::accessible(composites::Model::new()).warmed(),
    ))
}

#[library_benchmark]
#[bench::shell(setup = shell_tree)]
#[bench::menu(setup = menu_tree)]
#[bench::composites(setup = composites_tree)]
fn a11y_tree(mut tree: Tree) -> (TreeUpdate, Tree) {
    let update = match &mut tree {
        Tree::Shell(run) => run.ui.a11y_tree(),
        Tree::Menu(run) => run.popup.a11y_tree(),
        Tree::Composites(run) => run.ui.a11y_tree(),
    };
    (update, tree)
}

#[library_benchmark]
#[bench::unchanged(args = (None, Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 }))), setup = accessible_after)]
#[bench::row_selection(args = (None, Step::Update(Msg::Row(5))), setup = accessible_after)]
#[bench::page_away(args = (None, Step::Update(Msg::Page(1))), setup = accessible_after)]
#[bench::page_back(args = (Some(Step::Update(Msg::Page(1))), Step::Update(Msg::Page(0))), setup = accessible_after)]
#[bench::progress(args = (Some(Step::Update(Msg::Drawer)), Step::Update(Msg::Progress(41))), setup = accessible_after)]
#[bench::scroll(setup = accessible_scroll)]
#[bench::column_hide(args = (None, Step::Update(Msg::Table(TableEvent::Show(ColumnId(2), false)))), setup = accessible_after)]
#[bench::focus_restore(setup = accessible_restore)]
fn a11y_update(mut run: Box<Run>) -> (Option<TreeUpdate>, Box<Run>) {
    (run.a11y_update(), run)
}

/// Slice 6c: the composites page after its session and the overlays', a client listening, then
/// the palette open (`window` the popup) or a job's toast at 40 % then 41 %, its line not yet
/// brought up to date (the toast), then `step` with that window's update left to take.
fn overlay_accessible_after(
    window: composites::Window,
    step: composites::Step,
) -> (Box<composites::Run>, composites::Window) {
    let run = overlaid(composites::Run::accessible(composites::Model::new()));
    let mut run = if window == composites::Window::Toast {
        let mut run = run.job_running();
        run.step(composites::Step::Update(composites::Msg::Progress(41)));
        run
    } else {
        run.palette_open()
    };
    run.take_a11y = false;
    run.step(step);
    (Box::new(run), window)
}

// U12 for slice 6c's windows: Down in the palette (the highlight moves, the field's active
// descendant with it), and the toast's line brought up to date (a polite live region). A function
// of its own, so `a11y_update`'s cases keep their one window (a sum type would cost its small
// cases about 4 Ir, more than `unchanged`'s 5 %).
#[library_benchmark]
#[bench::palette_arrow(args = (composites::Window::Popup, composites::popup_key(KeyCode::Down)), setup = overlay_accessible_after)]
#[bench::toast_line(args = (composites::Window::Toast, composites::Step::Update(composites::Msg::JobLine)), setup = overlay_accessible_after)]
fn a11y_window_update(
    (mut run, window): (Box<composites::Run>, composites::Window),
) -> (Option<TreeUpdate>, Box<composites::Run>) {
    (run.a11y_update_of(window), run)
}

library_benchmark_group!(
    name = a11y,
    benchmarks = [a11y_tree, a11y_table_tree, a11y_update, a11y_window_update]
);

/// The composites page after its session, settled, then `step` to run. Boxed, as the U12 runs
/// are, so moving it isn't counted.
fn composites_with(step: composites::Step) -> (Box<composites::Run>, composites::Step) {
    (
        Box::new(composites::Run::with(composites::Model::new()).warmed()),
        step,
    )
}

// The batch bar's count changes, as a row's check box toggles: its label alone changes.
#[library_benchmark]
#[bench::three_to_four(args = (composites::Step::Update(composites::Msg::Count(4))), setup = composites_with)]
fn batch_count((mut run, step): (Box<composites::Run>, composites::Step)) -> Box<composites::Run> {
    run.step(step);
    run
}

// A scan card's size arrives, 1.2 GB to 1.3 GB: its fixed slot alone changes.
#[library_benchmark]
#[bench::one_card(args = (composites::Step::Update(composites::Msg::Size(0, 1_395_864_371))), setup = composites_with)]
fn scan_size((mut run, step): (Box<composites::Run>, composites::Step)) -> Box<composites::Run> {
    run.step(step);
    run
}

// The window narrowed from 1000 to 640 px: the summary cards wrap two to a line, and the page
// lays out again.
#[library_benchmark]
#[bench::narrow(args = (composites::Step::Resize(composites::NARROW)), setup = composites_with)]
fn cards_wrap_resize(
    (mut run, step): (Box<composites::Run>, composites::Step),
) -> Box<composites::Run> {
    run.step(step);
    run
}

// ---- Slice 6c (A3.7: U14, U17) ---------------------------------------------------------------

/// The composites page after its session and slice 6c's (the palette, a job and the dialog,
/// opened and closed), settled.
fn overlaid(mut run: composites::Run) -> composites::Run {
    run = run.warmed();
    for s in composites::Run::overlay_session() {
        run.step(s);
    }
    run.settle();
    run
}

/// [`overlaid`] with the palette open, then `step`.
fn palette_with(step: composites::Step) -> (Box<composites::Run>, composites::Step) {
    let run = overlaid(composites::Run::with(composites::Model::new()));
    (Box::new(run.palette_open()), step)
}

/// [`palette_with`], painted in the light theme: the page under the scrim, and the palette.
fn palette_painted_with(step: composites::Step) -> (Box<composites::Run>, composites::Step) {
    let run = overlaid(composites::Run::painted(
        composites::Model::new(),
        Theme::LIGHT,
    ));
    (Box::new(run.palette_open()), step)
}

/// [`overlaid`] with a job at 40 %, its toast and its card showing, then `step`.
fn job_with(step: composites::Step) -> (Box<composites::Run>, composites::Step) {
    let run = overlaid(composites::Run::with(composites::Model::new()));
    (Box::new(run.job_running()), step)
}

// U14's view part: a character typed in the palette's field. The results filter, its 50 rows are
// built again, and the popup is laid out and painted (the page under the scrim doesn't change).
#[library_benchmark]
#[bench::one_char(args = (composites::popup_char('c')), setup = palette_painted_with)]
fn palette_keystroke(
    (mut run, step): (Box<composites::Run>, composites::Step),
) -> Box<composites::Run> {
    run.step(step);
    run
}

// Down in the palette: the highlight moves to the next row, the field keeping the focus.
#[library_benchmark]
#[bench::down(args = (composites::popup_key(KeyCode::Down)), setup = palette_with)]
fn palette_arrow(
    (mut run, step): (Box<composites::Run>, composites::Step),
) -> Box<composites::Run> {
    run.step(step);
    run
}

// U17: one progress event, 40 to 41 %, with the toast and the job card both showing it: their
// two bars change.
#[library_benchmark]
#[bench::both(args = (composites::Step::Update(composites::Msg::Progress(41))), setup = job_with)]
fn toast_progress(
    (mut run, step): (Box<composites::Run>, composites::Step),
) -> Box<composites::Run> {
    run.step(step);
    run
}

library_benchmark_group!(
    name = composite,
    benchmarks = [
        batch_count,
        scan_size,
        cards_wrap_resize,
        palette_keystroke,
        palette_arrow,
        toast_progress
    ]
);

/// U18's benches (TD-0009 A3.7, slice 6b), in a module of their own: the group's `fling` and
/// `event` are named as A3.7 names them, beside `table`'s and `interact`'s.
mod grouped_rows {
    use std::hint::black_box;

    use dpro_ui::{Input, KeyCode, KeyEvent, Mods};
    use gungraun::{library_benchmark, library_benchmark_group};

    use super::fling_table::{GROUP_ROWS, Grouped, Step, grid_row};

    /// U18's table at scale 1, its first frame painted, and the frames `table::fling.frames`
    /// counts: the script's first 60 and 30 of its thumb drag.
    fn fling_frames() -> (Box<Grouped>, Vec<f32>) {
        let f = Grouped::new(1.0);
        let script = f.script();
        let mut frames: Vec<f32> = script.frames.iter().take(60).copied().collect();
        frames.extend(script.frames.iter().skip(script.drag).take(30));
        (Box::new(f), frames)
    }

    #[library_benchmark]
    #[bench::frames(setup = fling_frames)]
    fn fling((mut f, frames): (Box<Grouped>, Vec<f32>)) -> (Box<Grouped>, Vec<f32>) {
        for &offset in &frames {
            f.frame(black_box(offset));
        }
        (f, frames)
    }

    /// A key pressed, as the platform delivers it.
    const fn key(key: KeyCode) -> Step {
        Step::Input(Input::Key(KeyEvent {
            key,
            down: true,
            mods: Mods {
                shift: false,
                ctrl: false,
                alt: false,
                win: false,
            },
        }))
    }

    /// U18's table warmed, frames ending at layout and damage, group 0's last row current and
    /// focused at the view's bottom; then `step`.
    fn group_end_with(step: Step) -> (Box<Grouped>, Step) {
        let f = Grouped::new(1.0)
            .unpainted()
            .current_focused(GROUP_ROWS - 1);
        (Box::new(f), step)
    }

    /// U18's table warmed with a client listening; then `step`, its update made in its frame.
    fn accessible_with(step: Step) -> (Box<Grouped>, Step) {
        (Box::new(Grouped::new(1.0).unpainted().accessible()), step)
    }

    #[library_benchmark]
    #[bench::row_down(args = (key(KeyCode::Down)), setup = group_end_with)]
    #[bench::realize(args = (Step::Realize(grid_row(50_000))), setup = accessible_with)]
    fn event((mut f, step): (Box<Grouped>, Step)) -> Box<Grouped> {
        f.step(black_box(step));
        f
    }

    library_benchmark_group!(name = grouped, benchmarks = [fling, event]);
}

use grouped_rows::grouped;

// Slice 6d: the drawer frame (TD-0009 A3.5 (6), A3.7's U16). `fixtures/drawer.rs`'s page, warmed
// by its session. A3.7 names these `composite::drawer_open` and `composite::drawer_frame`; they're
// a group of their own, so the slices that add composites alongside never edit the same lines.

/// The drawer page after its session, settled and closed, framed at layout and damage (painted
/// too, if `painted`); and the step that opens the drawer.
fn drawer_closed(painted: bool) -> (Box<drawer_page::Run>, drawer_page::Step) {
    let model = drawer_page::Model::new();
    let run = if painted {
        drawer_page::Run::painted(model, Theme::LIGHT)
    } else {
        drawer_page::Run::with(model)
    };
    (
        Box::new(run.warmed()),
        drawer_page::Step::Update(drawer_page::Msg::Open),
    )
}

/// The drawer just opened, 5 frames into its slide; and the 6th frame.
fn drawer_sliding(painted: bool) -> (Box<drawer_page::Run>, drawer_page::Step) {
    let (mut run, open) = drawer_closed(painted);
    run.step(open);
    for _ in 0..5 {
        run.step(drawer_page::Step::Tick(drawer_page::FRAME_MS));
    }
    (run, drawer_page::Step::Tick(drawer_page::FRAME_MS))
}

// "Review changes" opens the drawer as a pane: the page takes its new width, and the drawer is
// built and laid out once, at its place.
#[library_benchmark]
#[bench::pane(args = (false), setup = drawer_closed)]
#[bench::painted(args = (true), setup = drawer_closed)]
fn drawer_open(
    (mut run, step): (Box<drawer_page::Run>, drawer_page::Step),
) -> Box<drawer_page::Run> {
    run.step(black_box(step));
    run
}

// A frame of the slide: the paint moves and nothing is laid out. The frame damages where the
// drawer was and where it is (U16).
#[library_benchmark]
#[bench::mid_slide(args = (false), setup = drawer_sliding)]
#[bench::painted(args = (true), setup = drawer_sliding)]
fn drawer_frame(
    (mut run, step): (Box<drawer_page::Run>, drawer_page::Step),
) -> Box<drawer_page::Run> {
    run.step(black_box(step));
    run
}

library_benchmark_group!(
    name = composite_drawer,
    benchmarks = [drawer_open, drawer_frame]
);

// ---- notice (TD-0009 A4.7 U21, U22; slice 7b) --------------------------------------------------

/// [`warm_accessible`], the drawer open and its job started 2 s ago, then a progress report that
/// crosses a step: the announcer speaks, the shell writes the notice's text, the Ui queues it, and
/// the frame runs. A4.7's `interact::event.notice`.
fn noticed() -> (Box<Run>, Step) {
    let mut run = warm_accessible().drawer_open();
    run.step(Step::Tick(dpro_app::announce::JOB_GAP_MS));
    (Box::new(run), Step::Update(Msg::Progress(41)))
}

#[library_benchmark]
#[bench::progress_step(setup = noticed)]
fn notice_event((mut run, step): (Box<Run>, Step)) -> Box<Run> {
    run.step(black_box(step));
    run
}

/// The job toast's own window's `Ui`: slice 6c's [`Cx::toast`], the job running at 41 % or
/// failed. Slice 7c repointed it from 7b's stand-in (A4's 7b/7d integration note 4).
fn toast_view(cx: &mut Cx<'_, u8>, failed: bool) {
    cx.toast(
        Key::index(0),
        &Toast {
            tone: if failed {
                ToastTone::Failed
            } else {
                ToastTone::Running
            },
            region: "Job status",
            title: "Apply 20 changes",
            line: if failed {
                "Failed. 3 of 20 changes were applied."
            } else {
                "41 %"
            },
            progress: (!failed).then_some(0.41),
            actions: [failed.then_some(("View", 2)), None],
            dismiss: ("Dismiss", 1),
        },
    );
}

/// A running job's toast, a client listening, then the job fails: the line's text changes and
/// turns assertive, the glyph turns to the failure's, the bar goes and View comes. Its update is
/// A4.7's `a11y_update.live_toast` (U21: ≤ 5,000 Ir).
fn toast_failed() -> Box<Ui<u8>> {
    let size = Size {
        w: tokens::toast::WIDTH,
        h: 160.0,
    };
    let mut ui = Ui::new(size, 1.0, Motion::STANDARD);
    ui.build(|cx| toast_view(cx, false));
    ui.layout(&mut FakeText);
    drop(ui.a11y_tree());
    ui.build(|cx| toast_view(cx, true));
    ui.layout(&mut FakeText);
    Box::new(ui)
}

#[library_benchmark]
#[bench::failed(setup = toast_failed)]
fn live_toast(mut ui: Box<Ui<u8>>) -> (Option<TreeUpdate>, Box<Ui<u8>>) {
    (ui.a11y_update(), ui)
}

library_benchmark_group!(name = notice, benchmarks = [notice_event, live_toast]);

// ---- end notice --------------------------------------------------------------------------------

main!(
    // Instruction counts only: the cache simulation would slow every run for metrics nothing gates.
    config = LibraryBenchmarkConfig::default().tool(Callgrind::with_args(["--cache-sim=no"]));
    library_benchmark_groups = interact, paint, popup, table, a11y, composite, grouped,
        composite_drawer, notice
);
