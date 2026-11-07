//! TD-0009 A2.5 slice 5b3: sizes, dates and times in the user's formats, written before the code.
//!
//! - A row's value cells take a size ([`Cells::size`]), a calendar day ([`Cells::date`]) or a
//!   moment ([`Cells::time`]); the window's [`Formats`] turn them into the text they show and
//!   read as.
//! - Sizes are base 1024 with three significant digits ([`format_size`]): whole bytes under
//!   1,000, and a value that would read 1,000 or more moves up a unit, up to exabytes. The
//!   decimal separator and the units' labels around the number are the locale's and the app's.
//! - A day is its midnight UTC, shown as that day wherever the user is. A moment is shown in local
//!   time; [`InvariantFormats`] (tests, benches, and a window's until its host sets the user's) has
//!   no time zone, ISO 8601 days and 24-hour times in UTC.
//! - A value is formatted once: a frame that changes nothing formats nothing, and neither do rows
//!   scrolled back into view. A value that changes is formatted again, and new formats
//!   ([`Ui::set_formats`]) format every value cell again at the next frame.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers: a node that isn't there, or a table that doesn't settle, fails the test"
)]

use std::{cell::Cell, rc::Rc};

use dpro_ui::{
    Align, Bytes, Cells, Column, ColumnId, Cx, Dim, Element, FakeText, Formats, Input,
    InvariantFormats, Key, Motion, NodeId, Order, Point, Row, RowKey, Size, SizeUnits, Style,
    TABLE_BODY, Table, TableEvent, Track, Ui, UnixMillis, accesskit, format_size,
};

const NAME: ColumnId = ColumnId(0);
const SIZE: ColumnId = ColumnId(1);
const INSTALLED: ColumnId = ColumnId(2);
const MODIFIED: ColumnId = ColumnId(3);

const COLUMNS: [Column<'static>; 4] = [
    column(NAME, "Name", Track::Share(1.0), Align::Start),
    column(SIZE, "Size", Track::Fixed(96.0), Align::End),
    column(INSTALLED, "Installed", Track::Fixed(112.0), Align::Start),
    column(MODIFIED, "Modified", Track::Fixed(160.0), Align::Start),
];

