//! Summary and scan cards (TD-0009 A3.5 (4), (5), A3.9).
//!
//! Summary cards: a clickable card is a Button named by its label, described by "{value}.
//! {detail}" and its delta, with presentational children; a static one is a Group named by its
//! label; a grid wraps them at 220 px each, a loading card's skeleton shows in high contrast, and
//! its value arriving repaints the value alone. Scan cards: Space toggles, Mixed shows, Details only
//! when the scan failed, a busy box's reason is heard, a size update damages only its slot at any
//! text size, and a scan starting or ending repaints its slot alone, its bar under the size.

#![allow(missing_docs)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use composites::{
    BUSY, CARDS, CATEGORIES, LABELS, Model, Msg, NARROW, PAGE, Run, SCANS, START, Step, key, key_up,
};
use dpro_ui::{
    Canvas, Color, FakeText, Input, Key, KeyCode, Mods, Motion, Rect, Role, Size, SysColors, Theme,
    Tri, Ui,
    accesskit::{Role as AkRole, Toggled},
    composite::{
        CARD_LABEL, CARD_VALUE, CardValue, SCAN_CHECK, SCAN_DETAILS, SCAN_SIZE, ScanSize,
        SummaryCard,
    },
    tokens::{self, card, scan},
};

/// Summary card `i`'s path.
fn card(i: u64) -> [Key; 3] {
    [PAGE, CARDS, Key::index(i)]
}

/// Scan card `i`'s path.
fn scan(i: u64) -> [Key; 3] {
    [PAGE, SCANS, Key::index(i)]
}

fn rect(run: &Run, path: &[Key]) -> Rect {
    run.find(path)
        .and_then(|id| run.ui.info(id))
        .map_or(Rect::default(), |n| n.rect)
}

#[test]
fn a_static_card_is_a_group_named_by_its_label() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let id = run.find(&card(1)).expect("the status card");
    let node = run.ak(&tree, id).expect("in the tree");
    assert_eq!(node.role(), AkRole::Group);
    assert_eq!(node.label(), Some("System health"));
}

#[test]
fn a_static_cards_label_is_a_level_2_heading() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let label = run.part(&card(1), CARD_LABEL).expect("its label");
    let node = run.ak(&tree, label).expect("in the tree");
    assert_eq!(node.role(), AkRole::Heading);
    // AccessKit's level is aria-level less one.
    assert_eq!(node.level(), Some(1));
}

#[test]
fn a_clickable_card_is_a_button_that_opens_its_page() {
    let mut run = Run::with(Model::new());
    let id = run.find(&card(0)).expect("the first card");
    assert_eq!(run.ui.info(id).map(|n| n.role), Some(Role::Button));
    let at = run.center(&card(0));
    run.step(Step::Input(Input::PointerDown(at)));
    run.step(Step::Input(Input::PointerUp(at)));
    assert_eq!(run.model.last, Some(Msg::Open(0)));
}

#[test]
fn a_clickable_card_is_named_by_its_label_and_described_by_value_and_detail() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let id = run.find(&card(0)).expect("the first card");
    let node = run.ak(&tree, id).expect("in the tree");
    assert_eq!(node.role(), AkRole::Button);
    assert_eq!(node.label(), Some("Space to free"));
    // Then its delta, which is presentational too, so a client hears it only here.
    assert_eq!(
        node.description(),
        Some("5.6 GB. Across 4 categories. 1.1 GB more than last week")
    );
    // Its parts are presentational: read once, as the button.
    assert_eq!(
        node.children(),
        &[],
        "a clickable card's parts are in the tree"
    );
}

#[test]
fn the_cards_are_at_least_220_px_wide() {
    let run = Run::with(Model::new());
    for i in 0..4 {
        let r = rect(&run, &card(i));
        assert!(r.w + 0.01 >= card::MIN_W, "card {i} is {} wide", r.w);
    }
}

