//! Table models for lists of up to 100k rows (TD-0009 §5.1, §5.3, §7 U11).
//!
//! - [`SortKeys`] and [`FilterIndex`] are built once per data load, off the UI thread above
//!   [`INLINE_ROWS`] rows. Sorting is O(n log n) on pre-folded keys; filtering is O(total
//!   characters), and a narrower query only rechecks the previous matches.
//! - [`TableModel`] lives on the UI thread. It installs their results, tagged with a
//!   [`Generation`] so a stale result is dropped, keeps the visible order, and the selection.
//!
//! Text sorts case-insensitively, with digit runs compared as numbers ("Item 9" before
//! "Item 10") and digits before letters, like Explorer. The order is by code point after folding,
//! not a locale's collation.

use dpro_core::units::{Bytes, UnixMillis};

use crate::Rev;
use crate::fold::{lower_into, search_text};

/// Up to this many rows, sorting and filtering run inline on the UI thread (TD-0009 §5.3).
pub const INLINE_ROWS: u32 = 5_000;

/// Sort keys keep at most this many bytes per cell: enough to order any real name.
const KEY_MAX: usize = 256;
/// A filter query keeps at most this many bytes, which bounds the work per keystroke.
const QUERY_MAX: usize = 1_024;

/// A column of a table, as the page defines them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ColumnId(pub u16);

/// A row's identity that survives a refresh of the data (a package name, a driver's published
/// name), so selection and focus can follow the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowKey(pub u64);

/// Sort direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Order {
    /// A to Z, smallest first, oldest first.
    Ascending,
    /// Z to A, largest first, newest first.
    Descending,
}

/// A check box state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tri {
    /// Unchecked.
    Off,
    /// Some children checked.
    Mixed,
    /// Checked.
    On,
}

