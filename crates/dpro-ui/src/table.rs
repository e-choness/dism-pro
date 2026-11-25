//! The data table (TD-0009 A2.5, slice 5b1): [`Cx::table`] builds a grid of a header over a
//! scroll viewport of rows, building only the rows in view.
//!
//! - **Rows** have one height: the body text's line and the row padding, at least the row token,
//!   in whole physical pixels. Row `i` is at `i × row` pixels of the content, exactly, so only the
//!   rows in view, half a view either side and the current row are built, and a 100k-row table has
//!   as many nodes as a 100-row one. Layout places them from a base, a multiple of
//!   [`BASE_ROWS`] rows above the first one built, so no layout coordinate grows past what `f32`
//!   holds exactly (the viewport's offset is `i64`, and paint, hit tests and the accessibility
//!   tree move the rows by the offset less the base).
//! - **Two builds.** A build needs the row height and the rows' view, which only layout measures:
//!   until it has, the build guesses, and [`Ui::wants_build`] asks for one more. A scroll can bring
//!   other rows into view than the last build made, which asks too.
//! - **Columns** are tracks the header and every row share: fixed widths, then shares of what's
//!   left, each at least its minimum. Past the view, the rows scroll sideways, and the header, a
//!   follower viewport, scrolls with them.
//! - **The current row** is the view's: a click, the arrows, Page Up, Page Down, Home and End ask
//!   the view to move it ([`TableEvent::Current`]), and when it moves to another row, by any
//!   means, it's scrolled into view and focus follows it if focus was on the rows. A sort moves
//!   it to another index, not another row, so nothing scrolls. Its key tells which.
//! - **Checks** are the view's too: Space, Ctrl+click and a click on a row's check box toggle a
//!   row, Shift extends from the anchor, Ctrl+A and the check column's header check all. A row
//!   without a check box (a locked one, whose tooltip says why) takes Space and Ctrl+click as
//!   moves only; a Shift range passes over it, as the model never checks a locked row.

use std::{fmt, mem, rc::Rc};

use accesskit::Action;
use dpro_app::table::{ColumnId, Order, Pick, RowKey, Tri, Width};
use dpro_core::units::{Bytes, UnixMillis};

use crate::{
    build::Cx,
    composite::BadgeTone,
    geom::{Insets, Point, Rect, Size},
    input::{KeyCode, KeyEvent, Mods},
    scroll::{to_logical, to_px},
    style::{Align, Axis, Justify, Style},
    text::{TextStyle, TextSystem},
    theme::{
        Tone,
        tokens::{font, popup, table as tok},
    },
    tree::{Element, Extra, Group, Key, Look, NIL, Node, NodeId, Role, rare},
    ui::Ui,
    values::{Formats, Value, ValueCache},
};

/// The header's key under the table, for [`Ui::find`].
pub const TABLE_HEADER: Key = Key::name("dpro-ui.table.header");
/// The rows' viewport's key under the table. A row's key under it is `Key::index` of its
/// [`RowKey`].
pub const TABLE_BODY: Key = Key::name("dpro-ui.table.body");
/// The check column's key, in the header and in each row. A text column's is `Key::index` of its
/// [`ColumnId`].
pub const TABLE_CHECK: Key = Key::name("dpro-ui.table.check");
/// Group header rows' keys under the rows' viewport are this, mixed with the group's index; see
/// [`table_group`].
const GROUP_KEY: Key = Key::name("dpro-ui.table.group");

/// Group `group`'s header row's key under the rows' viewport ([`TABLE_BODY`]), for
/// [`Ui::find`] (TD-0009 A3.5 (10)).
#[must_use]
#[allow(clippy::cast_lossless, reason = "u64::from isn't const")]
pub const fn table_group(group: u32) -> Key {
    Key(GROUP_KEY.0 ^ Key::index(group as u64).0)
}

/// The empty table's status line and its action, under the rows' viewport.
const STATUS_TEXT: Key = Key::name("dpro-ui.table.status");
const STATUS_ACTION: Key = Key::name("dpro-ui.table.action");

/// Rows are laid out from a multiple of this many rows: at 200 px a row, 204,800 px, where `f32`
/// is still exact to 1/64 px.
const BASE_ROWS: u32 = 1024;
/// A line's height over its text size, for the guess a build makes before layout has measured:
/// Segoe UI's is about this.
const LINE_GUESS: f32 = 1.25;
/// Windows' default drag distance (`SM_CXDRAG`) at 100 %, until the host sets the system's.
pub(crate) const DRAG_DISTANCE: f32 = 4.0;

/// How wide a column is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Track {
    /// Logical pixels, at least the column's minimum.
    Fixed(f32),
    /// A share of what the fixed columns leave, at least the column's minimum.
    Share(f32),
}

/// A column of a [`Table`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column<'s> {
    pub id: ColumnId,
    /// Its header's text, which names its cells to assistive technology.
    pub header: &'s str,
    pub width: Track,
    /// Its least width, in logical pixels.
    pub min: f32,
    /// Where its text sits: [`Align::End`] for numbers.
    pub align: Align,
    /// Its text in the monospace face (versions, identifiers).
    pub mono: bool,
    /// The order its header sorts by first, the other one after; `None` if it doesn't sort.
    pub sort: Option<Order>,
    /// The column menu can hide it. The column that names a row usually can't.
    pub hideable: bool,
}

/// The column menu's labels, from the view's strings ([`Cx::column_menu`]).
#[derive(Clone, Copy, Debug)]
pub struct ColumnMenuLabels<'s> {
    /// The menu's name, for assistive technology ("Columns").
    pub name: &'s str,
    /// Size column to fit.
    pub fit: &'s str,
    /// Size all columns to fit.
    pub fit_all: &'s str,
    /// Reset columns.
    pub reset: &'s str,
}

/// The column menu's Size column to fit, for [`Ui::find`] in its popup. A column's check item's
/// key is `Key::index` of its [`ColumnId`].
pub const COLUMN_FIT: Key = Key::name("dpro-ui.columns.fit");
/// The column menu's Size all columns to fit.
pub const COLUMN_FIT_ALL: Key = Key::name("dpro-ui.columns.fit-all");
/// The column menu's Reset columns.
pub const COLUMN_RESET: Key = Key::name("dpro-ui.columns.reset");
const COLUMN_SEP: Key = Key::name("dpro-ui.columns.sep");

/// What an empty table shows: a line, and an action (Clear filter).
#[derive(Clone, Debug)]
pub struct Status<'s, M> {
    pub text: &'s str,
    pub action: Option<(&'s str, M)>,
}

/// How many lines a table's rows have (TD-0009 A3.5 (10)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowLines {
    /// A line: rows are the body text's line and the row padding, at least
    /// [`ROW`](tok::ROW).
    #[default]
    One,
    /// Two: a row's [two-line cell](Cells::two) shows its second line under its first, in
    /// caption secondary text (Leftovers' evidence, Activity's history). Rows are both lines, the
    /// gap between them and the row padding, at least [`ROW_TWO`](tok::ROW_TWO).
    Two,
}

/// A group of a table's rows, under a header row (TD-0009 A3.5 (10), A3.10 item 5): "Registry",
/// "3 items". The header isn't a row of the view: it can't be current, checked or selected, and
/// the keyboard passes over it. A client sees a row of the grid with one row header spanning
/// every column, named by `name`, and counts it in the grid's rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowGroup<'s> {
    /// Its first row, by index in the view's order. Groups ascend by it, and an empty group isn't
    /// given.
    pub start: u32,
    /// Its header's words, at the header's start ("Registry").
    pub label: &'s str,
    /// How many rows it has, in the user's words, at the header's end (`ui-group-count`: "3
    /// items").
    pub count: &'s str,
    /// What a client reads for the header (`ui-group-header`: "Registry, 3 items").
    pub name: &'s str,
}

/// A data table, as a view describes it for [`Cx::table`].
#[derive(Clone, Debug)]
pub struct Table<'s, M> {
    /// How many rows the view has: indices `0..rows`, in the order shown.
    pub rows: u32,
    /// How many lines each row has.
    pub lines: RowLines,
    /// Its groups, by first row ascending, each under a header row; empty for none. Rows before
    /// the first group have no header. A group out of order, or past the rows, is left out with
    /// the groups after it.
    pub groups: &'s [RowGroup<'s>],
    pub columns: &'s [Column<'s>],
    /// Each column's width as the user left it, in `columns`' order; a column past its end has
    /// its own ([`Width::Auto`]).
    pub widths: &'s [Width],
    /// Whether each column is hidden, in `columns`' order; a column past its end shows.
    pub hidden: &'s [bool],
    /// The column the rows are sorted by, and which way.
    pub sorted: Option<(ColumnId, Order)>,
    /// What a screen reader says of the sorted column's header, ascending then descending
    /// ("Sorted ascending", "Sorted descending"): its item status, in the user's language. Empty
    /// for none.
    pub sort_status: [&'s str; 2],
    /// A check column: its header's name ("Select") and the rows' checks together.
    pub checks: Option<(&'s str, Tri)>,
    /// The current row's index.
    pub current: Option<u32>,
    /// What it shows with no rows.
    pub status: Option<Status<'s, M>>,
    /// A context request on a row makes it current and asks for the model's menu.
    pub context: bool,
    /// The view's message for each of its events.
    pub on: fn(TableEvent) -> M,
}

/// A row, as the view gives it: its key, and its check (`None`: it has no check box, and
/// `locked` says why).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row<'s> {
    pub key: RowKey,
    pub check: Option<Tri>,
    /// Its tooltip, and its check cell's: why it can't be checked. Empty for none.
    pub locked: &'s str,
}

/// A row's cells, as the view gives them to [`Cx::table`]: each column's text or value, in
/// order.
pub struct Cells<'b> {
    text: &'b mut String,
    ends: &'b mut Vec<usize>,
    /// The cells that aren't one line of text: two-line cells and badges (TD-0009 A3.5 (10)).
    marks: &'b mut Vec<Mark>,
    values: &'b mut ValueCache,
    formats: &'b dyn Formats,
    /// The one column a search reads, or [`ALL`] for a build. A search goes through every row, so
    /// it formats only that column's values, and keeps none: the others are left empty.
    keep: usize,
    /// The table's rows have two lines, so a two-line cell keeps its second.
    two: bool,
    /// Where this row's cells start in `ends`: after the current row's, which a build pushes
    /// first and keeps.
    first: usize,
}

/// [`Cells::keep`] for a build: every value is formatted, or kept from an earlier build.
const ALL: usize = usize::MAX;

/// A cell that isn't one line of text, by its place in its row's cells (TD-0009 A3.5 (10)).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mark {
    cell: u32,
    kind: MarkKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MarkKind {
    /// Two lines: the first is this many bytes of the cell's text, the second the rest.
    Two(u32),
    /// A badge of the cell's text, in this tone.
    Badge(BadgeTone),
}

/// The longest first line a two-line cell keeps a second line after: its length is in its node's
/// value ([`lines_value`]), which an `f32` holds exactly below 2^24.
const MAX_SPLIT: usize = (1 << 24) - 2;

/// A two-line cell's node value: one more than its first line's length, so a one-line cell's is
/// 0. A cell has no other value.
#[allow(
    clippy::cast_precision_loss,
    reason = "below 2^24 (MAX_SPLIT), exact in f32"
)]
pub(crate) const fn lines_value(split: u32) -> f32 {
    (split + 1) as f32
}

impl<'b> Cells<'b> {
    fn new(
        text: &'b mut String,
        ends: &'b mut Vec<usize>,
        marks: &'b mut Vec<Mark>,
        values: &'b mut ValueCache,
        formats: &'b dyn Formats,
        keep: usize,
        two: bool,
    ) -> Self {
        let first = ends.len();
        Self {
            text,
            ends,
            marks,
            values,
            formats,
            keep,
            two,
            first,
        }
    }

    /// The next cell's place in the row, its column as [`CellText::mark`] counts them.
    fn next_cell(&self) -> u32 {
        u32::try_from(self.ends.len() - self.first).unwrap_or(u32::MAX)
    }

    /// The next column's text. It's copied, so it can come from anywhere.
    pub fn push(&mut self, text: &str) {
        self.text.push_str(text);
        self.ends.push(self.text.len());
    }

    /// The next column's size, in the window's [`Formats`].
    #[inline]
    pub fn size(&mut self, bytes: Bytes) {
        self.value(Value::Size(bytes.0));
    }

    /// The next column's calendar day (its midnight UTC), in the window's [`Formats`].
    #[inline]
    pub fn date(&mut self, day: UnixMillis) {
        self.value(Value::Date(day.0));
    }

    /// The next column's moment, in local time, in the window's [`Formats`].
    #[inline]
    pub fn time(&mut self, t: UnixMillis) {
        self.value(Value::Time(t.0));
    }

    /// The next column's text on two lines (TD-0009 A3.5 (10)): `primary` in its column's face
    /// (mono for paths), and `secondary` under it in caption secondary text, in a table whose
    /// rows have [two lines](RowLines::Two). A row's first text cell, usually. The cell is named
    /// by its first line, which names its row too, and described by its second. In a table of
    /// one-line rows, the second line is left out.
    pub fn two(&mut self, primary: &str, secondary: &str) {
        // A search reads first lines only, and a one-line row shows only them.
        if self.two && self.keep == ALL && !secondary.is_empty() && primary.len() <= MAX_SPLIT {
            let cell = self.next_cell();
            let split = u32::try_from(primary.len()).unwrap_or(u32::MAX);
            self.marks.push(Mark {
                cell,
                kind: MarkKind::Two(split),
            });
            self.text.push_str(primary);
            self.text.push_str(secondary);
            self.ends.push(self.text.len());
        } else {
            self.push(primary);
        }
    }

    /// The next column's badge (TD-0009 A3.5 (1), (10)): `text` in `tone`'s colors, where its
    /// column puts it (Activity's history shows each job's state so). The cell is named by the
    /// word.
    pub fn badge(&mut self, text: &str, tone: BadgeTone) {
        if self.keep == ALL {
            let cell = self.next_cell();
            self.marks.push(Mark {
                cell,
                kind: MarkKind::Badge(tone),
            });
        }
        self.push(text);
    }

    /// A value's text, kept from an earlier build or formatted now.
    ///
    /// Always inlined into the view's row closure, as plain `#[inline]` no longer was once the
    /// search's branch came in: called, it cost a frame of the shell's rows ~870 Ir (+0.8 %).
    #[expect(
        clippy::inline_always,
        reason = "measured: called, event.unchanged cost 871 Ir (0.8 %) more (C1)"
    )]
    #[inline(always)]
    fn value(&mut self, v: Value) {
        if self.keep == ALL {
            self.values.push(v, self.formats, self.text);
        } else {
            self.searched(v);
        }
        self.ends.push(self.text.len());
    }

    /// [`value`](Self::value) in a search: formatted if it's the column the search reads, and
    /// not kept. A search starts each row's `ends` empty, so their length is this column. Out of
    /// line, so a build's values don't carry it.
    #[cold]
    #[inline(never)]
    fn searched(&mut self, v: Value) {
        if self.ends.len() == self.keep {
            v.format(self.formats, self.text);
        }
    }
}

