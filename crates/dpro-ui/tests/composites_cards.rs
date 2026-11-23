//! Summary and scan cards (TD-0009 A3.5 (4), (5), A3.9).
//!
//! Summary cards: a clickable card is a Button named by its label, described by "{value}.
//! {detail}", with presentational children; a static one is a Group named by its label; a grid
//! wraps them at 220 px each. Scan cards: Space toggles, Mixed shows, Details only when the scan
//! failed, and a size update damages only its slot.

#![allow(missing_docs)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use composites::{
    CARDS, CATEGORIES, Model, Msg, NARROW, PAGE, Run, SCANS, START, Step, key, key_up,
};
use dpro_ui::{
    Input, Key, KeyCode, Mods, Rect, Role, Tri,
    accesskit::{Role as AkRole, Toggled},
    composite::{CARD_LABEL, SCAN_CHECK, SCAN_DETAILS, SCAN_SIZE},
    tokens::card,
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
#[ignore = "slice 6a: lands with the code"]
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
#[ignore = "slice 6a: lands with the code"]
fn a_clickable_card_is_named_by_its_label_and_described_by_value_and_detail() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let id = run.find(&card(0)).expect("the first card");
    let node = run.ak(&tree, id).expect("in the tree");
    assert_eq!(node.role(), AkRole::Button);
    assert_eq!(node.label(), Some("Space to free"));
    assert_eq!(node.description(), Some("5.6 GB. Across 4 categories"));
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
#[ignore = "slice 6a: lands with the code"]
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
#[ignore = "slice 6a: lands with the code"]
fn a_scan_cards_box_is_described_by_description_size_and_badge() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let check = run.part(&scan(1), SCAN_CHECK).expect("its box");
    let node = run.ak(&tree, check).expect("in the tree");
    assert_eq!(
        node.description(),
        Some("Downloaded updates already installed. 3.4 GB. Undoable")
    );
    // The name and the description inside the box are presentational (A3.5 addition 9).
    assert_eq!(node.children(), &[]);
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

#[test]
fn a_size_update_damages_only_its_slot() {
    let mut run = Run::with(Model::new()).warmed();
    let slot = rect(&run, &[PAGE, SCANS, Key::index(0), SCAN_SIZE]).to_px(1.0);
    run.step(Step::Update(Msg::Size(0, 1_395_864_371)));
    assert!(!run.damage.is_empty(), "the new size is drawn");
    for d in &run.damage {
        assert_eq!(
            d.union(slot),
            slot,
            "{d:?} is outside the size's slot {slot:?}"
        );
    }
}