/// One cell, borrowed from the page's columnar data. Badge and icon cells arrive with the
/// widgets that draw them (TD-0009 §11, step 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell<'a> {
    /// Nothing to show.
    Empty,
    /// Proportional text.
    Text(&'a str),
    /// Paths, IDs and versions, in the mono face.
    Mono(&'a str),
    /// A size, formatted at paint time.
    Size(Bytes),
    /// A time, formatted at paint time.
    Time(UnixMillis),
    /// A check box.
    Check(Tri),
}

/// Columnar row data a table shows. Rows are numbered `0..len()` in the source's own order.
pub trait RowSource {
    /// Number of rows.
    fn len(&self) -> u32;
    /// Whether there are no rows.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The row's identity, stable across refreshes.
    fn key(&self, row: u32) -> RowKey;
    /// One cell. Called for visible rows while painting, and for every row while building keys.
    fn cell(&self, row: u32, col: ColumnId) -> Cell<'_>;
}

// Classes order cells of different kinds in one column; the payload orders cells of one kind.
const CLASS_CHECK: u8 = 1;
const CLASS_SIZE: u8 = 2;
const CLASS_TIME: u8 = 3;
const CLASS_TEXT: u8 = 4;
/// Starts a digit run in a text key: below every printable character, so digits sort first.
const DIGITS: u8 = 0x01;

/// One column's sort keys: each row's cell folded into bytes that compare in the wanted order.
#[derive(Clone, Debug, Default)]
pub struct SortKeys {
    /// The first 8 key bytes, big-endian, zero-padded: most comparisons end here.
    prefix: Vec<u64>,
    bytes: Vec<u8>,
    /// Exclusive end of each row's key in `bytes`.
    ends: Vec<usize>,
}

impl SortKeys {
    /// Folds column `col` of every row. O(total characters).
    #[must_use]
    pub fn build(src: &impl RowSource, col: ColumnId) -> Self {
        let n = src.len() as usize;
        let mut k = Self {
            prefix: Vec::with_capacity(n),
            bytes: Vec::with_capacity(n.saturating_mul(24)),
            ends: Vec::with_capacity(n),
        };
        for row in 0..src.len() {
            let start = k.bytes.len();
            encode(src.cell(row, col), &mut k.bytes);
            k.bytes.truncate(start + KEY_MAX);
            k.ends.push(k.bytes.len());
            k.prefix.push(prefix_of(&k.bytes[start..]));
        }
        k
    }

    /// Number of rows.
    #[must_use]
    pub fn len(&self) -> u32 {
        u32::try_from(self.ends.len()).unwrap_or(u32::MAX)
    }

    /// Whether there are no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// Every row, sorted. Equal keys keep source order in both directions, so a sort is stable
    /// and the same data always gives the same order.
    #[must_use]
    pub fn order(&self, order: Order) -> Vec<u32> {
        let mut v: Vec<(u64, u32)> = self.prefix.iter().copied().zip(0u32..).collect();
        let keys = |a: &(u64, u32), b: &(u64, u32)| {
            a.0.cmp(&b.0).then_with(|| self.key(a.1).cmp(self.key(b.1)))
        };
        match order {
            Order::Ascending => v.sort_unstable_by(|a, b| keys(a, b).then(a.1.cmp(&b.1))),
            Order::Descending => v.sort_unstable_by(|a, b| keys(b, a).then(a.1.cmp(&b.1))),
        }
        v.into_iter().map(|(_, row)| row).collect()
    }

    fn key(&self, row: u32) -> &[u8] {
        let row = row as usize;
        let start = match row.checked_sub(1) {
            Some(prev) => self.ends.get(prev).copied().unwrap_or(0),
            None => 0,
        };
        let end = self.ends.get(row).copied().unwrap_or(start);
        self.bytes.get(start..end).unwrap_or_default()
    }
}

fn prefix_of(key: &[u8]) -> u64 {
    let mut p = [0u8; 8];
    let n = key.len().min(8);
    p[..n].copy_from_slice(&key[..n]);
    u64::from_be_bytes(p)
}

fn encode(cell: Cell<'_>, out: &mut Vec<u8>) {
    match cell {
        Cell::Empty => {}
        Cell::Check(t) => out.extend_from_slice(&[
            CLASS_CHECK,
            match t {
                Tri::Off => 0,
                Tri::Mixed => 1,
                Tri::On => 2,
            },
        ]),
        Cell::Size(Bytes(b)) => {
            out.push(CLASS_SIZE);
            out.extend_from_slice(&b.to_be_bytes());
        }
        Cell::Time(UnixMillis(t)) => {
            out.push(CLASS_TIME);
            out.extend_from_slice(&t.to_be_bytes());
        }
        Cell::Text(s) | Cell::Mono(s) => {
            out.push(CLASS_TEXT);
            natural_key(s, out);
        }
    }
}

/// Lowercases `s` into `out`, writing each ASCII digit run as [`DIGITS`], its length without
/// leading zeros, then the digits: a longer number is a larger one.
fn natural_key(s: &str, out: &mut Vec<u8>) {
    let limit = out.len() + KEY_MAX;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && out.len() < limit {
        let b = bytes[i];
        if b.is_ascii_digit() {
            let run = bytes[i..].iter().take_while(|d| d.is_ascii_digit()).count();
            let digits = &bytes[i..i + run];
            let significant = digits
                .iter()
                .position(|&d| d != b'0')
                .map_or(&digits[run - 1..], |first| &digits[first..]);
            out.push(DIGITS);
            out.push(u8::try_from(significant.len()).unwrap_or(u8::MAX));
            out.extend_from_slice(significant);
            i += run;
        } else if b.is_ascii() {
            out.push(b.to_ascii_lowercase());
            i += 1;
        } else {
            let Some(c) = s.get(i..).and_then(|rest| rest.chars().next()) else {
                break;
            };
            let mut buf = [0u8; 4];
            for l in c.to_lowercase() {
                out.extend_from_slice(l.encode_utf8(&mut buf).as_bytes());
            }
            i += c.len_utf8();
        }
    }
}

/// A filter query: terms split at whitespace, folded. A row matches when every term occurs in
/// one of its indexed cells.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    terms: Vec<String>,
}

impl Query {
    /// Folds `text` into terms. Duplicate terms and terms inside another term are dropped: they
    /// can't change the result.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let folded = search_text(text, QUERY_MAX);
        let mut terms: Vec<String> = Vec::new();
        for t in folded.split(' ').filter(|t| !t.is_empty()) {
            if terms.iter().any(|have| have.contains(t)) {
                continue;
            }
            terms.retain(|have| !t.contains(have.as_str()));
            terms.push(t.to_owned());
        }
        // Longest first: the most selective term drives the scan.
        terms.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        Self { terms }
    }

    /// Whether the query filters nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Whether every row this query matches also matched `previous`: each of its terms lies
    /// inside one of ours. Then filtering only needs to recheck `previous`'s matches.
    #[must_use]
    pub fn narrows(&self, previous: &Query) -> bool {
        previous
            .terms
            .iter()
            .all(|p| self.terms.iter().any(|t| t.contains(p.as_str())))
    }
}

