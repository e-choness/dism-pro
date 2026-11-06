//! TD-0009 §12 item 7 and §4.4: Windows' text size (Settings › Accessibility › Text size, 100 to
//! 225 %) multiplies every text style, and layouts reflow rather than clip. On every view of the
//! shell fixture (the table page, the drawer open over it, a summary page and Settings), at 100 %
//! and at 225 %, no box with text is shorter than one line of it. A switch lays the page out
//! again once and leaves nothing running; setting the same size again changes nothing.

#![allow(missing_docs)]

#[path = "../benches/fixtures/shell.rs"]
mod shell;

use dpro_ui::{FakeText, NodeId, RectPx, Role, TextSystem, Ui};
use shell::{Msg, Run, SETTINGS, Shell, Step, view};

/// Windows' largest text size.
const LARGEST: f32 = 2.25;

/// The views every check walks, each reached from the one before.
const VIEWS: [(&str, Option<Msg>); 4] = [
    ("the table page", None),
    ("the drawer open", Some(Msg::Drawer)),
    ("a summary page", Some(Msg::Page(1))),
    ("Settings", Some(Msg::Page(SETTINGS))),
];

/// Whether `role` draws text of its own.
fn has_text(role: Role) -> bool {
    matches!(
        role,
        Role::Label | Role::Button | Role::Tab | Role::CheckBox | Role::Radio | Role::Field
    )
}

/// Every node with text under `id` whose box is shorter than one line of its style, described;
/// returns how many nodes with text it saw.
fn clipped(ui: &Ui<Msg>, id: NodeId, out: &mut Vec<String>) -> usize {
    let mut seen = 0;
    for c in ui.children(id) {
        let Some(n) = ui.info(c) else { continue };
        if has_text(n.role) {
            seen += 1;
            let line = FakeText.measure("", &n.text_style, f32::INFINITY).height;
            if n.rect.h + 0.01 < line {
                out.push(format!(
                    "{:?} {:?}: {} px high, a line of its {} px text is {line}",
                    n.role, n.text, n.rect.h, n.text_style.size
                ));
            }
        }
        seen += clipped(ui, c, out);
    }
    seen
}

#[test]
fn no_text_is_clipped_on_any_view_at_100_or_225_percent() {
    for scale in [1.0, LARGEST] {
        let mut run = Run::text_scaled(Shell::new(), scale);
        let mut short = Vec::new();
        for (name, msg) in VIEWS {
            if let Some(m) = msg {
                run.step(Step::Update(m));
            }
            let mut here = Vec::new();
            let seen = clipped(&run.ui, run.ui.root(), &mut here);
            assert!(seen > 0, "{name} at {scale}: no text at all");
            short.extend(here.into_iter().map(|s| format!("{name} at {scale}: {s}")));
        }
        assert_eq!(short, [] as [String; 0], "text clipped");
    }
}

/// The text sizes under `id`, in tree order.
fn sizes(ui: &Ui<Msg>, id: NodeId, out: &mut Vec<f32>) {
    for c in ui.children(id) {
        if let Some(n) = ui.info(c).filter(|n| has_text(n.role)) {
            out.push(n.text_style.size);
        }
        sizes(ui, c, out);
    }
}

#[test]
fn every_text_style_is_multiplied_by_the_text_size() {
    let (mut normal, mut largest) = (Vec::new(), Vec::new());
    let one = Run::text_scaled(Shell::new(), 1.0);
    sizes(&one.ui, one.ui.root(), &mut normal);
    let big = Run::text_scaled(Shell::new(), LARGEST);
    sizes(&big.ui, big.ui.root(), &mut largest);
    assert_eq!(normal.len(), largest.len(), "the same nodes either way");
    assert_ne!(normal.len(), 0, "text to measure");
    for (a, b) in normal.iter().zip(&largest) {
        assert!((a * LARGEST - b).abs() < 1e-4, "{a} px at 225 % is {b} px");
    }
    // Out of range or not a number, the size is clamped to Windows' range.
    for (asked, got) in [(0.5, 1.0), (3.0, LARGEST), (f32::NAN, 1.0)] {
        let run = Run::text_scaled(Shell::new(), asked);
        assert!(
            (run.ui.text_scale() - got).abs() < 1e-6,
            "{asked} gives {}",
            run.ui.text_scale()
        );
    }
}

#[test]
fn a_switch_lays_out_once_and_the_same_size_again_changes_nothing() {
    let mut run = Run::with(Shell::new());
    for s in run.session() {
        run.step(s);
    }
    // The session's hover fades end.
    run.settle();
    let tall = |run: &Run| {
        let header = run.ui.children(run.ui.root()).next().expect("a header");
        run.ui.info(header).expect("its info").rect.h
    };
    let before = tall(&run);

    // Larger: everything is laid out and painted again in the frame that sees it, and then
    // nothing is left running.
    run.ui.set_text_scale(LARGEST);
    run.ui.build(|cx| view(&run.shell, cx));
    run.ui.layout(&mut FakeText);
    let (w, h) = run.ui.pixel_size();
    let window = RectPx {
        x0: 0,
        y0: 0,
        x1: i32::try_from(w).expect("width"),
        y1: i32::try_from(h).expect("height"),
    };
    let damaged = run.ui.damage().iter().copied().reduce(RectPx::union);
    assert_eq!(
        damaged.map(|d| d.intersect(window)),
        Some(window),
        "a text size switch repaints the whole window"
    );
    run.ui.clear_damage();
    assert!(tall(&run) > before, "the header grows with its text");
    // The layout moved under the pointer, so a hover may fade; within a second nothing runs.
    for _ in 0..60 {
        if !run.ui.wants_frame() {
            break;
        }
        run.step(Step::Tick(16));
    }
    assert!(
        !run.ui.wants_frame(),
        "something still runs a second after a switch"
    );

    // The same size again: no change, no damage.
    run.ui.set_text_scale(LARGEST);
    run.ui.build(|cx| view(&run.shell, cx));
    run.ui.layout(&mut FakeText);
    assert_eq!(
        run.ui.damage().len(),
        0,
        "the same text size damaged something"
    );

    // And back.
    run.step(Step::TextScale(1.0));
    assert!(
        (tall(&run) - before).abs() < 1e-4,
        "back to 100 %, the header is as it was"
    );
}
