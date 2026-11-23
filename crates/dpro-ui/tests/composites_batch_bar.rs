//! The batch selection bar (TD-0009 A3.5 (11), A3.9): it shows at 1 and hides at 0, says how many
//! the filter hides, is one Tab stop whose arrows move between its buttons, and a count change
//! damages only its label, whose slot keeps the text after it in place. It's the inverse of the
//! page, with high contrast's border, and a quiet action shows its hover on it.

#![allow(missing_docs)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use composites::{BATCH, Model, Msg, PAGE, Run, Step, key};
use dpro_ui::{
    Canvas, Color, FakeText, Input, Key, KeyCode, Mods, Motion, Point, Rect, Role, Size, SysColors,
    Theme, Tone, Ui,
    accesskit::Role as AkRole,
    composite::{BATCH_CLEAR, BATCH_COUNT, BATCH_HIDDEN, BatchBar, Emphasis},
    tokens::{batch, font},
};

#[test]
fn it_shows_while_rows_are_checked_and_hides_at_0() {
    let mut run = Run::with(Model::new());
    assert!(run.find(&[PAGE, BATCH]).is_some(), "3 checked");
    run.step(Step::Update(Msg::Count(0)));
    assert!(run.find(&[PAGE, BATCH]).is_none(), "none checked");
    run.step(Step::Update(Msg::Count(1)));
    let count = run.part(&[PAGE, BATCH], BATCH_COUNT).expect("the count");
    assert_eq!(run.ui.info(count).map(|n| n.text), Some("1 selected"));
}

#[test]
fn it_says_how_many_the_filter_hides_only_when_some_are() {
    let mut run = Run::with(Model::new());
    let hidden = run.part(&[PAGE, BATCH], BATCH_HIDDEN).expect("1 hidden");
    assert_eq!(
        run.ui.info(hidden).map(|n| n.text),
        Some("1 hidden by the filter")
    );
    run.step(Step::Update(Msg::Hidden(0)));
    assert!(run.part(&[PAGE, BATCH], BATCH_HIDDEN).is_none());
}

#[test]
fn clear_selection_clears() {
    let mut run = Run::with(Model::new());
    let clear = run
        .part(&[PAGE, BATCH], BATCH_CLEAR)
        .expect("Clear selection");
    run.ui.set_focus(Some(clear));
    run.step(key(KeyCode::Enter, Mods::default()));
    assert_eq!(run.model.last, Some(Msg::Clear));
    assert!(run.find(&[PAGE, BATCH]).is_none());
}

#[test]
fn a_client_sees_a_named_toolbar() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let bar = run.find(&[PAGE, BATCH]).expect("the bar");
    let node = run.ak(&tree, bar).expect("in the tree");
    assert_eq!(node.role(), AkRole::Toolbar);
    assert_eq!(node.label(), Some("Selection"));
}

#[test]
fn clear_selection_is_named_and_its_glyph_is_presentational() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let clear = run
        .part(&[PAGE, BATCH], BATCH_CLEAR)
        .expect("Clear selection");
    let node = run.ak(&tree, clear).expect("in the tree");
    assert_eq!(node.role(), AkRole::Button);
    assert_eq!(node.label(), Some("Clear selection"));
    assert_eq!(node.children(), &[]);
    // Its label isn't drawn: the Cancel glyph is.
    assert!(run.ui.info(clear).is_some_and(|n| n.rect.w <= 40.0));
}

