//! The inline error panel (TD-0009 A3.5 (9), A3.9): Details toggles, the excerpt is capped at 8
//! lines and sanitized, the actions come in order, and a client sees a group named by the title,
//! a level-2 heading that's live when the panel came unbidden. Its tint, border and glyph come
//! from the token table, and expanding reflows its column once.

#![allow(missing_docs)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use composites::{ERROR, INFO, Model, Msg, PAGE, Run, Step, key, key_up};
use dpro_ui::{
    Canvas, Color, Element, FakeText, Key, KeyCode, Live, Mods, Motion, NodeId, Rect, Role, Size,
    SysColors, Theme, Tone, Ui,
    accesskit::{Live as AkLive, Role as AkRole},
    composite::{
        ERROR_ACTIONS, ERROR_CODE, ERROR_COPY, ERROR_DETAILS, ERROR_GLYPH, ERROR_LOG, ERROR_TITLE,
        ErrorPanel, ErrorTone, LogExcerpt,
    },
    tokens::{self, error},
};

#[test]
fn details_toggles_the_code_the_call_and_the_log() {
    let mut run = Run::with(Model::new());
    assert!(run.part(&[PAGE, ERROR], ERROR_CODE).is_none(), "collapsed");
    let details = run.part(&[PAGE, ERROR], ERROR_DETAILS).expect("Details");
    assert_eq!(run.ui.info(details).and_then(|n| n.expanded), Some(false));
    run.ui.set_focus(Some(details));
    run.step(key(KeyCode::Enter, Mods::default()));
    assert_eq!(run.model.last, Some(Msg::Expand));
    assert!(run.part(&[PAGE, ERROR], ERROR_CODE).is_some(), "expanded");
    assert!(
        run.part(&[PAGE, ERROR], ERROR_LOG).is_some(),
        "the log shows"
    );
    let details = run.part(&[PAGE, ERROR], ERROR_DETAILS).expect("Details");
    assert_eq!(run.ui.info(details).and_then(|n| n.expanded), Some(true));
    run.step(key(KeyCode::Space, Mods::default()));
    run.step(key_up(KeyCode::Space));
    assert!(
        run.part(&[PAGE, ERROR], ERROR_CODE).is_none(),
        "collapsed again"
    );
}

#[test]
fn the_excerpt_is_the_logs_last_8_lines() {
    let mut run = Run::with(Model::new());
    run.step(Step::Update(Msg::Expand));
    let log = run.part(&[PAGE, ERROR], ERROR_LOG).expect("the excerpt");
    let text = run.ui.info(log).map(|n| n.text.to_owned()).expect("text");
    assert_eq!(text.lines().count(), error::LOG_LINES, "{text}");
    assert!(text.ends_with("Job 42 ended: partially applied"), "{text}");
    assert!(composites::LOG.ends_with(&text), "the last lines, whole");
}

/// A panel alone, expanded, over `log`.
fn alone(log: &str) -> Ui<u8> {
    alone_as(ErrorTone::Error, log)
}

/// A panel of `tone` alone, expanded, over `log`.
fn alone_as(tone: ErrorTone, log: &str) -> Ui<u8> {
    let mut ui = Ui::new(
        Size { w: 800.0, h: 600.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    );
    ui.build(|cx| {
        cx.error_panel(
            Key::name("e"),
            &ErrorPanel {
                tone,
                title: "Couldn't clean",
                code: "Code 0x80070005",
                api: "DeleteFileW",
                next: "Try again as administrator.",
                log: Some(LogExcerpt {
                    source: "job.log",
                    text: log,
                }),
                expanded: true,
                details: ("Details", 0),
                actions: [Some(("Try again", 1)), None],
                copy: ("Copy details", 2),
                live: Live::Off,
            },
        );
        cx.add(Element::text(Key::name("after"), ""));
    });
    ui.layout(&mut FakeText);
    ui
}

/// The first node keyed `key` anywhere in `ui`.
fn part<M>(ui: &Ui<M>, key: Key) -> Option<NodeId> {
    let mut stack = vec![ui.root()];
    while let Some(id) = stack.pop() {
        for c in ui.children(id) {
            if ui.info(c).is_some_and(|n| n.key == key) {
                return Some(c);
            }
            stack.push(c);
        }
    }
    None
}

#[test]
fn the_excerpt_is_sanitized() {
    // Control characters and bidirectional overrides from a log never reach the screen or a
    // client: they'd reorder or hide text (TD-0009 §8's field rules).
    let ui = alone("ok\u{1b}[31m red\u{7}\n\u{202e}evil\u{202c} line\r\nlast\u{0}");
    let log = part(&ui, ERROR_LOG)
        .and_then(|id| ui.info(id))
        .map(|n| n.text.to_owned())
        .expect("the excerpt");
    assert!(
        log.chars()
            .all(|c| c == '\n' || !(c.is_control() || ('\u{202a}'..='\u{202e}').contains(&c))),
        "{log:?}"
    );
    assert!(log.contains("red") && log.contains("last"), "{log:?}");
}

#[test]
fn the_actions_come_in_order_then_copy_details() {
    let run = Run::with(Model::new());
    let row = run
        .part(&[PAGE, ERROR], ERROR_ACTIONS)
        .expect("the actions");
    let labels: Vec<String> = run
        .ui
        .children(row)
        .filter_map(|c| run.ui.info(c))
        .filter(|n| n.role == Role::Button)
        .map(|n| n.text.to_owned())
        .collect();
    assert_eq!(labels, ["Try again", "Open log", "Copy details"]);
    let copy = run.part(&[PAGE, ERROR], ERROR_COPY).expect("Copy details");
    assert_eq!(run.ui.info(copy).map(|n| n.text), Some("Copy details"));
}

#[test]
fn a_client_sees_a_group_named_by_the_title() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let panel = run.find(&[PAGE, ERROR]).expect("the panel");
    let node = run.ak(&tree, panel).expect("in the tree");
    assert_eq!(node.role(), AkRole::Group);
    assert_eq!(node.label(), Some("3 of 7 categories couldn't be cleaned"));
}

