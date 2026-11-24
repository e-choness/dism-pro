//! The info bar (TD-0009 A3.5 (14), A3.9): its tones (tint, border, glyph; none in high
//! contrast), live only when it came unbidden, Dismiss, its targets inside the bar at any text size
//! and width, and one reflow when it comes or goes.

#![allow(missing_docs)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use composites::{INFO, Model, Msg, NARROW, PAGE, Run, SETTINGS, START, Step, key};
use dpro_ui::{
    Canvas, Color, FakeText, Key, KeyCode, Live, Mods, Motion, NodeId, Rect, Size, SysColors,
    Theme, Tone, Ui,
    accesskit::{Live as AkLive, Role as AkRole},
    composite::{INFO_ACTION, INFO_DISMISS, INFO_GLYPH, INFO_MESSAGE, InfoBar, InfoTone},
    tokens::{self, info},
};

/// A bar alone, of `tone`, `live`.
fn alone(tone: InfoTone, live: Live) -> Ui<u8> {
    let mut ui = Ui::new(
        Size { w: 800.0, h: 200.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    );
    ui.build(|cx| {
        cx.info_bar(
            Key::name("i"),
            &InfoBar {
                tone,
                title: "Title",
                message: "Message",
                action: None,
                dismiss: Some(("Dismiss", 1)),
                live,
            },
        );
    });
    ui.layout(&mut FakeText);
    ui
}

#[test]
fn dismiss_sends_its_message_and_the_bar_goes() {
    let mut run = Run::with(Model::new());
    let dismiss = run.part(&[PAGE, INFO], INFO_DISMISS).expect("Dismiss");
    run.ui.set_focus(Some(dismiss));
    run.step(key(KeyCode::Enter, Mods::default()));
    assert_eq!(run.model.last, Some(Msg::Dismiss));
    assert!(run.find(&[PAGE, INFO]).is_none());
}

#[test]
fn a_client_sees_a_group_named_by_the_title() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let bar = run.find(&[PAGE, INFO]).expect("the bar");
    let node = run.ak(&tree, bar).expect("in the tree");
    assert_eq!(node.role(), AkRole::Group);
    assert_eq!(node.label(), Some("Windows Update is running"));
}

#[test]
fn each_tone_has_its_glyph_and_the_message_is_live_only_when_unbidden() {
    for (tone, live, want) in [
        (InfoTone::Info, Live::Polite, Some(AkLive::Polite)),
        (InfoTone::Warning, Live::Polite, Some(AkLive::Polite)),
        (InfoTone::Error, Live::Assertive, Some(AkLive::Assertive)),
        (InfoTone::Error, Live::Off, None),
    ] {
        let mut ui = alone(tone, live);
        let tree = ui.a11y_tree();
        let lives: Vec<_> = tree.nodes.iter().filter_map(|(_, n)| n.live()).collect();
        assert_eq!(lives.first().copied(), want, "{tone:?} {live:?}");
        assert!(lives.len() <= 1, "{tone:?}: one live node, the message");
        // Its glyph: Info, Warning or ErrorBadge, by tone.
        let mut stack = vec![ui.root()];
        let mut glyph = None;
        while let Some(id) = stack.pop() {
            for c in ui.children(id) {
                if ui.info(c).is_some_and(|n| n.key == INFO_GLYPH) {
                    glyph = Some(c);
                }
                stack.push(c);
            }
        }
        assert!(glyph.is_some(), "{tone:?} has a glyph");
    }
}

#[test]
fn the_page_bar_is_polite_while_it_came_unbidden() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let message = run.part(&[PAGE, INFO], INFO_MESSAGE).expect("the message");
    let node = run.ak(&tree, message).expect("in the tree");
    assert_eq!(node.live(), Some(AkLive::Polite));
}