impl fmt::Debug for Cells<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cells")
            .field("text", &self.text)
            .field("ends", &self.ends)
            .field("marks", &self.marks)
            .finish_non_exhaustive()
    }
}

/// What the user did to a table, by row index, and to its columns. Every table's header takes
/// context requests, for [`ColumnMenu`](Self::ColumnMenu).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TableEvent {
    /// Make this row current: a click, a move, a context request, a client's `Focus` or `Click`.
    Current(u32),
    /// Check this row: toggle it, or extend the checks from the anchor to it (and make it current).
    Check(u32, Pick),
    /// Check every row, or none if all are: Ctrl+A, or the check column's header.
    CheckAll,
    /// Open it: Enter, or a double-click.
    Activate(u32),
    /// Sort by this column: its header was clicked.
    Sort(ColumnId),
    /// Give this column this many logical pixels: its divider was let go after a drag (the
    /// shared columns left of it, at the widths they held, first), or the table measured the fit
    /// the view asked for.
    Resize(ColumnId, f32),
    /// Fit this column, or every shown one: a double-click on its divider, or the column menu.
    Fit(Option<ColumnId>),
    /// Open the column menu, for this column (`None`: the check column, or past the last one): a
    /// context request on the header.
    ColumnMenu(Option<ColumnId>),
    /// Show (`true`) or hide this column: the column menu.
    Show(ColumnId, bool),
    /// Every column's own width, and the columns the page shows at first: the column menu.
    ResetColumns,
}

/// The last press, for a row's click: where, with which modifiers, and whether it was a
/// double-click's second.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Press {
    pub(crate) at: Point,
    pub(crate) mods: Mods,
    pub(crate) double: bool,
}

/// A shown column as layout needs it, with the width the user left it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Col {
    id: ColumnId,
    width: Track,
    min: f32,
    /// The view asks for a fit ([`Width::Fit`]): until it's measured, the column has its own
    /// track.
    fit: bool,
    /// The fit is measured: `width` is it, and the view has been sent it.
    fitted: bool,
    /// Its header sorts: a fit leaves room for the chevron a sorted one shows.
    sorts: bool,
}

/// A column's divider being dragged: table slot `t`'s shown column `col`, pressed at `press_x`
/// when it was `start_w` wide.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Divider {
    t: u32,
    col: usize,
    press_x: f32,
    start_w: f32,
    min: f32,
    /// A double-click's second press: its release asks to fit the column, unless it's resized.
    double: bool,
    /// The pointer has gone the drag distance from the press: a drag, not a click.
    moved: bool,
}

/// A table's state, in the Ui's table table, reached from its rows' viewport's scroll slot.
#[derive(Debug)]
pub(crate) struct TableState<M> {
    pub(crate) grid: u32,
    pub(crate) header: u32,
    pub(crate) body: u32,
    pub(crate) on: Option<fn(TableEvent) -> M>,
    pub(crate) rows: u32,
    /// The shown columns, in order.
    pub(crate) columns: Vec<Col>,
    /// The next build's columns, made here and swapped in if they differ.
    cols_next: Vec<Col>,
    /// A divider's drag: the shown column and its width, which layout gives it while the
    /// columns left of it hold the widths in `frozen` (from the press).
    drag: Option<(usize, f32)>,
    frozen: Vec<f32>,
    pub(crate) checks: bool,
    /// The check column's name, which names every check cell.
    pub(crate) check_label: String,
    /// The sorted column header's status for each order, ascending then descending.
    pub(crate) sort_status: [String; 2],
    /// Each column's start and width (logical), the check column first: from layout.
    tracks: Vec<(f32, f32)>,
    /// The columns' width (logical): their tracks' sum, from layout.
    pub(crate) content_w: f32,
    /// A row's height in whole physical pixels, from layout; 0 until it has measured.
    pub(crate) row_px: i64,
    /// A row's height in logical pixels, before rounding: for a new scale.
    row_h: f32,
    /// The row height the last build used: measured, or guessed.
    built_px: i64,
    /// The rows' view's height in physical pixels, from layout; 0 until it has laid it out.
    view_px: i64,
    /// The row layout places at the viewport's origin.
    base_row: u32,
    /// The rows built, as (node, index), in order.
    built: Vec<(u32, u32)>,
    scratch: Vec<(u32, u32)>,
    /// What the last build made.
    span: Span,
    current: Option<u32>,
    current_key: Option<Key>,
    /// A row or group header to scroll into view at the next build, once the view is known.
    reveal: Option<Item>,
    /// The text of the values its cells showed.
    values: ValueCache,
    /// A client's search by name, which the next build runs (TD-0009 A2.5).
    find: Option<Find>,
    /// The row the last search found, for [`Ui::take_found`].
    found: Option<u32>,
    // Grouped and two-line rows (TD-0009 A3.5 (10), slice 6b).
    /// Its rows have two lines.
    two: bool,
    /// Its groups' first rows, ascending: the view's, up to the first out of order or past the
    /// rows.
    starts: Vec<u32>,
    /// A group header's height in whole physical pixels, from layout; 0 until it has measured.
    group_px: i64,
    /// A group header's height in logical pixels, before rounding: for a new scale.
    group_h: f32,
    /// The header height the last build used: measured, or guessed.
    built_gp: i64,
    /// The group headers built, as (node, group), in order.
    heads: Vec<(u32, u32)>,
    heads_scratch: Vec<(u32, u32)>,
    /// The cells of the rows the last build made that aren't one line of text.
    marks: Vec<Mark>,
}

/// A search for a table's first row from `from` on whose name (its first shown column's text) is
/// `name`, in the view's order.
#[derive(Debug)]
struct Find {
    from: u32,
    name: String,
}

impl<M> TableState<M> {
    fn new() -> Self {
        Self {
            grid: NIL,
            header: NIL,
            body: NIL,
            on: None,
            rows: 0,
            columns: Vec::new(),
            cols_next: Vec::new(),
            drag: None,
            frozen: Vec::new(),
            checks: false,
            check_label: String::new(),
            sort_status: [String::new(), String::new()],
            tracks: Vec::new(),
            content_w: 0.0,
            row_px: 0,
            row_h: 0.0,
            built_px: 0,
            view_px: 0,
            base_row: 0,
            built: Vec::new(),
            scratch: Vec::new(),
            span: Span::default(),
            current: None,
            current_key: None,
            reveal: None,
            values: ValueCache::default(),
            find: None,
            found: None,
            two: false,
            starts: Vec::new(),
            group_px: 0,
            group_h: 0.0,
            built_gp: 0,
            heads: Vec::new(),
            heads_scratch: Vec::new(),
            marks: Vec::new(),
        }
    }

    /// Where its rows and headers are, at the heights the last build used until layout has
    /// measured them.
    fn geom(&self) -> Geom<'_> {
        Geom {
            rows: self.rows,
            rp: if self.row_px > 0 {
                self.row_px
            } else {
                self.built_px
            },
            gp: if self.group_px > 0 {
                self.group_px
            } else {
                self.built_gp
            },
            starts: &self.starts,
        }
    }

    /// Takes `table`'s lines and groups (A3.5 (10)): the groups' first rows as far as they're in
    /// order and within the rows, compared in place, as they're the same at almost every build.
    /// Returns whether the row height and whether the groups changed.
    fn take_shape(&mut self, table: &Table<'_, M>) -> (bool, bool) {
        let two = table.lines == RowLines::Two;
        let reshaped = self.two != two;
        self.two = two;
        let valid = valid_groups(table.groups, table.rows);
        let regrouped = self.starts.len() != valid
            || !(self.starts.iter().zip(table.groups)).all(|(&s, g)| s == g.start);
        if regrouped {
            self.starts.clear();
            self.starts
                .extend(table.groups[..valid].iter().map(|g| g.start));
        }
        (reshaped, regrouped)
    }

    /// Whether the last build used the heights layout measured. A table without groups doesn't
    /// wait on its headers' height.
    fn built_measured(&self) -> bool {
        self.built_px == self.row_px && (self.starts.is_empty() || self.built_gp == self.group_px)
    }

    /// Where a view at offset `off` scrolls to show `item`: no further than it must. A group's
    /// first row brings its header with it, if both fit. `None` if there's no such item.
    fn reveal_to(&self, item: Item, off: i64) -> Option<i64> {
        let geom = self.geom();
        let view = self.view_px;
        match item {
            Item::Row(r) if r < self.rows => {}
            Item::Head(g) if (g as usize) < self.starts.len() => {}
            _ => return None,
        }
        let (mut top, bottom) = geom.extent(item);
        if let Item::Row(r) = item
            && self.starts.binary_search(&r).is_ok()
            && bottom - (top - geom.gp) <= view
        {
            top -= geom.gp;
        }
        Some(if top < off || bottom - top > view {
            top
        } else if bottom > off + view {
            bottom - view
        } else {
            off
        })
    }
}

/// How many of `groups` a table of `rows` rows keeps: those in order, each past the last and
/// within the rows, up to the first that isn't.
fn valid_groups(groups: &[RowGroup<'_>], rows: u32) -> usize {
    let mut next = 0;
    groups
        .iter()
        .take_while(|g| {
            let ok = g.start >= next && g.start < rows;
            next = g.start.saturating_add(1);
            ok
        })
        .count()
}

/// A two-line cell's text as its lines, the first `split` bytes long (both in the first if
/// that isn't a character's edge).
pub(crate) fn lines_of(text: &str, split: usize) -> (&str, &str) {
    text.split_at_checked(split).unwrap_or((text, ""))
}

/// A table's heights from its text: a row's and a group header's, logical and in whole physical
/// pixels, and the column header's in pixels.
#[derive(Clone, Copy, Debug)]
struct Heights {
    row_h: f32,
    rp: i64,
    group_h: f32,
    gp: i64,
    head: i64,
}

/// What a table's rows' view holds: a row, or a group's header (TD-0009 A3.5 (10)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Row(u32),
    Head(u32),
}

/// What a build makes: rows `rows.0..rows.1` and group headers `heads.0..heads.1`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Span {
    rows: (u32, u32),
    heads: (u32, u32),
}

/// Where a table's rows and group headers are down its rows' view, in whole physical pixels
/// (TD-0009 A3.5 (10)). Rows have one height and headers another, so row `i` is at `i × rp`
/// and a header for each group that starts at or before it; finding what's at a height is a
/// binary search over the groups. Without groups it's A2's arithmetic.
#[derive(Clone, Copy, Debug)]
struct Geom<'a> {
    rows: u32,
    rp: i64,
    gp: i64,
    starts: &'a [u32],
}