#[test]
fn the_grid_wraps_the_cards_two_to_a_line_when_narrow() {
    let mut run = Run::with(Model::new());
    let tops = |run: &Run| (0..4).map(|i| rect(run, &card(i)).y).collect::<Vec<_>>();
    let wide = tops(&run);
    assert!(
        wide.iter().all(|y| (y - wide[0]).abs() < 0.01),
        "one line at 1000 px: {wide:?}"
    );
    run.step(Step::Resize(NARROW));
    let narrow = tops(&run);
    assert!((narrow[0] - narrow[1]).abs() < 0.01, "{narrow:?}");
    assert!((narrow[2] - narrow[3]).abs() < 0.01, "{narrow:?}");
    assert!(
        narrow[2] > narrow[0] + 1.0,
        "two lines at 640 px: {narrow:?}"
    );
    // On each line the cards grow to share it.
    let grid = rect(&run, &[PAGE, CARDS]);
    let line = rect(&run, &card(0)).w + rect(&run, &card(1)).w;
    assert!(
        line > grid.w * 0.9,
        "the line's cards fill it: {line} of {}",
        grid.w
    );
    run.step(Step::Resize(START));
    assert_eq!(tops(&run), wide, "back on one line");
}

#[test]
fn space_toggles_a_scan_cards_box() {
    let mut run = Run::with(Model::new());
    let check = run.part(&scan(0), SCAN_CHECK).expect("its box");
    run.ui.set_focus(Some(check));
    run.step(key(KeyCode::Space, Mods::default()));
    run.step(key_up(KeyCode::Space));
    assert_eq!(run.model.last, Some(Msg::Toggle(0)));
    assert_eq!(run.model.scans[0].check, Tri::Off);
    assert_eq!(run.ui.info(check).map(|n| n.check), Some(Tri::Off));
}

#[test]
fn a_scan_cards_box_is_named_by_its_category_and_shows_mixed() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let check = run.part(&scan(1), SCAN_CHECK).expect("its box");
    let node = run.ak(&tree, check).expect("in the tree");
    assert_eq!(node.role(), AkRole::CheckBox);
    assert_eq!(node.label(), Some(CATEGORIES[1].0));
    assert_eq!(node.toggled(), Some(Toggled::Mixed));
}

#[test]
fn a_scan_cards_box_is_described_by_description_size_and_badge() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let check = run.part(&scan(1), SCAN_CHECK).expect("its box");
    let node = run.ak(&tree, check).expect("in the tree");
    // The size as the window's formats write it: three significant digits, as Explorer does.
    assert_eq!(
        node.description(),
        Some("Downloaded updates already installed. 3.40 GB. Undoable")
    );
    // The name and the description inside the box are presentational (A3.5 addition 9).
    assert_eq!(node.children(), &[]);
}

/// A busy box's reason is its tooltip, and its description ends with it: a description takes
/// the tooltip's place for a client (A3.5 addition 4), which must still hear why the box is
/// disabled (A3.4, `ui-busy-target`).
#[test]
fn a_busy_scan_cards_box_shows_its_reason_and_a_client_hears_it() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let check = run.part(&scan(6), SCAN_CHECK).expect("its box");
    let node = run.ak(&tree, check).expect("in the tree");
    assert!(node.is_disabled());
    let description = node.description().expect("a description");
    let reason = format!("{}{BUSY}", LABELS.separator);
    assert!(description.ends_with(&reason), "{description:?}");
    // The tooltip still shows the reason alone, in a window tall enough to show the card.
    run.step(Step::Resize(Size {
        w: START.w,
        h: START.h * 3.0,
    }));
    let at = run.ui.info(check).expect("its box").rect.center();
    run.step(Step::Input(Input::PointerMove(at)));
    run.settle();
    let due = run.wake().expect("a pending tip's wake");
    run.step(Step::Tick(due));
    // The platform runs the tip's one-shot timer when it's due.
    assert!(run.wake().is_some(), "a showing tip's wake, to hide it");
    assert_eq!(run.ui.tooltip().map(|(_, text)| text), Some(BUSY));
}

