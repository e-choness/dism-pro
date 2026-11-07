//! TD-0009 A2 slice 5b1: the data table, written before the code (A2.4, A2.5, A2.9).
//!
//! - [`Cx::table`] builds a grid: a header row of column headers, and a scroll viewport of rows.
//!   Rows have one height, so only the rows in view, half a view either side and the current row
//!   are built: a 100k-row table has as many nodes as a 100-row one. Row `i` is at exactly
//!   `i × row height` physical pixels, at any depth.
//! - The row height is the body text's line and the row padding, at least the row token, whole
//!   physical pixels; the header is as high. Until a layout has measured it and the rows' view, a
//!   build guesses, and [`Ui::wants_build`] asks for a second build.
//! - Columns are tracks: fixed widths, then shares of what's left, each at least its minimum.
//!   Past the view, the rows scroll sideways and the header follows.
//! - The header is one tab stop: Left, Right, Home and End move between column headers, and
//!   Enter, Space or a click sorts by a sortable one. The rows are another: Tab lands on the
//!   current row; the arrows, Page Up, Page Down, Home and End move it; Space checks it, Shift
//!   extends the checks from the anchor, Ctrl+A checks every row; Enter and a double-click
//!   activate. A click makes a row current; a click on its check box, Ctrl+click and Shift+click
//!   change the checks. A row without a check box takes the moves only.
//! - Focus follows the current row, and a row that becomes current is scrolled into view; a sort
//!   that moves the current row doesn't scroll. [`Ui::realize`] builds and reveals any row.
//! - A table with no rows shows the view's status line, and its action.
//! - A locked row has no check box; its tooltip says why.
//! - Accessibility: a grid with its row and column counts and the rows' scroll offsets; a row
//!   group of column headers (sortable ones take Click and give their order); rows with their
//!   index, the current one selected; cells with their row and column, the first text column's
//!   as row headers, check cells toggled.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers: a node that isn't there, or a table that doesn't settle, fails the test"
)]

use dpro_app::table::{RowMap, RowSource, TableModel};
use dpro_ui::{
    Align, Cells, Column, ColumnId, Cx, Dim, Element, FakeText, Input, Insets, Key, KeyCode,
    KeyEvent, Mods, Motion, NodeId, Order, Pick, Point, Rect, Role, Row, RowKey, Size, Status,
    Style, TABLE_BODY, TABLE_CHECK, TABLE_HEADER, Table, TableEvent, TextMetrics, TextStyle,
    TextSystem, Track, Tri, Ui,
    accesskit::{
        self, Action, ActionRequest, NodeId as AkId, Role as AkRole, SortDirection, Toggled, TreeId,
    },
};

const NAME: ColumnId = ColumnId(0);
const VERSION: ColumnId = ColumnId(1);
const SIZE: ColumnId = ColumnId(2);

const COLUMNS: [Column<'static>; 3] = [
    Column {
        id: NAME,
        header: "Name",
        width: Track::Share(1.0),
        min: 120.0,
        align: Align::Start,
        mono: false,
        sort: Some(Order::Ascending),
    },
    Column {
        id: VERSION,
        header: "Version",
        width: Track::Fixed(80.0),
        min: 60.0,
        align: Align::Start,
        mono: true,
        sort: None,
    },
    Column {
        id: SIZE,
        header: "Size",
        width: Track::Fixed(90.0),
        min: 60.0,
        align: Align::End,
        mono: false,
        sort: Some(Order::Descending),
    },
];

/// 600 × 400 at scale 1: the header is a row high, and the rows' view the 364 px under it.
const WINDOW: Size = Size { w: 600.0, h: 400.0 };
/// A row: 13 px text in `FakeText`'s 16.25 px lines, 9 px above and below, make 34.25, under
/// the 36 px row token (A2.10 item 2).
const ROW: f32 = 36.0;
const VIEW: f32 = WINDOW.h - ROW;
/// The check column (A2.10 item 2).
const CHECK: f32 = 28.0;
const TABLE: Key = Key::name("table");

#[derive(Clone, Copy, Debug, PartialEq)]
enum Msg {
    Table(TableEvent),
    ClearFilter,
}

/// Rows keyed by their number.
struct Rows(u32);

impl RowSource for Rows {
    fn len(&self) -> u32 {
        self.0
    }
    fn key(&self, row: u32) -> RowKey {
        RowKey(u64::from(row))
    }
    fn cell(&self, _: u32, _: ColumnId) -> dpro_app::table::Cell<'_> {
        dpro_app::table::Cell::Empty
    }
}

/// A page with a table of `rows` programs: "Program 00042", a version, a size.
struct Page {
    model: TableModel,
    names: Vec<String>,
    versions: Vec<String>,
    sizes: Vec<u32>,
    size_text: Vec<String>,
    checks: bool,
    /// A source row that can't be checked.
    locked: Option<u32>,
    /// A source row the view gives no size, so its last cell is empty.
    no_size: Option<u32>,
    /// The empty table's line, with Clear filter.
    status: Option<&'static str>,
    columns: &'static [Column<'static>],
    /// The table's height: the window's, or [`Dim::Auto`] for its rows'.
    height: Dim,
    /// The table's padding left and right.
    pad: f32,
}

impl Page {
    fn new(rows: u32) -> Self {
        let sizes: Vec<u32> = (0..rows).map(|r| (r * 37) % 1000).collect();
        Self {
            model: TableModel::with_keys(RowMap::build(&Rows(rows))),
            names: (0..rows).map(|r| format!("Program {r:05}")).collect(),
            versions: (0..rows).map(|r| format!("{}.{}", r % 10, r % 7)).collect(),
            size_text: sizes.iter().map(|s| format!("{s} MB")).collect(),
            sizes,
            checks: true,
            locked: None,
            no_size: None,
            status: None,
            columns: &COLUMNS,
            height: Dim::Fixed(WINDOW.h),
            pad: 0.0,
        }
    }

    fn update(&mut self, msg: Msg) {
        let Msg::Table(e) = msg else {
            self.status = None;
            return;
        };
        match e {
            TableEvent::Current(at) => self.model.set_current(at),
            TableEvent::Check(at, Pick::Extend) => self.model.extend_to(at),
            TableEvent::Check(at, how) => {
                self.model.pick(at, how);
                self.model.set_current(at);
            }
            TableEvent::CheckAll => self.model.select_all(),
            TableEvent::Activate(_) => {}
            TableEvent::Sort(col) => self.sort(col),
        }
    }