impl Geom<'_> {
    fn groups(&self) -> u32 {
        u32::try_from(self.starts.len()).unwrap_or(u32::MAX)
    }

    /// How many groups start at or before row `r`: its header, and those above it.
    fn through(&self, r: u32) -> u32 {
        if self.starts.is_empty() {
            return 0;
        }
        u32::try_from(self.starts.partition_point(|&s| s <= r)).unwrap_or(u32::MAX)
    }

    fn row_top(&self, r: u32) -> i64 {
        i64::from(r) * self.rp + i64::from(self.through(r)) * self.gp
    }

    fn head_top(&self, g: u32) -> i64 {
        i64::from(self.starts[g as usize]) * self.rp + i64::from(g) * self.gp
    }

    /// The top of what's first at row `r`: its group's header if it starts one, else the row.
    fn item_top(&self, r: u32) -> i64 {
        let before = if self.starts.is_empty() {
            0
        } else {
            self.starts.partition_point(|&s| s < r)
        };
        i64::from(r) * self.rp + i64::try_from(before).unwrap_or(i64::MAX) * self.gp
    }

    /// Every row and header.
    fn height(&self) -> i64 {
        i64::from(self.rows)
            .saturating_mul(self.rp)
            .saturating_add(i64::from(self.groups()) * self.gp)
    }

    /// How many headers start at or above `y`.
    fn heads_to(&self, y: i64) -> u32 {
        let (mut lo, mut hi) = (0, self.groups());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.head_top(mid) <= y {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// What's at `y`, which is in `0..height()`; the table has rows.
    fn at(&self, y: i64) -> Item {
        let last = self.rows.saturating_sub(1);
        let row = |px: i64| u32::try_from(px / self.rp).unwrap_or(u32::MAX);
        if self.starts.is_empty() {
            return Item::Row(row(y).min(last));
        }
        match self.heads_to(y).checked_sub(1) {
            // Above the first header: rows without a group.
            None => Item::Row(row(y).min(last)),
            Some(g) => {
                let below = y - self.head_top(g) - self.gp;
                if below < 0 {
                    Item::Head(g)
                } else {
                    Item::Row(self.starts[g as usize].saturating_add(row(below)).min(last))
                }
            }
        }
    }

    /// The rows and headers at least partly between `lo` and `hi`.
    fn span(&self, lo: i64, hi: i64) -> Span {
        let end = self.height();
        let lo = lo.max(0);
        if self.rows == 0 || self.rp <= 0 || hi <= lo {
            return Span::default();
        }
        let (rows, groups) = (self.rows, self.groups());
        if lo >= end {
            return Span {
                rows: (rows, rows),
                heads: (groups, groups),
            };
        }
        let first = match self.at(lo) {
            Item::Row(r) => r,
            Item::Head(g) => self.starts[g as usize],
        };
        let to = match self.at(hi.min(end) - 1) {
            Item::Row(r) => r + 1,
            Item::Head(g) => self.starts[g as usize],
        };
        let (g0, g1) = if self.starts.is_empty() {
            (0, 0)
        } else {
            (self.heads_to(lo - self.gp), self.heads_to(hi - 1))
        };
        Span {
            rows: (first.min(to), to),
            heads: (g0.min(g1), g1),
        }
    }

    /// What a build makes for a view of `view` at offset `off`: what's in view, and half a view
    /// either side.
    fn span_for(&self, off: i64, view: i64) -> Span {
        if view <= 0 {
            return Span::default();
        }
        // Without groups, A2's arithmetic: every build of most tables asks.
        if self.starts.is_empty() {
            if self.rows == 0 || self.rp <= 0 {
                return Span::default();
            }
            let (rows, rp) = (self.rows, self.rp);
            let start = off.saturating_sub(view / 2).max(0);
            let end = off.saturating_add(view).saturating_add(view / 2);
            let row = |px: i64| u32::try_from(px).unwrap_or(u32::MAX).min(rows);
            let to = row(end.saturating_add(rp - 1) / rp);
            return Span {
                rows: (row(start / rp).min(to), to),
                heads: (0, 0),
            };
        }
        self.span(
            off.saturating_sub(view / 2),
            off.saturating_add(view).saturating_add(view / 2),
        )
    }

    /// The last row whose top is at or above `y`.
    fn row_by(&self, y: i64) -> u32 {
        if y < 0 {
            return 0;
        }
        match self.at(y.min(self.height() - 1)) {
            Item::Row(r) => r,
            Item::Head(g) => self.starts[g as usize].saturating_sub(1),
        }
    }

    /// The first row whose top is at or below `y`.
    fn row_from(&self, y: i64) -> u32 {
        if y <= 0 {
            return 0;
        }
        match self.at(y.min(self.height() - 1)) {
            Item::Row(r) if self.row_top(r) >= y => r,
            Item::Row(r) => (r + 1).min(self.rows.saturating_sub(1)),
            Item::Head(g) => self.starts[g as usize],
        }
    }

    /// Offset `off` in this geometry, in `new`'s (the same rows at other heights): the row or
    /// header at it keeps its place at the top, as far into it as it was.
    fn rescaled(&self, new: &Geom<'_>, off: i64) -> i64 {
        if self.rows == 0 || self.rp <= 0 || off <= 0 {
            return off.max(0);
        }
        let item = self.at(off.min(self.height() - 1));
        let (top, bottom) = self.extent(item);
        let (new_top, new_bottom) = new.extent(item);
        let (into, old_h, new_h) = (off - top, bottom - top, new_bottom - new_top);
        if old_h <= 0 {
            return new_top;
        }
        new_top + into / old_h * new_h + into % old_h * new_h / old_h
    }

    /// Where `item` is, top and bottom.
    fn extent(&self, item: Item) -> (i64, i64) {
        match item {
            Item::Row(r) => {
                let top = self.row_top(r);
                (top, top + self.rp)
            }
            Item::Head(g) => {
                let top = self.head_top(g);
                (top, top + self.gp)
            }
        }
    }
}

/// Row `r`'s index as a client counts the grid's rows: the group headers above it counted
/// (A3.10 item 5).
fn grid_of(starts: &[u32], r: u32) -> u32 {
    if starts.is_empty() {
        return r;
    }
    let through = u32::try_from(starts.partition_point(|&s| s <= r)).unwrap_or(u32::MAX);
    r.saturating_add(through)
}

/// Group `g`'s header's index as a client counts the grid's rows.
fn head_grid_of(starts: &[u32], g: u32) -> u32 {
    starts[g as usize].saturating_add(g)
}

/// What's at a client's index `k` in a table of `rows` rows: a row, or a group's header. `None`
/// past the end.
fn item_of_grid(starts: &[u32], rows: u32, k: u32) -> Option<Item> {
    let total = rows.saturating_add(u32::try_from(starts.len()).unwrap_or(u32::MAX));
    if k >= total {
        return None;
    }
    if starts.is_empty() {
        return Some(Item::Row(k));
    }
    // The headers at or before `k`: header g is at starts[g] + g.
    let (mut lo, mut hi) = (0usize, starts.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if head_grid_of(starts, u32::try_from(mid).unwrap_or(u32::MAX)) <= k {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let heads = u32::try_from(lo).unwrap_or(u32::MAX);
    if let Some(g) = heads.checked_sub(1)
        && head_grid_of(starts, g) == k
    {
        return Some(Item::Head(g));
    }
    Some(Item::Row(k - heads))
}

/// What a build makes: the rows `from..to` and the group headers `heads`, and `extra`, the
/// current row, out of that range.
#[derive(Clone, Copy, Debug)]
struct Plan {
    from: u32,
    to: u32,
    heads: (u32, u32),
    extra: Option<u32>,
    /// Focus goes to the current row once it's built.
    follow: bool,
    /// Focus was on one of the rows: if the build drops that row, focus stays in the table.
    focused: bool,
    /// Focus was on the rows' viewport, which takes it only while it has no rows: it moves onto
    /// them once they're built (TD-0009 A4.4 × A3.5 (12), wave 3).
    empty: bool,
}

/// A column header's sort as its node's value: 0 none, 1 ascending, 2 descending.
pub(crate) const fn sort_value(order: Option<Order>) -> f32 {
    match order {
        None => 0.0,
        Some(Order::Ascending) => 1.0,
        Some(Order::Descending) => 2.0,
    }
}

/// The sort a column header's value says.
pub(crate) fn sort_of(value: f32) -> Option<Order> {
    [Order::Ascending, Order::Descending]
        .into_iter()
        .find(|&o| sort_value(Some(o)).to_bits() == value.to_bits())
}

const fn justify_of(align: Align) -> Justify {
    match align {
        Align::Start | Align::Stretch => Justify::Start,
        Align::Center => Justify::Center,
        Align::End => Justify::End,
    }
}

/// A cell's and a column header's style: its text from its sides, where its column puts it.
fn cell_style(align: Align) -> Style {
    Style {
        justify: justify_of(align),
        padding: Insets::xy(tok::CELL_PAD_X, 0.0),
        ..Style::default()
    }
}

fn header_text() -> TextStyle {
    TextStyle {
        size: tok::HEADER_TEXT,
        ..TextStyle::default()
    }
}

/// A column header's look: secondary text.
const HEADER: Look = Look {
    ink: Tone::TextSecondary,
    ..Look::PLAIN
};

/// The current row's look: the selection fill.
const CURRENT: Look = Look {
    fill: Some(Tone::Selected),
    ..Look::PLAIN
};

/// A group header's parts under its row (TD-0009 A3.5 (10)): its one cell, and the label and
/// count in it.
const GROUP_CELL: Key = Key::name("dpro-ui.table.group.cell");
const GROUP_LABEL: Key = Key::name("dpro-ui.table.group.label");
const GROUP_COUNT: Key = Key::name("dpro-ui.table.group.count");
/// A badge cell's badge.
const BADGE: Key = Key::name("dpro-ui.table.badge");

/// A two-line cell's second line, under a first line in `first`: caption text, regular, in the
/// same proportion to the body text at any text size (A3.5 (10)).
pub(crate) fn second_line(first: TextStyle) -> TextStyle {
    TextStyle {
        size: first.size * font::CAPTION / font::BODY,
        weight: font::REGULAR,
        mono: false,
        wrap: false,
    }
}

impl<M: Clone> Cx<'_, M> {
    /// A data table (TD-0009 A2.5): `el` (an [`Element::table`]) holding a header of `table`'s
    /// columns over a scroll viewport of its rows. `row(index, cells)` gives the row at an index
    /// and pushes its cells' text, a column at a time; it's called only for the rows built, the
    /// current one first. Returns the table's node, for [`Ui::realize`].
    pub fn table<'r>(
        &mut self,
        el: Element<'_, M>,
        table: &Table<'_, M>,
        mut row: impl FnMut(u32, &mut Cells<'_>) -> Row<'r>,
    ) -> NodeId {
        let grid = self.place(el);
        let mut cx = self.child(grid);
        let header = cx.table_header(table);
        let body = cx.place(body_element(table.rows));
        let t = cx.ui.table_slot(grid, header, body);
        // The Ui's cell buffers, the table's kept text and the window's formats for what it
        // hasn't kept.
        let mut bufs = CellBufs::take(cx.ui, t);
        let formats = Rc::clone(&cx.ui.formats.0);
        let shown = Shown::of(table);
        let two = table.lines == RowLines::Two;
        // A client's search, through the view's rows, reveals the row it finds (A2.5).
        if let Some(find) = (t != NIL)
            .then(|| cx.ui.tables[t as usize].find.take())
            .flatten()
        {
            let mut cells = bufs.cells(&*formats, shown.first, two);
            let found = search(&find, table.rows, shown.first, &mut cells, &mut row);
            bufs.clear();
            let st = &mut cx.ui.tables[t as usize];
            st.found = found;
            st.reveal = found.map(Item::Row).or(st.reveal);
        }
        // The current row first: whether its key changed says whether it's another row.
        let current = table.current.filter(|&c| c < table.rows);
        let cur = current.map(|c| row(c, &mut bufs.cells(&*formats, ALL, two)));
        let kept = bufs.lens();
        let plan = (t != NIL).then(|| {
            cx.ui
                .table_plan(t, table, current, cur.map(|r| Key::index(r.key.0)))
        });
        {
            let mut bx = cx.child(body);
            if let Some(plan) = plan
                && table.rows > 0
            {
                let before = plan.extra.filter(|&c| c < plan.from);
                let after = plan.extra.filter(|&c| c >= plan.to);
                // The group headers in range, each built before its first row (A3.5 (10)).
                let mut heads = plan.heads;
                for at in before.into_iter().chain(plan.from..plan.to).chain(after) {
                    if heads.0 < heads.1 {
                        bx.group_headers(t, table, &mut heads, Some(at));
                    }
                    // Each row's cells end where its own pushes did: the current row's before
                    // `kept`, so a row built after it can't lend it cells it didn't give.
                    let (r, cells) = if let Some(r) = cur
                        && current == Some(at)
                    {
                        (r, bufs.text_of((0, 0, 0), kept))
                    } else {
                        bufs.truncate(kept);
                        let r = row(at, &mut bufs.cells(&*formats, ALL, two));
                        (r, bufs.text_of(kept, bufs.lens()))
                    };
                    let node = bx.table_row(table, shown, at, current == Some(at), r, &cells);
                    bx.ui.tables[t as usize].scratch.push((node, at));
                }
                bx.group_headers(t, table, &mut heads, None);
            } else if let Some(status) = &table.status {
                bx.status(status);
            }
            bx.finish();
        }
        bufs.give(cx.ui, t);
        if let Some(plan) = plan {
            cx.ui.table_built(t, plan);
        }
        cx.finish();
        self.id(grid)
    }

    /// The header: a follower viewport, one Tab stop whose arrows move between its column
    /// headers, and where a context request asks for the column menu. Returns its node.
    fn table_header(&mut self, table: &Table<'_, M>) -> u32 {
        let header = self.place(Element {
            rare: rare::SCROLL_X | rare::CONTEXT,
            extra: Extra::Group(Group::List),
            style: Style {
                axis: Axis::Row,
                ..Style::default()
            },
            ..Element::new(TABLE_HEADER, Role::Container, "", None)
        });
        let focus = self.ui.focus;
        let focused = (focus != NIL && self.ui.nodes[focus as usize].parent == header)
            .then(|| self.ui.sibling_index(focus));
        let mut hx = self.child(header);
        hx.column_headers(table);
        hx.finish();
        // The column header focus was on went (its column hidden): focus moves to the one now in
        // its place, or the one before if it was the last.
        if let Some(k) = focused
            && self.ui.focus == NIL
        {
            // None if every column went and there's no check column.
            let mut to = self.ui.nodes[header as usize].first;
            for _ in 0..k {
                if to == NIL || self.ui.nodes[to as usize].next == NIL {
                    break;
                }
                to = self.ui.nodes[to as usize].next;
            }
            if to != NIL {
                self.ui.focus_node(to);
            }
        }
        header
    }

    /// `table`'s column menu (TD-0009 A2.4), opened for column `at` (`None`: from the check
    /// column, or past the last column), as the content of a menu popup: each hideable column as
    /// a check item, checked while it shows, which shows or hides it (the last one shown can't be
    /// hidden); then Size column to fit (for `at`), Size all columns to fit, and Reset columns.
    /// Each sends `table`'s event.
    pub fn column_menu(
        &mut self,
        key: Key,
        table: &Table<'_, M>,
        at: Option<ColumnId>,
        labels: &ColumnMenuLabels<'_>,
    ) {
        let on = table.on;
        self.add_with(Element::menu(key).name(labels.name), |cx| {
            let count = shown(table).count();
            let mut any = false;
            for (j, col) in table.columns.iter().enumerate() {
                if !col.hideable {
                    continue;
                }
                any = true;
                let showing = !table.hidden.get(j).copied().unwrap_or(false);
                cx.add(
                    Element::menu_check(
                        Key::index(u64::from(col.id.0)),
                        col.header,
                        showing,
                        on(TableEvent::Show(col.id, !showing)),
                    )
                    .enabled(!showing || count > 1),
                );
            }
            if any {
                cx.add(Element::separator(COLUMN_SEP));
            }
            cx.add(
                Element::menu_item(COLUMN_FIT, labels.fit, on(TableEvent::Fit(at)))
                    .enabled(at.is_some()),
            );
            cx.add(Element::menu_item(
                COLUMN_FIT_ALL,
                labels.fit_all,
                on(TableEvent::Fit(None)),
            ));
            cx.add(Element::menu_item(
                COLUMN_RESET,
                labels.reset,
                on(TableEvent::ResetColumns),
            ));
        });
    }

    /// The empty table's status line, and its action as a secondary button.
    fn status(&mut self, status: &Status<'_, M>) {
        self.add(Element::text(STATUS_TEXT, status.text));
        if let Some((label, msg)) = &status.action {
            self.add(
                Element::button(STATUS_ACTION, label, msg.clone())
                    .look(Look::SECONDARY)
                    .style(Style {
                        min: Size {
                            w: 0.0,
                            h: popup::ITEM_HEIGHT,
                        },
                        padding: Insets::xy(popup::ITEM_PAD, 0.0),
                        ..Style::default()
                    }),
            );
        }
    }

    /// The header's column headers: the check column's, which checks all, then each column's,
    /// which sorts by it if it sorts.
    fn column_headers(&mut self, table: &Table<'_, M>) {
        if let Some((label, all)) = table.checks {
            // In body text, as a row's check box is: its label is only its name.
            self.add(Element {
                check: all,
                look: HEADER,
                ..Element::new(
                    TABLE_CHECK,
                    Role::ColumnHeader,
                    label,
                    Some((table.on)(TableEvent::CheckAll)),
                )
            });
        }
        for (_, col) in shown(table) {
            let sorted = table
                .sorted
                .and_then(|(c, order)| (c == col.id).then_some(order));
            self.add(Element {
                style: cell_style(col.align),
                text_style: header_text(),
                look: HEADER,
                extra: Extra::Sort(sorted),
                ..Element::new(
                    Key::index(u64::from(col.id.0)),
                    Role::ColumnHeader,
                    col.header,
                    col.sort.map(|_| (table.on)(TableEvent::Sort(col.id))),
                )
            });
        }
    }

    /// Row `at` and its cells. Returns its node.
    fn table_row(
        &mut self,
        table: &Table<'_, M>,
        shown: Shown,
        at: u32,
        current: bool,
        r: Row<'_>,
        cells: &CellText<'_>,
    ) -> u32 {
        let mut el = Element {
            selected: Some(current),
            look: if current { CURRENT } else { Look::PLAIN },
            // Named by its first column shown.
            ..Element::new(
                Key::index(r.key.0),
                Role::Row,
                cells.get(shown.first),
                Some((table.on)(TableEvent::Current(at))),
            )
        }
        .tooltip(r.locked);
        if table.context {
            el = el.context();
        }
        let i = self.place(el);
        let mut rx = self.child(i);
        if table.checks.is_some() {
            rx.add(
                Element {
                    check: r.check.unwrap_or(Tri::Off),
                    enabled: r.check.is_some(),
                    ..Element::new(TABLE_CHECK, Role::Cell, "", None)
                }
                .tooltip(r.locked),
            );
        }
        // A hidden column's cell isn't built: the view gave its text, which is skipped. Every
        // cell of every row built passes here, so it tests only when some column is hidden, and
        // only the first cell built can be semibold.
        let mut weight = if current {
            font::SEMIBOLD
        } else {
            font::REGULAR
        };
        // Most rows have only one-line cells: they test nothing more (A3.5 (10)).
        let plain = cells.marks.is_empty();
        for (j, col) in table.columns.iter().enumerate() {
            if shown.any_hidden && is_hidden(table, j) {
                continue;
            }
            let key = Key::index(u64::from(col.id.0));
            let text_style = TextStyle {
                weight,
                mono: col.mono,
                ..TextStyle::default()
            };
            match if plain { None } else { cells.mark(j) } {
                None => {
                    rx.add(Element {
                        style: cell_style(col.align),
                        text_style,
                        ..Element::new(key, Role::Cell, cells.all(j), None)
                    });
                }
                Some(mark) => rx.marked_cell(key, col.align, text_style, cells, j, mark),
            }
            weight = font::REGULAR;
        }
        rx.finish();
        i
    }

    /// A cell that isn't one line of text (A3.5 (10)): a two-line cell, whose text is both lines
    /// and whose value says where the first ends, or a badge cell, named by its word, holding the
    /// badge that shows it.
    #[inline(never)]
    fn marked_cell(
        &mut self,
        key: Key,
        align: Align,
        text_style: TextStyle,
        cells: &CellText<'_>,
        j: usize,
        mark: MarkKind,
    ) {
        let style = cell_style(align);
        match mark {
            MarkKind::Two(split) => {
                self.add(Element {
                    style,
                    text_style,
                    extra: Extra::Lines(split),
                    ..Element::new(key, Role::Cell, cells.all(j), None)
                });
            }
            MarkKind::Badge(tone) => {
                let word = cells.all(j);
                self.add_with(
                    Element {
                        style: Style {
                            axis: Axis::Row,
                            align: Align::Center,
                            ..style
                        },
                        text_style,
                        ..Element::new(key, Role::Cell, word, None)
                    },
                    |cx| {
                        cx.add(Element::badge(BADGE, word, tone));
                    },
                );
            }
        }
    }

    /// Group `g`'s header row (TD-0009 A3.5 (10)): one cell across every column, named by the
    /// group's words, holding its label at its start and its count at its end, in caption
    /// secondary text. It isn't a row of the view, so nothing in it takes input. Returns its
    /// node.
    fn group_header(&mut self, table: &Table<'_, M>, g: u32) -> u32 {
        let group = table.groups[g as usize];
        let i = self.place(Element {
            style: Style {
                axis: Axis::Row,
                ..Style::default()
            },
            ..Element::new(table_group(g), Role::Container, "", None)
        });
        let mut hx = self.child(i);
        hx.add_with(
            Element {
                style: Style {
                    axis: Axis::Row,
                    align: Align::Center,
                    justify: Justify::SpaceBetween,
                    padding: Insets::xy(tok::CELL_PAD_X, 0.0),
                    gap: tok::CELL_PAD_X,
                    grow: 1.0,
                    ..Style::default()
                },
                ..Element::new(GROUP_CELL, Role::Cell, group.name, None)
            },
            |cx| {
                for (key, text, weight) in [
                    (GROUP_LABEL, group.label, font::SEMIBOLD),
                    (GROUP_COUNT, group.count, font::REGULAR),
                ] {
                    cx.add(Element {
                        text_style: TextStyle {
                            size: font::CAPTION,
                            weight,
                            ..TextStyle::default()
                        },
                        look: HEADER,
                        ..Element::new(key, Role::Label, text, None)
                    });
                }
            },
        );
        hx.finish();
        i
    }

    /// Table slot `t`'s group headers from `heads.0` to `heads.1`, up to the one row `upto`
    /// starts (to the last if `None`), each recorded for layout.
    fn group_headers(
        &mut self,
        t: u32,
        table: &Table<'_, M>,
        heads: &mut (u32, u32),
        upto: Option<u32>,
    ) {
        while heads.0 < heads.1
            && upto.is_none_or(|at| self.ui.tables[t as usize].starts[heads.0 as usize] <= at)
        {
            let g = heads.0;
            let node = self.group_header(table, g);
            self.ui.tables[t as usize].heads_scratch.push((node, g));
            heads.0 += 1;
        }
    }
}

/// The first row from `find.from` up to `rows` whose `name` column reads `find.name`, through
/// the view's `row`, which pushes each row's cells into `cells` (a search's: that column only).
fn search<'r>(
    find: &Find,
    rows: u32,
    name: usize,
    cells: &mut Cells<'_>,
    row: &mut impl FnMut(u32, &mut Cells<'_>) -> Row<'r>,
) -> Option<u32> {
    (find.from..rows).find(|&at| {
        cells.text.clear();
        cells.ends.clear();
        row(at, cells);
        let named = CellText {
            text: cells.text,
            ends: cells.ends,
            marks: &[],
            start: 0,
        };
        named.get(name) == find.name
    })
}

/// `table`'s shown columns, with their places in its columns: the cells' indices.
fn shown<'t, 's, M>(table: &'t Table<'s, M>) -> impl Iterator<Item = (usize, &'t Column<'s>)> {
    table
        .columns
        .iter()
        .enumerate()
        .filter(|&(j, _)| !is_hidden(table, j))
}

/// Shown column `c`, `table`'s column `j`, at the width the user left it. A fit keeps the one
/// measured in `old`, the columns of the last build, while the view still asks for it.
fn col_of<M>(table: &Table<'_, M>, j: usize, c: &Column<'_>, old: &[Col]) -> Col {
    let mut col = Col {
        id: c.id,
        width: c.width,
        min: c.min,
        fit: false,
        fitted: false,
        sorts: c.sort.is_some(),
    };
    match table.widths.get(j).copied().unwrap_or_default() {
        Width::Auto => {}
        Width::Px(w) => col.width = Track::Fixed(w),
        Width::Fit => {
            col.fit = true;
            if let Some(o) = old.iter().find(|o| o.id == c.id && o.fit) {
                (col.width, col.fitted) = (o.width, o.fitted);
            }
        }
    }
    col
}

impl<M> Ui<M> {
    /// Sends `i`'s children from the `from`th on to a client again: what it learns from their
    /// places changed.
    fn a11y_touch_children(&mut self, i: u32, from: usize) {
        let mut c = self.nodes[i as usize].first;
        let mut k = 0;
        while c != NIL {
            if k >= from {
                self.a11y_touch(c);
            }
            k += 1;
            c = self.nodes[c as usize].next;
        }
    }
}

/// Whether `table`'s column `j` is hidden (a column past the end of `hidden` shows).
fn is_hidden<M>(table: &Table<'_, M>, j: usize) -> bool {
    table.hidden.get(j).copied().unwrap_or(false)
}

/// Which of a table's columns show, worked out once a build for every row it builds.
#[derive(Clone, Copy)]
struct Shown {
    /// Whether any column is hidden. If not, a row's cells test nothing.
    any_hidden: bool,
    /// The first shown column, which names a row (`usize::MAX` if none shows).
    first: usize,
}

impl Shown {
    fn of<M>(table: &Table<'_, M>) -> Self {
        Self {
            any_hidden: (0..table.columns.len()).any(|j| is_hidden(table, j)),
            first: shown(table).next().map_or(usize::MAX, |(j, _)| j),
        }
    }
}

/// The rows' viewport of a table with `rows` rows: one Tab stop if it has any, else the status
/// line and its action, from the view's corner.
fn body_element<'s, M>(rows: u32) -> Element<'s, M> {
    Element {
        rare: rare::SCROLL,
        extra: if rows > 0 {
            Extra::Group(Group::Rows)
        } else {
            Extra::None
        },
        style: Style {
            align: Align::Start,
            padding: Insets::all(tok::STATUS_PAD),
            gap: tok::STATUS_GAP,
            ..Style::default()
        },
        ..Element::new(TABLE_BODY, Role::Container, "", None)
    }
}

/// A row's cells' text in the Ui's cell buffer: cell `j` ends at `ends[j]`, and the first starts
/// at `start`. `marks` are its cells that aren't one line of text.
struct CellText<'b> {
    text: &'b str,
    ends: &'b [usize],
    marks: &'b [Mark],
    start: usize,
}

impl CellText<'_> {
    /// Cell `j`'s text, a two-line cell's first line; empty if the view gave fewer.
    fn get(&self, j: usize) -> &str {
        let all = self.all(j);
        if self.marks.is_empty() {
            return all;
        }
        match self.mark(j) {
            Some(MarkKind::Two(split)) => all.get(..split as usize).unwrap_or(all),
            _ => all,
        }
    }

    /// Cell `j`'s whole text: both a two-line cell's lines.
    // Out of line, as A2's was: inlined in a row's loop, it keeps `Cx::add` from being.
    #[inline(never)]
    fn all(&self, j: usize) -> &str {
        let Some(&end) = self.ends.get(j) else {
            return "";
        };
        let from = match j.checked_sub(1) {
            Some(k) => self.ends[k],
            None => self.start,
        };
        self.text.get(from..end).unwrap_or("")
    }

    /// What cell `j` is if it isn't one line of text.
    fn mark(&self, j: usize) -> Option<MarkKind> {
        self.marks
            .iter()
            .find(|m| m.cell as usize == j)
            .map(|m| m.kind)
    }
}