const fn column(id: ColumnId, header: &'static str, width: Track, align: Align) -> Column<'static> {
    Column {
        id,
        header,
        width,
        min: 64.0,
        align,
        mono: false,
        sort: Some(Order::Ascending),
        hideable: true,
    }
}

const WINDOW: Size = Size { w: 600.0, h: 400.0 };
const TABLE: Key = Key::name("table");
/// The app's labels, as its catalog gives them.
const UNITS: [&str; 7] = ["{} B", "{} KB", "{} MB", "{} GB", "{} TB", "{} PB", "{} EB"];
/// 2026-11-06 15:04:00 UTC.
const NOV_6_1504: u64 = 1_793_977_440_000;
/// 2026-11-06 00:00:00 UTC, the day.
const NOV_6: u64 = 1_793_923_200_000;
const DAY_MS: u64 = 86_400_000;

fn units() -> SizeUnits {
    SizeUnits::new(UNITS)
}

fn size(bytes: u64) -> String {
    let mut out = String::new();
    format_size(Bytes(bytes), ".", &units(), &mut out);
    out
}

#[test]
fn sizes_have_three_significant_digits_in_base_1024() {
    let cases: [(u64, &str); 22] = [
        (0, "0 B"),
        (1, "1 B"),
        (999, "999 B"),
        // A number never has four digits: 1,000 bytes are most of a kilobyte.
        (1_000, "0.98 KB"),
        (1_023, "1.00 KB"),
        (1_024, "1.00 KB"),
        (1_536, "1.50 KB"),
        // 9.995 KB rounds to 10.0, not 10.00.
        (10_235, "10.0 KB"),
        (10_240, "10.0 KB"),
        (102_400, "100 KB"),
        (1_023_487, "999 KB"),
        // 999.5 KB rounds to 1,000, which is most of a megabyte.
        (1_023_488, "0.98 MB"),
        (1_048_576, "1.00 MB"),
        (1_572_864, "1.50 MB"),
        // Rounded half up, once: 100.495 KB is 100, not 101.
        (102_906, "100 KB"),
        (102_907, "100 KB"),
        (5_000_000_000, "4.66 GB"),
        (1 << 40, "1.00 TB"),
        (1 << 50, "1.00 PB"),
        (1 << 60, "1.00 EB"),
        ((1 << 60) * 10, "10.0 EB"),
        (u64::MAX, "16.0 EB"),
    ];
    for (bytes, want) in cases {
        assert_eq!(size(bytes), want, "{bytes} bytes");
    }
}

#[test]
fn sizes_take_the_locale_s_decimal_separator_and_the_app_s_labels() {
    let french = SizeUnits::new(["{} o", "{} Ko", "{} Mo", "{} Go", "{} To", "{} Po", "{} Eo"]);
    let mut out = String::new();
    format_size(Bytes(1_536), ",", &french, &mut out);
    assert_eq!(out, "1,50 Ko");
    // A label may come before the number, and a separator may be more than a character.
    let before = SizeUnits::new(["B{}", "KB{}", "MB{}", "GB{}", "TB{}", "PB{}", "EB{}"]);
    out.clear();
    format_size(Bytes(1_572_864), "٫", &before, &mut out);
    assert_eq!(out, "MB1٫50");
    // It appends.
    format_size(Bytes(7), ".", &before, &mut out);
    assert_eq!(out, "MB1٫50B7");
    // Without labels, the number alone.
    out.clear();
    format_size(Bytes(1_536), ".", &SizeUnits::default(), &mut out);
    assert_eq!(out, "1.50");
}

#[test]
fn the_invariant_formats_read_iso_days_and_utc_times() {
    let f = InvariantFormats::new(units());
    let text = |put: &dyn Fn(&mut String)| {
        let mut out = String::new();
        put(&mut out);
        out
    };
    assert_eq!(text(&|o| f.date(UnixMillis(0), o)), "1970-01-01");
    assert_eq!(text(&|o| f.time(UnixMillis(0), o)), "1970-01-01 00:00");
    assert_eq!(text(&|o| f.date(UnixMillis(NOV_6), o)), "2026-11-06");
    assert_eq!(
        text(&|o| f.time(UnixMillis(NOV_6_1504), o)),
        "2026-11-06 15:04"
    );
    // The last millisecond of a leap day.
    assert_eq!(
        text(&|o| f.time(UnixMillis(951_868_799_999), o)),
        "2000-02-29 23:59"
    );
    assert_eq!(text(&|o| f.size(Bytes(1_536), o)), "1.50 KB");
    let comma = InvariantFormats::new(units()).with_decimal(",");
    assert_eq!(text(&|o| comma.size(Bytes(1_536), o)), "1,50 KB");
    // The far future still reads as something.
    assert_ne!(text(&|o| f.time(UnixMillis(u64::MAX), o)).len(), 0);
}

#[test]
fn a_table_shows_its_sizes_days_and_moments_formatted() {
    let run = Run::new(20);
    assert_eq!(run.text(0, NAME), "Program 0");
    assert_eq!(run.text(0, SIZE), "1.50 KB");
    assert_eq!(run.text(0, INSTALLED), "2026-11-06");
    assert_eq!(run.text(0, MODIFIED), "2026-11-06 15:04");
    assert_eq!(run.text(1, INSTALLED), "2026-11-07");
    assert_eq!(run.text(1, MODIFIED), "2026-11-07 15:05");
    // A client reads them as shown.
    let mut ui = run.ui;
    let tree = ui.a11y_tree();
    let id = ui
        .find(&[TABLE, TABLE_BODY, Key::index(0), Key::index(1)])
        .expect("the cell");
    assert_eq!(node(&tree, &ui, id).label(), Some("1.50 KB"));
}

#[test]
fn a_frame_that_changes_nothing_formats_nothing() {
    let mut run = Run::new(1_000);
    let built = run.calls.get();
    // Every value cell of the rows built, once: three per row.
    assert!(built > 0 && built.is_multiple_of(3), "{built}");
    run.frame();
    run.send(Input::PointerMove(Point { x: 5.0, y: 5.0 }));
    run.frame();
    assert_eq!(run.calls.get(), built);
}

#[test]
fn rows_scrolled_back_into_view_are_not_formatted_again() {
    let mut run = Run::new(1_000);
    let first = run.calls.get();
    for _ in 0..3 {
        run.wheel(-120);
    }
    let after_down = run.calls.get();
    assert!(after_down > first, "the rows that came into view");
    for _ in 0..3 {
        run.wheel(120);
    }
    assert_eq!(run.calls.get(), after_down, "the rows it built before");
}

#[test]
fn a_value_that_changes_is_formatted_again() {
    let mut run = Run::new(20);
    let before = run.calls.get();
    run.page.sizes[0] = Bytes(1_572_864);
    run.frame();
    assert_eq!(run.text(0, SIZE), "1.50 MB");
    assert_eq!(run.calls.get(), before + 1, "that cell alone");
}

#[test]
fn new_formats_show_in_every_value_cell_at_the_next_frame() {
    let mut run = Run::new(20);
    drop(run.ui.a11y_tree());
    let comma = Counting {
        calls: Rc::clone(&run.calls),
        inner: InvariantFormats::new(units()).with_decimal(","),
    };
    run.ui.set_formats(comma);
    let before = run.calls.get();
    run.frame();
    assert_eq!(run.text(0, SIZE), "1,50 KB");
    assert_eq!(run.text(0, NAME), "Program 0");
    assert!(run.calls.get() > before, "every value cell again");
    // A client hears of the new text.
    let update = run.ui.a11y_update().expect("an update");
    let id = run.cell(0, SIZE);
    assert_eq!(node(&update, &run.ui, id).label(), Some("1,50 KB"));
}

/// Text too long to keep is formatted at each build, and shown in full.
#[test]
fn a_value_too_long_to_keep_is_still_shown_in_full() {
    let mut run = Run::new(5);
    let calls = Rc::new(Cell::new(0));
    run.ui.set_formats(Long {
        calls: Rc::clone(&calls),
    });
    run.frame();
    let before = calls.get();
    assert!(before > 0);
    run.frame();
    assert_eq!(run.text(0, MODIFIED), LONG);
    assert_eq!(run.text(1, MODIFIED), LONG);
    assert!(calls.get() > before, "long text isn't kept");
}

// ---- the page ------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Msg {
    Table(TableEvent),
}

