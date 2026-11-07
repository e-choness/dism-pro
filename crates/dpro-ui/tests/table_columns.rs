//! TD-0009 A2 slice 5b2: a table's columns resized, fitted, shown and hidden, written before the
//! code (A2.4, A2.5, A2.9).
//!
//! - **Widths.** The view gives each column the width the user left it ([`Width`], from the
//!   model's [`ColumnLayout`]): its own track, pixels (at least its minimum), or a fit for the table
//!   to measure.
//! - **Dividers.** A column's right edge in the header is an 8 px strip that shows the left-right
//!   arrows. Dragging it resizes the column live: the columns left of it hold still, and the shared
//!   ones right of it take up the difference. Letting go sends its width, and the widths the
//!   shared columns left of it hold, which stop sharing so nothing jumps after the drag. Esc, or
//!   the window going inactive, puts the column back and sends nothing. A press on a strip never
//!   sorts.
//! - **Fit.** A double-click on a divider asks the view to fit its column. A column the view says
//!   to fit is measured at the next layout from its header (and a sortable one's chevron) and its
//!   cells in the rows built, not every row. The table lays it out at that width at once and sends
//!   the width once ([`Ui::take_messages`]); the host updates the model with it and builds again.
//! - **Hidden columns** leave the header, the rows and the grid's column count. A row is named by
//!   its first shown column. Focus on a column header that goes moves to the next one.
//! - **The column menu.** A context request on the header (Shift+F10 or the Menu key on a column
//!   header, or a right-click anywhere on it) asks the view for the column menu, for the column
//!   it's on (none for the check column). [`Cx::column_menu`] builds it: each hideable column as a
//!   check item, then Size column to fit, Size all columns to fit and Reset columns. Check items'
//!   labels and the others' line up, a check showing beside the shown columns.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers: a node that isn't there, or a table that doesn't settle, fails the test"
)]

use dpro_app::table::{ColumnLayout, RowMap, RowSource, TableModel};
use dpro_ui::{
    Align, COLUMN_FIT, COLUMN_FIT_ALL, COLUMN_RESET, Canvas, Cells, Column, ColumnId,
    ColumnMenuLabels, Cursor, Cx, Dim, Element, FakeText, Input, Key, KeyCode, KeyEvent, Mods,
    Motion, NodeId, Order, Point, Rect, Role, Row, RowKey, Size, Style, TABLE_BODY, TABLE_CHECK,
    TABLE_HEADER, Table, TableEvent, Theme, Track, Tri, Ui, Width,
    accesskit::{self, NodeId as AkId, Role as AkRole, Toggled},
};

const NAME: ColumnId = ColumnId(0);
const VERSION: ColumnId = ColumnId(1);
const SIZE: ColumnId = ColumnId(2);

/// Name shares what Version and Size leave, and can't be hidden from the menu.
const COLUMNS: [Column<'static>; 3] = [
    Column {
        id: NAME,
        header: "Name",
        width: Track::Share(1.0),
        min: 120.0,
        align: Align::Start,
        mono: false,
        sort: Some(Order::Ascending),
        hideable: false,
    },
    Column {
        id: VERSION,
        header: "Version",
        width: Track::Fixed(80.0),
        min: 60.0,
        align: Align::Start,
        mono: true,
        sort: None,
        hideable: true,
    },
    Column {
        id: SIZE,
        header: "Size",
        width: Track::Fixed(90.0),
        min: 60.0,
        align: Align::End,
        mono: false,
        sort: Some(Order::Descending),
        hideable: true,
    },
];

/// Name and Size share what Version leaves.
const SHARED: [Column<'static>; 3] = [
    COLUMNS[0],
    COLUMNS[1],
    Column {
        width: Track::Share(1.0),
        ..COLUMNS[2]
    },
];

/// Every column hideable.
const ALL_HIDEABLE: [Column<'static>; 3] = [
    Column {
        hideable: true,
        ..COLUMNS[0]
    },
    COLUMNS[1],
    COLUMNS[2],
];

/// 600 × 400 at scale 1. With the 28 px check column, Name shares 402 px; its divider is at x 430,
/// Version's at 510, Size's at 600.
const WINDOW: Size = Size { w: 600.0, h: 400.0 };
const CHECK: f32 = 28.0;
/// The header's middle: it's a 36 px row.
const HEADER_Y: f32 = 18.0;
const TABLE: Key = Key::name("table");
const MENU: Key = Key::name("column-menu");
const LABELS: ColumnMenuLabels<'static> = ColumnMenuLabels {
    name: "Columns",
    fit: "Size column to fit",
    fit_all: "Size all columns to fit",
    reset: "Reset columns",
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Msg {
    Table(TableEvent),
}

fn ev(e: TableEvent) -> Msg {
    Msg::Table(e)
}

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

/// A page with a table of `rows` programs and the columns as the user arranged them.
struct Page {
    model: TableModel,
    defs: &'static [Column<'static>],
    columns: ColumnLayout,
    names: Vec<String>,
    versions: Vec<String>,
    sizes: Vec<String>,
    /// The page ignores the widths the table sends, so a fit stays a fit.
    keep_fit: bool,
    /// The table has a check column.
    checks: bool,
    /// The view hides every column, which the menu never would (a page may).
    hide_all: bool,
}

impl Page {
    fn new(rows: u32) -> Self {
        Self::with_columns(rows, &COLUMNS)
    }