/// The folded text of a table's searchable columns, one row after another.
#[derive(Clone, Debug, Default)]
pub struct FilterIndex {
    /// Each row's indexed cells, folded, each followed by `\n`. Terms never contain whitespace,
    /// so a match can't span two cells or two rows.
    text: String,
    /// Exclusive end of each row in `text`.
    ends: Vec<usize>,
}

impl FilterIndex {
    /// Folds the text and mono cells of `columns` for every row. O(total characters).
    #[must_use]
    pub fn build(src: &impl RowSource, columns: &[ColumnId]) -> Self {
        let n = src.len() as usize;
        let mut ix = Self {
            text: String::with_capacity(n.saturating_mul(32)),
            ends: Vec::with_capacity(n),
        };
        for row in 0..src.len() {
            for &col in columns {
                if let Cell::Text(s) | Cell::Mono(s) = src.cell(row, col) {
                    lower_into(s, &mut ix.text);
                    ix.text.push('\n');
                }
            }
            // A row with nothing indexed gets an empty span, which no term matches.
            ix.ends.push(ix.text.len());
        }
        ix
    }

    /// Number of rows.
    #[must_use]
    pub fn len(&self) -> u32 {
        u32::try_from(self.ends.len()).unwrap_or(u32::MAX)
    }

    /// Whether there are no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// Writes the rows matching `query` to `out`, ascending. With `previous`, the last query and
    /// its matches, only those rows are rechecked when `query` [narrows](Query::narrows) it.
    pub fn filter(&self, query: &Query, previous: Option<(&Query, &[u32])>, out: &mut Vec<u32>) {
        out.clear();
        if query.is_empty() {
            out.extend(0..self.len());
            return;
        }
        match previous {
            Some((last, rows)) if !last.is_empty() && query.narrows(last) => {
                out.extend(
                    rows.iter()
                        .copied()
                        .filter(|&r| self.row_matches(r, &query.terms)),
                );
            }
            _ => self.scan(query, out),
        }
    }

    /// Finds the first term across all rows, then checks the others in each row it hits.
    fn scan(&self, query: &Query, out: &mut Vec<u32>) {
        let Some((lead, rest)) = query.terms.split_first() else {
            return;
        };
        let mut from = 0;
        let mut row = 0usize;
        while let Some(at) = self.text.get(from..).and_then(|t| t.find(lead.as_str())) {
            let at = from + at;
            while self.ends.get(row).is_some_and(|&end| end <= at) {
                row += 1;
            }
            let Some(&end) = self.ends.get(row) else {
                return;
            };
            let Ok(r) = u32::try_from(row) else {
                return;
            };
            if self.row_matches(r, rest) {
                out.push(r);
            }
            // The rest of this row can't add anything.
            from = end;
        }
    }

    fn row_matches(&self, row: u32, terms: &[String]) -> bool {
        let row = row as usize;
        let start = match row.checked_sub(1) {
            Some(prev) => self.ends.get(prev).copied().unwrap_or(0),
            None => 0,
        };
        let Some(text) = self
            .ends
            .get(row)
            .and_then(|&end| self.text.get(start..end))
        else {
            return false;
        };
        terms.iter().all(|t| text.contains(t.as_str()))
    }
}

/// Tags an off-thread sort or filter, so the model can drop a result that a newer request
/// replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Generation(u32);

/// How a click or key changes the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pick {
    /// Click, or arrow keys: only this row.
    Only,
    /// Ctrl+click, Ctrl+Space: this row flips; the others stay.
    Toggle,
    /// Shift+click, Shift+arrow: the rows from the anchor to this one, in view order.
    Extend,
}

/// Selected source rows: a bitset, so it survives sorting and filtering.
#[derive(Clone, Debug, Default)]
struct Selection {
    bits: Vec<u64>,
    count: u32,
    /// Where Shift ranges start: the last row picked with `Only` or `Toggle`.
    anchor: Option<u32>,
}

impl Selection {
    fn new(rows: u32) -> Self {
        Self {
            bits: vec![0; (rows as usize).div_ceil(64)],
            count: 0,
            anchor: None,
        }
    }

    fn contains(&self, row: u32) -> bool {
        let row = row as usize;
        self.bits
            .get(row / 64)
            .is_some_and(|w| w & (1 << (row % 64)) != 0)
    }

    fn set(&mut self, row: u32, on: bool) {
        let i = row as usize;
        let Some(word) = self.bits.get_mut(i / 64) else {
            return;
        };
        let bit = 1u64 << (i % 64);
        if (*word & bit != 0) == on {
            return;
        }
        *word ^= bit;
        if on {
            self.count += 1;
        } else {
            self.count -= 1;
        }
    }

