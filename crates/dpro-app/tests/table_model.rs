//! TD-0009 A2.5 and A2.9: the table model's current row, its view positions, and refreshing the
//! data by `RowKey`, written before the code.
//!
//! - The current row (the one the detail pane shows) is a source row, moved by a view index. It
//!   sets the Shift anchor; extending from the anchor moves it without moving the anchor.
//! - `position` is the inverse of the view. The current row's view index is kept with the view,
//!   so a build finds it in O(1).
//! - A refresh installs new data and carries the checks, the current row and the anchor over by
//!   their keys. A current row the data lost hands over to the row now at its place. The view
//!   keeps its order for the rows still there, new rows after them, until the page's new sort
//!   and filter results land; results begun before the refresh are dropped.
//! - The key map can be built off the UI thread, above `INLINE_ROWS`, and a stale one is dropped.

#![allow(missing_docs)]

use dpro_app::table::{Cell, ColumnId, Order, Pick, Query, RowKey, RowMap, RowSource, TableModel};

/// Rows named by their keys: row `i` is `keys[i]`.
struct Keys(Vec<u64>);

impl RowSource for Keys {
    fn len(&self) -> u32 {
        u32::try_from(self.0.len()).unwrap_or(u32::MAX)
    }
    fn key(&self, row: u32) -> RowKey {
        RowKey(self.0[row as usize])
    }
    fn cell(&self, _row: u32, _col: ColumnId) -> Cell<'_> {
        Cell::Empty
    }
}

const C: ColumnId = ColumnId(0);
const NONE: [u32; 0] = [];

fn model(keys: &[u64]) -> TableModel {
    TableModel::with_keys(RowMap::build(&Keys(keys.to_vec())))
}

fn checked(m: &TableModel) -> Vec<u32> {
    m.selected_rows().collect()
}

/// The keys of the checked rows, under the data `keys`.
fn checked_keys(m: &TableModel, keys: &[u64]) -> Vec<u64> {
    m.selected_rows().map(|r| keys[r as usize]).collect()
}

fn sorted(m: &mut TableModel, order: Vec<u32>) {
    let g = m.begin_sort();
    assert!(m.finish_sort(g, (C, Order::Ascending), order));
}

#[test]
fn the_current_row_is_a_source_row_moved_by_its_view_index() {
    let mut m = model(&[10, 11, 12, 13, 14]);
    assert_eq!((m.current(), m.current_view()), (None, None));
    sorted(&mut m, vec![4, 3, 2, 1, 0]);
    m.set_current(1);
    assert_eq!((m.current(), m.current_view()), (Some(3), Some(1)));
    // It follows its row through a new sort, and is out of view while filtered away.
    sorted(&mut m, vec![0, 1, 2, 3, 4]);
    assert_eq!(m.current_view(), Some(3));
    let g = m.begin_filter();
    assert!(m.finish_filter(g, Query::new("x"), vec![0, 4]));
    assert_eq!((m.current(), m.current_view()), (Some(3), None));
    // Past the end changes nothing.
    m.set_current(9);
    assert_eq!(m.current(), Some(3));
    let r = m.rev;
    m.set_current(1);
    assert_eq!((m.current(), m.current_view()), (Some(4), Some(1)));
    assert_ne!(m.rev, r, "the view sees it");
}

#[test]
fn position_is_the_inverse_of_the_view() {
    let mut m = model(&[0, 1, 2, 3, 4, 5]);
    sorted(&mut m, vec![5, 3, 1, 0, 2, 4]);
    let g = m.begin_filter();
    assert!(m.finish_filter(g, Query::new("x"), vec![0, 1, 4, 5]));
    assert_eq!(m.view(), [5, 1, 0, 4]);
    let at: Vec<Option<u32>> = (0..7).map(|r| m.position(r)).collect();
    assert_eq!(
        at,
        [Some(2), Some(1), None, None, Some(3), Some(0), None],
        "row 6 isn't in the table at all"
    );
    for (i, &row) in (0u32..).zip(m.view()) {
        assert_eq!(m.position(row), Some(i));
    }
}

#[test]
fn the_current_row_sets_the_anchor_and_extending_moves_it_alone() {
    let mut m = model(&[0, 1, 2, 3, 4, 5, 6, 7]);
    m.set_current(2);
    // Shift+Down twice: rows 2 to 3, then 2 to 4, the current row following.
    m.extend_to(3);
    assert_eq!((checked(&m), m.current_view()), (vec![2, 3], Some(3)));
    m.extend_to(4);
    assert_eq!((checked(&m), m.current_view()), (vec![2, 3, 4], Some(4)));
    // Back up past the anchor.
    m.extend_to(1);
    assert_eq!(checked(&m), [1, 2]);
    // A plain move doesn't touch the checks; it moves the anchor.
    m.set_current(6);
    assert_eq!(checked(&m), [1, 2]);
    m.extend_to(7);
    assert_eq!(checked(&m), [6, 7]);
    // A toggle keeps the others, and anchors at its row.
    m.pick(0, Pick::Toggle);
    m.extend_to(1);
    assert_eq!(checked(&m), [0, 1]);
    // Past the end changes nothing.
    m.extend_to(99);
    assert_eq!((checked(&m), m.current_view()), (vec![0, 1], Some(1)));
}