    fn with_columns(rows: u32, defs: &'static [Column<'static>]) -> Self {
        let ids: Vec<(ColumnId, bool)> = defs.iter().map(|c| (c.id, false)).collect();
        Self {
            model: TableModel::with_keys(RowMap::build(&Rows(rows))),
            defs,
            columns: ColumnLayout::new(&ids),
            names: (0..rows).map(|r| format!("Program {r:05}")).collect(),
            versions: (0..rows).map(|r| format!("{}.{}", r % 10, r % 7)).collect(),
            sizes: (0..rows)
                .map(|r| format!("{} MB", (r * 37) % 1000))
                .collect(),
            keep_fit: false,
            checks: true,
            hide_all: false,
        }
    }

    fn update(&mut self, msg: Msg) {
        let Msg::Table(e) = msg;
        match e {
            TableEvent::Current(at) => self.model.set_current(at),
            TableEvent::Resize(c, w) if !self.keep_fit => {
                self.columns.resize(c, w);
            }
            TableEvent::Fit(c) => {
                self.columns.fit(c);
            }
            TableEvent::Show(c, on) => {
                self.columns.set_hidden(c, !on);
            }
            TableEvent::ResetColumns => {
                self.columns.reset();
            }
            _ => {}
        }
    }

    fn table(&self) -> Table<'_, Msg> {
        Table {
            rows: u32::try_from(self.model.view().len()).expect("rows"),
            columns: self.defs,
            widths: self.columns.widths(),
            hidden: if self.hide_all {
                &[true; 3]
            } else {
                self.columns.hidden()
            },
            sorted: self.model.sort(),
            checks: self.checks.then_some(("Select", Tri::Off)),
            current: self.model.current_view(),
            status: None,
            context: true,
            on: Msg::Table,
        }
    }

    fn view(&self, cx: &mut Cx<'_, Msg>) {
        let table = self.table();
        cx.table(
            Element::table(TABLE)
                .style(Style {
                    height: Dim::Fixed(WINDOW.h),
                    ..Style::default()
                })
                .name("Programs"),
            &table,
            |at, cells: &mut Cells<'_>| {
                let r = self.model.view()[at as usize];
                let i = r as usize;
                // Every column's text, the hidden ones' too: the table leaves those out.
                for c in self.defs {
                    cells.push(match c.id {
                        NAME => &self.names[i],
                        VERSION => &self.versions[i],
                        _ => &self.sizes[i],
                    });
                }
                Row {
                    key: RowKey(u64::from(r)),
                    check: Some(Tri::Off),
                    locked: "",
                }
            },
        );
    }

    /// The column menu at `at` in a popup's Ui, sized to it and laid out, as its window shows.
    fn menu_ui(&self, at: Option<ColumnId>) -> Ui<Msg> {
        let mut ui = Ui::new(Size { w: 0.0, h: 0.0 }, 1.0, Motion::REDUCED);
        ui.set_active(true);
        let table = self.table();
        ui.build(|cx| cx.column_menu(MENU, &table, at, &LABELS));
        let size = ui.natural_size(&mut FakeText, WINDOW.w);
        ui.set_viewport(size);
        ui.layout(&mut FakeText);
        ui
    }
}

/// A Ui and its page, framed as the host frames them.
struct Run {
    ui: Ui<Msg>,
    page: Page,
    /// The widths the table sent from its layouts (fits), applied to the page as they came.
    fits: Vec<Msg>,
}

impl Run {
    fn new(rows: u32) -> Self {
        Self::with(Page::new(rows))
    }

    fn with(page: Page) -> Self {
        let mut run = Self {
            ui: Ui::new(WINDOW, 1.0, Motion::REDUCED),
            page,
            fits: Vec::new(),
        };
        run.ui.set_active(true);
        run.frame();
        run
    }