    fn clear(&mut self) {
        self.bits.fill(0);
        self.count = 0;
    }
}

/// A table's state on the UI thread: what is visible, in which order, and what is selected.
#[derive(Clone, Debug)]
pub struct TableModel {
    rows: u32,
    sort: Option<(ColumnId, Order)>,
    /// Every source row in sort order; `None` keeps source order.
    sorted: Option<Vec<u32>>,
    query: Query,
    /// The rows `query` matches, ascending; `None` when nothing is filtered.
    matches: Option<Vec<u32>>,
    /// Sorted, then filtered: the rows as shown, top to bottom.
    view: Vec<u32>,
    /// Scratch for intersecting the sort with the filter.
    mask: Vec<u64>,
    selection: Selection,
    sort_gen: u32,
    filter_gen: u32,
    sort_pending: bool,
    filter_pending: bool,
    /// Bumped whenever the view or the selection changes.
    pub rev: Rev,
}

impl TableModel {
    /// A table of `rows` rows in source order, unfiltered, nothing selected.
    #[must_use]
    pub fn new(rows: u32) -> Self {
        Self {
            rows,
            sort: None,
            sorted: None,
            query: Query::default(),
            matches: None,
            view: (0..rows).collect(),
            mask: Vec::new(),
            selection: Selection::new(rows),
            sort_gen: 0,
            filter_gen: 0,
            sort_pending: false,
            filter_pending: false,
            rev: Rev::default(),
        }
    }

    /// Number of source rows.
    #[must_use]
    pub fn rows(&self) -> u32 {
        self.rows
    }

    /// Whether sorting and filtering run inline rather than off the UI thread.
    #[must_use]
    pub fn runs_inline(&self) -> bool {
        self.rows <= INLINE_ROWS
    }

    /// The source rows as shown, top to bottom.
    #[must_use]
    pub fn view(&self) -> &[u32] {
        &self.view
    }

    /// The current sort, if any.
    #[must_use]
    pub fn sort(&self) -> Option<(ColumnId, Order)> {
        self.sort
    }

    /// The current filter.
    #[must_use]
    pub fn query(&self) -> &Query {
        &self.query
    }

