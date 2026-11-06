//! TD-0009 §10: damage covers every changed pixel. After every step of the scripted session, and
//! after every tick of the hover fades it starts, the incrementally painted canvas must equal the
//! whole window painted from scratch, pixel for pixel. At scale 1 in light, and at 1.5 in dark,
//! where node edges fall between pixels. Then the same for editing in the filter field (§11 step
//! 3): the caret moving and blinking, a selection, and a paste that scrolls the text; also in high
//! contrast, where selected text has its own color.

#![allow(missing_docs)]

#[path = "../benches/fixtures/shell.rs"]
mod shell;

use dpro_ui::{Caret, Color, KeyCode, Mods, SysColors, Theme};
use shell::{CTRL, Msg, PATH, Run, Shell, Step, key};

fn check(scale: f32, theme: Theme, steps: impl FnOnce(&Run) -> Vec<Step>) {
    let mut run = Run::painted(Shell::new(), scale, theme);
    let steps = steps(&run);
    let mut full = Vec::new();
    for (n, s) in steps.iter().enumerate() {
        run.step(s.clone());
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

/// The session, with ticks after every step: the hover fade is 120 ms, so ticks at 50, 100 and
/// 150 ms show it partway and done.
fn session(run: &Run) -> Vec<Step> {
    let mut steps = Vec::new();
    for s in run.session() {
        steps.push(s);
        steps.extend((0..3).map(|_| Step::Tick(50)));
    }
    steps
}

/// Editing in the filter: typing, the caret moving by character and word, selections by Shift
/// and Ctrl+A, a paste wider than the field (which scrolls its text), Home and End, the caret
/// blinking until it stops, and clearing.
fn editing(run: &Run) -> Vec<Step> {
    let plain = |k| key(k, true, Mods::default());
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    let blink = u64::from(Caret::WINDOWS.blink_ms);
    let mut steps = Vec::new();
    steps.extend(run.focus_filter());
    steps.extend("contoso up".chars().map(Step::Type));
    steps.extend([
        plain(KeyCode::Left),
        plain(KeyCode::Left),
        key(KeyCode::Left, true, CTRL),
        key(KeyCode::Home, true, shift),
        key(KeyCode::Right, true, CTRL),
        key(KeyCode::End, true, shift),
        Step::Paste(PATH),
        plain(KeyCode::Home),
        key(KeyCode::Right, true, CTRL),
        plain(KeyCode::End),
        key(KeyCode::Backspace, true, CTRL),
    ]);
    // Blinks, then the caret stays on.
    steps.extend((0..12).map(|_| Step::Tick(blink)));
    steps.extend([
        key(KeyCode::Other(u16::from(b'A')), true, CTRL),
        Step::Tick(blink),
        plain(KeyCode::Backspace),
    ]);
    steps
}

#[test]
fn incremental_painting_matches_a_full_repaint_at_scale_1() {
    check(1.0, Theme::LIGHT, session);
}

#[test]
fn incremental_painting_matches_a_full_repaint_at_scale_1_5() {
    check(1.5, Theme::DARK, session);
}

/// Windows' text size switched up and down (TD-0009 §12 item 7), with the drawer and a page
/// switch between, so every switch reflows a different page.
fn text_sizes(_: &Run) -> Vec<Step> {
    vec![
        Step::TextScale(2.25),
        Step::Update(Msg::Drawer),
        Step::TextScale(1.75),
        Step::Update(Msg::Drawer),
        Step::Update(Msg::Page(1)),
        Step::TextScale(1.0),
        Step::Update(Msg::Page(0)),
        Step::TextScale(1.25),
    ]
}

#[test]
fn a_text_size_switch_paints_exactly_its_damage() {
    check(1.0, Theme::LIGHT, text_sizes);
    check(1.5, Theme::DARK, text_sizes);
}

#[test]
fn editing_paints_exactly_its_damage() {
    check(1.0, Theme::LIGHT, editing);
    check(1.5, Theme::DARK, editing);
    // Distinct system colors, so selected text drawn in the wrong one would show.
    let sys = SysColors {
        window: Color::hex(0x00_0001),
        window_text: Color::hex(0x00_0002),
        highlight: Color::hex(0x00_0003),
        highlight_text: Color::hex(0x00_0004),
        hot_light: Color::hex(0x00_0005),
        gray_text: Color::hex(0x00_0006),
        btn_face: Color::hex(0x00_0007),
        btn_text: Color::hex(0x00_0008),
    };
    check(1.25, Theme::high_contrast(sys), editing);
}