#[test]
fn details_shows_only_when_the_scan_failed_and_nothing_to_clean_disables_the_box() {
    let run = Run::with(Model::new());
    for i in 0..7 {
        let failed = i == 4;
        assert_eq!(
            run.part(&scan(i), SCAN_DETAILS).is_some(),
            failed,
            "scan card {i}"
        );
    }
    let nothing = run.part(&scan(5), SCAN_CHECK).expect("its box");
    assert_eq!(run.ui.info(nothing).map(|n| n.enabled), Some(false));
    let busy = run.part(&scan(6), SCAN_CHECK).expect("its box");
    assert_eq!(run.ui.info(busy).map(|n| n.enabled), Some(false));
}

/// The rectangle of scan card `i`'s part `key`, wherever the card nests it.
fn part_rect(run: &Run, i: u64, key: Key) -> Rect {
    run.part(&scan(i), key)
        .and_then(|id| run.ui.info(id))
        .map_or(Rect::default(), |n| n.rect)
}

/// Scan card `i`'s size slot's parts: its word, then the scan's bar while the category scans.
fn slot_parts(run: &Run, i: u64) -> Vec<Rect> {
    run.part(&scan(i), SCAN_SIZE)
        .map(|slot| {
            run.ui
                .children(slot)
                .filter_map(|id| run.ui.info(id).map(|n| n.rect))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_size_update_damages_only_its_slot() {
    for scale in [1.0, 2.25] {
        let mut run = Run::text_scaled(Model::new(), scale).warmed();
        let slot = part_rect(&run, 0, SCAN_SIZE).to_px(1.0);
        assert!(!slot.is_empty(), "the slot at {scale}");
        run.step(Step::Update(Msg::Size(0, 1_395_864_371)));
        assert!(!run.damage.is_empty(), "the new size is drawn at {scale}");
        for d in &run.damage {
            assert_eq!(
                d.union(slot),
                slot,
                "at {scale}: {d:?} is outside the size's slot {slot:?}"
            );
        }
    }
}

#[test]
fn a_scanning_card_has_a_bar_under_its_size_and_the_others_none() {
    let run = Run::with(Model::new()).warmed();
    // After the session, cards 2 and 3 scan.
    for (i, s) in (0..).zip(&run.model.scans) {
        let scanning = s.size == ScanSize::Scanning;
        assert_eq!(scanning, i == 2 || i == 3, "scan card {i}");
        let parts = slot_parts(&run, i);
        assert_eq!(parts.len(), if scanning { 2 } else { 1 }, "scan card {i}");
    }
    let slot = part_rect(&run, 2, SCAN_SIZE);
    let parts = slot_parts(&run, 2);
    let (word, bar) = (parts[0], parts[1]);
    assert!((bar.h - scan::BAR).abs() < 0.01, "{bar:?}");
    assert!(
        (bar.w - scan::SIZE_W).abs() < 0.01,
        "a size's width: {bar:?}"
    );
    assert!(bar.y >= word.bottom(), "{bar:?} under {word:?}");
    assert!(
        (bar.right() - slot.right()).abs() < 0.01 && bar.bottom() <= slot.bottom() + 0.01,
        "{bar:?} in {slot:?}"
    );
}

#[test]
fn a_scan_starting_and_ending_repaints_its_slot_alone_and_moves_nothing() {
    let mut run = Run::with(Model::new()).warmed();
    let card = rect(&run, &scan(0));
    let idle = part_rect(&run, 0, SCAN_SIZE).to_px(1.0);
    run.step(Step::Update(Msg::Scanning(0)));
    let scanning = part_rect(&run, 0, SCAN_SIZE).to_px(1.0);
    let allowed = idle.union(scanning);
    assert_eq!(slot_parts(&run, 0).len(), 2, "the bar shows");
    for (what, step) in [
        ("a scan starting", None),
        ("a scan ending", Some(Msg::Size(0, 1_288_490_189))),
    ] {
        if let Some(m) = step {
            run.step(Step::Update(m));
        }
        assert!(!run.damage.is_empty(), "{what}: nothing drawn");
        for d in &run.damage {
            assert_eq!(
                d.union(allowed),
                allowed,
                "{what}: {d:?} is outside the slot {allowed:?}"
            );
        }
        assert_eq!(rect(&run, &scan(0)), card, "{what} moved the card");
    }
    assert_eq!(slot_parts(&run, 0).len(), 1, "the bar goes");
}

#[test]
fn a_loading_cards_value_arriving_repaints_the_value_alone() {
    let mut ui = Ui::<u8>::new(
        Size { w: 400.0, h: 300.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    );
    let build = |ui: &mut Ui<u8>, value| {
        ui.build(|cx| {
            cx.summary_card(
                Key::name("card"),
                &SummaryCard {
                    label: "Undo history",
                    value,
                    detail: "Kept for 7 days",
                    delta: None,
                    open: None,
                    separator: ". ",
                },
            );
        });
        ui.layout(&mut FakeText);
    };
    build(&mut ui, CardValue::Loading);
    let at = |ui: &Ui<u8>, path: &[Key]| {
        ui.find(path)
            .and_then(|id| ui.info(id))
            .map_or(Rect::default(), |n| n.rect)
    };
    let card = at(&ui, &[Key::name("card")]);
    let skeleton = at(&ui, &[Key::name("card"), CARD_VALUE]);
    assert!(
        skeleton.w > 0.0 && skeleton.h > 0.0,
        "a skeleton: {skeleton:?}"
    );
    ui.clear_damage();
    build(&mut ui, CardValue::Metric("12 jobs"));
    assert_eq!(at(&ui, &[Key::name("card")]), card, "the card moved");
    let value = at(&ui, &[Key::name("card"), CARD_VALUE]);
    assert!(
        (value.h - skeleton.h).abs() < 0.01,
        "{value:?} vs {skeleton:?}"
    );
    let allowed = skeleton.to_px(1.0);
    assert!(!ui.damage().is_empty(), "the metric is drawn");
    for d in ui.damage() {
        assert_eq!(
            d.union(allowed),
            allowed,
            "{d:?} is outside the value {allowed:?}"
        );
    }
}

#[test]
fn a_loading_cards_skeleton_shows_in_high_contrast() {
    let mut ui = Ui::<u8>::new(
        Size { w: 400.0, h: 300.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    );
    ui.build(|cx| {
        cx.summary_card(
            Key::name("card"),
            &SummaryCard {
                label: "Undo history",
                value: CardValue::Loading,
                detail: "",
                delta: None,
                open: None,
                separator: ". ",
            },
        );
    });
    ui.layout(&mut FakeText);
    let skeleton = ui
        .find(&[Key::name("card"), CARD_VALUE])
        .and_then(|id| ui.info(id))
        .map_or(Rect::default(), |n| n.rect);
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
    let (w, h) = ui.pixel_size();
    let mut pixels = vec![0; usize::try_from(w * h).unwrap_or(0)];
    if let Some(mut canvas) = Canvas::new(&mut pixels, w, h) {
        ui.paint(&mut canvas, &mut FakeText, &Theme::high_contrast(sys));
    }
    let mid = skeleton.center();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a pixel inside a 400 × 300 window"
    )]
    let at = (mid.y as u32) * w + mid.x as u32;
    let pixel = pixels
        .get(usize::try_from(at).unwrap_or(usize::MAX))
        .copied()
        .unwrap_or(0);
    let face = sys.btn_face;
    assert_eq!(
        pixel,
        0xFF00_0000 | u32::from(face.r) << 16 | u32::from(face.g) << 8 | u32::from(face.b),
        "the skeleton at {skeleton:?}"
    );
}