/// [`InvariantFormats`], counting the values it formats.
struct Counting {
    calls: Rc<Cell<u32>>,
    inner: InvariantFormats,
}

impl Formats for Counting {
    fn size(&self, bytes: Bytes, out: &mut String) {
        self.calls.set(self.calls.get() + 1);
        self.inner.size(bytes, out);
    }
    fn date(&self, day: UnixMillis, out: &mut String) {
        self.calls.set(self.calls.get() + 1);
        self.inner.date(day, out);
    }
    fn time(&self, t: UnixMillis, out: &mut String) {
        self.calls.set(self.calls.get() + 1);
        self.inner.time(t, out);
    }
}

/// A moment's text longer than any formats' cache keeps (128 bytes).
const LONG: &str = "Friday, the sixth of November, two thousand and twenty-six, at four minutes \
                    past three in the afternoon, Coordinated Universal Time, as the clock read";

/// Formats whose moments read as [`LONG`], counting them.
struct Long {
    calls: Rc<Cell<u32>>,
}

impl Formats for Long {
    fn size(&self, bytes: Bytes, out: &mut String) {
        format_size(bytes, ".", &SizeUnits::default(), out);
    }
    fn date(&self, _: UnixMillis, out: &mut String) {
        out.push('-');
    }
    fn time(&self, _: UnixMillis, out: &mut String) {
        self.calls.set(self.calls.get() + 1);
        out.push_str(LONG);
    }
}

