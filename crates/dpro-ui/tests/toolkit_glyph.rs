//! Slice 6a's layout additions that aren't wrapping (TD-0009 A3.5 additions 1, 2 and 9's layout):
//! the icons' code points, a glyph laid out and drawn at its text style's size, a pill's radius,
//! and a check box holding its label lines after its box, which a click on them toggles.

#![allow(missing_docs)]

use dpro_ui::{
    Align, Axis, Canvas, Color, Dim, Element, FakeText, Icon, Input, Insets, Key, Look, Motion,
    Radius, Rect, RectPx, Size, Style, TextStyle, Theme, Tone, Tri, Ui,
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
    // end is painted. The end's outermost pixel is 99 % covered (its center is 11.51 px from the
    // half circle's), so the end's middle is checked a pixel in.
    assert_ne!(at(r.x + 0.5, r.y + 0.5), accent, "the corner is square");
    assert_ne!(
        at(r.x + 3.0, r.y + 1.0),
        accent,
        "the corner is barely rounded"
    );
    assert_eq!(at(r.x + 1.5, r.y + r.h / 2.0), accent, "the end's middle");
    assert_eq!(at(r.x + r.w / 2.0, r.y + 0.5), accent, "the top's middle");
}

#[test]
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

/// The check box of [`a_check_box_holds_its_label_lines_after_its_box`], its name a label line's.
fn check_with_lines(ui: &mut Ui<u8>) {
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
}

#[test]
fn a_check_box_with_label_lines_draws_no_text_of_its_own() {
    let mut ui = ui();
    check_with_lines(&mut ui);
    let name = rect(&ui, &[Key::name("check"), Key::name("name")]);
    let what = rect(&ui, &[Key::name("check"), Key::name("what")]);
    // Between the lines' ink: its own label, centered in its two lines, would be inked here.
    let at = paint(&ui);
    let gap = (name.y + name.h + what.y + font::BODY / 4.0) / 2.0;
    assert_eq!(
        at(name.x + 3.0, gap),
        px(Theme::LIGHT.color(Tone::App)),
        "its name is drawn over its lines"
    );
}

#[test]
fn a_click_on_a_check_boxs_label_line_toggles_it() {
    let mut ui = ui();
    check_with_lines(&mut ui);
    let at = rect(&ui, &[Key::name("check"), Key::name("what")]).center();
    let mut msgs = Vec::new();
    ui.dispatch(Input::PointerDown(at), &mut msgs);
    ui.dispatch(Input::PointerUp(at), &mut msgs);
    assert_eq!(msgs, [1]);
}

#[test]
fn a_glyph_draws_at_its_text_size_whatever_its_box() {
    let mut ui = ui();
    ui.build(|cx| {
        cx.add_with(
            Element::stack(Key::name("row")).style(Style {
                axis: Axis::Row,
                ..Style::default()
            }),
            |cx| {
                cx.add(Element::glyph(Key::name("g"), Icon::Info).style(Style {
                    width: Dim::Fixed(40.0),
                    height: Dim::Fixed(40.0),
                    ..Style::default()
                }));
            },
        );
    });
    ui.layout(&mut FakeText);
    let r = rect(&ui, &[Key::name("row"), Key::name("g")]);
    assert_eq!(r, Rect::new(0.0, 0.0, 40.0, 40.0));
    // FakeText inks an icon's middle half: of a 13 px square centered in the box, 16.75 to
    // 23.25. As large as the box allows, it would ink from 10 to 30.
    let at = paint(&ui);
    let c = r.center();
    let ink = px(Theme::LIGHT.color(Tone::Text));
    assert_eq!(at(c.x, c.y), ink);
    assert_ne!(at(c.x - 6.0, c.y), ink, "drawn as large as its box");
}