    /// The column's first order, or the other one if it's sorted already. Sizes sort by size,
    /// the rest keep the rows' order.
    fn sort(&mut self, col: ColumnId) {
        let first = COLUMNS
            .iter()
            .find(|c| c.id == col)
            .and_then(|c| c.sort)
            .expect("a sortable column");
        let order = match self.model.sort() {
            Some((c, Order::Ascending)) if c == col => Order::Descending,
            Some((c, Order::Descending)) if c == col => Order::Ascending,
            _ => first,
        };
        let mut rows: Vec<u32> = (0..self.model.rows()).collect();
        if col == SIZE {
            rows.sort_by_key(|&r| (self.sizes[r as usize], r));
        }
        if order == Order::Descending {
            rows.reverse();
        }
        let g = self.model.begin_sort();
        assert!(self.model.finish_sort(g, (col, order), rows));
    }

    fn all_checked(&self) -> Tri {
        match self.model.selected_count() {
            0 => Tri::Off,
            n if n == self.model.rows() => Tri::On,
            _ => Tri::Mixed,
        }
    }

    fn view(&self, cx: &mut Cx<'_, Msg>) {
        let table = Table {
            rows: u32::try_from(self.model.view().len()).expect("rows"),
            columns: self.columns,
            sorted: self.model.sort(),
            checks: self.checks.then(|| ("Select", self.all_checked())),
            current: self.model.current_view(),
            status: self.status.map(|text| Status {
                text,
                action: Some(("Clear filter", Msg::ClearFilter)),
            }),
            context: true,
            on: Msg::Table,
        };
        cx.table(
            Element::table(TABLE)
                .style(Style {
                    height: self.height,
                    padding: Insets::xy(self.pad, 0.0),
                    ..Style::default()
                })
                .name("Programs"),
            &table,
            |at, cells: &mut Cells<'_>| {
                let r = self.model.view()[at as usize];
                let i = r as usize;
                cells.push(&self.names[i]);
                cells.push(&self.versions[i]);
                if self.no_size != Some(r) {
                    cells.push(&self.size_text[i]);
                }
                let locked = self.locked == Some(r);
                Row {
                    key: RowKey(u64::from(r)),
                    check: (!locked).then(|| {
                        if self.model.is_selected(r) {
                            Tri::On
                        } else {
                            Tri::Off
                        }
                    }),
                    locked: if locked { "A system component" } else { "" },
                }
            },
        );
    }
}

/// A Ui and its page, framed as the host frames them.
struct Run {
    ui: Ui<Msg>,
    page: Page,
}

impl Run {
    fn new(rows: u32) -> Self {
        Self::with(Page::new(rows))
    }

    fn with(page: Page) -> Self {
        Self::scaled(page, 1.0, 1.0)
    }

    fn scaled(page: Page, scale: f32, text_scale: f32) -> Self {
        let mut run = Self {
            ui: Ui::new(WINDOW, scale, Motion::REDUCED),
            page,
        };
        run.ui.set_text_scale(text_scale);
        run.ui.set_active(true);
        run.frame();
        run
    }

    /// The view built and laid out, and again if the table asks (it guessed its rows' view).
    fn frame(&mut self) {
        for _ in 0..2 {
            let page = &self.page;
            self.ui.build(|cx| page.view(cx));
            self.ui.layout(&mut FakeText);
            if !self.ui.wants_build() {
                return;
            }
        }
        panic!("a second build settles the table");
    }

    /// `inputs`, their messages applied to the page, then a frame. Returns the messages.
    fn send_all(&mut self, inputs: &[Input]) -> Vec<Msg> {
        let mut out = Vec::new();
        for &input in inputs {
            self.ui.dispatch(input, &mut out);
        }
        for m in &out {
            self.page.update(*m);
        }
        self.frame();
        out
    }

    fn send(&mut self, input: Input) -> Vec<Msg> {
        self.send_all(&[input])
    }

    /// A key pressed and released.
    fn key(&mut self, key: KeyCode, mods: Mods) -> Vec<Msg> {
        let k = |down| Input::Key(KeyEvent { key, down, mods });
        self.send_all(&[k(true), k(false)])
    }

    fn click(&mut self, at: Point, mods: Mods) -> Vec<Msg> {
        self.send_all(&[
            Input::Press {
                at,
                mods,
                double: false,
            },
            Input::PointerUp(at),
        ])
    }

    fn table(&self) -> NodeId {
        self.ui.find(&[TABLE]).expect("the table")
    }

    fn body(&self) -> NodeId {
        self.ui.find(&[TABLE, TABLE_BODY]).expect("the rows' view")
    }

    /// Row `r` (its key), if it's built.
    fn row(&self, r: u64) -> Option<NodeId> {
        self.ui.find(&[TABLE, TABLE_BODY, Key::index(r)])
    }

    fn rect(&self, id: NodeId) -> Rect {
        self.ui.info(id).expect("a live node").rect
    }

    /// The rows built, by key, in order.
    fn built(&self) -> Vec<u64> {
        let keys: Vec<Key> = self
            .ui
            .children(self.body())
            .map(|id| self.ui.info(id).expect("a row").key)
            .collect();
        keys.iter()
            .map(|&k| {
                (0..u64::from(self.page.model.rows()))
                    .find(|&r| Key::index(r) == k)
                    .expect("a row's key")
            })
            .collect()
    }

    fn scroll(&self) -> dpro_ui::ScrollInfo {
        self.ui
            .info(self.body())
            .and_then(|n| n.scroll)
            .expect("a viewport")
    }

    fn offset(&self) -> f32 {
        self.scroll().offset.y
    }

    fn focused_row(&self) -> Option<u64> {
        let focus = self.ui.focus()?;
        (0..u64::from(self.page.model.rows())).find(|&r| self.row(r) == Some(focus))
    }

    /// Row `r`'s cell in column `col`, or its check cell.
    fn cell(&self, r: u64, col: Option<ColumnId>) -> NodeId {
        let key = col.map_or(TABLE_CHECK, |c| Key::index(u64::from(c.0)));
        self.ui
            .find(&[TABLE, TABLE_BODY, Key::index(r), key])
            .expect("the cell")
    }

    /// Column `col`'s header, or the check column's.
    fn header(&self, col: Option<ColumnId>) -> NodeId {
        let key = col.map_or(TABLE_CHECK, |c| Key::index(u64::from(c.0)));
        self.ui
            .find(&[TABLE, TABLE_HEADER, key])
            .expect("the column header")
    }

    fn wheel(&mut self, notches: i32) {
        let at = self.rect(self.body()).center();
        for _ in 0..notches.unsigned_abs() {
            self.send(Input::Wheel {
                at,
                delta: -120 * notches.signum(),
                horizontal: false,
            });
        }
    }
}

fn plain() -> Mods {
    Mods::default()
}

fn shift() -> Mods {
    Mods {
        shift: true,
        ..Mods::default()
    }
}

fn ctrl() -> Mods {
    Mods {
        ctrl: true,
        ..Mods::default()
    }
}