    /// Whether a sort or filter result is still to come. The old view stays until it arrives.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.sort_pending || self.filter_pending
    }

    /// Starts a sort: the returned tag must come back with its result.
    pub fn begin_sort(&mut self) -> Generation {
        self.sort_gen = self.sort_gen.wrapping_add(1);
        self.sort_pending = true;
        Generation(self.sort_gen)
    }

    /// Installs a sort result: every source row once, in order. Returns false, changing nothing,
    /// if a newer sort was started or the result isn't an order of exactly this table's rows.
    pub fn finish_sort(
        &mut self,
        generation: Generation,
        by: (ColumnId, Order),
        rows: Vec<u32>,
    ) -> bool {
        if generation != Generation(self.sort_gen)
            || !self.sort_pending
            || !self.is_permutation(&rows)
        {
            return false;
        }
        self.sort_pending = false;
        self.sort = Some(by);
        self.sorted = Some(rows);
        self.rebuild_view();
        true
    }

    /// Back to source order. Cancels a pending sort.
    pub fn clear_sort(&mut self) {
        self.sort_gen = self.sort_gen.wrapping_add(1);
        self.sort_pending = false;
        self.sort = None;
        self.sorted = None;
        self.rebuild_view();
    }

    /// Starts a filter: the returned tag must come back with its result.
    pub fn begin_filter(&mut self) -> Generation {
        self.filter_gen = self.filter_gen.wrapping_add(1);
        self.filter_pending = true;
        Generation(self.filter_gen)
    }

    /// Installs a filter result: the rows `query` matches, ascending. An empty query shows every
    /// row. Returns false, changing nothing, if a newer filter was started or the rows aren't
    /// ascending rows of this table. Selected rows the filter hides stay selected;
    /// [`hidden_selected`](Self::hidden_selected) counts them.
    pub fn finish_filter(&mut self, generation: Generation, query: Query, rows: Vec<u32>) -> bool {
        let ascending = rows.windows(2).all(|w| w[0] < w[1]);
        if generation != Generation(self.filter_gen)
            || !self.filter_pending
            || !ascending
            || rows.last().is_some_and(|&r| r >= self.rows)
        {
            return false;
        }
        self.filter_pending = false;
        self.matches = if query.is_empty() { None } else { Some(rows) };
        self.query = query;
        self.rebuild_view();
        true
    }

    /// Whether `rows` holds each row of this table exactly once. O(n), using the mask as scratch.
    fn is_permutation(&mut self, rows: &[u32]) -> bool {
        if rows.len() != self.rows as usize {
            return false;
        }
        self.mask.clear();
        self.mask.resize((self.rows as usize).div_ceil(64), 0);
        for &r in rows {
            if r >= self.rows {
                return false;
            }
            let r = r as usize;
            let bit = 1u64 << (r % 64);
            let Some(w) = self.mask.get_mut(r / 64) else {
                return false;
            };
            if *w & bit != 0 {
                return false;
            }
            *w |= bit;
        }
        true
    }

    fn rebuild_view(&mut self) {
        self.view.clear();
        match (&self.sorted, &self.matches) {
            (None, None) => self.view.extend(0..self.rows),
            (Some(sorted), None) => self.view.extend_from_slice(sorted),
            (None, Some(matches)) => self.view.extend_from_slice(matches),
            (Some(sorted), Some(matches)) => {
                self.mask.clear();
                self.mask.resize((self.rows as usize).div_ceil(64), 0);
                for &r in matches {
                    let r = r as usize;
                    if let Some(w) = self.mask.get_mut(r / 64) {
                        *w |= 1 << (r % 64);
                    }
                }
                let mask = &self.mask;
                self.view.extend(sorted.iter().copied().filter(|&r| {
                    let r = r as usize;
                    mask.get(r / 64).is_some_and(|w| w & (1 << (r % 64)) != 0)
                }));
            }
        }
        self.rev.bump();
    }

    /// Changes the selection at view position `at` (ignored past the end).
    pub fn pick(&mut self, at: u32, how: Pick) {
        let Some(&row) = self.view.get(at as usize) else {
            return;
        };
        match how {
            Pick::Only => {
                self.selection.clear();
                self.selection.set(row, true);
                self.selection.anchor = Some(row);
            }
            Pick::Toggle => {
                let on = !self.selection.contains(row);
                self.selection.set(row, on);
                self.selection.anchor = Some(row);
            }
            Pick::Extend => {
                // Without a visible anchor, Shift acts like a plain click.
                let from = self
                    .selection
                    .anchor
                    .and_then(|a| self.view.iter().position(|&r| r == a));
                let Some(from) = from else {
                    return self.pick(at, Pick::Only);
                };
                let to = at as usize;
                self.selection.clear();
                for &r in &self.view[from.min(to)..=from.max(to)] {
                    self.selection.set(r, true);
                }
            }
        }
        self.rev.bump();
    }

    /// Selects every visible row (Ctrl+A). Rows the filter hides keep their state.
    pub fn select_all(&mut self) {
        for &r in &self.view {
            self.selection.set(r, true);
        }
        self.rev.bump();
    }

    /// Deselects every row, visible or not (Esc).
    pub fn clear_selection(&mut self) {
        self.selection.clear();
        self.selection.anchor = None;
        self.rev.bump();
    }

    /// Whether source row `row` is selected.
    #[must_use]
    pub fn is_selected(&self, row: u32) -> bool {
        self.selection.contains(row)
    }

    /// Number of selected rows, visible or not.
    #[must_use]
    pub fn selected_count(&self) -> u32 {
        self.selection.count
    }

    /// Selected rows the filter hides. O(visible rows); call when the count is shown.
    #[must_use]
    pub fn hidden_selected(&self) -> u32 {
        if self.matches.is_none() {
            return 0;
        }
        let visible = self
            .view
            .iter()
            .filter(|&&r| self.selection.contains(r))
            .count();
        self.selection.count - u32::try_from(visible).unwrap_or(self.selection.count)
    }

    /// The selected source rows, ascending.
    pub fn selected_rows(&self) -> impl Iterator<Item = u32> + '_ {
        (0..self.rows).filter(|&r| self.selection.contains(r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rows(Vec<Cell<'static>>);

    impl RowSource for Rows {
        fn len(&self) -> u32 {
            u32::try_from(self.0.len()).unwrap()
        }
        fn key(&self, row: u32) -> RowKey {
            RowKey(u64::from(row))
        }
        fn cell(&self, row: u32, _col: ColumnId) -> Cell<'_> {
            self.0[row as usize]
        }
    }

    const C: ColumnId = ColumnId(0);
    const NO_ROWS: [u32; 0] = [];

    fn texts(v: &[&'static str]) -> Rows {
        Rows(v.iter().map(|&s| Cell::Text(s)).collect())
    }

    fn sorted(rows: &[&'static str], order: Order) -> Vec<&'static str> {
        let keys = SortKeys::build(&texts(rows), C);
        keys.order(order)
            .iter()
            .map(|&r| rows[r as usize])
            .collect()
    }

    #[test]
    fn text_sorts_naturally_and_case_insensitively() {
        let rows = [
            "item 10",
            "Item 9",
            "item 9b",
            "ITEM 009",
            "item",
            "item 1.10",
            "item 1.9",
            "Apple",
            "",
        ];
        // "Item 9" and "ITEM 009" tie, so source order decides.
        assert_eq!(
            sorted(&rows, Order::Ascending),
            [
                "",
                "Apple",
                "item",
                "item 1.9",
                "item 1.10",
                "Item 9",
                "ITEM 009",
                "item 9b",
                "item 10",
            ]
        );
    }

    #[test]
    fn digits_sort_before_letters_and_zero_runs_count() {
        assert_eq!(
            sorted(&["a b", "a1", "a0", "a00", "ab"], Order::Ascending),
            ["a0", "a00", "a1", "a b", "ab"]
        );
    }

    #[test]
    fn descending_reverses_keys_but_keeps_ties_in_source_order() {
        let rows = ["b", "A", "a", "c", "B"];
        assert_eq!(sorted(&rows, Order::Ascending), ["A", "a", "b", "B", "c"]);
        assert_eq!(sorted(&rows, Order::Descending), ["c", "b", "B", "A", "a"]);
    }

    #[test]
    fn unicode_folds_and_long_keys_still_order_by_their_start() {
        // 400 bytes of "é", which sorts after "ét…".
        let long_a = "é".repeat(200) + "a";
        let long_b = "é".repeat(200) + "b";
        let rows: Vec<&'static str> = vec![
            "Été",
            "ÉTÉ",
            "été 2",
            Box::leak(long_b.into_boxed_str()),
            Box::leak(long_a.into_boxed_str()),
        ];
        let got = sorted(&rows, Order::Ascending);
        assert_eq!(&got[..3], ["Été", "ÉTÉ", "été 2"]);
        // Past KEY_MAX bytes the two long names tie, so source order decides.
        assert_eq!(got[3].chars().last(), Some('b'));
    }

    #[test]
    fn numbers_and_mixed_kinds_sort_by_class_then_value() {
        let rows = Rows(vec![
            Cell::Size(Bytes(10)),
            Cell::Text("z"),
            Cell::Size(Bytes(9)),
            Cell::Empty,
            Cell::Time(UnixMillis(1)),
            Cell::Size(Bytes(u64::MAX)),
            Cell::Check(Tri::On),
            Cell::Check(Tri::Off),
        ]);
        let order = SortKeys::build(&rows, C).order(Order::Ascending);
        assert_eq!(order, [3, 7, 6, 2, 0, 5, 4, 1]);
    }

    #[test]
    fn sort_keys_of_an_empty_source() {
        let keys = SortKeys::build(&texts(&[]), C);
        assert!(keys.is_empty());
        assert_eq!(keys.order(Order::Ascending), NO_ROWS);
    }

    fn index(rows: &[(&'static str, &'static str, &'static str)]) -> (FilterIndex, Rows3) {
        let src = Rows3(rows.to_vec());
        (FilterIndex::build(&src, &[ColumnId(0), ColumnId(1)]), src)
    }

    struct Rows3(Vec<(&'static str, &'static str, &'static str)>);

    impl RowSource for Rows3 {
        fn len(&self) -> u32 {
            u32::try_from(self.0.len()).unwrap()
        }
        fn key(&self, row: u32) -> RowKey {
            RowKey(u64::from(row))
        }
        fn cell(&self, row: u32, col: ColumnId) -> Cell<'_> {
            let r = self.0[row as usize];
            match col.0 {
                0 => Cell::Text(r.0),
                1 => Cell::Mono(r.1),
                _ => Cell::Text(r.2),
            }
        }
    }

    fn filter(ix: &FilterIndex, q: &str) -> Vec<u32> {
        let mut out = Vec::new();
        ix.filter(&Query::new(q), None, &mut out);
        out
    }

    #[test]
    fn filter_needs_every_term_in_indexed_columns_only() {
        let (ix, _) = index(&[
            ("Contoso Updater", "1.2.3", "secret"),
            ("Fabrikam Toolbox", "10.0", "contoso"),
            ("contoso toolbox", "2.0", ""),
            ("", "", "updater"),
        ]);
        assert_eq!(filter(&ix, "contoso"), [0, 2]);
        assert_eq!(filter(&ix, "TOOLBOX contoso"), [2]);
        assert_eq!(filter(&ix, "updater 1.2"), [0]);
        assert_eq!(filter(&ix, "secret"), NO_ROWS);
        assert_eq!(filter(&ix, "  "), [0, 1, 2, 3]);
    }

    #[test]
    fn a_term_cannot_span_cells_or_rows() {
        let (ix, _) = index(&[("ab", "cd", ""), ("ef", "", "")]);
        assert_eq!(filter(&ix, "bc"), NO_ROWS);
        assert_eq!(filter(&ix, "de"), NO_ROWS);
        assert_eq!(filter(&ix, "def"), NO_ROWS);
        assert_eq!(filter(&ix, "d"), [0]);
    }

    #[test]
    fn narrowing_reuses_previous_matches_and_agrees_with_a_full_scan() {
        let (ix, _) = index(&[
            ("Contoso Updater Pro", "1", ""),
            ("Contoso Toolbox", "2", ""),
            ("Northwind Updater", "3", ""),
            ("contoso upd", "4", ""),
        ]);
        let steps = [
            "c",
            "co",
            "con",
            "contoso",
            "contoso u",
            "contoso up",
            "contoso upd",
            "upd contoso pro",
        ];
        let mut last = Query::default();
        let mut previous: Vec<u32> = Vec::new();
        for s in steps {
            let q = Query::new(s);
            let mut out = Vec::new();
            ix.filter(&q, Some((&last, previous.as_slice())), &mut out);
            assert_eq!(out, filter(&ix, s), "query {s:?}");
            last = q;
            previous = out;
        }
        assert_eq!(previous, [0]);
    }

    #[test]
    fn a_wider_query_does_not_narrow() {
        assert!(Query::new("contoso upd").narrows(&Query::new("contoso up")));
        assert!(Query::new("upd contoso").narrows(&Query::new("contoso up")));
        assert!(!Query::new("contoso").narrows(&Query::new("contoso up")));
        assert!(!Query::new("contoso up").narrows(&Query::new("contoso upd")));
    }

    #[test]
    fn query_drops_terms_another_term_covers() {
        assert_eq!(Query::new("up update UPD"), Query::new("update"));
        assert!(Query::new(" \t ").is_empty());
    }

    #[test]
    fn rows_with_nothing_indexed_never_match() {
        let src = Rows(vec![
            Cell::Empty,
            Cell::Text("a"),
            Cell::Size(Bytes(1)),
            Cell::Empty,
        ]);
        let ix = FilterIndex::build(&src, &[C]);
        assert_eq!(ix.len(), 4);
        assert_eq!(filter(&ix, "a"), [1]);
        assert_eq!(filter(&ix, "1"), NO_ROWS);
    }

    fn model_with(
        rows: u32,
        order: Option<Vec<u32>>,
        matches: Option<(&str, Vec<u32>)>,
    ) -> TableModel {
        let mut m = TableModel::new(rows);
        if let Some(o) = order {
            let g = m.begin_sort();
            assert!(m.finish_sort(g, (C, Order::Ascending), o));
        }
        if let Some((q, rows)) = matches {
            let g = m.begin_filter();
            assert!(m.finish_filter(g, Query::new(q), rows));
        }
        m
    }

    #[test]
    fn view_is_the_sort_order_restricted_to_matches() {
        let m = model_with(6, Some(vec![5, 3, 1, 0, 2, 4]), Some(("x", vec![0, 1, 4])));
        assert_eq!(m.view(), [1, 0, 4]);
        let m = model_with(6, None, Some(("x", vec![0, 1, 4])));
        assert_eq!(m.view(), [0, 1, 4]);
        let m = model_with(3, Some(vec![2, 0, 1]), None);
        assert_eq!(m.view(), [2, 0, 1]);
        assert_eq!(TableModel::new(3).view(), [0, 1, 2]);
    }

    #[test]
    fn stale_or_misfit_results_are_dropped() {
        let mut m = TableModel::new(3);
        let old = m.begin_sort();
        let new = m.begin_sort();
        assert!(m.is_pending());
        assert!(!m.finish_sort(old, (C, Order::Ascending), vec![2, 1, 0]));
        assert!(!m.finish_sort(new, (C, Order::Ascending), vec![1, 0]));
        assert!(m.finish_sort(new, (C, Order::Descending), vec![2, 1, 0]));
        assert!(!m.finish_sort(new, (C, Order::Ascending), vec![0, 1, 2]));
        assert_eq!(m.view(), [2, 1, 0]);
        assert_eq!(m.sort(), Some((C, Order::Descending)));

        let f = m.begin_filter();
        m.clear_sort();
        assert!(m.finish_filter(f, Query::new("a"), vec![0, 2]));
        assert!(!m.is_pending());
        assert_eq!(m.view(), [0, 2]);

        let g = m.begin_sort();
        m.clear_sort();
        assert!(!m.finish_sort(g, (C, Order::Ascending), vec![0, 1, 2]));
        assert_eq!(m.sort(), None);
    }

    #[test]
    fn results_with_foreign_rows_are_rejected() {
        let mut m = TableModel::new(3);
        for bad in [
            vec![0, 1, 3],
            vec![0, 0, 2],
            vec![2, 1, 1],
            vec![0, 1, 2, 3],
        ] {
            let g = m.begin_sort();
            assert!(
                !m.finish_sort(g, (C, Order::Ascending), bad.clone()),
                "{bad:?}"
            );
        }
        for bad in [vec![0, 3], vec![1, 1], vec![2, 0], vec![5]] {
            let g = m.begin_filter();
            assert!(!m.finish_filter(g, Query::new("x"), bad.clone()), "{bad:?}");
        }
        assert_eq!(m.view(), [0, 1, 2]);
        let g = m.begin_filter();
        assert!(m.finish_filter(g, Query::new("x"), Vec::new()));
        assert_eq!(m.view(), NO_ROWS);
    }

    #[test]
    fn an_empty_query_removes_the_filter() {
        let mut m = model_with(3, None, Some(("x", vec![1])));
        let g = m.begin_filter();
        assert!(m.finish_filter(g, Query::new(""), Vec::new()));
        assert_eq!(m.view(), [0, 1, 2]);
        assert!(m.query().is_empty());
    }

    #[test]
    fn every_change_bumps_the_revision() {
        let mut m = TableModel::new(3);
        let r0 = m.rev;
        let g = m.begin_sort();
        m.finish_sort(g, (C, Order::Ascending), vec![0, 1, 2]);
        assert_ne!(m.rev, r0);
        let r1 = m.rev;
        m.pick(0, Pick::Only);
        assert_ne!(m.rev, r1);
    }

    #[test]
    fn picks_follow_view_order() {
        let mut m = model_with(6, Some(vec![5, 4, 3, 2, 1, 0]), None);
        m.pick(1, Pick::Only); // row 4
        m.pick(3, Pick::Extend); // rows 4, 3, 2
        assert_eq!(m.selected_rows().collect::<Vec<_>>(), [2, 3, 4]);
        m.pick(0, Pick::Extend); // the anchor stays: rows 5, 4
        assert_eq!(m.selected_rows().collect::<Vec<_>>(), [4, 5]);
        m.pick(5, Pick::Toggle); // + row 0, new anchor
        assert_eq!(m.selected_count(), 3);
        m.pick(4, Pick::Extend); // rows 1, 0
        assert_eq!(m.selected_rows().collect::<Vec<_>>(), [0, 1]);
        m.pick(4, Pick::Toggle);
        assert!(!m.is_selected(1));
        m.pick(99, Pick::Only);
        assert_eq!(m.selected_count(), 1);
    }

    #[test]
    fn extend_without_a_visible_anchor_acts_like_a_click() {
        let mut m = TableModel::new(4);
        m.pick(2, Pick::Extend);
        assert_eq!(m.selected_rows().collect::<Vec<_>>(), [2]);
        let g = m.begin_filter();
        m.finish_filter(g, Query::new("x"), vec![0, 1]);
        m.pick(1, Pick::Extend);
        assert_eq!(m.selected_rows().collect::<Vec<_>>(), [1]);
    }

    #[test]
    fn the_filter_keeps_hidden_selections_and_counts_them() {
        let mut m = TableModel::new(5);
        m.select_all();
        assert_eq!(m.selected_count(), 5);
        assert_eq!(m.hidden_selected(), 0);
        let g = m.begin_filter();
        m.finish_filter(g, Query::new("x"), vec![1, 3]);
        assert_eq!(m.hidden_selected(), 3);
        m.pick(0, Pick::Toggle); // row 1 off
        assert_eq!(m.selected_count(), 4);
        assert_eq!(m.hidden_selected(), 3);
        m.clear_selection();
        assert_eq!(m.selected_count(), 0);
        m.select_all();
        assert_eq!(m.selected_rows().collect::<Vec<_>>(), [1, 3]);
    }

    #[test]
    fn inline_threshold() {
        assert!(TableModel::new(INLINE_ROWS).runs_inline());
        assert!(!TableModel::new(INLINE_ROWS + 1).runs_inline());
    }
}
