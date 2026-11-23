//! Badges (TD-0009 A3.5 (1), A3.9): a tone per spec, high contrast's border, the description in the
//! accessibility tree, and a badge is text, never a Tab stop.

#![allow(missing_docs)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use std::collections::HashSet;

use composites::{BADGE_SPECS, BADGES, Model, PAGE, Run};
use dpro_ui::{
    Align, Axis, Canvas, Color, Element, FakeText, Key, Motion, Rect, Role, Size, Style, SysColors,
    Theme, Tone, Ui,
    accesskit::{Action, Role as AkRole},
    composite::BadgeTone,
    tokens::{self, badge, font},
};

const ALL: [BadgeTone; 5] = [
    BadgeTone::Undoable,
    BadgeTone::Permanent,
    BadgeTone::Neutral,
    BadgeTone::Info,
    BadgeTone::Caution,
];

const KEY: Key = Key::name("badge");
/// The badge, in a row that lets it be as wide as its text.
const BADGE: [Key; 2] = [Key::name("row"), KEY];

/// A badge alone in a Ui, laid out.
fn alone(tone: BadgeTone) -> Ui<()> {
    let mut ui = Ui::new(
        Size { w: 400.0, h: 100.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    );
    ui.build(|cx| {
        cx.add_with(
            Element::stack(Key::name("row")).style(Style {
                axis: Axis::Row,
                align: Align::Start,
                ..Style::default()
            }),
            |cx| {
                cx.add(Element::badge(KEY, "Undoable", tone));
            },
        );
    });
    ui.layout(&mut FakeText);
    ui
}

/// A badge alone, painted in `theme`: its box, and the pixels at its left padding's middle (its
/// fill) and its left edge's middle (its border, if any).
fn painted(tone: BadgeTone, theme: &Theme) -> (Rect, u32, u32) {
    let ui = alone(tone);
    let rect = ui
        .find(&BADGE)
        .and_then(|id| ui.info(id))
        .map_or(Rect::default(), |n| n.rect);
    let (w, h) = ui.pixel_size();
    let mut pixels = vec![0; usize::try_from(w * h).unwrap_or(0)];
    if let Some(mut canvas) = Canvas::new(&mut pixels, w, h) {
        ui.paint(&mut canvas, &mut FakeText, theme);
    }
    let at = |x: f32, y: f32| {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a pixel inside a 400 × 100 window"
        )]
        let (x, y) = (x as u32, y as u32);
        pixels
            .get(usize::try_from(y * w + x).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or(0)
    };
    let mid = rect.y + rect.h / 2.0;
    (rect, at(rect.x + badge::PAD_X / 2.0, mid), at(rect.x, mid))
}

/// `c` as an opaque canvas pixel.
fn px(c: Color) -> u32 {
    0xFF00_0000 | u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
}

#[test]
fn a_badge_is_12_px_semibold_text_padded_3_by_8() {
    let ui = alone(BadgeTone::Undoable);
    let n = ui
        .find(&BADGE)
        .and_then(|id| ui.info(id))
        .expect("the badge");
    assert_eq!(n.role, Role::Label);
    assert!((n.text_style.size - font::CAPTION).abs() < 1e-6);
    assert_eq!(n.text_style.weight, font::SEMIBOLD);
    // FakeText: half the size per character, lines 1.25 high.
    let text_w = 8.0 * font::CAPTION * 0.5;
    let line = font::CAPTION * 1.25;
    assert!(
        (n.rect.w - (text_w + 2.0 * badge::PAD_X)).abs() < 0.01,
        "{:?}",
        n.rect
    );
    assert!(
        (n.rect.h - (line + 2.0 * badge::PAD_Y)).abs() < 0.01,
        "{:?}",
        n.rect
    );
}

#[test]
fn badges_are_text_and_never_a_tab_stop() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    for (i, spec) in (0..).zip(BADGE_SPECS) {
        let id = run.find(&[PAGE, BADGES, Key::index(i)]).expect("a badge");
        let node = run.ak(&tree, id).expect("in the tree");
        assert_eq!(node.role(), AkRole::Label, "{}", spec.text);
        assert_eq!(node.value().or(node.label()), Some(spec.text));
        assert!(
            !node.supports_action(Action::Focus),
            "{} takes focus",
            spec.text
        );
    }
}

#[test]
fn a_badges_description_is_in_the_accessibility_tree() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    for (i, spec) in (0..).zip(BADGE_SPECS) {
        let id = run.find(&[PAGE, BADGES, Key::index(i)]).expect("a badge");
        let node = run.ak(&tree, id).expect("in the tree");
        assert_eq!(node.description(), spec.description, "{}", spec.text);
    }
}

#[test]
fn each_tone_has_its_own_fill_from_the_token_table() {
    // Decision #45's pairs: five tones, five distinct fills, in light and in dark.
    for theme in [Theme::LIGHT, Theme::DARK] {
        let mut fills = HashSet::new();
        for tone in ALL {
            let (_, fill, _) = painted(tone, &theme);
            assert!(
                fills.insert(fill),
                "{tone:?}'s fill {fill:#x} isn't its own"
            );
        }
    }
    let light = |tone| painted(tone, &Theme::LIGHT).1;
    assert_eq!(light(BadgeTone::Undoable), px(Color::hex(0xE7_F6EC)));
    assert_eq!(light(BadgeTone::Caution), px(Color::hex(0xFE_F6E7)));
    // Permanent reuses the permanent pair.
    assert_eq!(
        light(BadgeTone::Permanent),
        px(Theme::LIGHT.color(Tone::PermanentFill))
    );
}

#[test]
fn high_contrast_draws_no_fill_and_a_border_in_the_text_color() {
    let sys = SysColors {
        window: Color::hex(0x00_0000),
        window_text: Color::hex(0xFF_FFFF),
        highlight: Color::hex(0x1A_EBFF),
        highlight_text: Color::hex(0x00_0000),
        hot_light: Color::hex(0xFF_FF00),
        gray_text: Color::hex(0x3F_F23F),
        btn_face: Color::hex(0x20_2040),
        btn_text: Color::hex(0xFF_FFFE),
    };
    let theme = Theme::high_contrast(sys);
    for tone in ALL {
        let (rect, fill, edge) = painted(tone, &theme);
        assert_eq!(fill, px(sys.window), "{tone:?} at {rect:?}: a fill");
        assert_eq!(edge, px(sys.window_text), "{tone:?} at {rect:?}: no border");
    }
}
