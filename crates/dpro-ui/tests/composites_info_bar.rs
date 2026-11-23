//! The info bar (TD-0009 A3.5 (14), A3.9): its tones, live only when it came unbidden, Dismiss, and
//! one reflow when it comes or goes.

#![allow(missing_docs)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use composites::{INFO, Model, Msg, PAGE, Run, SETTINGS, Step, key};
use dpro_ui::{
    FakeText, Key, KeyCode, Live, Mods, Motion, Size, Ui,
    accesskit::{Live as AkLive, Role as AkRole},
    composite::{INFO_DISMISS, INFO_GLYPH, INFO_MESSAGE, InfoBar, InfoTone},
    tokens,
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
#[ignore = "slice 6a: lands with the code"]
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
#[ignore = "slice 6a: lands with the code"]
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