/// How much of each of [`CellBufs`]' buffers is filled: text, ends, marks.
type Lens = (usize, usize, usize);

/// The buffers a table's build fills with its rows' cells: the Ui's text and ends, and the
/// table's marks and kept values, taken for the build and given back after it.
struct CellBufs {
    text: String,
    ends: Vec<usize>,
    marks: Vec<Mark>,
    values: ValueCache,
}

impl CellBufs {
    /// Table slot `t`'s buffers (fresh ones if it has none), emptied, its values for this
    /// build's formats.
    fn take<M>(ui: &mut Ui<M>, t: u32) -> Self {
        let mut text = mem::take(&mut ui.cell_text);
        let mut ends = mem::take(&mut ui.cell_ends);
        text.clear();
        ends.clear();
        let (mut values, mut marks) = if t == NIL {
            (ValueCache::default(), Vec::new())
        } else {
            let st = &mut ui.tables[t as usize];
            (mem::take(&mut st.values), mem::take(&mut st.marks))
        };
        marks.clear();
        values.begin(ui.formats_generation);
        Self {
            text,
            ends,
            marks,
            values,
        }
    }

    /// Gives the buffers back, emptied, for the next build.
    fn give<M>(mut self, ui: &mut Ui<M>, t: u32) {
        self.clear();
        ui.cell_text = self.text;
        ui.cell_ends = self.ends;
        if t != NIL {
            let st = &mut ui.tables[t as usize];
            st.values = self.values;
            st.marks = self.marks;
        }
    }

    /// A row's cells to push into them.
    #[inline]
    fn cells<'b>(&'b mut self, formats: &'b dyn Formats, keep: usize, two: bool) -> Cells<'b> {
        Cells::new(
            &mut self.text,
            &mut self.ends,
            &mut self.marks,
            &mut self.values,
            formats,
            keep,
            two,
        )
    }

    fn clear(&mut self) {
        self.truncate((0, 0, 0));
    }

    #[inline]
    fn lens(&self) -> Lens {
        (self.text.len(), self.ends.len(), self.marks.len())
    }

    #[inline]
    fn truncate(&mut self, (text, ends, marks): Lens) {
        self.text.truncate(text);
        self.ends.truncate(ends);
        self.marks.truncate(marks);
    }

    /// The cells pushed from `from` to `to`.
    #[inline]
    fn text_of(&self, from: Lens, to: Lens) -> CellText<'_> {
        CellText {
            text: &self.text,
            ends: &self.ends[from.1..to.1],
            marks: &self.marks[from.2..to.2],
            start: from.0,
        }
    }
}

impl<M> Ui<M> {
    /// Whether the last build should run again, now that layout has measured what it guessed: a
    /// table's row height or rows' view, or a scroll that brought other rows into view. The host
    /// builds and lays out again (at most once more) when it says so.
    #[must_use]
    pub fn wants_build(&self) -> bool {
        self.tables.iter().any(|st| {
            if st.body == NIL || st.row_px <= 0 {
                return false;
            }
            let s = self.nodes[st.body as usize].scroll;
            if s == NIL {
                return false;
            }
            if (st.reveal.is_some() && st.view_px > 0) || !st.built_measured() {
                return true;
            }
            let off = self.scrolls[s as usize].offset[1];
            st.span != st.geom().span_for(off, st.view_px)
        })
    }

    /// Scrolls the row at `row`, a client's index, into view and builds it, at the next build
    /// ([`wants_build`](Self::wants_build) says so): a search result, or a client asking for a
    /// row it can't see. A client counts a grouped table's header rows (TD-0009 A3.10 item 5),
    /// so an index can be a header's, which is revealed; without groups it's the row's index in
    /// the view's order. Returns whether `table` is a table with that row.
    pub fn realize(&mut self, table: NodeId, row: u32) -> bool {
        let Some(t) = self.index(table).and_then(|i| self.table_of_grid(i)) else {
            return false;
        };
        let st = &mut self.tables[t as usize];
        let Some(item) = item_of_grid(&st.starts, st.rows, row) else {
            return false;
        };
        st.reveal = Some(item);
        true
    }

    /// Searches `table`'s rows after `after` (a client's index, as [`realize`](Self::realize)
    /// takes; from the first if `None`), in the view's order, for the first named `name` (its
    /// first shown column's text), at the next build, which also realizes the row found: a
    /// client's search by name (TD-0009 A2.5). [`take_found`](Self::take_found) then says which
    /// row it was. The search goes through the view's rows, reading only that column; group
    /// headers aren't searched. False if `table` isn't a table.
    pub fn find_row(&mut self, table: NodeId, after: Option<u32>, name: &str) -> bool {
        let Some(t) = self.index(table).and_then(|i| self.table_of_grid(i)) else {
            return false;
        };
        let st = &mut self.tables[t as usize];
        let from = after.map_or(0, |at| match item_of_grid(&st.starts, st.rows, at) {
            Some(Item::Row(r)) => r.saturating_add(1),
            Some(Item::Head(g)) => st.starts[g as usize],
            None => st.rows,
        });
        st.found = None;
        st.find = Some(Find {
            from,
            name: name.to_owned(),
        });
        true
    }