    /// As the host frames: build and lay out, again while the table asks, and the messages its
    /// layout made applied to the page, which builds once more.
    fn frame(&mut self) {
        for _ in 0..4 {
            let page = &self.page;
            self.ui.build(|cx| page.view(cx));
            self.ui.layout(&mut FakeText);
            let mut out = Vec::new();
            self.ui.take_messages(&mut out);
            for m in &out {
                self.page.update(*m);
            }
            let fitted = !out.is_empty();
            self.fits.extend(out);
            if !fitted && !self.ui.wants_build() {
                return;
            }
        }
        panic!("the table settles in four builds");
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

    fn key(&mut self, key: KeyCode) -> Vec<Msg> {
        let k = |down| {
            Input::Key(KeyEvent {
                key,
                down,
                mods: Mods::default(),
            })
        };
        self.send_all(&[k(true), k(false)])
    }

    fn click(&mut self, at: Point) -> Vec<Msg> {
        self.send_all(&[Input::PointerDown(at), Input::PointerUp(at)])
    }

    /// A press at `from`, the pointer to `to`, and the release there.
    fn drag(&mut self, from: Point, to: Point) -> Vec<Msg> {
        self.send_all(&[
            Input::PointerMove(from),
            Input::PointerDown(from),
            Input::PointerMove(to),
            Input::PointerUp(to),
        ])
    }

    fn table(&self) -> NodeId {
        self.ui.find(&[TABLE]).expect("the table")
    }

    fn rect(&self, id: NodeId) -> Rect {
        self.ui.info(id).expect("a live node").rect
    }

    fn header_node(&self) -> NodeId {
        self.ui.find(&[TABLE, TABLE_HEADER]).expect("the header")
    }

    /// Column `col`'s header, or the check column's.
    fn header(&self, col: Option<ColumnId>) -> NodeId {
        let key = col.map_or(TABLE_CHECK, |c| Key::index(u64::from(c.0)));
        self.ui
            .find(&[TABLE, TABLE_HEADER, key])
            .expect("the column header")
    }

    fn row(&self, r: u64) -> NodeId {
        self.ui
            .find(&[TABLE, TABLE_BODY, Key::index(r)])
            .expect("a row built")
    }

    /// Row `r`'s cell in column `col`.
    fn cell(&self, r: u64, col: ColumnId) -> NodeId {
        self.ui
            .find(&[
                TABLE,
                TABLE_BODY,
                Key::index(r),
                Key::index(u64::from(col.0)),
            ])
            .expect("the cell")
    }

    /// Column `col`'s divider: its header's right edge, at the header's middle.
    fn divider(&self, col: ColumnId) -> Point {
        Point {
            x: self.rect(self.header(Some(col))).right(),
            y: HEADER_Y,
        }
    }

    /// The column headers' widths, the check column's first.
    fn widths(&self) -> Vec<f32> {
        self.ui
            .children(self.header_node())
            .map(|id| self.rect(id).w)
            .collect()
    }

    /// The keys of `parent`'s children.
    fn keys(&self, parent: NodeId) -> Vec<Key> {
        self.ui
            .children(parent)
            .map(|id| self.ui.info(id).expect("a node").key)
            .collect()
    }

    fn content_w(&self) -> f32 {
        let body = self.ui.find(&[TABLE, TABLE_BODY]).expect("the rows' view");
        self.ui
            .info(body)
            .and_then(|n| n.scroll)
            .expect("a viewport")
            .content
            .w
    }
}

fn at_x(p: Point, dx: f32) -> Point {
    Point { x: p.x + dx, ..p }
}

fn col_key(c: ColumnId) -> Key {
    Key::index(u64::from(c.0))
}

// ---- widths ----------------------------------------------------------------------------------

#[test]
fn a_column_takes_the_width_the_user_left_it_at_least_its_minimum() {
    let mut page = Page::new(100);
    page.columns.resize(NAME, 200.0);
    page.columns.resize(SIZE, 10.0);
    let run = Run::with(page);
    // Name stops sharing; Size can't go under its 60 px.
    assert_eq!(run.widths(), [CHECK, 200.0, 80.0, 60.0]);
    assert_eq!(run.content_w(), CHECK + 200.0 + 80.0 + 60.0);
    assert_eq!(
        run.rect(run.cell(0, SIZE)).w,
        60.0,
        "the rows share the tracks"
    );
}

#[test]
fn columns_past_the_end_of_widths_and_hidden_have_their_own_width_and_show() {
    let mut page = Page::new(100);
    page.columns.resize(NAME, 200.0);
    let table = Table {
        widths: &page.columns.widths()[..1],
        hidden: &[],
        ..page.table()
    };
    let mut ui: Ui<Msg> = Ui::new(WINDOW, 1.0, Motion::REDUCED);
    for _ in 0..2 {
        ui.build(|cx| {
            cx.table(Element::table(TABLE), &table, |at, cells| {
                cells.push(&page.names[at as usize]);
                Row {
                    key: RowKey(u64::from(at)),
                    check: None,
                    locked: "",
                }
            });
        });
        ui.layout(&mut FakeText);
    }
    let header = ui.find(&[TABLE, TABLE_HEADER]).expect("the header");
    let widths: Vec<f32> = ui
        .children(header)
        .map(|id| ui.info(id).expect("a node").rect.w)
        .collect();
    assert_eq!(widths, [CHECK, 200.0, 80.0, 90.0]);
}

// ---- dividers --------------------------------------------------------------------------------

#[test]
fn a_drag_on_a_divider_resizes_live_and_sends_the_widths_on_release() {
    let mut run = Run::new(100);
    let at = run.divider(VERSION);
    assert_eq!(at.x, CHECK + 402.0 + 80.0);
    let to = at_x(at, 30.0);
    assert_eq!(
        run.send_all(&[
            Input::PointerMove(at),
            Input::PointerDown(at),
            Input::PointerMove(to)
        ]),
        []
    );
    // Live: Name, left of it, holds still; Size moves along, and the columns overflow.
    assert_eq!(run.widths(), [CHECK, 402.0, 110.0, 90.0]);
    assert_eq!(run.rect(run.cell(0, VERSION)).w, 110.0);
    assert_eq!(run.content_w(), CHECK + 402.0 + 110.0 + 90.0);
    assert_eq!(
        run.page.columns.widths(),
        &[Width::Auto; 3],
        "nothing sent yet"
    );
    // Let go: Name stops sharing at the width it holds, so nothing jumps after the drag.
    assert_eq!(
        run.send(Input::PointerUp(to)),
        [
            ev(TableEvent::Resize(NAME, 402.0)),
            ev(TableEvent::Resize(VERSION, 110.0))
        ]
    );
    assert_eq!(run.widths(), [CHECK, 402.0, 110.0, 90.0]);
}

#[test]
fn the_shared_columns_right_of_a_divider_take_up_the_difference() {
    let mut run = Run::with(Page::with_columns(100, &SHARED));
    // Name and Size share 492 px.
    assert_eq!(run.widths(), [CHECK, 246.0, 80.0, 246.0]);
    let at = run.divider(NAME);
    let to = at_x(at, 50.0);
    run.send_all(&[
        Input::PointerMove(at),
        Input::PointerDown(at),
        Input::PointerMove(to),
    ]);
    assert_eq!(
        run.widths(),
        [CHECK, 296.0, 80.0, 196.0],
        "the view stays full"
    );
    assert_eq!(
        run.send(Input::PointerUp(to)),
        [ev(TableEvent::Resize(NAME, 296.0))]
    );
    assert_eq!(run.widths(), [CHECK, 296.0, 80.0, 196.0]);
    // Name has its own width now: nothing left of Version shares.
    let at = run.divider(VERSION);
    assert_eq!(
        run.drag(at, at_x(at, -20.0)),
        [ev(TableEvent::Resize(VERSION, 60.0))]
    );
    assert_eq!(run.widths(), [CHECK, 296.0, 60.0, 216.0]);
}

#[test]
fn a_divider_drag_stops_at_the_column_s_minimum() {
    let mut run = Run::new(100);
    let at = run.divider(NAME);
    assert_eq!(
        run.drag(at, at_x(at, -400.0)),
        [ev(TableEvent::Resize(NAME, 120.0))]
    );
    assert_eq!(run.widths(), [CHECK, 120.0, 80.0, 90.0]);
}

#[test]
fn esc_or_the_window_going_inactive_puts_a_dragged_column_back() {
    let mut run = Run::new(100);
    let at = run.divider(VERSION);
    run.send_all(&[
        Input::PointerMove(at),
        Input::PointerDown(at),
        Input::PointerMove(at_x(at, 30.0)),
    ]);
    assert_eq!(run.key(KeyCode::Escape), []);
    assert_eq!(run.widths(), [CHECK, 402.0, 80.0, 90.0]);
    assert_eq!(run.send(Input::PointerMove(at_x(at, 60.0))), []);
    assert_eq!(run.widths(), [CHECK, 402.0, 80.0, 90.0], "the drag is over");
    assert_eq!(run.send(Input::PointerUp(at_x(at, 60.0))), []);

    run.send_all(&[Input::PointerDown(at), Input::PointerMove(at_x(at, -10.0))]);
    assert_eq!(run.widths(), [CHECK, 402.0, 70.0, 90.0]);
    run.ui.cancel_press();
    run.frame();
    assert_eq!(run.widths(), [CHECK, 402.0, 80.0, 90.0]);
    assert_eq!(run.send(Input::PointerUp(at_x(at, -10.0))), []);
    assert_eq!(run.page.columns.widths(), &[Width::Auto; 3]);
}

#[test]
fn a_press_on_a_divider_never_sorts() {
    let mut run = Run::new(100);
    // Version's strip reaches 4 px into Size's header, which sorts.
    let at = at_x(run.divider(VERSION), 3.0);
    assert!(run.rect(run.header(Some(SIZE))).contains(at));
    assert_eq!(run.click(at), []);
    assert_eq!(run.click(at_x(run.divider(SIZE), -2.0)), []);
    // Away from the strips, it sorts.
    let mid = at_x(run.divider(VERSION), 20.0);
    assert_eq!(run.click(mid), [ev(TableEvent::Sort(SIZE))]);
    assert_eq!(run.page.columns.widths(), &[Width::Auto; 3]);
}

#[test]
fn a_divider_past_the_view_drags_where_the_header_scrolled_it() {
    let mut run = Run::new(100);
    // 300 px: Name at its 120 px minimum makes 318 px of columns.
    run.ui.set_viewport(Size { w: 300.0, ..WINDOW });
    run.frame();
    let body = run.ui.find(&[TABLE, TABLE_BODY]).expect("the rows' view");
    assert!(run.ui.scroll_to(body, [100, 0]), "clamped at 18 px");
    run.frame();
    let at = run.divider(SIZE);
    assert_eq!(at.x, 300.0, "scrolled 18 px across");
    let back = at_x(at, -2.0);
    assert_eq!(
        run.drag(back, at_x(back, -20.0)),
        [
            ev(TableEvent::Resize(NAME, 120.0)),
            ev(TableEvent::Resize(SIZE, 70.0))
        ]
    );
    assert_eq!(run.widths(), [CHECK, 120.0, 80.0, 70.0]);
}

#[test]
fn the_pointer_shows_the_left_right_arrows_over_a_divider_and_while_dragging() {
    let mut run = Run::new(100);
    let at = run.divider(VERSION);
    run.send(Input::PointerMove(at));
    assert_eq!(run.ui.cursor(), Cursor::SizeWE);
    run.send(Input::PointerMove(at_x(at, -3.5)));
    assert_eq!(run.ui.cursor(), Cursor::SizeWE, "4 px either side");
    run.send(Input::PointerMove(at_x(at, -5.0)));
    assert_eq!(run.ui.cursor(), Cursor::Arrow);
    run.send(Input::PointerMove(Point {
        x: CHECK,
        y: HEADER_Y,
    }));
    assert_eq!(run.ui.cursor(), Cursor::Arrow, "the check column has none");
    run.send(Input::PointerMove(Point { y: 60.0, ..at }));
    assert_eq!(run.ui.cursor(), Cursor::Arrow, "the rows have none");
    run.send_all(&[Input::PointerMove(at), Input::PointerDown(at)]);
    let away = Point {
        x: at.x + 40.0,
        y: 200.0,
    };
    run.send(Input::PointerMove(away));
    assert_eq!(run.ui.cursor(), Cursor::SizeWE, "the drag holds it");
    run.send(Input::PointerUp(away));
    assert_eq!(run.ui.cursor(), Cursor::Arrow);
}

// Found in review: a 1 px wobble made a click a resize, which pinned the shared columns left of
// the divider, and made a double-click's second press a resize instead of a fit. A press becomes
// a drag past Windows' drag distance (SM_CXDRAG: 4 px at 100 %), as the common controls' do.
#[test]
fn a_press_that_moves_less_than_the_drag_distance_resizes_nothing() {
    let mut run = Run::new(100);
    let at = run.divider(VERSION);
    let near = at_x(at, 3.0);
    assert_eq!(
        run.send_all(&[
            Input::PointerMove(at),
            Input::PointerDown(at),
            Input::PointerMove(near)
        ]),
        []
    );
    assert_eq!(run.widths(), [CHECK, 402.0, 80.0, 90.0], "not a drag yet");
    assert_eq!(run.send(Input::PointerUp(near)), []);
    assert_eq!(run.page.columns.widths(), &[Width::Auto; 3]);
    let press = |double| Input::Press {
        at,
        mods: Mods::default(),
        double,
    };
    let wobble = at_x(at, -2.0);
    let out = run.send_all(&[
        press(false),
        Input::PointerUp(at),
        press(true),
        Input::PointerMove(wobble),
        Input::PointerUp(wobble),
    ]);
    assert_eq!(out, [ev(TableEvent::Fit(Some(VERSION)))]);
}

#[test]
fn past_the_drag_distance_the_edge_follows_the_pointer_and_a_drag_back_sends_nothing() {
    let mut run = Run::new(100);
    let at = run.divider(VERSION);
    run.send_all(&[Input::PointerMove(at), Input::PointerDown(at)]);
    run.send(Input::PointerMove(at_x(at, 5.0)));
    assert_eq!(
        run.widths(),
        [CHECK, 402.0, 85.0, 90.0],
        "the edge goes to the pointer"
    );
    run.send(Input::PointerMove(at_x(at, 2.0)));
    assert_eq!(
        run.widths(),
        [CHECK, 402.0, 82.0, 90.0],
        "a drag now, however near"
    );
    assert_eq!(run.send(Input::PointerUp(at)), [], "back where it started");
    // A double-click's second press dragged out and back is a drag that changed nothing, not a
    // fit.
    let out = run.send_all(&[
        Input::Press {
            at,
            mods: Mods::default(),
            double: true,
        },
        Input::PointerMove(at_x(at, 20.0)),
        Input::PointerMove(at),
        Input::PointerUp(at),
    ]);
    assert_eq!(out, []);
}

#[test]
fn the_drag_distance_is_the_system_s() {
    let mut run = Run::new(100);
    run.ui.set_drag_distance(10);
    let at = run.divider(VERSION);
    let to = at_x(at, 8.0);
    assert_eq!(run.drag(at, to), [], "8 px is less than 10");
    run.ui.set_drag_distance(0);
    assert_eq!(
        run.drag(at, at_x(at, 4.0)),
        [
            ev(TableEvent::Resize(NAME, 402.0)),
            ev(TableEvent::Resize(VERSION, 84.0))
        ],
        "0 is Windows' default, 4"
    );
}

// Found in review: the drag outlived its column. Its pointer shape stayed, the next Esc was taken
// for it, and a column moved to its left was held at another's width.
#[test]
fn columns_the_view_changes_mid_drag_end_it() {
    let mut run = Run::new(100);
    let at = run.divider(VERSION);
    let to = at_x(at, 30.0);
    run.send_all(&[
        Input::PointerMove(at),
        Input::PointerDown(at),
        Input::PointerMove(to),
    ]);
    assert_eq!(run.widths(), [CHECK, 402.0, 110.0, 90.0]);
    // Even a column right of it: the rule is simple, and the view rarely does it.
    run.page.columns.set_hidden(SIZE, true);
    run.frame();
    assert_eq!(
        run.ui.cursor(),
        Cursor::Arrow,
        "no divider under the pointer"
    );
    assert_eq!(run.send(Input::PointerMove(at_x(at, 60.0))), []);
    assert_eq!(run.widths(), [CHECK, 492.0, 80.0], "the drag is over");
    assert_eq!(run.send(Input::PointerUp(at_x(at, 60.0))), []);
    assert_eq!(run.page.columns.widths(), &[Width::Auto; 3]);
}

// Found in review: the wheel moved the hover during a divider's drag, which holds it still as a
// thumb's drag does.
#[test]
fn the_wheel_leaves_the_hover_alone_during_a_drag() {
    let mut run = Run::new(100);
    let at = run.divider(VERSION);
    run.send_all(&[Input::PointerMove(at), Input::PointerDown(at)]);
    let below = Point { y: 200.0, ..at };
    run.send(Input::PointerMove(below));
    let hover = run.ui.hover();
    run.send(Input::Wheel {
        at: below,
        delta: -120,
        horizontal: false,
    });
    assert_eq!(run.ui.hover(), hover);
    run.send(Input::PointerUp(below));
}

// ---- fit -------------------------------------------------------------------------------------

#[test]
fn a_double_click_on_a_divider_asks_to_fit_its_column() {
    let mut run = Run::new(100);
    let at = run.divider(VERSION);
    let press = |double| Input::Press {
        at,
        mods: Mods::default(),
        double,
    };
    let out = run.send_all(&[
        Input::PointerMove(at),
        press(false),
        Input::PointerUp(at),
        press(true),
        Input::PointerUp(at),
    ]);
    assert_eq!(out, [ev(TableEvent::Fit(Some(VERSION)))]);
    // The page asks, and the table measures: "Version" is 7 characters of 6 px in the header's
    // 12 px text, 12 px either side, wider than any version.
    assert_eq!(run.fits, [ev(TableEvent::Resize(VERSION, 66.0))]);
    assert_eq!(run.page.columns.width(VERSION), Width::Px(66.0));
    assert_eq!(run.widths(), [CHECK, 416.0, 66.0, 90.0]);
}

#[test]
fn a_column_to_fit_is_laid_out_at_its_fit_at_once_and_sent_once() {
    let mut page = Page::new(100);
    page.keep_fit = true;
    page.columns.fit(Some(VERSION));
    let mut run = Run::with(page);
    assert_eq!(run.fits, [ev(TableEvent::Resize(VERSION, 66.0))]);
    assert_eq!(run.widths(), [CHECK, 416.0, 66.0, 90.0]);
    run.fits.clear();
    run.frame();
    assert_eq!(run.fits, [], "the view still says fit: it was measured");
    assert_eq!(run.widths(), [CHECK, 416.0, 66.0, 90.0]);
}

// Found in review: a fit asked for with a text-size change waits for the rows to settle at the new
// size. The build that settles them, every row built already, changed nothing in the grid, so the
// fit waited for something else to lay the table out.
#[test]
fn a_fit_asked_for_before_the_rows_settle_is_measured_when_they_do() {
    let mut run = Run::new(3);
    run.page.columns.fit(Some(VERSION));
    run.ui.set_text_scale(1.25);
    run.frame();
    assert_eq!(run.fits.len(), 1, "{:?}", run.fits);
}

#[test]
fn a_fit_counts_a_sortable_header_s_chevron_and_the_widest_cell_built() {
    let mut page = Page::new(100);
    page.names[3] = "Program 00003 Professional Edition".to_owned();
    for s in &mut page.sizes {
        *s = "1 MB".to_owned();
    }
    page.columns.fit(None);
    let run = Run::with(page);
    // Name: 34 characters of 6.5 px; Size: "Size" in 12 px text and a 15 px chevron, wider than
    // its cells; Version: its header.
    assert_eq!(
        run.fits,
        [
            ev(TableEvent::Resize(NAME, 34.0 * 6.5 + 24.0)),
            ev(TableEvent::Resize(VERSION, 66.0)),
            ev(TableEvent::Resize(SIZE, 4.0 * 6.0 + 15.0 + 24.0)),
        ]
    );
    assert_eq!(run.widths(), [CHECK, 245.0, 66.0, 63.0]);
}

#[test]
fn a_fit_measures_the_rows_built_not_every_row() {
    let mut page = Page::new(1_000);
    page.names[900] = "Program 00900 Professional Edition".to_owned();
    page.columns.fit(Some(NAME));
    let mut run = Run::with(page);
    // 13 characters of 6.5 px and the padding are under Name's 120 px minimum.
    assert_eq!(run.fits, [ev(TableEvent::Resize(NAME, 120.0))]);
    run.fits.clear();
    // Row 900 made current is scrolled to and built: now its cells count. The fit is measured
    // once the build that made it has, not from the rows the view had before.
    run.page.model.set_current(900);
    run.page.columns.fit(Some(NAME));
    run.frame();
    assert_eq!(run.fits, [ev(TableEvent::Resize(NAME, 245.0))]);
}

// ---- hidden columns --------------------------------------------------------------------------

#[test]
fn hidden_columns_leave_the_header_the_rows_and_the_grid() {
    let mut page = Page::new(100);
    page.columns.set_hidden(VERSION, true);
    let mut run = Run::with(page);
    let shown = [TABLE_CHECK, col_key(NAME), col_key(SIZE)];
    assert_eq!(run.keys(run.header_node()), shown);
    assert_eq!(run.keys(run.row(0)), shown);
    assert_eq!(
        run.widths(),
        [CHECK, 482.0, 90.0],
        "Name shares what Version left"
    );
    let tree = run.ui.a11y_tree();
    let grid = node(&tree, &run.ui, run.table());
    assert_eq!(grid.column_count(), Some(3));
    let size = node(&tree, &run.ui, run.cell(0, SIZE));
    assert_eq!(size.column_index(), Some(2));
    run.page.columns.set_hidden(VERSION, false);
    run.frame();
    assert_eq!(run.widths(), [CHECK, 402.0, 80.0, 90.0]);
}

#[test]
fn hiding_or_showing_a_column_tells_a_client_the_column_indices_that_moved() {
    // Found in review: Size kept its place on the screen (Name took up Version's room) and its
    // index changed, so nothing sent its header or its cells again.
    let mut run = Run::new(100);
    drop(run.ui.a11y_tree());
    run.page.columns.set_hidden(VERSION, true);
    run.frame();
    let update = run.ui.a11y_update().expect("an update");
    assert_eq!(node(&update, &run.ui, run.table()).column_count(), Some(3));
    for id in [run.header(Some(SIZE)), run.cell(0, SIZE), run.cell(9, SIZE)] {
        assert_eq!(node(&update, &run.ui, id).column_index(), Some(2));
    }
    // Only those: the check column's header kept its place and its index.
    let check = run.header(None);
    assert!(!sent(&update, &run.ui, check));
    run.page.columns.set_hidden(VERSION, false);
    run.frame();
    let update = run.ui.a11y_update().expect("an update");
    for id in [run.header(Some(SIZE)), run.cell(0, SIZE), run.cell(9, SIZE)] {
        assert_eq!(node(&update, &run.ui, id).column_index(), Some(3));
    }
    // A width that changed moves no index.
    run.page.columns.resize(SIZE, 70.0);
    run.frame();
    let update = run.ui.a11y_update().expect("an update");
    assert!(!sent(&update, &run.ui, run.header(None)));
}

/// Whether `update` sends node `id`.
fn sent(update: &accesskit::TreeUpdate, ui: &Ui<Msg>, id: NodeId) -> bool {
    update
        .nodes
        .iter()
        .any(|(k, _)| ui.a11y_node(*k) == Some(id))
}

#[test]
fn focus_on_a_column_header_when_every_column_goes_goes_with_them() {
    // Found in review: with no check column, the header left empty, focus mended from nothing.
    let mut page = Page::new(100);
    page.checks = false;
    let mut run = Run::with(page);
    run.ui.set_focus(Some(run.header(Some(SIZE))));
    run.page.hide_all = true;
    run.frame();
    assert_eq!(run.keys(run.header_node()), []);
    assert_eq!(run.ui.focus(), None);
}

#[test]
fn a_row_is_named_by_its_first_shown_column() {
    let mut page = Page::new(100);
    page.columns.set_hidden(NAME, true);
    // The view hides Name, which the menu wouldn't: the page may.
    let mut run = Run::with(page);
    let tree = run.ui.a11y_tree();
    assert_eq!(node(&tree, &run.ui, run.row(2)).label(), Some("2.2"));
    let version = node(&tree, &run.ui, run.cell(2, VERSION));
    assert_eq!(version.role(), AkRole::RowHeader);
    assert_eq!(version.column_index(), Some(1));
}

#[test]
fn focus_on_a_column_header_that_goes_moves_to_the_next_one() {
    let mut run = Run::new(100);
    run.ui.set_focus(Some(run.header(Some(VERSION))));
    run.page.columns.set_hidden(VERSION, true);
    run.frame();
    assert_eq!(run.ui.focus(), Some(run.header(Some(SIZE))));
    run.page.columns.set_hidden(SIZE, true);
    run.frame();
    assert_eq!(
        run.ui.focus(),
        Some(run.header(Some(NAME))),
        "the last one's goes to the one before"
    );
}

// ---- the column menu -------------------------------------------------------------------------

#[test]
fn a_context_request_on_the_header_asks_for_the_column_menu() {
    let mut run = Run::new(100);
    run.ui.set_focus(Some(run.header(Some(VERSION))));
    assert_eq!(
        run.send(Input::ContextMenu(None)),
        [ev(TableEvent::ColumnMenu(Some(VERSION)))]
    );
    assert_eq!(run.ui.take_context(), None, "not a row's menu");
    assert_eq!(
        run.ui.popup_anchor(),
        Some(run.rect(run.header(Some(VERSION))))
    );
    let at = run.rect(run.header(Some(SIZE))).center();
    assert_eq!(
        run.send(Input::ContextMenu(Some(at))),
        [ev(TableEvent::ColumnMenu(Some(SIZE)))]
    );
    assert_eq!(run.ui.popup_anchor(), Some(Rect::new(at.x, at.y, 0.0, 0.0)));
    let check = run.rect(run.header(None)).center();
    assert_eq!(
        run.send(Input::ContextMenu(Some(check))),
        [ev(TableEvent::ColumnMenu(None))]
    );
    // Past the last column, still on the header.
    run.page.columns.resize(NAME, 200.0);
    run.frame();
    let past = Point {
        x: 500.0,
        y: HEADER_Y,
    };
    assert_eq!(
        run.send(Input::ContextMenu(Some(past))),
        [ev(TableEvent::ColumnMenu(None))]
    );
}

/// The menu's items: key, role, label, enabled and check.
fn items(ui: &Ui<Msg>) -> Vec<(Key, Role, String, bool, Tri)> {
    let menu = ui.find(&[MENU]).expect("the menu");
    ui.children(menu)
        .map(|id| ui.info(id).expect("an item"))
        .map(|n| (n.key, n.role, n.text.to_owned(), n.enabled, n.check))
        .collect()
}

fn item(
    key: Key,
    role: Role,
    text: &str,
    enabled: bool,
    check: Tri,
) -> (Key, Role, String, bool, Tri) {
    (key, role, text.to_owned(), enabled, check)
}

#[test]
fn the_column_menu_lists_the_hideable_columns_then_its_commands() {
    let mut page = Page::new(100);
    page.columns.set_hidden(SIZE, true);
    let ui = page.menu_ui(Some(VERSION));
    let sep = items(&ui)[2].0;
    assert_eq!(
        items(&ui),
        [
            item(col_key(VERSION), Role::MenuItem, "Version", true, Tri::On),
            item(col_key(SIZE), Role::MenuItem, "Size", true, Tri::Off),
            item(sep, Role::Separator, "", true, Tri::Off),
            item(
                COLUMN_FIT,
                Role::MenuItem,
                "Size column to fit",
                true,
                Tri::Off
            ),
            item(
                COLUMN_FIT_ALL,
                Role::MenuItem,
                "Size all columns to fit",
                true,
                Tri::Off
            ),
            item(
                COLUMN_RESET,
                Role::MenuItem,
                "Reset columns",
                true,
                Tri::Off
            ),
        ]
    );
    // From the check column's header, no column to fit.
    let ui = page.menu_ui(None);
    assert!(!items(&ui)[3].3, "Size column to fit is disabled");
}

#[test]
fn the_column_menu_s_items_send_the_table_s_events() {
    let mut page = Page::new(100);
    page.columns.set_hidden(SIZE, true);
    let mut ui = page.menu_ui(Some(VERSION));
    let mut click = |key: Key| {
        let id = ui.find(&[MENU, key]).expect("an item");
        let at = ui.info(id).expect("an item").rect.center();
        let mut out = Vec::new();
        ui.dispatch(Input::PointerDown(at), &mut out);
        ui.dispatch(Input::PointerUp(at), &mut out);
        out
    };
    assert_eq!(
        click(col_key(VERSION)),
        [ev(TableEvent::Show(VERSION, false))]
    );
    assert_eq!(click(col_key(SIZE)), [ev(TableEvent::Show(SIZE, true))]);
    assert_eq!(click(COLUMN_FIT), [ev(TableEvent::Fit(Some(VERSION)))]);
    assert_eq!(click(COLUMN_FIT_ALL), [ev(TableEvent::Fit(None))]);
    assert_eq!(click(COLUMN_RESET), [ev(TableEvent::ResetColumns)]);
}

#[test]
fn the_last_column_shown_can_t_be_hidden_from_the_menu() {
    let mut page = Page::with_columns(100, &ALL_HIDEABLE);
    page.columns.set_hidden(NAME, true);
    page.columns.set_hidden(VERSION, true);
    let ui = page.menu_ui(Some(SIZE));
    let enabled: Vec<bool> = items(&ui)[..3].iter().map(|i| i.3).collect();
    assert_eq!(enabled, [true, true, false], "Size is the last one shown");
}

#[test]
fn a_check_item_is_a_menu_check_box_to_a_client() {
    let mut page = Page::new(100);
    page.columns.set_hidden(SIZE, true);
    let mut ui = page.menu_ui(Some(VERSION));
    let tree = ui.a11y_tree();
    let menu = node(&tree, &ui, ui.find(&[MENU]).expect("the menu"));
    assert_eq!((menu.role(), menu.label()), (AkRole::Menu, Some("Columns")));
    let version = node(
        &tree,
        &ui,
        ui.find(&[MENU, col_key(VERSION)]).expect("an item"),
    );
    assert_eq!(
        (version.role(), version.toggled()),
        (AkRole::MenuItemCheckBox, Some(Toggled::True))
    );
    let size = node(
        &tree,
        &ui,
        ui.find(&[MENU, col_key(SIZE)]).expect("an item"),
    );
    assert_eq!(size.toggled(), Some(Toggled::False));
    let fit = node(&tree, &ui, ui.find(&[MENU, COLUMN_FIT]).expect("an item"));
    assert_eq!((fit.role(), fit.toggled()), (AkRole::MenuItem, None));
}

/// The first column of `rect` (logical, at scale 1) that has ink in it: a pixel unlike the one at
/// its right end, along its middle.
fn first_ink(pixels: &[u32], width: u32, rect: Rect) -> Option<i32> {
    let r = rect.snap(1.0);
    let w = usize::try_from(width).ok()?;
    let y = usize::try_from(i32::midpoint(r.y0, r.y1)).ok()?;
    let row = pixels.get(y * w..(y + 1) * w)?;
    let px = |x: i32| usize::try_from(x).ok().and_then(|x| row.get(x)).copied();
    let bg = px(r.x1 - 2)?;
    (r.x0..r.x1).find(|&x| px(x) != Some(bg))
}

#[test]
fn check_items_show_a_check_beside_a_shown_column_and_every_label_lines_up() {
    let mut page = Page::new(100);
    page.columns.set_hidden(SIZE, true);
    let ui = page.menu_ui(Some(VERSION));
    let (w, h) = ui.pixel_size();
    let mut pixels = vec![0; usize::try_from(u64::from(w) * u64::from(h)).expect("pixels")];
    {
        let mut canvas = Canvas::new(&mut pixels, w, h).expect("a canvas");
        ui.paint(&mut canvas, &mut FakeText, &Theme::LIGHT);
    }
    let rect = |key| {
        let id = ui.find(&[MENU, key]).expect("an item");
        ui.info(id).expect("an item").rect
    };
    let shown = first_ink(&pixels, w, rect(col_key(VERSION))).expect("a check");
    let hidden = first_ink(&pixels, w, rect(col_key(SIZE))).expect("a label");
    let plain = first_ink(&pixels, w, rect(COLUMN_FIT)).expect("a label");
    assert!(shown < hidden, "the check comes before the label");
    assert_eq!(hidden, plain, "labels line up, checks or not");
}

/// A menu of two items, Alpha a check item if `check`, sized to it and laid out.
fn two_items(ui: &mut Ui<Msg>, check: bool, alpha: &str) -> Size {
    let msg = ev(TableEvent::ResetColumns);
    ui.build(|cx| {
        cx.add_with(Element::menu(MENU), |cx| {
            cx.add(if check {
                Element::menu_check(Key::name("alpha"), alpha, true, msg)
            } else {
                Element::menu_item(Key::name("alpha"), alpha, msg)
            });
            cx.add(Element::menu_item(Key::name("beta"), "Beta", msg));
        });
    });
    let size = ui.natural_size(&mut FakeText, WINDOW.w);
    ui.set_viewport(size);
    ui.layout(&mut FakeText);
    size
}

fn menu_ui() -> Ui<Msg> {
    let mut ui = Ui::new(Size { w: 0.0, h: 0.0 }, 1.0, Motion::REDUCED);
    ui.set_active(true);
    ui
}

// Found in review: an item's labels are measured past the gutter only in a menu with a check item,
// and nothing measured the others again when one became or stopped being one.
#[test]
fn a_menu_that_gains_or_loses_a_check_item_is_measured_again() {
    let mut ui = menu_ui();
    let plain = two_items(&mut ui, false, "Alpha");
    let checked = two_items(&mut ui, true, "Alpha");
    assert_eq!(checked, two_items(&mut menu_ui(), true, "Alpha"));
    assert!(checked.w > plain.w, "{checked:?} past {plain:?}");
    assert_eq!(two_items(&mut ui, false, "Alpha"), plain);
}

// Found in review: a check item without a label never showed its check.
#[test]
fn a_check_item_without_a_label_shows_its_check() {
    let mut ui = menu_ui();
    two_items(&mut ui, true, "");
    let (w, h) = ui.pixel_size();
    let mut pixels = vec![0; usize::try_from(u64::from(w) * u64::from(h)).expect("pixels")];
    {
        let mut canvas = Canvas::new(&mut pixels, w, h).expect("a canvas");
        ui.paint(&mut canvas, &mut FakeText, &Theme::LIGHT);
    }
    let id = ui.find(&[MENU, Key::name("alpha")]).expect("an item");
    let rect = ui.info(id).expect("an item").rect;
    assert!(first_ink(&pixels, w, rect).is_some(), "a check");
}

// ---- accessibility helpers -------------------------------------------------------------------

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