#[test]
fn extending_with_no_anchor_starts_at_the_current_row_or_the_row_itself() {
    let mut m = model(&[0, 1, 2, 3]);
    m.extend_to(2);
    assert_eq!((checked(&m), m.current_view()), (vec![2], Some(2)));
    let mut m = model(&[0, 1, 2, 3]);
    m.set_current(1);
    m.clear_selection();
    m.extend_to(3);
    assert_eq!(checked(&m), [1, 2, 3]);
}

#[test]
fn a_refresh_carries_checks_current_and_anchor_over_by_key() {
    let old = [10, 11, 12, 13, 14];
    let mut m = model(&old);
    m.pick(1, Pick::Toggle); // 11
    m.pick(3, Pick::Toggle); // 13, the anchor
    m.set_current(4); // 14, the anchor now
    m.pick(3, Pick::Toggle); // 13 off
    m.pick(0, Pick::Toggle); // 10, the anchor
    assert_eq!(checked_keys(&m, &old), [10, 11]);
    // 12 goes, 15 and 16 come, and the rest move.
    let new = [16, 14, 11, 10, 15, 13];
    let r = m.rev;
    m.refresh(&Keys(new.to_vec()));
    assert_ne!(m.rev, r);
    assert_eq!(m.rows(), 6);
    assert_eq!(checked_keys(&m, &new), [11, 10]);
    assert_eq!(m.current().map(|c| new[c as usize]), Some(14));
    // The view keeps the old order, new rows after it: 10, 11, 13, 14, then 16 and 15.
    let view: Vec<u64> = m.view().iter().map(|&r| new[r as usize]).collect();
    assert_eq!(view, [10, 11, 13, 14, 16, 15]);
    // The anchor came along: Shift from 10 to 13 checks 10, 11 and 13.
    let at_13 = m.position(5).expect("13 is in view");
    m.extend_to(at_13);
    let mut keys = checked_keys(&m, &new);
    keys.sort_unstable();
    assert_eq!(keys, [10, 11, 13]);
}

#[test]
fn the_view_keeps_its_order_and_new_rows_follow_until_new_results_land() {
    let old = [10, 11, 12, 13];
    let mut m = model(&old);
    sorted(&mut m, vec![3, 1, 0, 2]); // 13, 11, 10, 12
    let g = m.begin_filter();
    assert!(m.finish_filter(g, Query::new("1"), vec![0, 1, 3])); // 13, 11, 10
    // A sort and a filter for the old data are still out.
    let stale_sort = m.begin_sort();
    let stale_filter = m.begin_filter();
    let new = [11, 14, 13, 10, 15];
    m.refresh(&Keys(new.to_vec()));
    let view: Vec<u64> = m.view().iter().map(|&r| new[r as usize]).collect();
    assert_eq!(view, [13, 11, 10, 14, 15]);
    assert_eq!(
        m.sort(),
        Some((C, Order::Ascending)),
        "the order stays shown"
    );
    assert!(!m.query().is_empty(), "so does the filter");
    assert!(!m.is_pending());
    assert!(!m.finish_sort(stale_sort, (C, Order::Ascending), vec![0, 1, 2, 3, 4]));
    assert!(!m.finish_filter(stale_filter, Query::new("1"), vec![0]));
    // The page's results for the new data land as usual.
    sorted(&mut m, vec![4, 3, 2, 1, 0]);
    let g = m.begin_filter();
    assert!(m.finish_filter(g, Query::new("1"), vec![0, 2, 3, 4]));
    let view: Vec<u64> = m.view().iter().map(|&r| new[r as usize]).collect();
    assert_eq!(view, [15, 10, 13, 11]);
}

#[test]
fn a_current_row_the_data_lost_hands_over_to_the_row_at_its_place() {
    let mut m = model(&[1, 2, 3, 4]);
    m.set_current(2); // 3
    m.refresh(&Keys(vec![1, 2, 4]));
    assert_eq!((m.current(), m.current_view()), (Some(2), Some(2)));
    // At the end, the last row; with nothing left, none.
    m.refresh(&Keys(vec![1, 2]));
    assert_eq!(m.current_view(), Some(1));
    m.refresh(&Keys(Vec::new()));
    assert_eq!(
        (m.current(), m.current_view(), m.view()),
        (None, None, &NONE[..])
    );
    assert_eq!(m.selected_count(), 0);
}

#[test]
fn checks_on_rows_the_filter_hides_survive_a_refresh() {
    let old = [1, 2, 3];
    let mut m = model(&old);
    m.select_all();
    let g = m.begin_filter();
    assert!(m.finish_filter(g, Query::new("x"), vec![0]));
    assert_eq!(m.hidden_selected(), 2);
    let new = [3, 2, 1, 9];
    m.refresh(&Keys(new.to_vec()));
    let mut keys = checked_keys(&m, &new);
    keys.sort_unstable();
    assert_eq!(keys, [1, 2, 3]);
    // 1 is shown, 9 is new and shown until the filter runs again: 2 and 3 stay hidden.
    assert_eq!(m.hidden_selected(), 2);
}

