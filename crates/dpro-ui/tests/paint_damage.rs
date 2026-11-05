//! TD-0009 §10: damage covers every changed pixel. After every step of the scripted session, and
//! after every tick of the hover fades it starts, the incrementally painted canvas must equal the
//! whole window painted from scratch, pixel for pixel. At scale 1 in light, and at 1.5 in dark,
//! where node edges fall between pixels.

#![allow(missing_docs)]

#[path = "../benches/fixtures/shell.rs"]
mod shell;

use dpro_ui::Theme;
use shell::{Run, Shell, Step};

fn check(scale: f32, theme: Theme) {
    let mut run = Run::painted(Shell::new(), scale, theme);
    let mut steps = Vec::new();
    for s in run.session() {
        steps.push(s);
        // The hover fade is 120 ms: ticks at 50, 100 and 150 ms show it partway and done.
        steps.extend([Step::Tick(50); 3]);
    }
    let mut full = Vec::new();
    for (n, &s) in steps.iter().enumerate() {
        run.step(s);
        run.paint_all_into(&mut full);
        let (painted, width) = run.pixels();
        assert_eq!(painted.len(), full.len(), "step {n}");
        let differs = painted.iter().zip(&full).position(|(a, b)| a != b);
        let (i, width) = (differs.unwrap_or(0), usize::try_from(width).unwrap_or(1));
        assert!(
            differs.is_none(),
            "scale {scale}, step {n} ({s:?}): pixel ({}, {}) is {:08x}, a full repaint gives \
             {:08x}",
            i % width,
            i / width,
            painted[i],
            full[i]
        );
    }
}

#[test]
fn incremental_painting_matches_a_full_repaint_at_scale_1() {
    check(1.0, Theme::LIGHT);
}

#[test]
fn incremental_painting_matches_a_full_repaint_at_scale_1_5() {
    check(1.5, Theme::DARK);
}