#[test]
fn left_and_right_move_between_its_buttons_without_wrapping() {
    let mut run = Run::with(Model::new());
    let bar = run.find(&[PAGE, BATCH]).expect("the bar");
    let buttons: Vec<_> = {
        let mut out = Vec::new();
        let mut stack = vec![bar];
        while let Some(id) = stack.pop() {
            for c in run.ui.children(id) {
                if run.ui.info(c).is_some_and(|n| n.role == Role::Button) {
                    out.push(c);
                }
                stack.push(c);
            }
        }
        out.sort_by(|a, b| {
            let x = |id| run.ui.info(id).map_or(0.0, |n| n.rect.x);
            x(*a).total_cmp(&x(*b))
        });
        out
    };
    assert_eq!(buttons.len(), 3, "two actions and Clear selection");
    run.ui.set_focus(Some(buttons[0]));
    run.step(key(KeyCode::Left, Mods::default()));
    assert_eq!(run.ui.focus(), Some(buttons[0]), "no wrap at the start");
    run.step(key(KeyCode::Right, Mods::default()));
    assert_eq!(run.ui.focus(), Some(buttons[1]));
    run.step(key(KeyCode::End, Mods::default()));
    assert_eq!(run.ui.focus(), Some(buttons[2]));
    run.step(key(KeyCode::Right, Mods::default()));
    assert_eq!(run.ui.focus(), Some(buttons[2]), "no wrap at the end");
    run.step(key(KeyCode::Home, Mods::default()));
    assert_eq!(run.ui.focus(), Some(buttons[0]));
}

#[test]
fn a_count_change_damages_only_its_label() {
    let mut run = Run::with(Model::new()).warmed();
    let label = run
        .part(&[PAGE, BATCH], BATCH_COUNT)
        .and_then(|id| run.ui.info(id))
        .map(|n| n.rect.to_px(1.0))
        .expect("the count");
    run.step(Step::Update(Msg::Count(4)));
    assert!(!run.damage.is_empty(), "the new count is drawn");
    for d in &run.damage {
        assert_eq!(
            d.union(label),
            label,
            "{d:?} is outside the count {label:?}"
        );
    }
}

/// The rect of the node keyed `key` under `run`'s bar.
fn rect_in(run: &Run, key: Key) -> Rect {
    run.part(&[PAGE, BATCH], key)
        .and_then(|id| run.ui.info(id))
        .map_or(Rect::default(), |n| n.rect)
}

#[test]
fn the_counts_slot_keeps_what_follows_it_in_place_at_any_text_size() {
    for scale in [1.0, 2.25] {
        let mut run = Run::text_scaled(Model::new(), scale);
        let (count, hidden) = (rect_in(&run, BATCH_COUNT), rect_in(&run, BATCH_HIDDEN));
        assert!(
            (count.w - batch::COUNT_W * scale).abs() < 0.01,
            "{count:?} at {scale}"
        );
        run.step(Step::Update(Msg::Count(12_345)));
        assert_eq!(rect_in(&run, BATCH_COUNT), count, "at {scale}");
        assert_eq!(rect_in(&run, BATCH_HIDDEN), hidden, "at {scale}");
    }
    // "99,999 selected" in body semibold fits the slot (FakeText: half the size per character).
    const { assert!(15.0 * font::BODY * 0.5 <= batch::COUNT_W) };
}

#[test]
fn at_225_percent_in_a_narrow_window_the_buttons_wrap_and_nothing_overlaps() {
    let mut run = Run::text_scaled(Model::new(), 2.25);
    run.step(Step::Resize(Size { w: 560.0, h: 700.0 }));
    let bar = run.find(&[PAGE, BATCH]).expect("the bar");
    let outer = run.ui.info(bar).map_or(Rect::default(), |n| n.rect);
    let count = rect_in(&run, BATCH_COUNT);
    let buttons: Vec<Rect> = run
        .ui
        .children(bar)
        .filter_map(|c| run.ui.info(c))
        .filter(|n| n.role == Role::Button)
        .map(|n| n.rect)
        .collect();
    assert_eq!(buttons.len(), 3);
    assert!(
        buttons.iter().any(|b| b.y >= count.bottom()),
        "they don't all fit beside the count: {buttons:?}"
    );
    for b in buttons {
        assert!(
            b.intersect(count).is_empty(),
            "{b:?} overlaps the count {count:?}"
        );
        assert!(
            b.x >= outer.x && b.right() <= outer.right() + 0.01,
            "{b:?} outside the bar {outer:?}"
        );
    }
}