fn ev(e: TableEvent) -> Msg {
    Msg::Table(e)
}

// ---- realization -----------------------------------------------------------------------------

#[test]
fn rows_in_view_and_half_a_view_either_side_are_built() {
    let mut run = Run::new(1_000);
    // 364 px of view and 182 below it: rows 0 to 15.
    assert_eq!(run.built(), (0..16).collect::<Vec<_>>());
    // A notch is three wheel lines of a row each.
    run.wheel(1);
    assert_eq!(run.offset(), 3.0 * ROW);
    assert_eq!(run.built(), (0..19).collect::<Vec<_>>());
    run.wheel(2);
    // At 324 px, 182 above starts in row 3, and 546 below ends in row 24.
    assert_eq!(run.built(), (3..25).collect::<Vec<_>>());
}

#[test]
fn a_scroll_keeps_the_nodes_of_the_rows_still_in_range() {
    let mut run = Run::new(1_000);
    let ids: Vec<NodeId> = (5..16).map(|r| run.row(r).expect("built")).collect();
    run.wheel(3);
    let after: Vec<NodeId> = (5..16).map(|r| run.row(r).expect("still built")).collect();
    assert_eq!(after, ids);
}

#[test]
fn the_current_row_is_revealed_first_and_stays_built_out_of_range() {
    let mut page = Page::new(1_000);
    page.model.set_current(700);
    let mut run = Run::with(page);
    let row = run.rect(run.row(700).expect("built"));
    let body = run.rect(run.body());
    assert_eq!(
        row.bottom(),
        body.bottom(),
        "scrolled into view, at the bottom"
    );
    run.wheel(-300);
    assert_eq!(run.offset(), 0.0);
    let mut expect: Vec<u64> = (0..16).collect();
    expect.push(700);
    assert_eq!(run.built(), expect);
}

#[test]
fn a_100k_row_table_has_as_many_nodes_as_a_100_row_one() {
    let mut small = Run::new(100);
    let mut large = Run::new(100_000);
    // The grid, the header and its 4 column headers, the rows' view, and 16 rows of 4 cells.
    assert_eq!(small.ui.len(), 7 + 16 * 5);
    assert_eq!(large.ui.len(), small.ui.len());
    assert_eq!(
        large.ui.a11y_tree().nodes.len(),
        small.ui.a11y_tree().nodes.len()
    );
    // At the far end too.
    large.page.model.set_current(99_999);
    large.frame();
    assert_eq!(large.ui.len(), small.ui.len());
}

#[test]
fn rows_are_whole_pixels_at_any_depth() {
    let mut page = Page::new(100_000);
    page.model.set_current(99_999);
    // 225 % text at 150 %: 29.25 px text in 36.5625 px lines with 18 px around them, 81.84
    // physical px, rounded once to 82. The rows are 8.2 M px, past f32's exact integers.
    let run = Run::scaled(page, 1.5, 2.25);
    let body = run.rect(run.body()).snap(1.5);
    assert_eq!((body.y0, body.y1), (82, 600), "the header is a row high");
    for r in 99_990..100_000 {
        let rect = run.rect(run.row(r).expect("built")).snap(1.5);
        let up = i32::try_from(99_999 - r).expect("rows") * 82;
        assert_eq!((rect.y0, rect.y1), (518 - up, 600 - up), "row {r}");
    }
    assert_eq!(run.scroll().offset_px[1], 100_000 * 82 - 518);
}

/// `FakeText` with lines 1.2 times as high: 19.5 px at 13 px, so a 37.5 px row, 38 px.
struct Tall;

impl TextSystem for Tall {
    fn measure(&mut self, text: &str, style: &TextStyle, max_width: f32) -> TextMetrics {
        let m = FakeText.measure(text, style, max_width);
        TextMetrics {
            height: m.height * 1.2,
            ..m
        }
    }

    fn draw(&mut self, _: &str, _: &TextStyle, _: Rect, _: f32, _: &mut dyn dpro_ui::GlyphSink) {}

    fn icon(&mut self, _: dpro_ui::Icon, _: Rect, _: f32, _: &mut dyn dpro_ui::GlyphSink) {}

    fn caret_stops(&mut self, text: &str, style: &TextStyle, out: &mut Vec<dpro_ui::CaretStop>) {
        FakeText.caret_stops(text, style, out);
    }
}

#[test]
fn the_first_build_guesses_and_asks_for_one_more() {
    let mut ui: Ui<Msg> = Ui::new(WINDOW, 1.0, Motion::REDUCED);
    let page = Page::new(1_000);
    // Nothing is measured yet: the build guesses lines 1.25 times the text size and no view.
    ui.build(|cx| page.view(cx));
    ui.layout(&mut Tall);
    assert!(ui.wants_build());
    ui.build(|cx| page.view(cx));
    ui.layout(&mut Tall);
    assert!(!ui.wants_build());
    ui.build(|cx| page.view(cx));
    ui.layout(&mut Tall);
    assert!(!ui.wants_build(), "measured once, known after");
    let body = ui.find(&[TABLE, TABLE_BODY]).expect("the rows' view");
    let rows: Vec<Rect> = ui
        .children(body)
        .map(|id| ui.info(id).expect("a row").rect)
        .collect();
    assert_eq!((rows[0].y, rows[1].y, rows[1].h), (38.0, 76.0, 38.0));
    // 362 px of view and 181 below: 15 rows.
    assert_eq!(rows.len(), 15);
}

#[test]
fn a_text_size_or_dpi_change_keeps_the_top_row() {
    let mut run = Run::new(1_000);
    run.wheel(10);
    assert_eq!(run.offset(), 30.0 * ROW);
    run.ui.set_text_scale(2.25);
    run.frame();
    // 54.5625 px rows round to 55: row 30 is still the first shown.
    assert_eq!(run.offset(), 30.0 * 55.0);
    assert_eq!(
        run.rect(run.row(30).expect("built")).y,
        run.rect(run.body()).y
    );
    run.ui.set_text_scale(1.0);
    run.frame();
    run.ui.set_scale(1.5);
    run.frame();
    // 54 px rows at 150 %.
    assert_eq!(run.scroll().offset_px[1], 30 * 54);
}

// ---- columns ---------------------------------------------------------------------------------

#[test]
fn columns_are_tracks_the_header_and_every_row_share() {
    let run = Run::new(100);
    // The check column, then Name's share of what Version's 80 and Size's 90 leave.
    let expect = [
        (None, 0.0, CHECK),
        (Some(NAME), CHECK, 402.0),
        (Some(VERSION), 430.0, 80.0),
        (Some(SIZE), 510.0, 90.0),
    ];
    for (col, x, w) in expect {
        let h = run.rect(run.header(col));
        assert_eq!((h.x, h.w, h.y, h.h), (x, w, 0.0, ROW), "header {col:?}");
        for r in [0_u8, 7] {
            let c = run.rect(run.cell(u64::from(r), col));
            let y = ROW + ROW * f32::from(r);
            assert_eq!((c.x, c.w, c.y, c.h), (x, w, y, ROW), "row {r} {col:?}");
        }
    }
}

