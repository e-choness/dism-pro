//! Slice 6a's layout additions that aren't wrapping (TD-0009 A3.5 additions 1, 2 and 9's layout):
//! the icons' code points, a glyph laid out and drawn at its text style's size, a pill's radius,
//! and a check box holding its label lines after its box.

#![allow(missing_docs)]

use dpro_ui::{
    Canvas, Color, Element, FakeText, Icon, Key, Look, Motion, Radius, Rect, Size, Style,
    TextStyle, Theme, Tone, Tri, Ui,
    tokens::{self, font},
};

fn ui() -> Ui<u8> {
    Ui::new(
        Size { w: 400.0, h: 200.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    )
}

/// `c` as an opaque canvas pixel.
fn px(c: Color) -> u32 {
    0xFF00_0000 | u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
}

/// `ui` painted whole in the light theme, and a lookup of the pixel at a logical point.
fn paint(ui: &Ui<u8>) -> impl Fn(f32, f32) -> u32 {
    let (w, h) = ui.pixel_size();
    let mut pixels = vec![0; usize::try_from(w * h).unwrap_or(0)];
    if let Some(mut canvas) = Canvas::new(&mut pixels, w, h) {
        ui.paint(&mut canvas, &mut FakeText, &Theme::LIGHT);
    }
    move |x: f32, y: f32| {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a point inside the window"
        )]
        let (x, y) = (x as u32, y as u32);
        pixels
            .get(usize::try_from(y * w + x).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or(0)
    }
}

fn rect(ui: &Ui<u8>, path: &[Key]) -> Rect {
    ui.find(path)
        .and_then(|id| ui.info(id))
        .map_or(Rect::default(), |n| n.rect)
}

#[test]
fn every_icon_has_its_code_point_in_both_fonts() {
    // A3.3's verified table: Segoe Fluent Icons and Segoe MDL2 Assets share these.
    let want: [(Icon, u32); 14] = [
        (Icon::CheckMark, 0xE73E),
        (Icon::ChevronDown, 0xE70D),
        (Icon::ChevronUp, 0xE70E),
        (Icon::Search, 0xE721),
        (Icon::Cancel, 0xE711),
        (Icon::ChevronRight, 0xE76C),
        (Icon::Completed, 0xE930),
        (Icon::ErrorBadge, 0xEA39),
        (Icon::Warning, 0xE7BA),
        (Icon::Info, 0xE946),
        (Icon::Undo, 0xE7A7),
        (Icon::Folder, 0xE8B7),
        (Icon::Up, 0xE74A),
        (Icon::More, 0xE712),
    ];
    assert_eq!(Icon::ALL.len(), want.len());
    for ((icon, code), all) in want.iter().zip(Icon::ALL) {
        assert_eq!(*icon, all, "Icon::ALL is in declaration order");
        assert_eq!(icon.codepoint(), *code, "{icon:?}");
        // The Private Use Area, where both fonts put their glyphs.
        assert!((0xE000..=0xF8FF).contains(code), "{icon:?}");
    }
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn a_glyph_is_a_line_square_and_drawn_in_its_ink() {
    let mut ui = ui();
    let style = TextStyle {
        size: font::SUBTITLE,
        ..TextStyle::default()
    };
    ui.build(|cx| {
        cx.add(
            Element::glyph(Key::name("g"), Icon::Warning)
                .text_style(style)
                .look(Look {
                    ink: Tone::Warn,
                    ..Look::PLAIN
                }),
        );
    });
    ui.layout(&mut FakeText);
    let r = rect(&ui, &[Key::name("g")]);
    let line = font::SUBTITLE * 1.25;
    assert!(
        (r.w - line).abs() < 0.01 && (r.h - line).abs() < 0.01,
        "{r:?}"
    );
    let at = paint(&ui);
    let c = r.center();
    assert_eq!(at(c.x, c.y), px(Theme::LIGHT.color(Tone::Warn)));
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn a_pill_is_rounded_by_half_its_shorter_side() {
    let mut ui = ui();
    ui.build(|cx| {
        cx.add(
            Element::stack(Key::name("pill"))
                .style(Style {
                    min: Size { w: 120.0, h: 24.0 },
                    ..Style::default()
                })
                .look(Look {
                    fill: Some(Tone::Accent),
                    radius: Radius::Pill,
                    ..Look::PLAIN
                }),
        );
    });
    ui.layout(&mut FakeText);
    let r = rect(&ui, &[Key::name("pill")]);
    let at = paint(&ui);
    let accent = px(Theme::LIGHT.color(Tone::Accent));
    // A 12 px radius leaves the corner pixel and the one 3 px in unpainted; the middle of either
    // end is painted.
    assert_ne!(at(r.x + 0.5, r.y + 0.5), accent, "the corner is square");
    assert_ne!(
        at(r.x + 3.0, r.y + 1.0),
        accent,
        "the corner is barely rounded"
    );
    assert_eq!(at(r.x + 0.5, r.y + r.h / 2.0), accent, "the end's middle");
    assert_eq!(at(r.x + r.w / 2.0, r.y + 0.5), accent, "the top's middle");
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn a_check_box_holds_its_label_lines_after_its_box() {
    let mut ui = ui();
    ui.build(|cx| {
        cx.add_with(
            Element::toggle(Key::name("check"), "Temporary files", Tri::On, 1),
            |cx| {
                cx.add(Element::text(Key::name("name"), "Temporary files"));
                cx.add(Element::text(Key::name("what"), "Files apps left behind"));
            },
        );
    });
    ui.layout(&mut FakeText);
    let check = rect(&ui, &[Key::name("check")]);
    let name = rect(&ui, &[Key::name("check"), Key::name("name")]);
    let what = rect(&ui, &[Key::name("check"), Key::name("what")]);
    let line = font::BODY * 1.25;
    assert!(
        name.x + 0.01 >= check.x + line * 1.5,
        "after the box: {name:?} in {check:?}"
    );
    assert!(
        what.y >= name.y + name.h - 0.01,
        "the lines stack: {what:?} {name:?}"
    );
    // The box is painted at the start, a line square, in the accent while on.
    let at = paint(&ui);
    assert_eq!(
        at(check.x + 2.0, name.y + line / 2.0),
        px(Theme::LIGHT.color(Tone::Accent))
    );
}