#[test]
fn a_glyph_rebuilt_as_text_is_a_label_again() {
    let mut ui = ui();
    let line = font::BODY * 1.25;
    ui.build(|cx| {
        cx.add(Element::glyph(Key::name("g"), Icon::Info));
    });
    ui.layout(&mut FakeText);
    assert_eq!(
        rect(&ui, &[Key::name("g")]),
        Rect::new(0.0, 0.0, line, line)
    );
    // The same node, a label now: it stretches across the column, as text does.
    ui.build(|cx| {
        cx.add(Element::text(Key::name("g"), "Info"));
    });
    ui.layout(&mut FakeText);
    assert_eq!(
        rect(&ui, &[Key::name("g")]),
        Rect::new(0.0, 0.0, 400.0, line)
    );
}

#[test]
fn a_new_icon_repaints_the_glyph_alone() {
    let mut ui = ui();
    let page = |ui: &mut Ui<u8>, icon: Icon| {
        ui.build(|cx| {
            cx.add(Element::text(Key::name("t"), "Saved"));
            cx.add(Element::glyph(Key::name("g"), icon));
        });
        ui.layout(&mut FakeText);
    };
    page(&mut ui, Icon::Info);
    ui.clear_damage();
    page(&mut ui, Icon::Info);
    assert_eq!(ui.damage(), [] as [RectPx; 0], "nothing changed");
    page(&mut ui, Icon::Warning);
    let glyph = rect(&ui, &[Key::name("g")]).to_px(1.0);
    assert!(!ui.damage().is_empty(), "the new icon is drawn");
    for d in ui.damage() {
        assert_eq!(
            d.union(glyph),
            glyph,
            "{d:?} is outside the glyph {glyph:?}"
        );
    }
}

#[test]
fn a_pill_is_round_at_a_fractional_scale() {
    // A 9 px dot at 125 % (11.25 px, snapped to 12 here): a circle of half its painted side.
    let mut ui = Ui::new(
        Size { w: 40.0, h: 40.0 },
        1.25,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    );
    ui.build(|cx| {
        cx.add_with(
            Element::stack(Key::name("row")).style(Style {
                axis: Axis::Row,
                padding: Insets::all(4.3),
                align: Align::Start,
                ..Style::default()
            }),
            |cx| {
                cx.add(
                    Element::stack(Key::name("dot"))
                        .style(Style {
                            width: Dim::Fixed(9.0),
                            height: Dim::Fixed(9.0),
                            ..Style::default()
                        })
                        .look(Look {
                            fill: Some(Tone::Accent),
                            radius: Radius::Pill,
                            ..Look::PLAIN
                        }),
                );
            },
        );
    });
    ui.layout(&mut FakeText);
    let dot = rect(&ui, &[Key::name("row"), Key::name("dot")]).snap(1.25);
    let (w, h) = ui.pixel_size();
    let mut pixels = vec![0; usize::try_from(w * h).unwrap_or(0)];
    if let Some(mut canvas) = Canvas::new(&mut pixels, w, h) {
        ui.paint(&mut canvas, &mut FakeText, &Theme::LIGHT);
    }
    let at = |x: i32, y: i32| {
        let k = u32::try_from(y).unwrap_or(0) * w + u32::try_from(x).unwrap_or(0);
        pixels
            .get(usize::try_from(k).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or(0)
    };
    let accent = px(Theme::LIGHT.color(Tone::Accent));
    let (mx, my) = (i32::midpoint(dot.x0, dot.x1), i32::midpoint(dot.y0, dot.y1));
    assert_eq!(at(mx, my), accent, "the middle of {dot:?}");
    // A circle leaves every corner pixel out.
    for (x, y) in [
        (dot.x0, dot.y0),
        (dot.x1 - 1, dot.y0),
        (dot.x0, dot.y1 - 1),
        (dot.x1 - 1, dot.y1 - 1),
    ] {
        assert_ne!(at(x, y), accent, "the corner ({x}, {y}) of {dot:?}");
    }
}
