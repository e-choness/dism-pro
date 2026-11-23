//! The batch selection bar (TD-0009 A3.5 (11), A3.9): it shows at 1 and hides at 0, says how many
//! the filter hides, is one Tab stop whose arrows move between its buttons, and a count change
//! damages only its label.

#![allow(missing_docs)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use composites::{BATCH, Model, Msg, PAGE, Run, Step, key};
use dpro_ui::{
    KeyCode, Mods, Role,
    accesskit::Role as AkRole,
    composite::{BATCH_CLEAR, BATCH_COUNT, BATCH_HIDDEN},
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
#[ignore = "slice 6a: lands with the code"]
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