const BAR: Key = Key::name("bar");
const ACTIONS: [(&str, u8, Emphasis); 2] = [
    ("Clean selected", 1, Emphasis::Primary),
    ("Review", 2, Emphasis::Normal),
];

/// A bar alone, with a primary and a quiet action; hover is instant.
fn alone() -> Ui<u8> {
    let mut ui = Ui::new(Size { w: 800.0, h: 200.0 }, 1.0, Motion { hover_ms: 0 });
    ui.set_active(true);
    ui.build(|cx| {
        cx.batch_bar(
            BAR,
            &BatchBar {
                count: 3,
                hidden: 0,
                selected: "3 selected",
                hidden_text: "",
                note: "",
                actions: &ACTIONS,
                clear: ("Clear selection", 3),
                name: "Selection",
            },
        );
    });
    ui.layout(&mut FakeText);
    ui
}

/// The rect of the node at `path` in `ui`.
fn rect(ui: &Ui<u8>, path: &[Key]) -> Rect {
    ui.find(path)
        .and_then(|id| ui.info(id))
        .map_or(Rect::default(), |n| n.rect)
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
        reason = "a pixel inside an 800 × 200 window"
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
fn the_bar_is_the_inverse_of_the_page_and_high_contrast_borders_it() {
    let ui = alone();
    let bar = rect(&ui, &[BAR]);
    let primary = rect(&ui, &[BAR, Key::index(0)]);
    let mid = bar.y + bar.h / 2.0;
    let (pad, in_primary) = (bar.x + batch::PAD_X / 2.0, primary.x + batch::PAD_X / 2.0);
    for theme in [Theme::LIGHT, Theme::DARK] {
        assert_eq!(
            pixel(&ui, &theme, pad, mid),
            px(theme.color(Tone::InverseFill))
        );
        assert_eq!(
            pixel(&ui, &theme, in_primary, mid),
            px(theme.color(Tone::InverseAccent)),
            "the primary action"
        );
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
    let hc = Theme::high_contrast(sys);
    assert_eq!(pixel(&ui, &hc, pad, mid), px(sys.window), "no fill");
    assert_eq!(pixel(&ui, &hc, bar.x, mid), px(sys.window_text), "a border");
    assert_eq!(
        pixel(&ui, &hc, in_primary, mid),
        px(sys.highlight),
        "the primary action is a highlight"
    );
}

#[test]
fn a_quiet_action_shows_its_hover_on_the_bar() {
    // The state layer is the bar's ink, not the page's: Text at 6 % on InverseFill wouldn't show.
    let mut ui = alone();
    let review = rect(&ui, &[BAR, Key::index(1)]);
    let (x, y) = (review.x + batch::PAD_X / 2.0, review.y + review.h / 2.0);
    for theme in [Theme::LIGHT, Theme::DARK] {
        let before = pixel(&ui, &theme, x, y);
        assert_eq!(before, px(theme.color(Tone::InverseFill)));
        let mut out = Vec::new();
        ui.dispatch(Input::PointerMove(Point { x, y }), &mut out);
        let hovered = pixel(&ui, &theme, x, y);
        assert_ne!(hovered, before, "hovered in {:?}", theme.scheme());
        let lit = theme
            .layer(Tone::Hover, Tone::InverseFill)
            .over(theme.color(Tone::InverseFill));
        // The canvas rounds each half of the blend on its own: within 1 of `over`.
        let near = (0..3).all(|c| {
            let ch = |p: u32| i32::try_from((p >> (8 * c)) & 0xFF).unwrap_or(0);
            (ch(hovered) - ch(px(lit))).abs() <= 1
        });
        assert!(
            near,
            "the bar's ink at the hover's alpha: {hovered:08x} vs {lit:?}"
        );
        ui.dispatch(Input::PointerMove(Point { x: -1.0, y: -1.0 }), &mut out);
    }
}
