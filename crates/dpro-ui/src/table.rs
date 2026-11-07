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

/// A data table, as a view describes it for [`Cx::table`].
#[derive(Clone, Debug)]
pub struct Table<'s, M> {
    /// How many rows the view has: indices `0..rows`, in the order shown.
    pub rows: u32,
    pub columns: &'s [Column<'s>],
    /// Each column's width as the user left it, in `columns`' order; a column past its end has
    /// its own ([`Width::Auto`]).
    pub widths: &'s [Width],
    /// Whether each column is hidden, in `columns`' order; a column past its end shows.
    pub hidden: &'s [bool],
    /// The column the rows are sorted by, and which way.
    pub sorted: Option<(ColumnId, Order)>,
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
    values: &'b mut ValueCache,
    formats: &'b dyn Formats,
}

impl Cells<'_> {
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

    /// A value's text, kept from an earlier build or formatted now.
    #[inline]
    fn value(&mut self, v: Value) {
        self.values.push(v, self.formats, self.text);
        self.ends.push(self.text.len());
    }
}

impl fmt::Debug for Cells<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cells")
            .field("text", &self.text)
            .field("ends", &self.ends)
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
    /// The range the last build made: `from..to`.
    range: (u32, u32),
    current: Option<u32>,
    current_key: Option<Key>,
    /// A row to scroll into view at the next build, once the view is known.
    reveal: Option<u32>,
    /// The text of the values its cells showed.
    values: ValueCache,
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
            tracks: Vec::new(),
            content_w: 0.0,
            row_px: 0,
            row_h: 0.0,
            built_px: 0,
            view_px: 0,
            base_row: 0,
            built: Vec::new(),
            scratch: Vec::new(),
            range: (0, 0),
            current: None,
            current_key: None,
            reveal: None,
            values: ValueCache::default(),
        }
    }
}

/// What a build makes: the rows `from..to`, and `extra`, the current row, out of that range.
#[derive(Clone, Copy, Debug)]
struct Plan {
    from: u32,
    to: u32,
    extra: Option<u32>,
    /// Focus goes to the current row once it's built.
    follow: bool,
    /// Focus was on one of the rows: if the build drops that row, focus stays in the table.
    focused: bool,
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

/// The rows to build for a view of `view` px at offset `off`, with rows of `rp` px: those in
/// view, and half a view either side.
fn range_of(rows: u32, off: i64, view: i64, rp: i64) -> (u32, u32) {
    if rows == 0 || view <= 0 || rp <= 0 {
        return (0, 0);
    }
    let start = off.saturating_sub(view / 2).max(0);
    let end = off.saturating_add(view).saturating_add(view / 2);
    let row = |px: i64| u32::try_from(px).unwrap_or(u32::MAX).min(rows);
    let to = row(end.saturating_add(rp - 1) / rp);
    (row(start / rp).min(to), to)
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
        let mut text = mem::take(&mut cx.ui.cell_text);
        let mut ends = mem::take(&mut cx.ui.cell_ends);
        text.clear();
        ends.clear();
        // The table's kept text, and the window's formats for what it hasn't kept.
        let mut values = if t == NIL {
            ValueCache::default()
        } else {
            mem::take(&mut cx.ui.tables[t as usize].values)
        };
        values.begin(cx.ui.formats_generation);
        let formats = Rc::clone(&cx.ui.formats.0);
        // The current row first: whether its key changed says whether it's another row.
        let current = table.current.filter(|&c| c < table.rows);
        let cur = current.map(|c| {
            row(
                c,
                &mut Cells {
                    text: &mut text,
                    ends: &mut ends,
                    values: &mut values,
                    formats: &*formats,
                },
            )
        });
        let (kept_text, kept_ends) = (text.len(), ends.len());
        let shown = Shown::of(table);
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
                for at in before.into_iter().chain(plan.from..plan.to).chain(after) {
                    // Each row's cells end where its own pushes did: the current row's before
                    // `kept_ends`, so a row built after it can't lend it cells it didn't give.
                    let (r, cell_ends, start) = if let Some(r) = cur
                        && current == Some(at)
                    {
                        (r, 0..kept_ends, 0)
                    } else {
                        text.truncate(kept_text);
                        ends.truncate(kept_ends);
                        let r = row(
                            at,
                            &mut Cells {
                                text: &mut text,
                                ends: &mut ends,
                                values: &mut values,
                                formats: &*formats,
                            },
                        );
                        (r, kept_ends..ends.len(), kept_text)
                    };
                    let cells = CellText {
                        text: &text,
                        ends: &ends[cell_ends],
                        start,
                    };
                    let node = bx.table_row(table, shown, at, current == Some(at), r, &cells);
                    bx.ui.tables[t as usize].scratch.push((node, at));
                }
            } else if let Some(status) = &table.status {
                bx.status(status);
            }
            bx.finish();
        }
        text.clear();
        ends.clear();
        cx.ui.cell_text = text;
        cx.ui.cell_ends = ends;
        if t != NIL {
            cx.ui.tables[t as usize].values = values;
        }
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
        for (j, col) in table.columns.iter().enumerate() {
            if shown.any_hidden && is_hidden(table, j) {
                continue;
            }
            rx.add(Element {
                style: cell_style(col.align),
                text_style: TextStyle {
                    weight,
                    mono: col.mono,
                    ..TextStyle::default()
                },
                ..Element::new(
                    Key::index(u64::from(col.id.0)),
                    Role::Cell,
                    cells.get(j),
                    None,
                )
            });
            weight = font::REGULAR;
        }
        rx.finish();
        i
    }
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
/// at `start`.
struct CellText<'b> {
    text: &'b str,
    ends: &'b [usize],
    start: usize,
}

