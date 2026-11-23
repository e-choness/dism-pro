//! Settings rows (TD-0009 A3.5 (15), A3.9): the control is named and described by its row, the
//! row's text isn't read twice, the control wraps under the text below 600 px, and a radio set is
//! one Tab stop.

#![allow(missing_docs)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use composites::{CONTROL, Model, Msg, NARROW, PAGE, ROWS, Run, SETTINGS, Step, key};
use dpro_ui::{
    Key, KeyCode, Mods, Rect,
    accesskit::Role as AkRole,
    composite::{SETTINGS_HEADING, SETTINGS_LABEL},
};

/// Part `key` of row `r`, wherever the group nests its rows.
fn in_row(run: &Run, r: u64, key: Key) -> Option<dpro_ui::NodeId> {
    let row = run.part(&[PAGE, SETTINGS], Key::index(r))?;
    run.part_of(row, key)
}

fn rect(run: &Run, id: dpro_ui::NodeId) -> Rect {
    run.ui.info(id).map_or(Rect::default(), |n| n.rect)
}

#[test]
fn each_control_is_named_by_its_rows_label() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    for (r, spec) in (0..).zip(ROWS) {
        let control = in_row(&run, r, CONTROL).expect("its control");
        let node = run.ak(&tree, control).expect("in the tree");
        assert_eq!(node.label(), Some(spec.label), "row {r}");
    }
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn each_control_is_described_by_its_rows_description() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    for (r, spec) in (0..).zip(ROWS) {
        let control = in_row(&run, r, CONTROL).expect("its control");
        let node = run.ak(&tree, control).expect("in the tree");
        assert_eq!(node.description(), spec.description, "row {r}");
    }
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn a_rows_text_isnt_read_twice() {
    // The control says the label and the description; the row's own text is presentational.
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    for r in 0..4 {
        let label = in_row(&run, r, SETTINGS_LABEL).expect("its label");
        assert!(
            run.ak(&tree, label).is_none(),
            "row {r}'s label is in the tree"
        );
    }
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn the_groups_heading_is_level_2() {
    let mut run = Run::accessible(Model::new());
    let tree = run.ui.a11y_tree();
    let heading = run
        .part(&[PAGE, SETTINGS], SETTINGS_HEADING)
        .expect("heading");
    let node = run.ak(&tree, heading).expect("in the tree");
    assert_eq!(node.role(), AkRole::Heading);
    assert_eq!(
        node.level(),
        Some(1),
        "AccessKit's level is aria-level less one"
    );
}

#[test]
fn the_control_is_at_the_rows_end_when_wide() {
    let run = Run::with(Model::new());
    for r in [0, 2, 3] {
        let label = in_row(&run, r, SETTINGS_LABEL).expect("label");
        let control = in_row(&run, r, CONTROL).expect("control");
        let (l, c) = (rect(&run, label), rect(&run, control));
        assert!(
            c.x >= l.x + l.w,
            "row {r}: the control {c:?} after the text {l:?}"
        );
    }
}

#[test]
#[ignore = "slice 6a: lands with the code"]
fn the_control_wraps_under_the_text_below_600_px() {
    let mut run = Run::with(Model::new());
    run.step(Step::Resize(dpro_ui::Size { w: 560.0, ..NARROW }));
    for r in 0..4 {
        let label = in_row(&run, r, SETTINGS_LABEL).expect("label");
        let control = in_row(&run, r, CONTROL).expect("control");
        let (l, c) = (rect(&run, label), rect(&run, control));
        assert!(
            c.y >= l.y + l.h,
            "row {r}: the control {c:?} under the text {l:?}"
        );
    }
}

#[test]
fn a_radio_set_is_one_tab_stop_and_the_arrows_pick() {
    let mut run = Run::with(Model::new());
    let empty = in_row(&run, 0, CONTROL).expect("the check box");
    run.ui.set_focus(Some(empty));
    run.step(key(KeyCode::Tab, Mods::default()));
    let radios = in_row(&run, 1, CONTROL).expect("the radio set");
    let picked = run.ui.children(radios).nth(1).expect("the picked radio");
    assert_eq!(
        run.ui.focus(),
        Some(picked),
        "Tab lands on the picked radio"
    );
    run.step(key(KeyCode::Down, Mods::default()));
    assert_eq!(run.model.last, Some(Msg::When(2)));
    run.step(key(KeyCode::Tab, Mods::default()));
    let keep = in_row(&run, 2, CONTROL).expect("the dropdown");
    assert_eq!(run.ui.focus(), Some(keep), "one Tab leaves the set");
}