#[test]
fn columns_past_the_view_scroll_sideways_and_the_header_follows() {
    let mut run = Run::new(100);
    // 300 px: Name at its 120 px minimum makes 318 px of columns.
    run.ui.set_viewport(Size { w: 300.0, ..WINDOW });
    run.frame();
    let info = run.scroll();
    assert_eq!(info.content.w, CHECK + 120.0 + 80.0 + 90.0);
    assert!(info.bar_x.is_some());
    // On a row, Right scrolls a character, half the body size in whole pixels (6.5, 7).
    run.ui.set_focus(run.row(0));
    run.key(KeyCode::Right, plain());
    assert_eq!(run.scroll().offset.x, 7.0);
    assert_eq!(run.rect(run.header(Some(SIZE))).x, 228.0 - 7.0);
    assert_eq!(run.rect(run.cell(0, Some(SIZE))).x, 228.0 - 7.0);
    run.key(KeyCode::Left, plain());
    assert_eq!(run.rect(run.header(Some(SIZE))).x, 228.0);
    // The wheel over the header scrolls the rows, not the header alone.
    let at = run.rect(run.header(Some(NAME))).center();
    run.send(Input::Wheel {
        at,
        delta: 120,
        horizontal: true,
    });
    let x = run.scroll().offset.x;
    assert_eq!(x, 18.0, "three characters, to the end");
    assert_eq!(run.rect(run.header(Some(SIZE))).x, 228.0 - x);
}

/// Three shared columns whose minimums don't all fit their shares.
const SHARED: [Column<'static>; 3] = [
    Column {
        width: Track::Share(3.0),
        min: 120.0,
        ..COLUMNS[0]
    },
    Column {
        width: Track::Share(1.0),
        min: 160.0,
        ..COLUMNS[1]
    },
    Column {
        width: Track::Share(1.0),
        min: 80.0,
        ..COLUMNS[2]
    },
];

#[test]
fn a_share_under_its_minimum_takes_it_and_the_others_share_the_rest() {
    let mut page = Page::new(100);
    page.columns = &SHARED;
    let run = Run::with(page);
    // 572 px to share at 114.4 a share would leave Version under its 160: it takes 160, and
    // the other four shares split 412 at 103. The columns fill the width, and nothing scrolls.
    let expect = [
        (None, 0.0, CHECK),
        (Some(NAME), CHECK, 309.0),
        (Some(VERSION), 337.0, 160.0),
        (Some(SIZE), 497.0, 103.0),
    ];
    for (col, x, w) in expect {
        let h = run.rect(run.header(col));
        assert_eq!((h.x, h.w), (x, w), "header {col:?}");
        let c = run.rect(run.cell(3, col));
        assert_eq!((c.x, c.w), (x, w), "row 3 {col:?}");
    }
    assert_eq!(run.scroll().content.w, WINDOW.w);
    assert_eq!(run.scroll().bar_x, None);
}

#[test]
fn columns_that_fill_the_view_scroll_nothing_sideways_at_any_scale() {
    let mut page = Page::new(100);
    page.pad = 2.0;
    let mut run = Run::scaled(page, 1.25, 1.0);
    // The rows' view is 598 px wide at x 2: at 125 %, 2.5 to 750 physical pixels, which snap to
    // 3 and 750. The columns fill its 598 px, 747.5 physical, which rounds to 748: one more.
    run.ui.set_viewport(Size { w: 602.0, ..WINDOW });
    run.frame();
    assert_eq!(run.scroll().content.w, 598.0);
    assert_eq!(run.scroll().bar_x, None);
    let at = run.rect(run.body()).center();
    run.send(Input::Wheel {
        at,
        delta: 120,
        horizontal: true,
    });
    assert_eq!(run.scroll().offset_px, [0, 0]);
}

#[test]
fn a_table_as_high_as_its_rows_grows_with_the_text_size() {
    let mut page = Page::new(3);
    page.height = Dim::Auto;
    let mut run = Run::with(page);
    // The header and 3 rows.
    assert_eq!(run.rect(run.table()).h, 4.0 * ROW);
    // 54.5625 px rows round to 55: measured when the grid is, not after.
    run.ui.set_text_scale(2.25);
    run.frame();
    assert_eq!(run.rect(run.table()).h, 4.0 * 55.0);
    assert_eq!(run.rect(run.row(2).expect("built")).bottom(), 4.0 * 55.0);
}

#[test]
fn a_cell_shows_its_column_in_its_face() {
    let run = Run::new(10);
    let version = run.ui.info(run.cell(3, Some(VERSION))).expect("a cell");
    assert_eq!(version.role, Role::Cell);
    assert_eq!(version.text, "3.3");
    assert!(version.text_style.mono);
    assert_eq!(version.text_style.size, 13.0);
    let name = run.ui.info(run.cell(3, Some(NAME))).expect("a cell");
    assert!(!name.text_style.mono);
    let header = run.ui.info(run.header(Some(NAME))).expect("a header");
    assert_eq!(
        (header.role, header.text_style.size),
        (Role::ColumnHeader, 12.0)
    );
}

// ---- the rows' keys --------------------------------------------------------------------------

#[test]
fn tab_lands_on_the_header_then_the_current_row() {
    let mut page = Page::new(100);
    page.model.set_current(4);
    let mut run = Run::with(page);
    run.key(KeyCode::Tab, plain());
    assert_eq!(run.ui.focus(), Some(run.header(None)));
    run.key(KeyCode::Tab, plain());
    assert_eq!(run.focused_row(), Some(4));
    // The rows are one stop: Tab leaves them, and wraps to the header.
    run.key(KeyCode::Tab, plain());
    assert_eq!(run.ui.focus(), Some(run.header(None)));
    run.key(KeyCode::Tab, shift());
    assert_eq!(run.focused_row(), Some(4));
}

#[test]
fn tab_lands_on_the_first_row_in_view_without_a_current_row() {
    let mut run = Run::new(100);
    run.key(KeyCode::Tab, plain());
    run.key(KeyCode::Tab, plain());
    assert_eq!(run.focused_row(), Some(0));
    // Rows are built half a view above and below: Tab and Shift+Tab land in view, not on the
    // first or last built, and scroll nothing. 10.5 rows down, row 11 is the first wholly in.
    run.key(KeyCode::Tab, plain());
    assert!(run.ui.scroll_to(run.body(), [0, 378]));
    run.frame();
    run.key(KeyCode::Tab, shift());
    assert_eq!((run.focused_row(), run.offset()), (Some(11), 10.5 * ROW));
    run.key(KeyCode::Tab, plain());
    run.key(KeyCode::Tab, plain());
    assert_eq!((run.focused_row(), run.offset()), (Some(11), 10.5 * ROW));
}