    /// The row the last build's [`find_row`](Self::find_row) found in `table`, by a client's
    /// index. A search no build ran (the view didn't build the table) is dropped.
    pub fn take_found(&mut self, table: NodeId) -> Option<u32> {
        let t = self.index(table).and_then(|i| self.table_of_grid(i))?;
        let st = &mut self.tables[t as usize];
        st.find = None;
        let found = st.found.take()?;
        Some(grid_of(&st.starts, found))
    }

    /// Scrolls `viewport` to `to`, physical pixels x then y, each clamped: where the host put it
    /// last session. A table's header scrolls its rows. Returns whether it moved; `false` too if
    /// `viewport` isn't one.
    pub fn scroll_to(&mut self, viewport: NodeId, to: [i64; 2]) -> bool {
        let Some(i) = self.index(viewport) else {
            return false;
        };
        let s = self.nodes[i as usize].scroll;
        if s == NIL {
            return false;
        }
        let s = self.leader_of(s);
        self.scroll_set(s, to)
    }

    // ---- the side table ----------------------------------------------------------------------

    /// The table slot for the rows' viewport `body`, made if it's new, its header following it.
    /// [`NIL`] if `body` isn't a viewport (it always is).
    fn table_slot(&mut self, grid: u32, header: u32, body: u32) -> u32 {
        let s = self.nodes[body as usize].scroll;
        let h = self.nodes[header as usize].scroll;
        if s == NIL {
            return NIL;
        }
        let mut t = self.scrolls[s as usize].table;
        if t == NIL {
            t = if let Some(t) = self.free_tables.pop() {
                t
            } else {
                self.tables.push(TableState::new());
                u32::try_from(self.tables.len() - 1).unwrap_or(NIL)
            };
            self.scrolls[s as usize].table = t;
        }
        let st = &mut self.tables[t as usize];
        st.grid = grid;
        st.header = header;
        st.body = body;
        if h != NIL {
            self.scrolls[s as usize].follower = h;
            self.scrolls[h as usize].leader = s;
        }
        t
    }

    /// Frees table slot `t`, keeping its buffers for the next.
    pub(crate) fn release_table(&mut self, t: u32) {
        let Some(st) = self.tables.get_mut(t as usize) else {
            return;
        };
        let mut fresh = TableState::new();
        fresh.columns = mem::take(&mut st.columns);
        fresh.columns.clear();
        fresh.cols_next = mem::take(&mut st.cols_next);
        fresh.cols_next.clear();
        fresh.frozen = mem::take(&mut st.frozen);
        fresh.frozen.clear();
        fresh.check_label = mem::take(&mut st.check_label);
        fresh.check_label.clear();
        fresh.sort_status = mem::take(&mut st.sort_status);
        fresh.sort_status.iter_mut().for_each(String::clear);
        fresh.tracks = mem::take(&mut st.tracks);
        fresh.tracks.clear();
        fresh.built = mem::take(&mut st.built);
        fresh.built.clear();
        fresh.scratch = mem::take(&mut st.scratch);
        fresh.scratch.clear();
        fresh.starts = mem::take(&mut st.starts);
        fresh.starts.clear();
        fresh.heads = mem::take(&mut st.heads);
        fresh.heads.clear();
        fresh.heads_scratch = mem::take(&mut st.heads_scratch);
        fresh.heads_scratch.clear();
        fresh.marks = mem::take(&mut st.marks);
        fresh.marks.clear();
        *st = fresh;
        self.free_tables.push(t);
        // A drag on a table that went ends with it.
        if self.divider.is_some_and(|d| d.t == t) {
            self.divider = None;
        }
    }

    // ---- building ----------------------------------------------------------------------------

    /// Takes `table`'s facts into slot `t`, scrolls to a row that became current or was
    /// realized, and says which rows to build.
    fn table_plan(
        &mut self,
        t: u32,
        table: &Table<'_, M>,
        current: Option<u32>,
        key: Option<Key>,
    ) -> Plan {
        self.table_facts(t, table);
        let body = self.tables[t as usize].body;
        let s = self.nodes[body as usize].scroll;
        let rows = table.rows;
        let focus_here = self.focus != NIL && {
            let f = &self.nodes[self.focus as usize];
            f.role == Role::Row && f.parent == body
        };
        // Only an empty table's viewport takes focus (A2.4): rows are its stops otherwise.
        let empty = self.focus == body;
        // Until layout has measured them, the heights are guessed from the text size.
        let (row_guess, header_guess) = {
            let size = self.nodes[body as usize].text_style.size;
            let line = size * LINE_GUESS;
            let row = if table.lines == RowLines::Two {
                (line + tok::LINE_GAP + line * font::CAPTION / font::BODY + 2.0 * tok::ROW_PAD_Y)
                    .max(tok::ROW_TWO)
            } else {
                (line + 2.0 * tok::ROW_PAD_Y).max(tok::ROW)
            };
            let group =
                (line * font::CAPTION / font::BODY + 2.0 * tok::GROUP_PAD_Y).max(tok::GROUP);
            (
                to_px(row, self.scale).max(1),
                to_px(group, self.scale).max(1),
            )
        };
        let st = &mut self.tables[t as usize];
        st.current = current;
        st.scratch.clear();
        st.heads_scratch.clear();
        let mut follow = false;
        if st.current_key != key {
            st.current_key = key;
            if let Some(c) = current {
                st.reveal = Some(Item::Row(c));
                follow = focus_here;
            }
        }
        st.built_px = if st.row_px > 0 { st.row_px } else { row_guess };
        st.built_gp = if st.group_px > 0 {
            st.group_px
        } else {
            header_guess
        };
        let view = st.view_px;
        // The content is every row and header, exactly.
        if rows > 0 {
            let content = st.geom().height();
            let sc = &mut self.scrolls[s as usize];
            if sc.content_px[1] != content {
                sc.content_px[1] = content;
                sc.content.h = to_logical(content, self.scale);
                self.damage_strips(body);
                self.touch_grid(s);
            }
            // Fewer rows may end above the offset.
            let off = self.scrolls[s as usize].offset[1];
            self.scroll_axis(s, 1, off);
        }
        if view > 0
            && let Some(item) = self.tables[t as usize].reveal.take()
        {
            let off = self.scrolls[s as usize].offset[1];
            if let Some(to) = self.tables[t as usize].reveal_to(item, off) {
                self.scroll_axis(s, 1, to);
            }
        }
        let off = self.scrolls[s as usize].offset[1];
        let st = &mut self.tables[t as usize];
        let geom = st.geom();
        let span = geom.span_for(off, view);
        let (from, to) = span.rows;
        let base_row = from / BASE_ROWS * BASE_ROWS;
        let base_px = geom.item_top(base_row);
        st.span = span;
        let rebased = st.base_row != base_row;
        if rebased || self.scrolls[s as usize].base[1] != base_px {
            st.base_row = base_row;
            self.scrolls[s as usize].base[1] = base_px;
            // Every row shows where it did, laid out elsewhere.
            self.damage_view(body);
            self.mark_dirty(body);
            self.a11y_touch(body);
        }
        if rebased {
            // A row can keep its place on the screen and change its index: a client learns it.
            self.touch_built(t);
        }
        Plan {
            from,
            to,
            heads: span.heads,
            extra: current.filter(|&c| c < from || c >= to),
            follow,
            focused: focus_here,
            empty,
        }
    }

    /// Every row and group header built for slot `t`, and their cells, to send a client again:
    /// their indices changed.
    fn touch_built(&mut self, t: u32) {
        let st = &self.tables[t as usize];
        let (rows, heads) = (st.built.len(), st.heads.len());
        for k in 0..rows + heads {
            let st = &self.tables[t as usize];
            let node = match k.checked_sub(rows) {
                None => st.built[k].0,
                Some(h) => st.heads[h].0,
            };
            self.a11y_touch(node);
            self.a11y_touch_children(node, 0);
        }
    }

    /// Copies what the grid tells a client and what layout needs from `table` into slot `t`.
    fn table_facts(&mut self, t: u32, table: &Table<'_, M>) {
        let st = &mut self.tables[t as usize];
        st.on = Some(table.on);
        let mut seen = false;
        let mut relayout = false;
        if st.rows != table.rows {
            st.rows = table.rows;
            seen = true;
        }
        let checks = table.checks.is_some();
        if st.checks != checks {
            st.checks = checks;
            seen = true;
            relayout = true;
        }
        let label = table.checks.map_or("", |(label, _)| label);
        let renamed = st.check_label != label;
        if renamed {
            st.check_label.clear();
            st.check_label.push_str(label);
        }
        let (reshaped, regrouped) = st.take_shape(table);
        seen |= regrouped;
        relayout |= reshaped || regrouped;
        let mut restated = false;
        for (kept, given) in st.sort_status.iter_mut().zip(table.sort_status) {
            if kept != given {
                kept.clear();
                kept.push_str(given);
                restated = true;
            }
        }
        // The shown columns at the widths the user left them. A fit asked for again keeps the
        // one measured: it's measured once, and sent once. Compared in place, as they're the
        // same at almost every build.
        let mut k = 0;
        let same = shown(table).all(|(j, c)| {
            let eq = st.columns.get(k) == Some(&col_of(table, j, c, &st.columns));
            k += 1;
            eq
        }) && k == st.columns.len();
        // The first shown column whose place changed: the headers and cells from it on have new
        // indices. A width that changed moves none.
        let mut moved_from = None;
        if !same {
            let mut next = mem::take(&mut st.cols_next);
            next.clear();
            next.extend(shown(table).map(|(j, c)| col_of(table, j, c, &st.columns)));
            mem::swap(&mut st.columns, &mut next);
            let kept = next.len().min(st.columns.len());
            let first = (next.iter().zip(&st.columns))
                .position(|(old, new)| old.id != new.id)
                .unwrap_or(kept);
            if first < next.len().max(st.columns.len()) {
                moved_from = Some(first + usize::from(st.checks));
            }
            seen = true;
            relayout = true;
            st.cols_next = next;
        }
        // A fit waits for the rows to settle. Until it's measured, each build lays the grid out:
        // the build that settles them may change nothing else in it.
        relayout |= st.columns.iter().any(|c| c.fit && !c.fitted);
        let (grid, header) = (st.grid, st.header);
        if !same {
            // The view changed the columns under a drag: it ends, with nothing sent. The columns
            // it held may have moved.
            if self.tables[t as usize].drag.take().is_some()
                && self.divider.is_some_and(|d| d.t == t)
            {
                self.divider = None;
            }
        }
        if let Some(from) = moved_from {
            // A column that came or went moves the ones after it, which may keep their places on
            // the screen: a client learns their new indices from their headers and cells.
            self.a11y_touch_children(header, from);
            for k in 0..self.tables[t as usize].built.len() {
                let (node, _) = self.tables[t as usize].built[k];
                self.a11y_touch_children(node, from);
            }
        }
        if renamed {
            // Every check cell is named as the column is.
            for k in 0..self.tables[t as usize].built.len() {
                let (node, _) = self.tables[t as usize].built[k];
                let c = self.nodes[node as usize].first;
                if c != NIL && self.nodes[c as usize].key == TABLE_CHECK {
                    self.a11y_touch(c);
                }
            }
        }
        if restated {
            // The sorted header says its order in the new words.
            self.a11y_touch_children(header, 0);
        }
        if regrouped {
            // Every row and header moves, and a client counts them anew.
            let body = self.tables[t as usize].body;
            self.damage_view(body);
            self.mark_dirty(body);
            self.touch_built(t);
        }
        if relayout {
            self.mark_dirty(grid);
        }
        if seen {
            self.a11y_touch(grid);
        }
    }

    /// The rows built for slot `t` are its scratch list: layout places them again if they
    /// changed, and focus follows the current row if it should.
    fn table_built(&mut self, t: u32, plan: Plan) {
        let st = &mut self.tables[t as usize];
        let changed = st.scratch != st.built || st.heads_scratch != st.heads;
        mem::swap(&mut st.built, &mut st.scratch);
        st.scratch.clear();
        mem::swap(&mut st.heads, &mut st.heads_scratch);
        st.heads_scratch.clear();
        let body = st.body;
        if changed {
            self.mark_dirty(body);
        }
        // A focused row the build dropped (scrolled away from, or gone with a refresh) took focus
        // with it: it comes back to the current row, else the first row in view, so the keyboard
        // and a client's cursor stay in the table. With no row left (a refresh that emptied it, as
        // the in-app picker's folder does while it loads), to the rows' viewport, the stop an
        // empty table has (A2.4), or its status line's action; and from there onto the rows once
        // they're built (TD-0009 A4.4 × A3.5 (12), wave 3), so Enter and Backspace keep going.
        let dropped = plan.focused && self.focus == NIL;
        let arrived = plan.empty && self.focus == body && !self.tables[t as usize].built.is_empty();
        if plan.follow || dropped || arrived {
            let to = self.current_row_node(t).or_else(|| {
                let at = self.first_row_in_view(t)?;
                self.built_node(t, at)
            });
            if let Some(node) = to {
                self.focus_node(node);
            } else if dropped {
                let stop = self.first_stop_in(body);
                if stop != NIL {
                    self.focus_node(stop);
                }
            }
        }
    }

    /// The node of table `t`'s row `at`, if it's built.
    fn built_node(&self, t: u32, at: u32) -> Option<u32> {
        let st = &self.tables[t as usize];
        st.built
            .iter()
            .find(|&&(_, a)| a == at)
            .map(|&(node, _)| node)
    }

    /// Table `t`'s current row's node, if it has a current row (it's always built).
    pub(crate) fn current_row_node(&self, t: u32) -> Option<u32> {
        self.built_node(t, self.tables[t as usize].current?)
    }

    /// Table `t`'s first row wholly in view, or the one partly in view if none is wholly (a view
    /// lower than a row). `None` if it has no rows or hasn't been laid out.
    fn first_row_in_view(&self, t: u32) -> Option<u32> {
        let st = &self.tables[t as usize];
        let (rows, view) = (st.rows, st.view_px);
        let geom = Geom {
            rp: st.built_px,
            gp: st.built_gp,
            ..st.geom()
        };
        if rows == 0 || geom.rp <= 0 || view <= 0 {
            return None;
        }
        let s = self.nodes[st.body as usize].scroll;
        let off = self.scrolls[s as usize].offset[1];
        let whole = geom.row_from(off);
        if geom.row_top(whole).saturating_add(geom.rp) <= off.saturating_add(view) {
            return Some(whole);
        }
        // A view lower than a row: the one at its top, or under the header there.
        Some(match geom.at(off.clamp(0, geom.height() - 1)) {
            Item::Row(r) => r,
            Item::Head(g) => st.starts[g as usize],
        })
    }

