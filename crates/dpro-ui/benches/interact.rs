//! TD-0009 §7 U2 and U3 on a shell page of 390 to 476 nodes (`fixtures/shell.rs`):
//! - `interact`, U2: UI-thread work per input event without raster: dispatch → update → view →
//!   reconcile → layout → damage. Budget: p99 ≤ 2 ms per event (doc 04 §5), moving within a menu
//!   (TD-0009 A1.7) and a wheel notch over the table (A2, slice 5a) included.
//! - `paint`, U3 without presenting: the same events with the damage painted into a 1000 × 700
//!   canvas, and whole-window repaints. Budget: input to visual ≤ 50 ms (doc 04 §2); presenting is
//!   measured on G1.
//!
//! C1: `docker compose run --rm bench -p dpro-ui --bench interact`. Advisory timings; the gate is
//! the instruction count in `ir.rs`.

#![allow(missing_docs)]

#[path = "fixtures/shell.rs"]
mod shell;

use std::hint::black_box;

use criterion::{
    BatchSize, BenchmarkGroup, Criterion, Throughput, criterion_group, criterion_main,
    measurement::WallTime,
};
use dpro_ui::{Input, KeyCode, Mods, Point, Size, Theme};
use shell::{Msg, Run, START, Shell, Step, click, key, popup_key};

/// The session, an unchanged event, a filter keystroke, a resize, a tab picked with the arrow
/// keys, a progress report, a text size switch, an arrow in a row's menu and a wheel notch over
/// the table, on `run`.
#[expect(
    clippy::too_many_lines,
    reason = "one bench per event, in the session's order"
)]
fn events(g: &mut BenchmarkGroup<'_, WallTime>, run: &mut Run) {
    let steps = run.session();

    // The scripted session, reported per event.
    g.throughput(Throughput::Elements(
        u64::try_from(steps.len()).unwrap_or(u64::MAX),
    ));
    g.bench_function("session_per_event", |b| {
        b.iter(|| {
            for s in &steps {
                run.step(black_box(s.clone()));
            }
        });
    });

    g.throughput(Throughput::Elements(1));
    // Nothing changes: the floor every event pays (view + reconcile compares).
    let outside = Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 }));
    g.bench_function("event_unchanged", |b| {
        b.iter(|| run.step(black_box(outside.clone())));
    });

    // A keystroke in the filter field refilters the table: rows move, appear and disappear.
    // Typed, then deleted with Backspace.
    for s in run.focus_filter() {
        run.step(s);
    }
    g.throughput(Throughput::Elements(2));
    g.bench_function("filter_keystroke", |b| {
        b.iter(|| {
            run.step(Step::Type('c'));
            run.step(key(KeyCode::Backspace, true, Mods::default()));
        });
    });
    let wide = Step::Resize(Size { w: 1400.0, ..START });
    let narrow = Step::Resize(START);
    g.bench_function("resize", |b| {
        b.iter(|| {
            run.step(wide.clone());
            run.step(narrow.clone());
        });
    });

    // The standard controls (§11 step 4). Right then Left on the focused tab: each picks a tab,
    // and the table refilters.
    for s in click(run.center(&Run::tab(0))) {
        run.step(s);
    }
    run.settle();
    let (right, left) = (
        key(KeyCode::Right, true, Mods::default()),
        key(KeyCode::Left, true, Mods::default()),
    );
    g.bench_function("tab_arrow", |b| {
        b.iter(|| {
            run.step(right.clone());
            run.step(left.clone());
        });
    });
    // The apply job's progress, reported to the open drawer.
    run.step(Step::Update(Msg::Drawer));
    run.settle();
    g.bench_function("progress_report", |b| {
        b.iter(|| {
            run.step(Step::Update(Msg::Progress(41)));
            run.step(Step::Update(Msg::Progress(42)));
        });
    });
    run.step(Step::Update(Msg::Drawer));
    run.settle();

    // Windows' text size up to 225 % and back (§12 item 7): every text node measured again and
    // the whole page laid out, each way.
    g.bench_function("text_scale", |b| {
        b.iter(|| {
            run.step(Step::TextScale(2.25));
            run.step(Step::TextScale(1.0));
        });
    });

    // A row's context menu (TD-0009 A1.7): Down then Up in the open menu, each moving the focus
    // between two items, in the popup's Ui.
    let row = run.ui.find(&Run::row(4));
    run.ui.set_focus(row);
    run.step(Step::Input(Input::ContextMenu(None)));
    run.settle();
    let (down, up) = (
        popup_key(KeyCode::Down, true, Mods::default()),
        popup_key(KeyCode::Up, true, Mods::default()),
    );
    g.bench_function("menu_arrow", |b| {
        b.iter(|| {
            run.step(down.clone());
            run.step(up.clone());
        });
    });
    run.step(popup_key(KeyCode::Escape, true, Mods::default()));
    run.settle();

    // Scrolling (TD-0009 A2, slice 5a): a wheel notch down the table and one back, the pointer
    // resting on it.
    run.step(Step::Input(Input::PointerMove(run.center(&Run::TABLE))));
    run.settle();
    let (down, up) = (run.table_wheel(-120), run.table_wheel(120));
    g.bench_function("wheel_notch", |b| {
        b.iter(|| {
            run.step(down.clone());
            run.step(up.clone());
        });
    });
    run.step(Step::Input(Input::PointerLeave));
    run.settle();

    // The table (TD-0009 A2, slice 5b1): Down on the focused current row and Up back, each a
    // message that moves the current row, the focus following it.
    run.step(Step::Update(Msg::Row(4)));
    let row = run.ui.find(&Run::row(4));
    run.ui.set_focus(row);
    let (down, up) = (
        key(KeyCode::Down, true, Mods::default()),
        key(KeyCode::Up, true, Mods::default()),
    );
    g.bench_function("row_down", |b| {
        b.iter(|| {
            run.step(down.clone());
            run.step(up.clone());
        });
    });
    run.ui.set_focus(None);
    run.settle();
}

fn interact(c: &mut Criterion) {
    let mut g = c.benchmark_group("interact");
    events(&mut g, &mut Run::with(Shell::new()));

    // Startup's share: a new Ui, the first build and layout of every node.
    g.throughput(Throughput::Elements(1));
    g.bench_function("first_frame", |b| {
        b.iter_batched(
            Shell::new,
            |s| black_box(Run::with(s)),
            BatchSize::LargeInput,
        );
    });
    g.finish();
}

fn paint(c: &mut Criterion) {
    let mut g = c.benchmark_group("paint");
    events(&mut g, &mut Run::painted(Shell::new(), 1.0, Theme::LIGHT));

    g.throughput(Throughput::Elements(1));
    g.bench_function("first_frame", |b| {
        b.iter_batched(
            Shell::new,
            |s| black_box(Run::painted(s, 1.0, Theme::LIGHT)),
            BatchSize::LargeInput,
        );
    });
    // The whole window, as after a theme switch: 1000 × 700 pixels, then 1500 × 1050.
    for (name, scale) in [("repaint_all_100", 1.0), ("repaint_all_150", 1.5)] {
        let mut run = Run::painted(Shell::new(), scale, Theme::LIGHT);
        g.bench_function(name, |b| b.iter(|| run.repaint_all()));
    }
    g.finish();
}

criterion_group!(benches, interact, paint);
criterion_main!(benches);