/// With no current row, a focused row the build drops (a scroll, a refresh) leaves focus on the
/// first row in view, not on nothing: the keyboard and a client's cursor stay in the table.
#[test]
fn a_focused_row_scrolled_out_of_range_leaves_focus_in_view() {
    let mut run = Run::new(1_000);
    run.key(KeyCode::Tab, plain());
    run.key(KeyCode::Tab, plain());
    assert_eq!(run.focused_row(), Some(0));
    assert!(run.ui.scroll_to(run.body(), [0, 60 * 36]));
    run.frame();
    assert_eq!(run.row(0), None);
    assert_eq!(run.focused_row(), Some(60));
    assert_eq!(
        run.key(KeyCode::Down, plain()),
        [ev(TableEvent::Current(61))]
    );
}

#[test]
fn the_arrows_page_keys_home_and_end_move_the_current_row() {
    let mut page = Page::new(1_000);
    page.model.set_current(0);
    let mut run = Run::with(page);
    run.ui.set_focus(run.row(0));
    assert_eq!(
        run.key(KeyCode::Down, plain()),
        [ev(TableEvent::Current(1))]
    );
    assert_eq!(run.key(KeyCode::Up, plain()), [ev(TableEvent::Current(0))]);
    assert_eq!(run.key(KeyCode::Up, plain()), [], "at the top");
    // A page is the 10 whole rows the view shows, less one.
    assert_eq!(
        run.key(KeyCode::PageDown, plain()),
        [ev(TableEvent::Current(9))]
    );
    assert_eq!(
        run.key(KeyCode::PageUp, plain()),
        [ev(TableEvent::Current(0))]
    );
    assert_eq!(
        run.key(KeyCode::End, plain()),
        [ev(TableEvent::Current(999))]
    );
    assert_eq!(
        run.focused_row(),
        Some(999),
        "focus follows the current row"
    );
    assert_eq!(run.offset(), 1_000.0 * ROW - VIEW);
    assert_eq!(run.key(KeyCode::Home, ctrl()), [ev(TableEvent::Current(0))]);
    assert_eq!((run.focused_row(), run.offset()), (Some(0), 0.0));
}

#[test]
fn down_at_the_bottom_edge_scrolls_the_new_current_row_into_view() {
    let mut page = Page::new(1_000);
    page.model.set_current(9);
    let mut run = Run::with(page);
    run.ui.set_focus(run.row(9));
    assert_eq!(run.offset(), 0.0);
    // Row 9 ends at 360 of 364; row 10 ends 32 px under the view.
    run.key(KeyCode::Down, plain());
    assert_eq!(run.focused_row(), Some(10));
    assert_eq!(run.offset(), 11.0 * ROW - VIEW);
    assert_eq!(run.rect(run.row(10).expect("built")).bottom(), WINDOW.h);
    // Back up: row 9 shows, so nothing scrolls.
    run.key(KeyCode::Up, plain());
    assert_eq!(run.offset(), 11.0 * ROW - VIEW);
}

#[test]
fn space_checks_shift_extends_from_the_anchor_and_ctrl_a_checks_all() {
    let mut page = Page::new(100);
    page.model.set_current(2);
    let mut run = Run::with(page);
    run.ui.set_focus(run.row(2));
    assert_eq!(
        run.key(KeyCode::Space, plain()),
        [ev(TableEvent::Check(2, Pick::Toggle))]
    );
    assert_eq!(
        run.key(KeyCode::Down, shift()),
        [ev(TableEvent::Check(3, Pick::Extend))]
    );
    assert_eq!(
        run.key(KeyCode::Down, shift()),
        [ev(TableEvent::Check(4, Pick::Extend))]
    );
    assert_eq!(
        run.page.model.selected_rows().collect::<Vec<_>>(),
        [2, 3, 4]
    );
    assert_eq!(run.focused_row(), Some(4));
    assert_eq!(
        run.key(KeyCode::Space, ctrl()),
        [ev(TableEvent::Check(4, Pick::Toggle))]
    );
    assert_eq!(
        run.key(KeyCode::Space, shift()),
        [ev(TableEvent::Check(4, Pick::Extend))]
    );
    assert_eq!(
        run.key(KeyCode::End, shift()),
        [ev(TableEvent::Check(99, Pick::Extend))]
    );
    assert_eq!(
        run.key(KeyCode::Other(u16::from(b'A')), ctrl()),
        [ev(TableEvent::CheckAll)]
    );
    assert_eq!(run.page.model.selected_count(), 100);
    assert_eq!(
        run.key(KeyCode::Enter, plain()),
        [ev(TableEvent::Activate(99))]
    );
}

#[test]
fn a_table_without_checks_takes_the_moves_only() {
    let mut page = Page::new(100);
    page.checks = false;
    page.model.set_current(2);
    let mut run = Run::with(page);
    run.ui.set_focus(run.row(2));
    assert_eq!(run.key(KeyCode::Space, plain()), []);
    assert_eq!(run.key(KeyCode::Other(u16::from(b'A')), ctrl()), []);
    assert_eq!(
        run.key(KeyCode::Down, shift()),
        [ev(TableEvent::Current(3))]
    );
    // No check column: Name starts at the left.
    assert_eq!(run.rect(run.cell(3, Some(NAME))).x, 0.0);
    assert_eq!(run.ui.children(run.row(3).expect("built")).count(), 3);
}