    /// Where Tab lands in table body `body`, either way: its current row, else its first row
    /// wholly in view (the rows are built half a view past it, so not the first built). `None`
    /// if `body` isn't a table's.
    pub(crate) fn table_entry(&self, body: u32) -> Option<u32> {
        let t = self.table_of_body(body)?;
        self.current_row_node(t)
            .or_else(|| self.built_node(t, self.first_row_in_view(t)?))
    }

    // ---- lookups -----------------------------------------------------------------------------

    /// The table slot of grid node `i`; `None` if it isn't a table built by [`Cx::table`].
    pub(crate) fn table_of_grid(&self, i: u32) -> Option<u32> {
        let s = self.grid_rows_slot(i);
        if s == NIL {
            return None;
        }
        let t = self.scrolls[s as usize].table;
        (t != NIL).then_some(t)
    }

    /// The scroll slot of grid node `i`'s rows' viewport; [`NIL`] if it has none.
    pub(crate) fn grid_rows_slot(&self, i: u32) -> u32 {
        let n = &self.nodes[i as usize];
        if n.role != Role::Grid || n.last == NIL {
            return NIL;
        }
        let body = &self.nodes[n.last as usize];
        if body.key == TABLE_BODY {
            body.scroll
        } else {
            NIL
        }
    }

    /// The table slot of node `body` if it's a table's rows' viewport.
    fn table_of_body(&self, body: u32) -> Option<u32> {
        let n = self.nodes.get(body as usize)?;
        if n.key != TABLE_BODY || n.scroll == NIL {
            return None;
        }
        let t = self.scrolls[n.scroll as usize].table;
        (t != NIL).then_some(t)
    }

    /// Row node `i`'s table slot and index; `None` if it isn't a table's row.
    pub(crate) fn row_place(&self, i: u32) -> Option<(u32, u32)> {
        let n = self.nodes.get(i as usize)?;
        if n.role != Role::Row {
            return None;
        }
        let t = self.table_of_body(n.parent)?;
        let at = self.tables[t as usize]
            .built
            .iter()
            .find(|&&(node, _)| node == i)?
            .1;
        Some((t, at))
    }

    /// Whether node `i` is a group's header row (TD-0009 A3.5 (10)): the only container a
    /// table's rows' viewport holds.
    pub(crate) fn is_group_header(&self, i: u32) -> bool {
        let n = &self.nodes[i as usize];
        n.role == Role::Container
            && self
                .nodes
                .get(n.parent as usize)
                .is_some_and(|p| p.key == TABLE_BODY && p.scroll != NIL)
    }

    /// Group header node `i`'s table slot and group; `None` if it isn't one.
    pub(crate) fn group_place(&self, i: u32) -> Option<(u32, u32)> {
        if !self.is_group_header(i) {
            return None;
        }
        let t = self.table_of_body(self.nodes[i as usize].parent)?;
        let g = self.tables[t as usize]
            .heads
            .iter()
            .find(|&&(node, _)| node == i)?
            .1;
        Some((t, g))
    }

    /// A client's index of group header node `i` (its group's first row's, less the rows' own
    /// headers), and its row's cells' span: every column. `None` if it isn't one.
    pub(crate) fn group_row(&self, i: u32) -> Option<(usize, usize)> {
        let (t, g) = self.group_place(i)?;
        let st = &self.tables[t as usize];
        Some((
            head_grid_of(&st.starts, g) as usize,
            st.columns.len() + usize::from(st.checks),
        ))
    }

    /// A client's index of row `at` of table `t`, its group headers counted (A3.10 item 5).
    pub(crate) fn grid_row(&self, t: u32, at: u32) -> usize {
        grid_of(&self.tables[t as usize].starts, at) as usize
    }

    /// Table `t`'s rows as a client counts them, its group headers with them.
    pub(crate) fn grid_rows(&self, t: u32) -> usize {
        let st = &self.tables[t as usize];
        st.rows as usize + st.starts.len()
    }

    /// Whether node `i` is a table's header.
    pub(crate) fn is_table_header(&self, i: u32) -> bool {
        let n = &self.nodes[i as usize];
        n.key == TABLE_HEADER
            && self
                .nodes
                .get(n.parent as usize)
                .is_some_and(|p| p.role == Role::Grid)
    }

    /// Column header `i`'s table slot; `None` if it isn't in a table's header.
    pub(crate) fn table_of_column_header(&self, i: u32) -> Option<u32> {
        let header = self.nodes.get(i as usize)?.parent;
        let grid = self.nodes.get(header as usize)?.parent;
        self.is_table_header(header)
            .then(|| self.table_of_grid(grid))
            .flatten()
    }

    /// Whether cell `i` is its row's first text cell, which names the row (a row header).
    pub(crate) fn is_row_header(&self, i: u32) -> bool {
        let n = &self.nodes[i as usize];
        if n.key == TABLE_CHECK {
            return false;
        }
        n.prev == NIL || self.nodes[n.prev as usize].key == TABLE_CHECK
    }

    /// How many siblings come before node `i`: a cell's or a column header's column.
    pub(crate) fn sibling_index(&self, i: u32) -> usize {
        let mut k = 0;
        let mut c = self.nodes[i as usize].prev;
        while c != NIL {
            k += 1;
            c = self.nodes[c as usize].prev;
        }
        k
    }

    /// Whether row `at` of slot `t` can be checked: a built row has an enabled check cell; one
    /// not built can, if the table has checks.
    fn row_checkable(&self, t: u32, at: u32) -> bool {
        let st = &self.tables[t as usize];
        if !st.checks {
            return false;
        }
        match st.built.iter().find(|&&(_, a)| a == at) {
            Some(&(node, _)) => {
                let c = self.nodes[node as usize].first;
                c != NIL && {
                    let cell = &self.nodes[c as usize];
                    cell.key == TABLE_CHECK && cell.enabled
                }
            }
            None => true,
        }
    }

    /// Row node `i`'s rectangle in its viewport's content as painted, exactly, as `[x0, y0, x1,
    /// y1]`: its index times the row height from the base, however far down. In `i64`: the
    /// current row, built wherever it is, can be past `i32` pixels from the base. `None` if it
    /// isn't a table's row.
    pub(crate) fn row_rect_px(&self, i: u32) -> Option<[i64; 4]> {
        let (t, at) = self.row_place(i)?;
        let st = &self.tables[t as usize];
        let geom = st.geom();
        if geom.rp <= 0 {
            return None;
        }
        let body = self.nodes[st.body as usize].rect.snap(self.scale);
        let r = self.nodes[i as usize].rect.snap(self.scale);
        let y0 = i64::from(body.y0) + geom.row_top(at) - geom.item_top(st.base_row);
        Some([i64::from(r.x0), y0, i64::from(r.x1), y0 + geom.rp])
    }

    /// Table `t`'s rows' and group headers' height exactly, in physical pixels, once layout has
    /// measured them; `None` before, or with no rows.
    pub(crate) fn table_content_px(&self, t: u32) -> Option<i64> {
        let st = &self.tables[t as usize];
        (st.rows > 0 && st.row_px > 0).then(|| st.geom().height())
    }

    // ---- layout ------------------------------------------------------------------------------

    /// Table `t`'s heights, measured from its body text. A row is its body text's line and the
    /// row padding above and below, at least the row token; a two-line row both lines, the gap
    /// between them and the padding, at least its token. A group header is a caption semibold
    /// line and its padding, at least its token. The column header stays a one-line row high
    /// over rows of two (A3.5 (10)).
    fn row_heights(&self, t: u32, text: &mut dyn TextSystem) -> Heights {
        let st = &self.tables[t as usize];
        let style = self.nodes[st.body as usize].text_style;
        let line = text.measure("", &style, f32::INFINITY).height;
        let one = (line + 2.0 * tok::ROW_PAD_Y).max(tok::ROW);
        let small = second_line(style);
        // Caption lines are measured only for a table that shows them: most tables have neither
        // two-line rows nor groups, and every layout measures (U1, the interact benches).
        let row_h = if st.two {
            let caption = text.measure("", &small, f32::INFINITY).height;
            (line + tok::LINE_GAP + caption + 2.0 * tok::ROW_PAD_Y).max(tok::ROW_TWO)
        } else {
            one
        };
        let group_h = if st.starts.is_empty() {
            tok::GROUP
        } else {
            let label = TextStyle {
                weight: font::SEMIBOLD,
                ..small
            };
            (text.measure("", &label, f32::INFINITY).height + 2.0 * tok::GROUP_PAD_Y)
                .max(tok::GROUP)
        };
        let px = |h: f32| to_px(h, self.scale).max(1);
        Heights {
            row_h,
            rp: px(row_h),
            group_h,
            gp: px(group_h),
            head: px(one),
        }
    }

    /// A grid's natural size: its columns' least widths, and the header, each row and each group
    /// header (or the status line). `None` if it isn't a table.
    pub(crate) fn grid_natural(&self, i: u32, text: &mut dyn TextSystem) -> Option<Size> {
        let t = self.table_of_grid(i)?;
        // Measured now, not the last layout's: after a text size change, the grid is measured
        // before it's arranged.
        let h = self.row_heights(t, text);
        let st = &self.tables[t as usize];
        let w = if st.checks { tok::CHECK_COL } else { 0.0 }
            + st.columns
                .iter()
                .map(|c| match c.width {
                    Track::Fixed(w) => w.max(c.min),
                    Track::Share(_) => c.min,
                })
                .sum::<f32>();
        let rows = Geom {
            rows: st.rows.max(1),
            rp: h.rp,
            gp: h.gp,
            starts: &st.starts,
        };
        let h = to_logical(h.head.saturating_add(rows.height()), self.scale);
        Some(Size { w, h })
    }

    /// Lays out a table's part: the grid, its header, its rows' viewport or a row. Returns whether
    /// `i` was one; other nodes go the usual way.
    #[inline]
    pub(crate) fn arrange_table(
        &mut self,
        i: u32,
        text: &mut dyn TextSystem,
        covered: bool,
    ) -> bool {
        let n = &self.nodes[i as usize];
        match n.role {
            Role::Grid | Role::Row => {}
            Role::Container if n.key == TABLE_BODY || n.key == TABLE_HEADER => {}
            _ => return false,
        }
        self.arrange_table_part(i, text, covered)
    }

    #[inline(never)]
    fn arrange_table_part(&mut self, i: u32, text: &mut dyn TextSystem, covered: bool) -> bool {
        let n = &self.nodes[i as usize];
        let (role, key, parent) = (n.role, n.key, n.parent);
        match role {
            Role::Grid => {
                let Some(t) = self.table_of_grid(i) else {
                    return false;
                };
                self.arrange_grid(i, t, text, covered);
            }
            Role::Row => {
                let Some(t) = self.table_of_body(parent) else {
                    return false;
                };
                let r = self.nodes[i as usize].rect;
                self.arrange_tracks(i, t, r, text, covered);
            }
            _ if key == TABLE_HEADER => {
                let Some(t) = self
                    .nodes
                    .get(parent as usize)
                    .and_then(|_| self.table_of_grid(parent))
                else {
                    return false;
                };
                if self.nodes[i as usize].scroll == NIL {
                    return false;
                }
                self.arrange_header(i, t, text, covered);
            }
            _ => {
                let Some(t) = self.table_of_body(i) else {
                    return false;
                };
                // With no rows, its status line and action go the usual way.
                if self.tables[t as usize].rows == 0 {
                    return false;
                }
                self.arrange_rows(i, t, text, covered);
            }
        }
        true
    }

    /// The grid: the row and group header heights from the rows' text, the columns' tracks, the
    /// header a one-line row high and the rows' viewport under it.
    fn arrange_grid(&mut self, i: u32, t: u32, text: &mut dyn TextSystem, covered: bool) {
        let scale = self.scale;
        let inner = {
            let n = &self.nodes[i as usize];
            n.rect.inset(n.style.padding)
        };
        let (header, body) = {
            let st = &self.tables[t as usize];
            (st.header, st.body)
        };
        let h = self.row_heights(t, text);
        let st = &mut self.tables[t as usize];
        st.row_h = h.row_h;
        st.group_h = h.group_h;
        let old = (st.row_px, st.group_px);
        if (h.rp, h.gp) != old {
            self.rows_resized(t, old, (h.rp, h.gp));
        }
        let row = to_logical(h.head, scale);
        let head = Rect::new(inner.x, inner.y, inner.w, row.min(inner.h));
        let rows = Rect::new(
            inner.x,
            inner.y + head.h,
            inner.w,
            (inner.h - head.h).max(0.0),
        );
        let view = rows.snap(scale);
        self.tables[t as usize].view_px = i64::from(view.y1 - view.y0);
        // A fit measures the rows layout wants built: once the build that made them has.
        if self.tables[t as usize]
            .columns
            .iter()
            .any(|c| c.fit && !c.fitted)
            && self.table_settled(t)
        {
            self.fit_columns(t, text);
        }
        if self.lay_tracks(t, inner.w) {
            // Every cell moves.
            self.nodes[header as usize].dirty = true;
            self.nodes[body as usize].dirty = true;
            for k in 0..self.tables[t as usize].built.len() {
                let (node, _) = self.tables[t as usize].built[k];
                self.nodes[node as usize].dirty = true;
            }
        }
        self.arrange(header, head, text, covered);
        self.arrange(body, rows, text, covered);
    }

    /// Whether table `t`'s last build made the rows layout wants: it used the row height measured,
    /// and the view (laid out) and its offset ask for no other rows and no reveal.
    fn table_settled(&self, t: u32) -> bool {
        let st = &self.tables[t as usize];
        let s = self.nodes[st.body as usize].scroll;
        if s == NIL || st.view_px <= 0 || !st.built_measured() || st.reveal.is_some() {
            return false;
        }
        let off = self.scrolls[s as usize].offset[1];
        st.span == st.geom().span_for(off, st.view_px)
    }