#[test]
fn a_map_built_off_the_thread_installs_once_and_a_stale_one_is_dropped() {
    let old = [5, 6, 7];
    let mut m = model(&old);
    m.pick(1, Pick::Toggle);
    let first = m.begin_refresh();
    let second = m.begin_refresh();
    let map_a = RowMap::build(&Keys(vec![7, 6]));
    let map_b = RowMap::build(&Keys(vec![6, 5, 7, 8]));
    assert_eq!((map_a.len(), map_b.len()), (2, 4));
    assert!(!m.finish_refresh(first, map_a), "a newer refresh began");
    assert_eq!(m.rows(), 3);
    assert!(m.finish_refresh(second, map_b));
    assert!(
        !m.finish_refresh(second, RowMap::build(&Keys(vec![1]))),
        "already installed"
    );
    assert_eq!((m.rows(), checked(&m)), (4, vec![0]));
}

#[test]
fn a_model_without_keys_starts_over_on_refresh() {
    let mut m = TableModel::new(3);
    m.select_all();
    m.set_current(1);
    m.refresh(&Keys(vec![1, 2, 3]));
    assert_eq!((m.selected_count(), m.current()), (0, None));
    assert_eq!(m.view(), [0, 1, 2]);
    // From then on it has them.
    m.pick(2, Pick::Toggle);
    m.refresh(&Keys(vec![3, 1]));
    assert_eq!(checked(&m), [0]);
}

#[test]
fn duplicate_keys_map_to_their_first_row() {
    let map = RowMap::build(&Keys(vec![4, 4, 5]));
    assert_eq!(
        (map.get(RowKey(4)), map.get(RowKey(5)), map.get(RowKey(6))),
        (Some(0), Some(2), None)
    );
}

/// An anchor the filter hides can't start a range: the current row does, and becomes the anchor,
/// so the range grows from it as Shift moves on.
#[test]
fn extending_past_an_anchor_the_filter_hides_grows_from_the_current_row() {
    let mut m = model(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    m.set_current(0); // the anchor, about to be hidden
    m.extend_to(2);
    assert_eq!(checked(&m), [0, 1, 2]);
    let g = m.begin_filter();
    assert!(m.finish_filter(g, Query::new("x"), (1..10).collect()));
    assert_eq!(m.current_view(), Some(1));
    m.extend_to(2); // row 3
    assert_eq!(checked(&m), [2, 3]);
    m.extend_to(4); // row 5
    assert_eq!(checked(&m), [2, 3, 4, 5], "from row 2, the new anchor");
}

/// Rows named by their keys, some locked.
struct Locked {
    keys: Vec<u64>,
    locked: Vec<u64>,
}

impl RowSource for Locked {
    fn len(&self) -> u32 {
        u32::try_from(self.keys.len()).unwrap_or(u32::MAX)
    }
    fn key(&self, row: u32) -> RowKey {
        RowKey(self.keys[row as usize])
    }
    fn cell(&self, _row: u32, _col: ColumnId) -> Cell<'_> {
        Cell::Empty
    }
    fn locked(&self, row: u32) -> bool {
        self.locked.contains(&self.keys[row as usize])
    }
}

/// TD-0009 A2.5: a locked row can be current, but nothing checks it: a toggle, a Shift range
/// over it, check-all, or a refresh that locks a checked row.
#[test]
fn a_locked_row_is_never_checked() {
    let old = Locked {
        keys: vec![10, 11, 12, 13, 14],
        locked: vec![12],
    };
    let mut m = TableModel::with_keys(RowMap::build(&old));
    assert!(m.is_locked(2) && !m.is_locked(1));
    m.pick(2, Pick::Toggle);
    assert_eq!(m.selected_count(), 0);
    m.set_current(2);
    assert_eq!(m.current(), Some(2), "it can be current");
    m.set_current(0);
    m.extend_to(4);
    assert_eq!(checked(&m), [0, 1, 3, 4]);
    m.select_all();
    assert_eq!(m.selected_count(), 4);
    // 10, checked, comes back locked: its check goes. 12 is no longer locked.
    let new = Locked {
        keys: vec![12, 10, 11, 16],
        locked: vec![10, 16],
    };
    m.refresh(&new);
    assert_eq!(checked_keys(&m, &new.keys), [11]);
    assert_eq!(
        (0..4).map(|r| m.is_locked(r)).collect::<Vec<_>>(),
        [false, true, false, true]
    );
    // Locking by hand unchecks; unlocking lets it be checked again.
    m.set_locked(2, true);
    assert_eq!(m.selected_count(), 0);
    m.set_locked(2, false);
    let at = m.position(2).expect("11 is in view");
    m.pick(at, Pick::Toggle);
    assert_eq!(checked_keys(&m, &new.keys), [11]);
    // A model without keys takes locks by hand, and ignores rows past the end.
    let mut m = TableModel::new(3);
    m.set_locked(1, true);
    m.set_locked(7, true);
    m.select_all();
    assert_eq!(checked(&m), [0, 2]);
}