#[test]
fn a_locked_row_can_be_current_but_not_checked_and_says_why() {
    let mut page = Page::new(100);
    page.locked = Some(2);
    page.model.set_locked(2, true);
    page.model.set_current(2);
    let mut run = Run::with(page);
    run.ui.set_focus(run.row(2));
    assert_eq!(run.key(KeyCode::Space, plain()), []);
    let check = run.ui.info(run.cell(2, None)).expect("an empty check cell");
    assert_eq!((check.text, check.enabled), ("", false));
    let tree = run.ui.a11y_tree();
    let row = node(&tree, &run.ui, run.row(2).expect("built"));
    assert_eq!(row.description(), Some("A system component"));
    let cell = node(&tree, &run.ui, run.cell(2, None));
    assert_eq!(cell.description(), Some("A system component"));
    assert_eq!(cell.toggled(), None);
    assert!(!cell.supports_action(Action::Click));
    // Shift+Down to a row with a check box extends. Back onto the locked one Shift extends too,
    // so the anchor stays where it was; the model skips the locked row. (A plain move there would
    // move the anchor, and the next Shift range would start on the locked row.)
    assert_eq!(
        run.key(KeyCode::Down, shift()),
        [ev(TableEvent::Check(3, Pick::Extend))]
    );
    assert_eq!(
        run.key(KeyCode::Up, shift()),
        [ev(TableEvent::Check(2, Pick::Extend))]
    );
    assert_eq!(
        (run.page.model.current(), run.page.model.selected_count()),
        (Some(2), 0)
    );
    // A Shift range over it, from row 3 (current, so the anchor) to the top, and Ctrl+A check
    // every row but it.
    assert_eq!(
        run.key(KeyCode::Down, plain()),
        [ev(TableEvent::Current(3))]
    );
    assert_eq!(
        run.key(KeyCode::Home, shift()),
        [ev(TableEvent::Check(0, Pick::Extend))]
    );
    let checked: Vec<u32> = run.page.model.selected_rows().collect();
    assert_eq!(checked, [0, 1, 3], "row 2 is locked");
    run.key(KeyCode::Other(u16::from(b'A')), ctrl());
    assert_eq!(run.page.model.selected_count(), 99);
    assert!(!run.page.model.is_selected(2));
}

/// The current row is built first, then again in its place among the others: a cell its view
/// didn't give stays empty, and doesn't show the text of a row built before it.
#[test]
fn a_cell_the_view_left_out_of_the_current_row_stays_empty() {
    for current in [5, 30] {
        let mut page = Page::new(100);
        page.no_size = Some(current);
        page.model.set_current(current);
        let run = Run::with(page);
        let size = |run: &Run, at: u32| {
            run.ui
                .info(run.cell(u64::from(at), Some(SIZE)))
                .expect("a size cell")
                .text
                .to_owned()
        };
        assert_eq!(size(&run, current), "", "current row {current}");
        if current == 5 {
            assert_eq!(size(&run, 4), "148 MB");
        }
    }
}

// ---- the pointer -----------------------------------------------------------------------------

#[test]
fn a_click_makes_a_row_current_and_leaves_the_checks() {
    let mut run = Run::new(100);
    let at = run.rect(run.cell(5, Some(NAME))).center();
    assert_eq!(run.click(at, plain()), [ev(TableEvent::Current(5))]);
    assert_eq!(run.focused_row(), Some(5));
    assert_eq!(run.page.model.selected_count(), 0);
    // On its check box, with Ctrl, with Shift: the checks. (Rows 0 to 9 show whole.)
    let check = run.rect(run.cell(6, None)).center();
    assert_eq!(
        run.click(check, plain()),
        [ev(TableEvent::Check(6, Pick::Toggle))]
    );
    let name = run.rect(run.cell(7, Some(NAME))).center();
    assert_eq!(
        run.click(name, ctrl()),
        [ev(TableEvent::Check(7, Pick::Toggle))]
    );
    let name = run.rect(run.cell(9, Some(NAME))).center();
    assert_eq!(
        run.click(name, shift()),
        [ev(TableEvent::Check(9, Pick::Extend))]
    );
    assert_eq!(
        run.page.model.selected_rows().collect::<Vec<_>>(),
        [7, 8, 9]
    );
    // Released on another row: nothing, and focus stays on the current row, which the keys act
    // on.
    assert_eq!(run.page.model.current_view(), Some(9));
    let a = run.rect(run.cell(1, Some(NAME))).center();
    let b = run.rect(run.cell(3, Some(NAME))).center();
    assert_eq!(
        run.send_all(&[Input::PointerDown(a), Input::PointerUp(b)]),
        []
    );
    assert_eq!(run.focused_row(), Some(9));
    assert_eq!(
        run.key(KeyCode::Down, plain()),
        [ev(TableEvent::Current(10))]
    );
}

#[test]
fn a_double_click_activates_once() {
    let mut run = Run::new(100);
    let at = run.rect(run.cell(5, Some(NAME))).center();
    let mut out = run.click(at, plain());
    out.extend(run.send_all(&[
        Input::Press {
            at,
            mods: plain(),
            double: true,
        },
        Input::PointerUp(at),
    ]));
    assert_eq!(
        out,
        [ev(TableEvent::Current(5)), ev(TableEvent::Activate(5))]
    );
}

#[test]
fn a_right_click_makes_its_row_current_and_asks_for_the_menu() {
    let mut run = Run::new(100);
    let at = run.rect(run.cell(6, Some(VERSION))).center();
    assert_eq!(
        run.send(Input::ContextMenu(Some(at))),
        [ev(TableEvent::Current(6))]
    );
    let asked = run.ui.take_context().and_then(|id| run.ui.info(id));
    assert_eq!(asked.map(|n| n.key), Some(Key::index(6)));
}

#[test]
fn rows_and_column_headers_are_big_enough_targets() {
    let run = Run::new(10);
    for id in [
        run.row(0).expect("a row"),
        run.header(None),
        run.header(Some(VERSION)),
    ] {
        let r = run.rect(id);
        assert!(r.h >= 24.0 && r.w >= 24.0, "{r:?}");
    }
}

// ---- the header ------------------------------------------------------------------------------

#[test]
fn the_header_is_one_stop_whose_arrows_move_between_columns() {
    let mut run = Run::new(100);
    run.ui.set_focus(Some(run.header(Some(NAME))));
    run.key(KeyCode::Right, plain());
    assert_eq!(run.ui.focus(), Some(run.header(Some(VERSION))));
    run.key(KeyCode::Right, plain());
    run.key(KeyCode::Right, plain());
    assert_eq!(run.ui.focus(), Some(run.header(Some(SIZE))), "no wrap");
    run.key(KeyCode::Home, plain());
    assert_eq!(run.ui.focus(), Some(run.header(None)));
}

#[test]
fn a_sortable_header_sorts_by_its_first_order_then_the_other() {
    let mut run = Run::new(100);
    run.ui.set_focus(Some(run.header(Some(SIZE))));
    assert_eq!(
        run.key(KeyCode::Enter, plain()),
        [ev(TableEvent::Sort(SIZE))]
    );
    assert_eq!(run.page.model.sort(), Some((SIZE, Order::Descending)));
    assert_eq!(
        run.key(KeyCode::Space, plain()),
        [ev(TableEvent::Sort(SIZE))]
    );
    assert_eq!(run.page.model.sort(), Some((SIZE, Order::Ascending)));
    let at = run.rect(run.header(Some(NAME))).center();
    assert_eq!(run.click(at, plain()), [ev(TableEvent::Sort(NAME))]);
    // Version doesn't sort.
    run.ui.set_focus(Some(run.header(Some(VERSION))));
    assert_eq!(run.key(KeyCode::Enter, plain()), []);
    let at = run.rect(run.header(Some(VERSION))).center();
    assert_eq!(run.click(at, plain()), []);
}