    /// Measures table `t`'s columns the view asks to fit: each at its header's text (and a
    /// chevron, if it sorts) or its widest cell in the rows built, whichever is wider, at least
    /// its minimum. Each takes that width now, and the view is sent it, once.
    #[inline(never)]
    fn fit_columns(&mut self, t: u32, text: &mut dyn TextSystem) {
        let (header, skip) = {
            let st = &self.tables[t as usize];
            (st.header, usize::from(st.checks))
        };
        let natural = |n: &Node<M>, text: &mut dyn TextSystem| {
            text.measure(&n.text, &n.text_style, f32::INFINITY).width + n.style.padding.horizontal()
        };
        // A two-line cell is its wider line; a badge cell, its badge.
        let cell = |ui: &Self, n: &Node<M>, text: &mut dyn TextSystem| {
            let pad = n.style.padding.horizontal();
            if n.first != NIL {
                return natural(&ui.nodes[n.first as usize], text) + pad;
            }
            match n.lines() {
                Some(split) => {
                    let (first, second) = lines_of(&n.text, split);
                    let a = text.measure(first, &n.text_style, f32::INFINITY).width;
                    let b = text
                        .measure(second, &second_line(n.text_style), f32::INFINITY)
                        .width;
                    a.max(b) + pad
                }
                None => natural(n, text),
            }
        };
        for k in 0..self.tables[t as usize].columns.len() {
            let c = self.tables[t as usize].columns[k];
            if !c.fit || c.fitted {
                continue;
            }
            let mut w = c.min;
            if let Some(h) = self.nth_child(header, skip + k) {
                let n = &self.nodes[h as usize];
                let chevron = if c.sorts {
                    text.measure("", &n.text_style, f32::INFINITY).height
                } else {
                    0.0
                };
                w = w.max(natural(n, text) + chevron);
            }
            for b in 0..self.tables[t as usize].built.len() {
                let (row, _) = self.tables[t as usize].built[b];
                if let Some(c) = self.nth_child(row, skip + k) {
                    w = w.max(cell(self, &self.nodes[c as usize], text));
                }
            }
            let st = &mut self.tables[t as usize];
            st.columns[k].width = Track::Fixed(w);
            st.columns[k].fitted = true;
            if let Some(on) = st.on {
                self.queued.push(on(TableEvent::Resize(c.id, w)));
            }
        }
    }

    /// Node `i`'s child `k`, counting from 0.
    fn nth_child(&self, i: u32, k: usize) -> Option<u32> {
        let mut c = self.nodes[i as usize].first;
        for _ in 0..k {
            if c == NIL {
                break;
            }
            c = self.nodes[c as usize].next;
        }
        (c != NIL).then_some(c)
    }

    /// The row and group header heights became `new` pixels from `old` (a text size, measured
    /// for the first time): the row or header at the top of the view stays there, as far into it
    /// as it was.
    fn rows_resized(&mut self, t: u32, old: (i64, i64), new: (i64, i64)) {
        let st = &self.tables[t as usize];
        let (body, base_row) = (st.body, st.base_row);
        let s = self.nodes[body as usize].scroll;
        let was = Geom {
            rp: old.0,
            gp: if old.1 > 0 { old.1 } else { new.1 },
            ..st.geom()
        };
        let now = Geom {
            rp: new.0,
            gp: new.1,
            ..was
        };
        let moved =
            (s != NIL && old.0 > 0).then(|| was.rescaled(&now, self.scrolls[s as usize].offset[1]));
        let base = now.item_top(base_row);
        let st = &mut self.tables[t as usize];
        (st.row_px, st.group_px) = new;
        if s == NIL {
            return;
        }
        if let Some(y) = moved {
            self.scrolls[s as usize].offset[1] = y;
            self.damage_view(body);
            self.a11y_touch(body);
            self.touch_grid(s);
            self.sync_follower(s);
        }
        self.scrolls[s as usize].base[1] = base;
        // Every row moves: it's laid out again below.
        self.nodes[body as usize].dirty = true;
    }

    /// The columns' tracks across `width`: the check column, fixed columns, then shares of what's
    /// left, each at least its minimum. While a divider is dragged, its column is the drag's
    /// width and those left of it the widths they had at the press: the shared ones right of it
    /// take up the difference. Returns whether they changed.
    fn lay_tracks(&mut self, t: u32, width: f32) -> bool {
        let TableState {
            columns,
            tracks,
            checks,
            content_w,
            drag,
            frozen,
            ..
        } = &mut self.tables[t as usize];
        let (drag, frozen) = (*drag, &*frozen);
        let track_of = |k: usize, c: &Col| match drag {
            Some((d, _)) if k < d => Track::Fixed(frozen.get(k).copied().unwrap_or(c.min)),
            Some((d, w)) if k == d => Track::Fixed(w),
            _ => c.width,
        };
        let check = checks.then_some(tok::CHECK_COL);
        let (fixed, shares) = columns.iter().enumerate().fold(
            (check.unwrap_or(0.0), 0.0f32),
            |(fixed, shares), (k, c)| match track_of(k, c) {
                Track::Fixed(w) => (fixed + w.max(c.min), shares),
                Track::Share(s) => (fixed, shares + s.max(0.0)),
            },
        );
        // A shared column whose share falls below its minimum takes the minimum, and the others
        // share what's left. Each one pinned lowers the rate the others get, so pinning only
        // grows: repeat until no more fall below, at most once per column.
        let rest = (width - fixed).max(0.0);
        let mut rate = if shares > 0.0 { rest / shares } else { 0.0 };
        let mut pinned = 0;
        loop {
            let (mut mins, mut held, mut count) = (0.0f32, 0.0f32, 0);
            for (k, c) in columns.iter().enumerate() {
                if let Track::Share(s) = track_of(k, c)
                    && s.max(0.0) * rate < c.min
                {
                    mins += c.min;
                    held += s.max(0.0);
                    count += 1;
                }
            }
            let left = shares - held;
            let next = if left > 0.0 {
                (rest - mins).max(0.0) / left
            } else {
                0.0
            };
            if count == pinned {
                break;
            }
            (pinned, rate) = (count, next);
        }
        let widths =
            check
                .into_iter()
                .chain(
                    columns
                        .iter()
                        .enumerate()
                        .map(|(k, c)| match track_of(k, c) {
                            Track::Fixed(w) => w.max(c.min),
                            Track::Share(s) => (s.max(0.0) * rate).max(c.min),
                        }),
                );
        // In place: a layout that changes nothing allocates nothing.
        let mut changed = false;
        let (mut x, mut k) = (0.0f32, 0);
        for w in widths {
            let track = (x, w);
            match tracks.get_mut(k) {
                Some(old) if *old == track => {}
                Some(old) => {
                    *old = track;
                    changed = true;
                }
                None => {
                    tracks.push(track);
                    changed = true;
                }
            }
            x += w;
            k += 1;
        }
        if tracks.len() != k {
            tracks.truncate(k);
            changed = true;
        }
        if content_w.to_bits() != x.to_bits() {
            *content_w = x;
            changed = true;
        }
        changed
    }

    /// A header's or a row's children at the tracks of slot `t`, across `r`.
    fn arrange_tracks(
        &mut self,
        parent: u32,
        slot: u32,
        across: Rect,
        text: &mut dyn TextSystem,
        covered: bool,
    ) {
        let mut child = self.nodes[parent as usize].first;
        let mut col = 0;
        while child != NIL {
            let next = self.nodes[child as usize].next;
            let (x, width) = self.tables[slot as usize]
                .tracks
                .get(col)
                .copied()
                .unwrap_or((0.0, 0.0));
            let rect = Rect::new(across.x + x, across.y, width, across.h);
            self.arrange(child, rect, text, covered);
            col += 1;
            child = next;
        }
    }

    /// The header: a follower viewport of column headers at the tracks, scrolling across with
    /// the rows.
    fn arrange_header(&mut self, i: u32, t: u32, text: &mut dyn TextSystem, covered: bool) {
        let (outer, _) = self.enter_viewport(i, Axis::Row);
        let r = self.nodes[i as usize].rect;
        self.arrange_tracks(i, t, r, text, covered);
        self.shown = outer;
        let content = Size {
            w: self.tables[t as usize].content_w,
            h: r.h,
        };
        let s = self.nodes[i as usize].scroll;
        self.scroll_laid_out(s, content, text);
    }