/// Programs, each a size, a day and a moment: row `i` is 1.5 KB × (i + 1), installed on day
/// `i` after 2026-11-06, and modified then at 15:04 plus `i` minutes.
struct Page {
    names: Vec<String>,
    sizes: Vec<Bytes>,
    days: Vec<UnixMillis>,
    moments: Vec<UnixMillis>,
}

impl Page {
    fn new(rows: u32) -> Self {
        let n = u64::from(rows);
        Self {
            names: (0..n).map(|i| format!("Program {i}")).collect(),
            sizes: (0..n).map(|i| Bytes(1_536 * (i + 1))).collect(),
            days: (0..n).map(|i| UnixMillis(NOV_6 + i * DAY_MS)).collect(),
            moments: (0..n)
                .map(|i| UnixMillis(NOV_6_1504 + i * (DAY_MS + 60_000)))
                .collect(),
        }
    }

    fn view(&self, cx: &mut Cx<'_, Msg>) {
        let table = Table {
            rows: u32::try_from(self.names.len()).expect("rows"),
            columns: &COLUMNS,
            widths: &[],
            hidden: &[],
            sorted: None,
            checks: None,
            current: None,
            status: None,
            context: false,
            on: Msg::Table,
        };
        cx.table(
            Element::table(TABLE)
                .style(Style {
                    height: Dim::Fixed(WINDOW.h),
                    ..Style::default()
                })
                .name("Programs"),
            &table,
            |at, cells: &mut Cells<'_>| {
                let i = at as usize;
                cells.push(&self.names[i]);
                cells.size(self.sizes[i]);
                cells.date(self.days[i]);
                cells.time(self.moments[i]);
                Row {
                    key: RowKey(u64::from(at)),
                    check: None,
                    locked: "",
                }
            },
        );
    }
}

/// A Ui with [`Counting`] formats and its page, framed as the host frames them.
struct Run {
    ui: Ui<Msg>,
    page: Page,
    calls: Rc<Cell<u32>>,
}

impl Run {
    fn new(rows: u32) -> Self {
        let calls = Rc::new(Cell::new(0));
        let mut ui = Ui::new(WINDOW, 1.0, Motion::REDUCED);
        ui.set_formats(Counting {
            calls: Rc::clone(&calls),
            inner: InvariantFormats::new(units()),
        });
        ui.set_active(true);
        let mut run = Self {
            ui,
            page: Page::new(rows),
            calls,
        };
        run.frame();
        run
    }

    /// The view built and laid out, and again if the table asks.
    fn frame(&mut self) {
        for _ in 0..3 {
            let page = &self.page;
            self.ui.build(|cx| page.view(cx));
            self.ui.layout(&mut FakeText);
            if !self.ui.wants_build() {
                return;
            }
        }
        panic!("the table settles");
    }

    fn send(&mut self, input: Input) {
        let mut out = Vec::new();
        self.ui.dispatch(input, &mut out);
        self.frame();
    }

    /// A wheel notch (`delta` 120 is up) over the rows.
    fn wheel(&mut self, delta: i32) {
        let body = self.ui.find(&[TABLE, TABLE_BODY]).expect("the rows' view");
        let r = self.ui.info(body).expect("live").rect;
        let at = Point {
            x: r.x + r.w / 2.0,
            y: r.y + r.h / 2.0,
        };
        self.send(Input::PointerMove(at));
        self.send(Input::Wheel {
            at,
            delta,
            horizontal: false,
        });
    }

    fn cell(&self, row: u64, col: ColumnId) -> NodeId {
        self.ui
            .find(&[
                TABLE,
                TABLE_BODY,
                Key::index(row),
                Key::index(u64::from(col.0)),
            ])
            .expect("the cell")
    }

    fn text(&self, row: u64, col: ColumnId) -> String {
        self.ui
            .info(self.cell(row, col))
            .expect("live")
            .text
            .to_owned()
    }
}

fn node<'t>(tree: &'t accesskit::TreeUpdate, ui: &Ui<Msg>, id: NodeId) -> &'t accesskit::Node {
    tree.nodes
        .iter()
        .find(|(k, _)| ui.a11y_node(*k) == Some(id))
        .map(|(_, n)| n)
        .expect("in the tree")
}