#[test]
fn the_check_header_checks_all() {
    let mut run = Run::new(100);
    let at = run.rect(run.header(None)).center();
    assert_eq!(run.click(at, plain()), [ev(TableEvent::CheckAll)]);
    assert_eq!(
        run.ui.info(run.header(None)).map(|n| n.check),
        Some(Tri::On)
    );
}

#[test]
fn a_sort_keeps_the_current_row_its_node_focus_and_the_offset() {
    let mut page = Page::new(1_000);
    page.model.set_current(3);
    let mut run = Run::with(page);
    run.ui.set_focus(run.row(3));
    let node = run.row(3);
    run.wheel(1);
    let offset = run.offset();
    // Largest first, row 3 (111 MB) moves far down, out of view.
    run.page.sort(SIZE);
    run.frame();
    assert_eq!(run.offset(), offset, "a sort doesn't scroll");
    assert_eq!(run.row(3), node, "still built, the same node");
    assert_eq!(run.ui.focus(), node);
    assert!(!run.built()[..19].contains(&3));
}

// ---- focus and the current row ---------------------------------------------------------------

#[test]
fn focus_follows_the_current_row_and_a_new_one_is_scrolled_into_view() {
    let mut page = Page::new(1_000);
    page.model.set_current(0);
    let mut run = Run::with(page);
    run.ui.set_focus(run.row(0));
    // The model moves it (a search, a refresh): focus and view follow.
    run.page.model.set_current(500);
    run.frame();
    assert_eq!(run.focused_row(), Some(500));
    let row = run.rect(run.row(500).expect("built"));
    assert_eq!(row.bottom(), run.rect(run.body()).bottom());
    // With focus elsewhere, the view still follows, and focus stays put.
    run.ui.set_focus(None);
    run.page.model.set_current(5);
    run.frame();
    assert_eq!(run.ui.focus(), None);
    assert_eq!(run.offset(), 5.0 * ROW);
}

#[test]
fn realize_builds_and_reveals_a_far_row() {
    let mut run = Run::new(100_000);
    assert!(run.row(99_999).is_none());
    assert!(run.ui.realize(run.table(), 99_999));
    assert!(run.ui.wants_build());
    run.frame();
    let row = run.rect(run.row(99_999).expect("built"));
    assert_eq!(row.bottom(), run.rect(run.body()).bottom());
    assert!(!run.ui.realize(run.table(), 100_000), "past the end");
    assert!(!run.ui.realize(run.body(), 0), "not a table");
}

// ---- states ----------------------------------------------------------------------------------

#[test]
fn an_empty_table_shows_its_status_line_and_action() {
    let mut page = Page::new(0);
    page.status = Some("No programs match “zz”");
    let mut run = Run::with(page);
    let lines: Vec<(Role, &str)> = run
        .ui
        .children(run.body())
        .map(|id| run.ui.info(id).expect("a node"))
        .map(|n| (n.role, n.text))
        .collect();
    assert_eq!(
        lines,
        [
            (Role::Label, "No programs match “zz”"),
            (Role::Button, "Clear filter")
        ]
    );
    let button = run.ui.children(run.body()).nth(1).expect("the action");
    let at = run.rect(button).center();
    assert_eq!(run.click(at, plain()), [Msg::ClearFilter]);
    assert_eq!(run.ui.children(run.body()).count(), 0);
    // The header stays.
    let header = run.ui.find(&[TABLE, TABLE_HEADER]).expect("the header");
    assert_eq!(run.ui.children(header).count(), 4);
}

/// With no rows, the status line is as wide as the columns: the header, which follows it, still
/// scrolls to a column header past the view that the keyboard moves to.
#[test]
fn an_empty_table_still_scrolls_its_header_across() {
    let mut page = Page::new(0);
    page.status = Some("No programs match “zz”");
    let mut run = Run::with(page);
    // 300 px: Name at its 120 px minimum makes 318 px of columns.
    run.ui.set_viewport(Size { w: 300.0, ..WINDOW });
    run.frame();
    assert_eq!(run.scroll().content.w, CHECK + 120.0 + 80.0 + 90.0);
    run.key(KeyCode::Tab, plain());
    assert_eq!(run.ui.focus(), Some(run.header(None)));
    run.key(KeyCode::End, plain());
    assert_eq!(run.ui.focus(), Some(run.header(Some(SIZE))));
    assert_eq!(run.scroll().offset.x, 18.0);
    assert_eq!(run.rect(run.header(Some(SIZE))).right(), 300.0);
}

// ---- looks -----------------------------------------------------------------------------------

#[test]
fn the_current_row_is_selected_and_its_first_cell_semibold() {
    let mut page = Page::new(10);
    page.model.set_current(3);
    page.model.pick(5, Pick::Toggle);
    let run = Run::with(page);
    let row = run.ui.info(run.row(3).expect("built")).expect("a row");
    assert_eq!((row.role, row.selected), (Role::Row, Some(true)));
    assert_eq!(row.text, "Program 00003", "named by its first column");
    let other = run.ui.info(run.row(4).expect("built")).expect("a row");
    assert_eq!(other.selected, Some(false));
    let weight = |r| {
        run.ui
            .info(run.cell(r, Some(NAME)))
            .expect("a cell")
            .text_style
            .weight
    };
    assert_eq!((weight(3), weight(4)), (600, 400));
    let check = |r| run.ui.info(run.cell(r, None)).expect("a cell").check;
    assert_eq!((check(3), check(5)), (Tri::Off, Tri::On));
}

// ---- accessibility ---------------------------------------------------------------------------

fn ak(ui: &Ui<Msg>, tree: &accesskit::TreeUpdate, id: NodeId) -> AkId {
    tree.nodes
        .iter()
        .map(|(k, _)| *k)
        .find(|k| ui.a11y_node(*k) == Some(id))
        .expect("in the tree")
}

fn node<'t>(tree: &'t accesskit::TreeUpdate, ui: &Ui<Msg>, id: NodeId) -> &'t accesskit::Node {
    let k = ak(ui, tree, id);
    &tree.nodes.iter().find(|(n, _)| *n == k).expect("a node").1
}