    /// The rows and group headers: each where [`Geom`] puts it, from the base, as wide as the
    /// columns or the view, whichever is wider.
    fn arrange_rows(&mut self, body: u32, slot: u32, text: &mut dyn TextSystem, covered: bool) {
        let (outer, _) = self.enter_viewport(body, Axis::Column);
        let view = self.nodes[body as usize].rect;
        let scale = f64::from(self.scale);
        let (base, content_h, content_w) = {
            let st = &self.tables[slot as usize];
            let geom = st.geom();
            (geom.item_top(st.base_row), geom.height(), st.content_w)
        };
        let width = content_w.max(view.w);
        // In f64 from the base, then once to f32: exact for a chunk of rows at any depth.
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "within a chunk of rows, f64 is exact and f32 keeps whole pixels"
        )]
        let at_px = |px: i64| (f64::from(view.y) + px as f64 / scale) as f32;
        let (rows, heads, rp, flat) = {
            let st = &self.tables[slot as usize];
            let rp = st.geom().rp;
            (st.built.len(), st.heads.len(), rp, st.starts.is_empty())
        };
        for k in 0..rows + heads {
            let (node, (top, bottom)) = {
                let st = &self.tables[slot as usize];
                match k.checked_sub(rows) {
                    // Without groups, A2's arithmetic: every layout of most tables passes here.
                    None if flat => {
                        let (node, at) = st.built[k];
                        let top = i64::from(at) * rp;
                        (node, (top, top + rp))
                    }
                    None => {
                        let (node, at) = st.built[k];
                        (node, st.geom().extent(Item::Row(at)))
                    }
                    Some(h) => {
                        let (node, g) = st.heads[h];
                        (node, st.geom().extent(Item::Head(g)))
                    }
                }
            };
            let (y0, y1) = (at_px(top - base), at_px(bottom - base));
            self.arrange(node, Rect::new(view.x, y0, width, y1 - y0), text, covered);
        }
        self.shown = outer;
        let content = Size {
            w: content_w,
            h: to_logical(content_h, self.scale),
        };
        let s = self.nodes[body as usize].scroll;
        self.scroll_laid_out(s, content, text);
    }

    /// The scale changed from `from`: slot `t`'s row and group header heights in the new pixels,
    /// and the offset `offset` (old pixels) keeping its top row or header. Returns the new
    /// offset, base and content height; 0 content if the rows aren't measured yet.
    pub(crate) fn rescale_rows(&mut self, t: u32, offset: i64, from: f32) -> (i64, i64, i64) {
        let scale = self.scale;
        let st = &self.tables[t as usize];
        if st.row_px <= 0 {
            return (to_px(to_logical(offset, from), scale), 0, 0);
        }
        let was = st.geom();
        let now = Geom {
            rp: to_px(st.row_h, scale).max(1),
            gp: to_px(st.group_h, scale).max(1),
            ..was
        };
        let out = (
            was.rescaled(&now, offset),
            now.item_top(st.base_row),
            now.height(),
        );
        let (rp, gp) = (now.rp, now.gp);
        let st = &mut self.tables[t as usize];
        (st.row_px, st.group_px) = (rp, gp);
        let grid = st.grid;
        // The rows are laid out again in the new pixels.
        if grid != NIL {
            self.mark_dirty(grid);
        }
        out
    }

    // ---- input -------------------------------------------------------------------------------

    /// Takes the messages layout made since the last call, appending them to `out`: the width a
    /// column the view asked to fit measured. The host updates its model with them and builds
    /// again.
    pub fn take_messages(&mut self, out: &mut Vec<M>) {
        out.append(&mut self.queued);
    }

    /// Windows' drag distance (`SM_CXDRAG`), in pixels at 100 %: how far the pointer moves before
    /// a press on a divider becomes a drag, so a click's or a double-click's wobble resizes
    /// nothing. 0 is taken as Windows' default, 4. The host sets it from the system's.
    pub fn set_drag_distance(&mut self, px: u32) {
        self.drag_distance = match u16::try_from(px) {
            Ok(0) => DRAG_DISTANCE,
            Ok(px) => f32::from(px),
            Err(_) => f32::from(u16::MAX),
        };
    }

    /// The table slot and shown column whose divider is under `p`: the strip
    /// [`DIVIDER`](tok::DIVIDER) wide centred on a column header's right edge (the check column
    /// has none), the nearer edge where two strips meet. Only where the header shows and nothing
    /// covers it.
    pub(crate) fn divider_at(&self, p: Point) -> Option<(u32, usize)> {
        let half = tok::DIVIDER / 2.0;
        for (t, st) in (0u32..).zip(&self.tables) {
            let header = st.header;
            let live = self
                .nodes
                .get(header as usize)
                .is_some_and(|n| n.live && n.key == TABLE_HEADER);
            if !live || !self.window_clip(header).contains(p) {
                continue;
            }
            // Under whatever covers the header (a drawer over the page), no divider.
            let mut up = self.deepest(p);
            while up != NIL && up != header {
                up = self.nodes[up as usize].parent;
            }
            if up != header {
                continue;
            }
            let mut best: Option<(usize, f32)> = None;
            let mut col = 0;
            let mut item = self.nodes[header as usize].first;
            while item != NIL {
                let node = &self.nodes[item as usize];
                if node.key != TABLE_CHECK {
                    let off = (p.x - self.window_rect(item).right()).abs();
                    if off <= half && best.is_none_or(|(_, b)| off < b) {
                        best = Some((col, off));
                    }
                    col += 1;
                }
                item = node.next;
            }
            if let Some((col, _)) = best {
                return Some((t, col));
            }
        }
        None
    }

    /// A press at `p` on a divider: its column's drag begins, the columns left of it holding
    /// their widths. Returns whether there was one.
    pub(crate) fn divider_press(&mut self, p: Point, double: bool) -> bool {
        // A drag whose release never came (the window dropped it) ends first.
        self.cancel_divider();
        let Some((t, col)) = self.divider_at(p) else {
            return false;
        };
        let st = &mut self.tables[t as usize];
        let skip = usize::from(st.checks);
        let Some(c) = st.columns.get(col).copied() else {
            return false;
        };
        let start_w = st.tracks.get(skip + col).map_or(c.min, |&(_, w)| w);
        st.frozen.clear();
        st.frozen
            .extend(st.tracks.iter().skip(skip).take(col).map(|&(_, w)| w));
        st.drag = Some((col, start_w));
        self.divider = Some(Divider {
            t,
            col,
            press_x: p.x,
            start_w,
            min: c.min,
            double,
            moved: false,
        });
        true
    }

    /// The pointer at `p` while a divider is dragged: its column follows, at least its minimum,
    /// and nothing is sent until the release. Returns whether a drag took it.
    pub(crate) fn divider_move(&mut self, p: Point) -> bool {
        let Some(mut d) = self.divider else {
            return false;
        };
        let dx = p.x - d.press_x;
        if !d.moved {
            // A click until the pointer goes the drag distance; then the edge goes to it.
            if dx.abs() < self.drag_distance {
                return true;
            }
            d.moved = true;
            self.divider = Some(d);
        }
        let w = (d.start_w + dx).max(d.min);
        let st = &mut self.tables[d.t as usize];
        match st.drag {
            // The column went (hidden, or the view rebuilt the table): the drag ends.
            None => self.divider = None,
            Some(now) if now == (d.col, w) => {}
            Some(_) => {
                st.drag = Some((d.col, w));
                let grid = st.grid;
                if grid != NIL {
                    self.mark_dirty(grid);
                }
            }
        }
        true
    }

    /// The release at `p` of a press on a divider. A drag that changed its column's width sends
    /// the view the widths the shared columns left of it held, then its own, so nothing moves
    /// when the view answers. A click (the pointer never went the drag distance) sends nothing,
    /// or asks to fit the column if it's a double-click's second press. Returns whether a press
    /// on a divider took it.
    pub(crate) fn divider_release(&mut self, p: Point, out: &mut Vec<M>) -> bool {
        let Some(d) = self.divider.take() else {
            return false;
        };
        let dx = p.x - d.press_x;
        let moved = d.moved || dx.abs() >= self.drag_distance;
        let w = if moved {
            (d.start_w + dx).max(d.min)
        } else {
            d.start_w
        };
        let st = &mut self.tables[d.t as usize];
        let live = st.drag.take().is_some();
        let grid = st.grid;
        if grid != NIL {
            self.mark_dirty(grid);
        }
        let st = &self.tables[d.t as usize];
        let (Some(on), true, Some(c)) = (st.on, live, st.columns.get(d.col)) else {
            return true;
        };
        if !moved {
            if d.double {
                out.push(on(TableEvent::Fit(Some(c.id))));
            }
            return true;
        }
        if w.to_bits() == d.start_w.to_bits() {
            return true;
        }
        for (k, left) in st.columns.iter().enumerate().take(d.col) {
            if matches!(left.width, Track::Share(_)) {
                let held = st.frozen.get(k).copied().unwrap_or(left.min);
                out.push(on(TableEvent::Resize(left.id, held)));
            }
        }
        out.push(on(TableEvent::Resize(c.id, w)));
        true
    }

    /// Ends a divider's drag with nothing sent, its column back as it was: Esc, or the window
    /// losing the pointer or going inactive. Returns whether there was one.
    pub(crate) fn cancel_divider(&mut self) -> bool {
        let Some(d) = self.divider.take() else {
            return false;
        };
        let st = &mut self.tables[d.t as usize];
        if st.drag.take().is_some() && st.grid != NIL {
            let grid = st.grid;
            self.mark_dirty(grid);
        }
        true
    }

    /// A context request on table header `header`, from node `start` (under the pointer at `at`,
    /// or focused): the view is asked for the column menu, for the column `start` is in, which
    /// anchors it. It isn't a node's context menu: [`Ui::take_context`] stays empty.
    pub(crate) fn column_menu_request(
        &mut self,
        header: u32,
        start: u32,
        at: Option<Point>,
        out: &mut Vec<M>,
    ) -> bool {
        let Some(t) = self.table_of_grid(self.nodes[header as usize].parent) else {
            return false;
        };
        let Some(on) = self.tables[t as usize].on else {
            return false;
        };
        let mut c = start;
        while c != NIL && self.nodes[c as usize].parent != header {
            c = self.nodes[c as usize].parent;
        }
        let item = (c != NIL).then_some(c);
        let col = item
            .filter(|&c| self.nodes[c as usize].key != TABLE_CHECK)
            .and_then(|c| {
                let st = &self.tables[t as usize];
                let k = self.sibling_index(c).checked_sub(usize::from(st.checks))?;
                st.columns.get(k).map(|c| c.id)
            });
        let anchor = item.unwrap_or(header);
        self.anchor_node = anchor;
        self.anchor = Some(match at {
            Some(p) => Rect::new(p.x, p.y, 0.0, 0.0),
            None => self.window_rect(anchor),
        });
        out.push(on(TableEvent::ColumnMenu(col)));
        true
    }

    /// A click released on row `i`: on its check box, with Ctrl or Shift, the checks; a
    /// double-click's second activates; else it becomes current.
    pub(crate) fn row_click(&mut self, i: u32, out: &mut Vec<M>)
    where
        M: Clone,
    {
        let Some((t, at)) = self.row_place(i) else {
            self.activate(i, out);
            return;
        };
        let Some(on) = self.tables[t as usize].on else {
            return;
        };
        let p = self.press;
        let c = self.nodes[i as usize].first;
        let on_box = c != NIL && {
            let cell = &self.nodes[c as usize];
            cell.key == TABLE_CHECK && cell.enabled && self.window_rect(c).contains(p.at)
        };
        let checkable = self.row_checkable(t, at);
        out.push(on(if on_box {
            TableEvent::Check(at, Pick::Toggle)
        } else if p.double {
            TableEvent::Activate(at)
        } else if p.mods.ctrl && checkable {
            TableEvent::Check(at, Pick::Toggle)
        } else if p.mods.shift && self.tables[t as usize].checks {
            // Onto a locked row too: the anchor stays, and the model skips the row.
            TableEvent::Check(at, Pick::Extend)
        } else {
            TableEvent::Current(at)
        }));
    }

    /// A key on a focused row: the moves, Space, Enter, Ctrl+A, and Left and Right, which
    /// scroll across. `None` if focus isn't on a table's row, or the key isn't the table's.
    pub(crate) fn table_key(&mut self, k: KeyEvent, out: &mut Vec<M>) -> Option<bool> {
        let f = self.focus;
        let (t, at) = self.row_place(f)?;
        if k.mods.alt || k.mods.win {
            return None;
        }
        let on = self.tables[t as usize].on?;
        let (shift, ctrl) = (k.mods.shift, k.mods.ctrl);
        if !k.down {
            // Space's release, after the press that checked.
            if k.key == KeyCode::Space {
                if self.key_pressed == f {
                    self.key_pressed = NIL;
                }
                return Some(true);
            }
            return None;
        }
        let st = &self.tables[t as usize];
        let last = st.rows.saturating_sub(1);
        // A page is the view's rows less one, as pixels down the rows: the group headers in
        // between count (A3.5 (10)). Without groups, that many rows.
        let geom = st.geom();
        let page = (st.view_px / geom.rp.max(1) - 1).max(1) * geom.rp.max(1);
        let (body, checks) = (st.body, st.checks);
        let to = match k.key {
            KeyCode::Up if !ctrl => at.saturating_sub(1),
            KeyCode::Down if !ctrl => at.saturating_add(1).min(last),
            KeyCode::PageUp if !ctrl => geom
                .row_from(geom.row_top(at) - page)
                .min(at.saturating_sub(1)),
            KeyCode::PageDown if !ctrl => geom
                .row_by(geom.row_top(at) + page)
                .max(at.saturating_add(1))
                .min(last),
            KeyCode::Home => 0,
            KeyCode::End => last,
            KeyCode::Space => {
                self.show_focus_ring();
                // A held key's repeats check nothing more.
                if self.key_pressed != f {
                    self.key_pressed = f;
                    if self.row_checkable(t, at) {
                        let pick = if shift { Pick::Extend } else { Pick::Toggle };
                        out.push(on(TableEvent::Check(at, pick)));
                    }
                }
                return Some(true);
            }
            KeyCode::Enter => {
                self.show_focus_ring();
                out.push(on(TableEvent::Activate(at)));
                return Some(true);
            }
            KeyCode::Other(0x41) if ctrl && !shift => {
                if !checks {
                    return None;
                }
                out.push(on(TableEvent::CheckAll));
                return Some(true);
            }
            KeyCode::Left | KeyCode::Right if !ctrl => {
                self.show_focus_ring();
                let s = self.nodes[body as usize].scroll;
                if s != NIL {
                    let line = self.line_px(s, 0);
                    let by = if k.key == KeyCode::Left { -line } else { line };
                    self.scroll_by(s, 0, by);
                }
                return Some(true);
            }
            _ => return None,
        };
        self.show_focus_ring();
        if to != at {
            // Shift extends onto a locked row too, so the anchor stays; the model skips the row.
            out.push(on(if shift && self.tables[t as usize].checks {
                TableEvent::Check(to, Pick::Extend)
            } else {
                TableEvent::Current(to)
            }));
        }
        Some(true)
    }

    /// A client's action on a table's part: `Click` on a check cell checks its row, and `Focus` on
    /// a row makes it current. `None` for the rest, which go the usual way.
    pub(crate) fn table_action(
        &mut self,
        i: u32,
        action: Action,
        out: &mut Vec<M>,
    ) -> Option<bool> {
        let n = &self.nodes[i as usize];
        match (n.role, action) {
            (Role::Cell, Action::Click) if n.key == TABLE_CHECK => {
                let enabled = n.enabled;
                let (t, at) = self.row_place(n.parent)?;
                let on = self.tables[t as usize].on?;
                if enabled {
                    out.push(on(TableEvent::Check(at, Pick::Toggle)));
                }
                Some(enabled)
            }
            (Role::Row, Action::Focus) if n.enabled => {
                let (t, at) = self.row_place(i)?;
                self.move_focus(i);
                self.show_focus_ring();
                let st = &self.tables[t as usize];
                if st.current != Some(at)
                    && let Some(on) = st.on
                {
                    out.push(on(TableEvent::Current(at)));
                }
                Some(true)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(rows: u32) -> Geom<'static> {
        Geom {
            rows,
            rp: 36,
            gp: 32,
            starts: &[],
        }
    }

    #[test]
    fn the_range_is_the_view_and_half_a_view_either_side() {
        // 364 px of 36 px rows: 0 to 546 px is rows 0 to 15.
        let rows = |g: Geom<'_>, off, view| g.span_for(off, view).rows;
        assert_eq!(rows(flat(1_000), 0, 364), (0, 16));
        assert_eq!(rows(flat(1_000), 324, 364), (3, 25));
        assert_eq!(rows(flat(10), 0, 364), (0, 10));
        assert_eq!(rows(flat(1_000), 0, 0), (0, 0), "no view yet");
        assert_eq!(rows(flat(0), 0, 364), (0, 0));
        assert_eq!(rows(flat(5), i64::MAX, 364), (5, 5));
        // Its arithmetic without groups is the search's with them.
        for (n, off, view) in [
            (1_000, 0, 364),
            (1_000, 324, 364),
            (10, 0, 364),
            (5, 400, 90),
        ] {
            let g = flat(n);
            assert_eq!(
                g.span_for(off, view),
                g.span(off - view / 2, off + view + view / 2),
                "{n} rows at {off}"
            );
        }
    }

    const STARTS: [u32; 4] = [0, 3, 10, 25];

    fn grouped() -> Geom<'static> {
        Geom {
            rows: 40,
            rp: 52,
            gp: 32,
            starts: &STARTS,
        }
    }

    #[test]
    fn grouped_rows_are_found_at_their_heights_and_spanned_with_their_headers() {
        let g = grouped();
        assert_eq!(g.height(), 40 * 52 + 4 * 32);
        assert_eq!(
            (g.at(0), g.at(31), g.at(32)),
            (Item::Head(0), Item::Head(0), Item::Row(0))
        );
        // Group 1's header under rows 0 to 2: 32 + 156.
        assert_eq!(g.head_top(1), 188);
        assert_eq!((g.at(219), g.at(220)), (Item::Head(1), Item::Row(3)));
        assert_eq!(g.row_top(3), 220);
        assert_eq!(g.item_top(3), 188, "row 3 starts group 1");
        assert_eq!(g.at(g.height() - 1), Item::Row(39));
        // Row 9 (532) starts before 546; group 2's header (584) doesn't.
        let span = g.span_for(0, 364);
        assert_eq!((span.rows, span.heads), ((0, 10), (0, 2)));
        // From 400 less half a view, 218, takes in the end of group 1's header; to 946, row 16
        // (928).
        let span = g.span_for(400, 364);
        assert_eq!((span.rows, span.heads), ((3, 17), (1, 3)));
        // Half a view above the end: rows 36 to 39.
        assert_eq!(g.span_for(g.height(), 364).rows, (36, 40));
        let past = g.span_for(i64::MAX, 364);
        assert_eq!((past.rows, past.heads), ((40, 40), (4, 4)));
    }

    #[test]
    fn grid_indices_count_the_headers_both_ways() {
        let mut k = 0;
        let mut g = 0u32;
        for r in 0..40 {
            while STARTS.get(g as usize).is_some_and(|&s| s <= r) {
                assert_eq!(item_of_grid(&STARTS, 40, k), Some(Item::Head(g)));
                assert_eq!(head_grid_of(&STARTS, g), k);
                k += 1;
                g += 1;
            }
            assert_eq!(grid_of(&STARTS, r), k);
            assert_eq!(item_of_grid(&STARTS, 40, k), Some(Item::Row(r)));
            k += 1;
        }
        assert_eq!(item_of_grid(&STARTS, 40, 44), None);
        assert_eq!(item_of_grid(&[], 40, 7), Some(Item::Row(7)));
        assert_eq!(grid_of(&[4], 3), 3, "before the first group");
    }

    #[test]
    fn groups_are_kept_while_in_order_and_within_the_rows() {
        let group = |start| RowGroup {
            start,
            label: "",
            count: "",
            name: "",
        };
        assert_eq!(valid_groups(&[group(0), group(3), group(9)], 10), 3);
        assert_eq!(valid_groups(&[group(3), group(2), group(5)], 10), 1);
        assert_eq!(valid_groups(&[group(3), group(3)], 10), 1, "empty");
        assert_eq!(valid_groups(&[group(0), group(10)], 10), 1, "past the rows");
    }

    #[test]
    fn a_new_height_keeps_the_row_at_the_top() {
        let g = grouped();
        let big = Geom {
            rp: 83,
            gp: 46,
            ..g
        };
        // Row 12's top, then 13 px into it, 13 × 83 / 52 in the new.
        assert_eq!(g.rescaled(&big, g.row_top(12)), big.row_top(12));
        assert_eq!(
            g.rescaled(&big, g.row_top(12) + 13),
            big.row_top(12) + 13 * 83 / 52
        );
        assert_eq!(g.rescaled(&big, g.head_top(2) + 16), big.head_top(2) + 23);
        // Without groups, A2's arithmetic.
        let (a, b) = (
            flat(100),
            Geom {
                rp: 45,
                ..flat(100)
            },
        );
        assert_eq!(
            a.rescaled(&b, 1_000),
            1_000 / 36 * 45 + 1_000 % 36 * 45 / 36
        );
    }

    #[test]
    fn a_header_s_sort_round_trips_through_its_value() {
        for order in [None, Some(Order::Ascending), Some(Order::Descending)] {
            assert_eq!(sort_of(sort_value(order)), order);
        }
    }
}