#[test]
fn the_title_is_assertive_when_the_panel_came_unbidden_and_silent_otherwise() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let title = run.part(&[PAGE, ERROR], ERROR_TITLE).expect("the title");
    let node = run.ak(&tree, title).expect("in the tree");
    assert_eq!(node.live(), Some(AkLive::Assertive));
    let mut quiet = alone("one line");
    let tree = quiet.a11y_tree();
    assert!(
        tree.nodes.iter().all(|(_, n)| n.live().is_none()),
        "Live::Off sets nothing"
    );
}

#[test]
fn the_title_is_a_level_2_heading_and_details_says_whether_its_open() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let title = run.part(&[PAGE, ERROR], ERROR_TITLE).expect("the title");
    let node = run.ak(&tree, title).expect("in the tree");
    assert_eq!(node.role(), AkRole::Heading);
    assert_eq!(node.level(), Some(1), "aria-level less one");
    let details = run.part(&[PAGE, ERROR], ERROR_DETAILS).expect("Details");
    let node = run.ak(&tree, details).expect("in the tree");
    assert_eq!(node.role(), AkRole::Button);
    assert_eq!(node.label(), Some("Details"));
    assert_eq!(node.is_expanded(), Some(false));
    // Its word and chevron are its own: a client reads the button once.
    assert_eq!(node.children(), &[]);
}

#[test]
fn expanding_reflows_the_column_once_and_settles() {
    let mut run = Run::with(Model::new()).warmed();
    let rect = |run: &Run, path: &[Key]| {
        run.find(path)
            .and_then(|id| run.ui.info(id))
            .map_or(Rect::default(), |n| n.rect)
    };
    let (panel, info) = (rect(&run, &[PAGE, ERROR]), rect(&run, &[PAGE, INFO]));
    let frames = run.frames;
    run.step(Step::Update(Msg::Expand));
    assert_eq!(run.frames, frames + 1, "one frame");
    assert!(!run.ui.wants_frame(), "nothing left to lay out or animate");
    let (open, moved) = (rect(&run, &[PAGE, ERROR]), rect(&run, &[PAGE, INFO]));
    let grew = open.h - panel.h;
    assert!(grew > 0.0, "{panel:?} to {open:?}");
    assert!(
        (moved.y - (info.y + grew)).abs() < 0.01,
        "what's under the panel moves by its growth: {info:?} to {moved:?}"
    );
}

/// `ui` painted in `theme`, read at `(x, y)` (logical pixels at scale 1).
fn pixel(ui: &Ui<u8>, theme: &Theme, x: f32, y: f32) -> u32 {
    let (w, h) = ui.pixel_size();
    let mut pixels = vec![0; usize::try_from(w * h).unwrap_or(0)];
    if let Some(mut canvas) = Canvas::new(&mut pixels, w, h) {
        ui.paint(&mut canvas, &mut FakeText, theme);
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a pixel inside an 800 × 600 window"
    )]
    let (x, y) = (x as u32, y as u32);
    pixels
        .get(usize::try_from(y * w + x).unwrap_or(usize::MAX))
        .copied()
        .unwrap_or(0)
}

/// `c` as an opaque canvas pixel.
fn px(c: Color) -> u32 {
    0xFF00_0000 | u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
}

#[test]
fn the_tint_and_the_glyph_come_from_the_token_table() {
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
    let hc = Theme::high_contrast(sys);
    for (tone, fill, status) in [
        (ErrorTone::Error, Tone::ErrorFill, Tone::Danger),
        (ErrorTone::Partial, Tone::WarnFill, Tone::Warn),
    ] {
        let ui = alone_as(tone, "one line");
        let info = |key| {
            part(&ui, key)
                .and_then(|id| ui.info(id))
                .map_or(Rect::default(), |n| n.rect)
        };
        let panel = ui
            .find(&[Key::name("e")])
            .and_then(|id| ui.info(id))
            .map_or(Rect::default(), |n| n.rect);
        let glyph = info(ERROR_GLYPH).center();
        let mid = panel.y + panel.h / 2.0;
        let pad = panel.x + error::PADDING / 2.0;
        for theme in [Theme::LIGHT, Theme::DARK] {
            assert_eq!(
                pixel(&ui, &theme, pad, mid),
                px(theme.color(fill)),
                "{tone:?}"
            );
            // The glyph is in the status color, not the border's.
            assert_eq!(
                pixel(&ui, &theme, glyph.x, glyph.y),
                px(theme.color(status)),
                "{tone:?}'s glyph"
            );
        }
        // High contrast: no tint, a `WindowText` border.
        assert_eq!(pixel(&ui, &hc, pad, mid), px(sys.window), "{tone:?}");
        assert_eq!(
            pixel(&ui, &hc, panel.x, mid),
            px(sys.window_text),
            "{tone:?}"
        );
        assert_eq!(
            pixel(&ui, &hc, glyph.x, glyph.y),
            px(sys.window_text),
            "{tone:?}'s glyph"
        );
    }
}