#[test]
fn the_tree_is_a_grid_of_rows_and_cells_with_their_indices() {
    let mut page = Page::new(1_000);
    page.model.pick(4, Pick::Toggle);
    page.model.set_current(2);
    let mut run = Run::with(page);
    run.page.sort(NAME);
    run.frame();
    let tree = run.ui.a11y_tree();
    let ui = &run.ui;

    let grid = node(&tree, ui, run.table());
    assert_eq!(grid.role(), AkRole::Grid);
    assert_eq!(grid.label(), Some("Programs"));
    assert_eq!(
        (grid.row_count(), grid.column_count()),
        (Some(1_000), Some(4))
    );
    // AccessKit computes neither (A2.3): Narrator says "2 of 1000", not "of 23" built.
    assert_eq!(grid.size_of_set(), Some(1_000));
    assert_eq!(grid.scroll_y(), Some(0.0));
    assert_eq!(grid.scroll_y_max(), Some(f64::from(1_000.0 * ROW - VIEW)));
    assert!(grid.supports_action(Action::ScrollDown));
    // The rows' view is left out of what a client sees.
    assert_eq!(node(&tree, ui, run.body()).role(), AkRole::GenericContainer);

    let header = ui.find(&[TABLE, TABLE_HEADER]).expect("the header");
    assert_eq!(node(&tree, ui, header).role(), AkRole::RowGroup);
    let name = node(&tree, ui, run.header(Some(NAME)));
    assert_eq!(name.role(), AkRole::ColumnHeader);
    assert_eq!((name.label(), name.column_index()), (Some("Name"), Some(1)));
    assert_eq!(name.sort_direction(), Some(SortDirection::Ascending));
    assert!(name.supports_action(Action::Click));
    let version = node(&tree, ui, run.header(Some(VERSION)));
    assert_eq!(version.sort_direction(), None);
    assert!(!version.supports_action(Action::Click));
    assert!(version.supports_action(Action::Focus));
    let check = node(&tree, ui, run.header(None));
    assert_eq!(
        (check.label(), check.column_index()),
        (Some("Select"), Some(0))
    );
    assert_eq!(check.toggled(), Some(Toggled::Mixed));

    let row = node(&tree, ui, run.row(2).expect("built"));
    assert_eq!(row.role(), AkRole::Row);
    assert_eq!((row.row_index(), row.position_in_set()), (Some(2), Some(2)));
    assert_eq!(row.is_selected(), Some(true));
    assert_eq!(row.label(), Some("Program 00002"));
    assert!(row.supports_action(Action::Focus));
    assert!(row.supports_action(Action::ScrollIntoView));
    let other = node(&tree, ui, run.row(3).expect("built"));
    assert_eq!(other.is_selected(), Some(false));

    let first = node(&tree, ui, run.cell(2, Some(NAME)));
    assert_eq!(first.role(), AkRole::RowHeader);
    assert_eq!(
        (first.row_index(), first.column_index()),
        (Some(2), Some(1))
    );
    let size = node(&tree, ui, run.cell(2, Some(SIZE)));
    assert_eq!(size.role(), AkRole::Cell);
    assert_eq!((size.row_index(), size.column_index()), (Some(2), Some(3)));
    assert_eq!(size.label(), Some("74 MB"));
    let checked = node(&tree, ui, run.cell(4, None));
    assert_eq!(checked.role(), AkRole::Cell);
    assert_eq!(
        (checked.label(), checked.toggled()),
        (Some("Select"), Some(Toggled::True))
    );
    assert!(checked.supports_action(Action::Click));
    let unchecked = node(&tree, ui, run.cell(5, None));
    assert_eq!(unchecked.toggled(), Some(Toggled::False));
}

#[test]
fn a_scroll_sends_the_rows_that_came_and_a_sort_only_the_rows_built() {
    let mut run = Run::new(1_000);
    drop(run.ui.a11y_tree());
    let at = run.rect(run.body()).center();
    let mut out = Vec::new();
    run.ui.dispatch(
        Input::Wheel {
            at,
            delta: -120,
            horizontal: false,
        },
        &mut out,
    );
    run.frame();
    let update = run.ui.a11y_update().expect("an update");
    // Rows 16 to 18 came, with their 4 cells each; the rows' view has new children and a new
    // transform; the grid a new offset.
    assert_eq!(update.nodes.len(), 3 * 5 + 2);
    let grid = ak(&run.ui, &update, run.table());
    assert!(update.nodes.iter().any(|(k, _)| *k == grid));
    // A sort replaces the 19 rows built, and sends nothing for the rest.
    run.page.sort(SIZE);
    run.frame();
    let update = run.ui.a11y_update().expect("an update");
    let rows = update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == AkRole::Row)
        .count();
    assert_eq!(rows, 19);
}

/// A client's `action` on `id`, its messages applied to the page, then a frame.
fn client(run: &mut Run, id: NodeId, action: Action) -> Vec<Msg> {
    let tree = run.ui.a11y_tree();
    let target = ak(&run.ui, &tree, id);
    let mut out = Vec::new();
    run.ui.a11y_action(
        &ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: target,
            data: None,
        },
        &mut out,
    );
    for m in &out {
        run.page.update(*m);
    }
    run.frame();
    out
}

#[test]
fn a_client_selects_focuses_checks_sorts_and_scrolls() {
    let mut run = Run::new(100);
    let row = run.row(6).expect("built");
    assert_eq!(
        client(&mut run, row, Action::Click),
        [ev(TableEvent::Current(6))]
    );
    let row = run.row(8).expect("built");
    assert_eq!(
        client(&mut run, row, Action::Focus),
        [ev(TableEvent::Current(8))]
    );
    assert_eq!(run.focused_row(), Some(8));
    let check = run.cell(9, None);
    assert_eq!(
        client(&mut run, check, Action::Click),
        [ev(TableEvent::Check(9, Pick::Toggle))]
    );
    let size = run.header(Some(SIZE));
    assert_eq!(
        client(&mut run, size, Action::Click),
        [ev(TableEvent::Sort(SIZE))]
    );
    let version = run.header(Some(VERSION));
    assert_eq!(client(&mut run, version, Action::Click), []);
    // The grid scrolls the rows: a page is the view less a row.
    let grid = run.table();
    client(&mut run, grid, Action::ScrollDown);
    assert_eq!(run.offset(), VIEW - ROW);
}

// ---- the host's offset -----------------------------------------------------------------------

#[test]
fn scroll_to_moves_a_viewport_to_an_offset_clamped() {
    let mut run = Run::new(1_000);
    assert!(run.ui.scroll_to(run.body(), [0, 720]));
    assert!(run.ui.wants_build(), "other rows are in view");
    run.frame();
    assert_eq!(run.offset(), 720.0);
    // 182 px above 720 starts in row 14.
    assert_eq!(run.built().first(), Some(&14));
    assert!(run.ui.scroll_to(run.body(), [0, i64::MAX]));
    run.frame();
    assert_eq!(run.offset(), 1_000.0 * ROW - VIEW);
    assert!(
        !run.ui.scroll_to(run.body(), [0, i64::MAX]),
        "already there"
    );
    assert!(!run.ui.scroll_to(run.table(), [0, 0]), "not a viewport");
}