impl CellText<'_> {
    /// Cell `j`'s text; empty if the view gave fewer.
    fn get(&self, j: usize) -> &str {
        let Some(&end) = self.ends.get(j) else {
            return "";
        };
        let from = match j.checked_sub(1) {
            Some(k) => self.ends[k],
            None => self.start,
        };
        self.text.get(from..end).unwrap_or("")
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
            if (st.reveal.is_some() && st.view_px > 0) || st.row_px != st.built_px {
                return true;
            }
            let off = self.scrolls[s as usize].offset[1];
            st.range != range_of(st.rows, off, st.view_px, st.row_px)
        })
    }

    /// Scrolls row `row` (an index in the view's order) of `table` into view and builds it, at
    /// the next build ([`wants_build`](Self::wants_build) says so): a search result, or a client
    /// asking for a row it can't see. Returns whether `table` is a table with that row.
    pub fn realize(&mut self, table: NodeId, row: u32) -> bool {
        let Some(t) = self.index(table).and_then(|i| self.table_of_grid(i)) else {
            return false;
        };
        let st = &mut self.tables[t as usize];
        if row >= st.rows {
            return false;
        }
        st.reveal = Some(row);
        true
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
        fresh.tracks = mem::take(&mut st.tracks);
        fresh.tracks.clear();
        fresh.built = mem::take(&mut st.built);
        fresh.built.clear();
        fresh.scratch = mem::take(&mut st.scratch);
        fresh.scratch.clear();
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
        let guess = {
            let size = self.nodes[body as usize].text_style.size;
            to_px(
                (size * LINE_GUESS + 2.0 * tok::ROW_PAD_Y).max(tok::ROW),
                self.scale,
            )
            .max(1)
        };
        let st = &mut self.tables[t as usize];
        st.current = current;
        st.scratch.clear();
        let mut follow = false;
        if st.current_key != key {
            st.current_key = key;
            if let Some(c) = current {
                st.reveal = Some(c);
                follow = focus_here;
            }
        }
        let rp = if st.row_px > 0 { st.row_px } else { guess };
        st.built_px = rp;
        let view = st.view_px;
        // The content is every row, exactly.
        if rows > 0 {
            let content = i64::from(rows).saturating_mul(rp);
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
            && let Some(r) = self.tables[t as usize].reveal.take()
            && r < rows
        {
            let top = i64::from(r) * rp;
            let off = self.scrolls[s as usize].offset[1];
            let to = if top < off || rp > view {
                top
            } else if top + rp > off + view {
                top + rp - view
            } else {
                off
            };
            self.scroll_axis(s, 1, to);
        }
        let off = self.scrolls[s as usize].offset[1];
        let (from, to) = range_of(rows, off, view, rp);
        let base_row = from / BASE_ROWS * BASE_ROWS;
        let base_px = i64::from(base_row) * rp;
        let st = &mut self.tables[t as usize];
        st.range = (from, to);
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
            for k in 0..self.tables[t as usize].built.len() {
                let (node, _) = self.tables[t as usize].built[k];
                self.a11y_touch(node);
                self.a11y_touch_children(node, 0);
            }
        }
        Plan {
            from,
            to,
            extra: current.filter(|&c| c < from || c >= to),
            follow,
            focused: focus_here,
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
        let changed = st.scratch != st.built;
        mem::swap(&mut st.built, &mut st.scratch);
        st.scratch.clear();
        let body = st.body;
        if changed {
            self.mark_dirty(body);
        }
        // A focused row the build dropped (scrolled away from, or gone with a refresh) took focus
        // with it: it comes back to the current row, else the first row in view, so the keyboard
        // and a client's cursor stay in the table.
        let dropped = plan.focused && self.focus == NIL;
        if plan.follow || dropped {
            let to = self.current_row_node(t).or_else(|| {
                let at = self.first_row_in_view(t)?;
                self.built_node(t, at)
            });
            if let Some(node) = to {
                self.focus_node(node);
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
        let (rows, rp, view) = (st.rows, st.built_px, st.view_px);
        if rows == 0 || rp <= 0 || view <= 0 {
            return None;
        }
        let s = self.nodes[st.body as usize].scroll;
        let off = self.scrolls[s as usize].offset[1];
        let whole = off.saturating_add(rp - 1) / rp;
        let at = if whole.saturating_add(1).saturating_mul(rp) <= off.saturating_add(view) {
            whole
        } else {
            off / rp
        };
        Some(u32::try_from(at).unwrap_or(u32::MAX).min(rows - 1))
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

    /// Whether node `i` is a table's header.
    pub(crate) fn is_table_header(&self, i: u32) -> bool {
        let n = &self.nodes[i as usize];
        n.key == TABLE_HEADER
            && self
                .nodes
                .get(n.parent as usize)
                .is_some_and(|p| p.role == Role::Grid)
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
        let rp = if st.row_px > 0 {
            st.row_px
        } else {
            st.built_px
        };
        if rp <= 0 {
            return None;
        }
        let body = self.nodes[st.body as usize].rect.snap(self.scale);
        let r = self.nodes[i as usize].rect.snap(self.scale);
        let y0 = i64::from(body.y0) + (i64::from(at) - i64::from(st.base_row)) * rp;
        Some([i64::from(r.x0), y0, i64::from(r.x1), y0 + rp])
    }

    // ---- layout ------------------------------------------------------------------------------

    /// Table `t`'s row height, logical and in whole physical pixels: its body text's line and the
    /// row padding above and below, at least the row token.
    fn row_height(&self, t: u32, text: &mut dyn TextSystem) -> (f32, i64) {
        let body = self.tables[t as usize].body;
        let style = self.nodes[body as usize].text_style;
        let line = text.measure("", &style, f32::INFINITY).height;
        let row_h = (line + 2.0 * tok::ROW_PAD_Y).max(tok::ROW);
        (row_h, to_px(row_h, self.scale).max(1))
    }

    /// A grid's natural size: its columns' least widths, and a row for the header and each row
    /// (or the status line). `None` if it isn't a table.
    pub(crate) fn grid_natural(&self, i: u32, text: &mut dyn TextSystem) -> Option<Size> {
        let t = self.table_of_grid(i)?;
        // Measured now, not the last layout's: after a text size change, the grid is measured
        // before it's arranged.
        let (_, rp) = self.row_height(t, text);
        let st = &self.tables[t as usize];
        let row = to_logical(rp, self.scale);
        let w = if st.checks { tok::CHECK_COL } else { 0.0 }
            + st.columns
                .iter()
                .map(|c| match c.width {
                    Track::Fixed(w) => w.max(c.min),
                    Track::Share(_) => c.min,
                })
                .sum::<f32>();
        #[allow(
            clippy::cast_precision_loss,
            reason = "a natural height; past 2^24 rows it's huge either way"
        )]
        let h = row * (st.rows.max(1) as f32 + 1.0);
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

    /// The grid: the row height from the rows' text, the columns' tracks, the header a row high
    /// and the rows' viewport under it.
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
        let (row_h, rp) = self.row_height(t, text);
        self.tables[t as usize].row_h = row_h;
        let old = self.tables[t as usize].row_px;
        if rp != old {
            self.rows_resized(t, old, rp);
        }
        let row = to_logical(rp, scale);
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
        if s == NIL || st.view_px <= 0 || st.built_px != st.row_px || st.reveal.is_some() {
            return false;
        }
        let off = self.scrolls[s as usize].offset[1];
        st.range == range_of(st.rows, off, st.view_px, st.row_px)
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
                if let Some(cell) = self.nth_child(row, skip + k) {
                    w = w.max(natural(&self.nodes[cell as usize], text));
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

    /// The row height became `new` pixels from `old` (a text size, measured for the first time):
    /// the row at the top of the view stays there, as far into it as it was.
    fn rows_resized(&mut self, t: u32, old: i64, new: i64) {
        let st = &mut self.tables[t as usize];
        st.row_px = new;
        let (body, base_row) = (st.body, st.base_row);
        let s = self.nodes[body as usize].scroll;
        if s == NIL {
            return;
        }
        if old > 0 {
            let sc = &mut self.scrolls[s as usize];
            let off = sc.offset[1];
            sc.offset[1] = off / old * new + off % old * new / old;
            self.damage_view(body);
            self.a11y_touch(body);
            self.touch_grid(s);
            self.sync_follower(s);
        }
        self.scrolls[s as usize].base[1] = i64::from(base_row) * new;
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

    /// The rows: each at its index times the row height from the base, as wide as the columns or
    /// the view, whichever is wider.
    fn arrange_rows(&mut self, body: u32, slot: u32, text: &mut dyn TextSystem, covered: bool) {
        let (outer, _) = self.enter_viewport(body, Axis::Column);
        let view = self.nodes[body as usize].rect;
        let scale = f64::from(self.scale);
        let (row_px, base, content_w) = {
            let st = &self.tables[slot as usize];
            let row_px = if st.row_px > 0 {
                st.row_px
            } else {
                st.built_px
            };
            (row_px, i64::from(st.base_row), st.content_w)
        };
        let width = content_w.max(view.w);
        // In f64 from the base, then once to f32: exact for a chunk of rows at any depth.
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "within a chunk of rows, f64 is exact and f32 keeps whole pixels"
        )]
        let at_px = |px: i64| (f64::from(view.y) + px as f64 / scale) as f32;
        for k in 0..self.tables[slot as usize].built.len() {
            let (node, at) = self.tables[slot as usize].built[k];
            let top = (i64::from(at) - base) * row_px;
            let (y0, y1) = (at_px(top), at_px(top + row_px));
            self.arrange(node, Rect::new(view.x, y0, width, y1 - y0), text, covered);
        }
        self.shown = outer;
        let rows = i64::from(self.tables[slot as usize].rows);
        let content = Size {
            w: content_w,
            h: to_logical(rows.saturating_mul(row_px), self.scale),
        };
        let s = self.nodes[body as usize].scroll;
        self.scroll_laid_out(s, content, text);
    }

    /// The scale changed from `from`: slot `t`'s row height in the new pixels, and the offset
    /// `offset` (old pixels) keeping its top row. Returns the new offset, base and content
    /// height; 0 content if the rows aren't measured yet.
    pub(crate) fn rescale_rows(&mut self, t: u32, offset: i64, from: f32) -> (i64, i64, i64) {
        let scale = self.scale;
        let st = &mut self.tables[t as usize];
        let old = st.row_px;
        if old <= 0 {
            return (to_px(to_logical(offset, from), scale), 0, 0);
        }
        let new = to_px(st.row_h, scale).max(1);
        st.row_px = new;
        let y = offset / old * new + offset % old * new / old;
        let grid = st.grid;
        let out = (
            y,
            i64::from(st.base_row) * new,
            i64::from(st.rows).saturating_mul(new),
        );
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
        let page = u32::try_from((st.view_px / st.row_px.max(1) - 1).max(1)).unwrap_or(1);
        let (body, checks) = (st.body, st.checks);
        let to = match k.key {
            KeyCode::Up if !ctrl => at.saturating_sub(1),
            KeyCode::Down if !ctrl => at.saturating_add(1).min(last),
            KeyCode::PageUp if !ctrl => at.saturating_sub(page),
            KeyCode::PageDown if !ctrl => at.saturating_add(page).min(last),
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

    #[test]
    fn the_range_is_the_view_and_half_a_view_either_side() {
        // 364 px of 36 px rows: 0 to 546 px is rows 0 to 15.
        assert_eq!(range_of(1_000, 0, 364, 36), (0, 16));
        assert_eq!(range_of(1_000, 324, 364, 36), (3, 25));
        assert_eq!(range_of(10, 0, 364, 36), (0, 10));
        assert_eq!(range_of(1_000, 0, 0, 36), (0, 0), "no view yet");
        assert_eq!(range_of(0, 0, 364, 36), (0, 0));
        assert_eq!(range_of(5, i64::MAX, 364, 36), (5, 5));
    }

    #[test]
    fn a_header_s_sort_round_trips_through_its_value() {
        for order in [None, Some(Order::Ascending), Some(Order::Descending)] {
            assert_eq!(sort_of(sort_value(order)), order);
        }
    }
}