#[test]
fn coming_and_going_reflows_the_page_in_one_frame() {
    let mut run = Run::with(Model::new()).warmed();
    let below = |run: &Run| {
        run.find(&[PAGE, SETTINGS])
            .and_then(|id| run.ui.info(id))
            .map(|n| n.rect.y)
            .expect("the settings group")
    };
    let shown = below(&run);
    run.step(Step::Update(Msg::Dismiss));
    assert!(below(&run) < shown, "what's below moves up at once");
    assert!(!run.ui.wants_frame(), "nothing animates");
    run.step(Step::Update(Msg::ShowInfo));
    assert!((below(&run) - shown).abs() < 0.01, "and back down at once");
    assert!(!run.ui.wants_frame(), "nothing animates");
}

/// A bar alone of `tone`, painted in `theme`: its box, and the pixels at its left padding's
/// middle (its tint) and its left edge's middle (its border).
fn painted(tone: InfoTone, theme: &Theme) -> (Rect, u32, u32) {
    let ui = alone(tone, Live::Off);
    let rect = ui
        .find(&[Key::name("i")])
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
            reason = "a pixel inside an 800 × 200 window"
        )]
        let (x, y) = (x as u32, y as u32);
        pixels
            .get(usize::try_from(y * w + x).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or(0)
    };
    let mid = rect.y + rect.h / 2.0;
    (rect, at(rect.x + info::PAD_X / 2.0, mid), at(rect.x, mid))
}

/// `c` as an opaque canvas pixel.
fn px(c: Color) -> u32 {
    0xFF00_0000 | u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
}

#[test]
fn each_tone_is_its_own_tint_in_a_border_and_high_contrast_has_none() {
    for (tone, fill) in [
        (InfoTone::Info, Tone::InfoFill),
        (InfoTone::Warning, Tone::WarnFill),
        (InfoTone::Error, Tone::ErrorFill),
    ] {
        for theme in [Theme::LIGHT, Theme::DARK] {
            let (rect, tint, edge) = painted(tone, &theme);
            assert_eq!(
                tint,
                px(theme.color(fill)),
                "{tone:?} at {rect:?}: its tint"
            );
            assert_ne!(edge, tint, "{tone:?} at {rect:?}: no border");
        }
    }
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
    for tone in [InfoTone::Info, InfoTone::Warning, InfoTone::Error] {
        let (rect, tint, edge) = painted(tone, &theme);
        assert_eq!(tint, px(sys.window), "{tone:?} at {rect:?}: a tint");
        assert_eq!(edge, px(sys.window_text), "{tone:?} at {rect:?}: no border");
    }
}

#[test]
fn the_action_and_dismiss_are_targets_inside_the_bar_at_any_text_size_and_width() {
    for scale in [1.0, 2.25] {
        for width in [START, NARROW] {
            let mut run = Run::text_scaled(Model::new(), scale);
            run.step(Step::Resize(width));
            let at = |id: Option<NodeId>| {
                id.and_then(|id| run.ui.info(id))
                    .map_or(Rect::default(), |n| n.rect)
            };
            let bar = at(run.find(&[PAGE, INFO]));
            for part in [INFO_ACTION, INFO_DISMISS] {
                let r = at(run.part(&[PAGE, INFO], part));
                let here = format!("{part:?} at {scale}, {} px: {r:?} in {bar:?}", width.w);
                assert!(r.w >= 24.0 && r.h >= 24.0, "{here}: under 24 px");
                assert!(
                    r.x >= bar.x && r.right() <= bar.right() + 0.01,
                    "{here}: past the bar's sides"
                );
                assert!(
                    r.y >= bar.y && r.bottom() <= bar.bottom() + 0.01,
                    "{here}: past the bar's top or bottom"
                );
            }
            // Dismiss stays at the bar's end, on whichever line it's on.
            let dismiss = at(run.part(&[PAGE, INFO], INFO_DISMISS));
            assert!(
                (dismiss.right() - (bar.right() - info::PAD_X)).abs() < 0.01,
                "Dismiss at {scale}, {} px: {dismiss:?} in {bar:?}",
                width.w
            );
        }
    }
}
