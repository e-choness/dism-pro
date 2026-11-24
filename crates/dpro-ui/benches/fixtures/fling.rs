//! TD-0009 §7 U1: S-01's fling script on the product table (A2.7), and A3.7's U18 on its grouped,
//! two-line form (slice 6b).
//!
//! A 100k-row table of 8 columns with checks fills a 1000 × 700 window. The script, ported from
//! S-01 (§3.4), is an offset per 60 Hz frame:
//! - flings at 3,000, 6,000 and 12,000 px/s, then one back at 8,000, each decaying 0.95 a frame
//!   until it's under 20 px/s;
//! - a 90-frame thumb drag to the end, and a fling back from it at 12,000;
//! - Home, End, Home, and 30 page-downs.
//!
//! Each frame scrolls the rows' view to its offset, builds (twice when the rows in view changed),
//! lays out and repaints the whole window. Shared by `fling.rs` (the frame times) and `ir.rs`
//! (their instruction counts).
//!
//! [`Grouped`] is U18's table (A3.7): the same rows, two lines each (the name, and an evidence
//! line under it), in 1,000 groups of 100 under their header rows. Its events (Down across a
//! group's end, a far row realized for a client) run on it too, their frames ending at layout and
//! damage as U2's do. It's a type of its own, so U1's [`Fling`], and its counts, stay as they
//! were.

#![allow(
    dead_code,
    reason = "each file that includes the fixture uses a part of it"
)]

use dpro_ui::{
    Align, Bytes, Canvas, Cells, Column, ColumnId, Cx, Dim, Element, FakeText, Input,
    InvariantFormats, Key, Motion, NodeId, Order, Row, RowGroup, RowKey, RowLines, Size, SizeUnits,
    Style, TABLE_BODY, Table, TableEvent, Theme, Track, Tri, Ui, UnixMillis, accesskit::TreeUpdate,
};

/// U1's table.
pub(crate) const ROWS: u32 = 100_000;
/// U18's groups, of [`GROUP_ROWS`] rows each (A3.7).
pub(crate) const GROUPS: u32 = 1_000;
pub(crate) const GROUP_ROWS: u32 = ROWS / GROUPS;
pub(crate) const WINDOW: Size = Size {
    w: 1000.0,
    h: 700.0,
};
const CELLS: usize = 8;
/// The text columns: all but Size and Installed, whose values the table formats (slice 5b3).
const TEXT: usize = CELLS - 2;
pub(crate) const KEY: Key = Key::name("table");
/// The size units' labels, as the app's catalog gives them.
const UNITS: [&str; 7] = ["{} B", "{} KB", "{} MB", "{} GB", "{} TB", "{} PB", "{} EB"];
/// 2026-01-01; row `i` was installed `i % 336` days after it.
const FIRST_DAY: u64 = 1_767_225_600_000;
const DAY_MS: u64 = 86_400_000;
const VENDORS: [&str; 4] = ["Contoso", "Fabrikam", "Northwind", "Tailspin"];
const PRODUCTS: [&str; 4] = ["Updater Pro", "Toolbox", "Media Suite", "Backup Agent"];

/// Per-frame velocity decay of a fling, close to Windows' inertia.
const FRICTION: f32 = 0.95;
const FPS: f32 = 60.0;

const COLUMNS: [Column<'static>; CELLS] = [
    column(0, "Name", Track::Share(2.0), Align::Start, false),
    column(1, "Publisher", Track::Share(1.0), Align::Start, false),
    column(2, "Version", Track::Fixed(96.0), Align::Start, true),
    column(3, "Size", Track::Fixed(80.0), Align::End, false),
    column(4, "Installed", Track::Fixed(96.0), Align::Start, false),
    column(5, "Status", Track::Fixed(80.0), Align::Start, false),
    column(6, "Location", Track::Share(1.0), Align::Start, false),
    column(7, "ID", Track::Fixed(300.0), Align::Start, true),
];

const fn column(
    id: u16,
    header: &'static str,
    width: Track,
    align: Align,
    mono: bool,
) -> Column<'static> {
    Column {
        id: ColumnId(id),
        header,
        width,
        min: 64.0,
        align,
        mono,
        sort: Some(Order::Ascending),
        hideable: true,
    }
}

/// U18's grid row of data row `row`: the row, and a header row for each group up to its own
/// (TD-0009 A3.10 item 5), as a client counts them.
pub(crate) const fn grid_row(row: u32) -> u32 {
    row + row / GROUP_ROWS + 1
}

/// The table's events; the script sends none.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Msg(pub(crate) TableEvent);

/// The rows' text and values, made once.
pub(crate) struct Data {
    cells: Vec<[String; TEXT]>,
    sizes: Vec<Bytes>,
    days: Vec<UnixMillis>,
    checked: Vec<bool>,
}

impl Data {
    pub(crate) fn new() -> Self {
        let n = ROWS as usize;
        Self {
            cells: (0..n)
                .map(|i| {
                    [
                        format!("{} {} {i}", VENDORS[i % 4], PRODUCTS[(i / 4) % 4]),
                        format!("{} Corporation", VENDORS[i % 4]),
                        format!("{}.{}.{}", i % 13, i % 100, i % 997),
                        "Installed".to_owned(),
                        format!("C:\\Program Files\\App {i}"),
                        format!("{{{i:08X}-0000-0000-0000-000000000000}}"),
                    ]
                })
                .collect(),
            sizes: (0..n as u64)
                .map(|i| Bytes(((i * 37) % 4096) * 1_048_576 + (i * 7_919) % 1_048_576))
                .collect(),
            days: (0..n as u64)
                .map(|i| UnixMillis(FIRST_DAY + (i % 336) * DAY_MS))
                .collect(),
            checked: (0..n).map(|i| i % 7 == 0).collect(),
        }
    }
}

/// U18's rows: U1's, each with an evidence line, in [`GROUPS`] groups.
pub(crate) struct GroupedData {
    rows: Data,
    /// Each row's second line, its evidence under its name.
    second: Vec<String>,
    /// The groups, their words kept for the fixture's life.
    groups: Vec<RowGroup<'static>>,
}

impl GroupedData {
    pub(crate) fn new() -> Self {
        let count: &'static str = leak(format!("{GROUP_ROWS} items"));
        Self {
            rows: Data::new(),
            second: (0..ROWS as usize)
                .map(|i| format!("Found under HKLM\\SOFTWARE\\{}\\App {i}", VENDORS[i % 4]))
                .collect(),
            groups: (0..GROUPS)
                .map(|g| RowGroup {
                    start: g * GROUP_ROWS,
                    label: leak(format!("Group {g}")),
                    count,
                    name: leak(format!("Group {g}, {GROUP_ROWS} items")),
                })
                .collect(),
        }
    }
}

/// `s` for the fixture's life: the groups borrow their words.
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn view(data: &Data, cx: &mut Cx<'_, Msg>) {
    let table = Table {
        rows: ROWS,
        lines: RowLines::One,
        groups: &[],
        columns: &COLUMNS,
        widths: &[],
        hidden: &[],
        sorted: None,
        sort_status: [""; 2],
        checks: Some(("Select", Tri::Mixed)),
        current: Some(2),
        status: None,
        context: true,
        on: Msg,
    };
    cx.table(
        Element::table(KEY)
            .style(Style {
                height: Dim::Fixed(WINDOW.h),
                ..Style::default()
            })
            .name("Programs"),
        &table,
        |at, cells: &mut Cells<'_>| {
            let i = at as usize;
            let text = &data.cells[i];
            for t in &text[..3] {
                cells.push(t);
            }
            cells.size(data.sizes[i]);
            cells.date(data.days[i]);
            for t in &text[3..] {
                cells.push(t);
            }
            Row {
                key: RowKey(u64::from(at)),
                check: Some(if data.checked[i] { Tri::On } else { Tri::Off }),
                locked: "",
            }
        },
    );
}

/// U18's table (A3.7): [`view`]'s, its rows two lines each in [`GROUPS`] groups, `current` the
/// current row.
fn view_grouped(data: &GroupedData, current: Option<u32>, cx: &mut Cx<'_, Msg>) {
    let table = Table {
        rows: ROWS,
        lines: RowLines::Two,
        groups: &data.groups,
        columns: &COLUMNS,
        widths: &[],
        hidden: &[],
        sorted: None,
        sort_status: [""; 2],
        checks: Some(("Select", Tri::Mixed)),
        current,
        status: None,
        context: true,
        on: Msg,
    };
    cx.table(
        Element::table(KEY)
            .style(Style {
                height: Dim::Fixed(WINDOW.h),
                ..Style::default()
            })
            .name("Leftovers"),
        &table,
        |at, cells: &mut Cells<'_>| {
            let i = at as usize;
            let rows = &data.rows;
            let text = &rows.cells[i];
            cells.two(&text[0], &data.second[i]);
            for t in &text[1..3] {
                cells.push(t);
            }
            cells.size(rows.sizes[i]);
            cells.date(rows.days[i]);
            for t in &text[3..] {
                cells.push(t);
            }
            Row {
                key: RowKey(u64::from(at)),
                check: Some(if rows.checked[i] { Tri::On } else { Tri::Off }),
                locked: "",
            }
        },
    );
}

struct Builder {
    frames: Vec<f32>,
    max: f32,
}

impl Builder {
    fn go(&mut self, s: f32) -> f32 {
        let s = s.clamp(0.0, self.max);
        self.frames.push(s);
        s
    }

    fn fling(&mut self, start: f32, v0: f32) -> f32 {
        let (mut s, mut v) = (start, v0);
        while v.abs() > 20.0 {
            s = self.go(s + v / FPS);
            v *= FRICTION;
        }
        s
    }
}

/// S-01's script: an offset per frame, in logical pixels, and where the thumb drag starts.
pub(crate) struct Script {
    pub(crate) frames: Vec<f32>,
    pub(crate) drag: usize,
}

/// The table's rows' view in `ui`.
fn body(ui: &Ui<Msg>) -> Option<NodeId> {
    ui.find(&[KEY, TABLE_BODY])
}

/// The most the rows of the table in `ui` scroll, and a page: logical pixels.
fn extent(ui: &Ui<Msg>) -> (f32, f32) {
    let Some((view, content, line)) = body(ui).and_then(|id| {
        let n = ui.info(id)?;
        let s = n.scroll?;
        Some((n.rect.h, s.content.h, s.line.h))
    }) else {
        return (0.0, 0.0);
    };
    ((content - view).max(0.0), view - line)
}

/// S-01's script for the table in `ui`.
fn script(ui: &Ui<Msg>) -> Script {
    let (max, page) = extent(ui);
    let mut b = Builder {
        frames: Vec::new(),
        max,
    };
    let mut s = 0.0;
    for v0 in [3_000.0, 6_000.0, 12_000.0, -8_000.0] {
        s = b.fling(s, v0);
    }
    // The thumb dragged from here to the end in 1.5 s.
    let drag = b.frames.len();
    let from = s;
    for f in 1..=90_u8 {
        s = b.go(from + (max - from) * f32::from(f) / 90.0);
    }
    b.fling(s, -12_000.0);
    b.go(0.0);
    b.go(max);
    let mut s = b.go(0.0);
    for _ in 0..30 {
        s = b.go(s + page);
    }
    Script {
        frames: b.frames,
        drag,
    }
}

/// The rows of the table in `ui` scrolled to `offset` (logical) at `scale`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "an offset within the table's 8 M px"
)]
fn scroll(ui: &mut Ui<Msg>, scale: f32, offset: f32) {
    if let Some(body) = body(ui) {
        let px = (f64::from(offset) * f64::from(scale)).round() as i64;
        ui.scroll_to(body, [0, px]);
    }
}

/// The window painted whole into `pixels`.
fn paint(ui: &mut Ui<Msg>, pixels: &mut [u32]) {
    ui.repaint_all();
    let (w, h) = ui.pixel_size();
    if let Some(mut canvas) = Canvas::new(pixels, w, h) {
        ui.paint(&mut canvas, &mut FakeText, &Theme::LIGHT);
    }
}

/// A window's pixels at `ui`'s size.
fn pixels(ui: &Ui<Msg>) -> Vec<u32> {
    let (w, h) = ui.pixel_size();
    vec![0; usize::try_from(u64::from(w) * u64::from(h)).unwrap_or(0)]
}

/// The table in its window, painted whole each frame.
pub(crate) struct Fling {
    pub(crate) ui: Ui<Msg>,
    data: Data,
    pixels: Vec<u32>,
    scale: f32,
}

impl Fling {
    /// The table at `scale`, its first frame painted.
    pub(crate) fn new(scale: f32) -> Self {
        let mut f = Self {
            ui: Ui::new(WINDOW, scale, Motion::REDUCED),
            data: Data::new(),
            pixels: Vec::new(),
            scale,
        };
        f.ui.set_active(true);
        f.ui.set_formats(InvariantFormats::new(SizeUnits::new(UNITS)));
        f.pixels = pixels(&f.ui);
        f.frame(0.0);
        f
    }

    /// S-01's script for this table.
    pub(crate) fn script(&self) -> Script {
        script(&self.ui)
    }

    /// One frame: the rows at `offset` (logical), built, laid out and the window painted whole.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "an offset within the table's 8 M px"
    )]
    pub(crate) fn frame(&mut self, offset: f32) {
        if let Some(body) = body(&self.ui) {
            let px = (f64::from(offset) * f64::from(self.scale)).round() as i64;
            self.ui.scroll_to(body, [0, px]);
        }
        let data = &self.data;
        for _ in 0..2 {
            self.ui.build(|cx| view(data, cx));
            self.ui.layout(&mut FakeText);
            if !self.ui.wants_build() {
                break;
            }
        }
        self.ui.repaint_all();
        let (w, h) = self.ui.pixel_size();
        if let Some(mut canvas) = Canvas::new(&mut self.pixels, w, h) {
            self.ui.paint(&mut canvas, &mut FakeText, &Theme::LIGHT);
        }
        self.ui.clear_damage();
    }

    /// The whole accessibility tree, as a client first asks for it.
    pub(crate) fn a11y_tree(&mut self) -> TreeUpdate {
        self.ui.a11y_tree()
    }
}

/// An event on U18's table, as the platform delivers it (A3.7's `grouped::event.*`).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Step {
    Input(Input),
    /// The table's grid row (its group headers counted) built and scrolled into view, as UI
    /// Automation's realizer asks for it (TD-0009 A2.5, A3.10 item 5).
    Realize(u32),
}

/// U18's grouped, two-line table in its window (A3.7).
pub(crate) struct Grouped {
    pub(crate) ui: Ui<Msg>,
    data: GroupedData,
    pixels: Vec<u32>,
    scale: f32,
    /// The current row, which the events move.
    current: Option<u32>,
    /// Frames paint the whole window; without, they end at layout and damage (U2's events).
    paint: bool,
    /// Each frame takes the accessibility update, as the platform does while a client listens.
    accessible: bool,
    /// The events' messages.
    msgs: Vec<Msg>,
}

impl Grouped {
    /// The table at `scale`, its first frame painted.
    pub(crate) fn new(scale: f32) -> Self {
        let mut f = Self {
            ui: Ui::new(WINDOW, scale, Motion::REDUCED),
            data: GroupedData::new(),
            pixels: Vec::new(),
            scale,
            current: Some(2),
            paint: true,
            accessible: false,
            msgs: Vec::with_capacity(4),
        };
        f.ui.set_active(true);
        f.ui.set_formats(InvariantFormats::new(SizeUnits::new(UNITS)));
        f.pixels = pixels(&f.ui);
        f.frame(0.0);
        f
    }

    /// S-01's script for this table.
    pub(crate) fn script(&self) -> Script {
        script(&self.ui)
    }

    /// One frame: the rows at `offset` (logical), built, laid out and the window painted whole.
    pub(crate) fn frame(&mut self, offset: f32) {
        scroll(&mut self.ui, self.scale, offset);
        self.render();
    }

    /// The view built (twice when the rows in view changed) and laid out; the window painted
    /// whole unless frames end at layout; the accessibility update taken while a client listens.
    fn render(&mut self) {
        let data = &self.data;
        let current = self.current;
        for _ in 0..2 {
            self.ui.build(|cx| view_grouped(data, current, cx));
            self.ui.layout(&mut FakeText);
            if !self.ui.wants_build() {
                break;
            }
        }
        if self.paint {
            paint(&mut self.ui, &mut self.pixels);
        }
        self.ui.clear_damage();
        if self.accessible {
            drop(self.ui.a11y_update());
        }
    }

    /// The whole accessibility tree, as a client first asks for it.
    pub(crate) fn a11y_tree(&mut self) -> TreeUpdate {
        self.ui.a11y_tree()
    }

    /// From here, frames end at layout and damage, as U2's events do; the script's first fling
    /// and the way back up warm the table first, as a window the user has scrolled is.
    pub(crate) fn unpainted(mut self) -> Self {
        self.paint = false;
        let frames: Vec<f32> = self.script().frames.into_iter().take(60).collect();
        for offset in frames {
            self.frame(offset);
        }
        self.frame(0.0);
        self
    }

    /// From here, a client listens: the whole tree is served, then every frame's update taken.
    pub(crate) fn accessible(mut self) -> Self {
        drop(self.ui.a11y_tree());
        self.accessible = true;
        self.render();
        self
    }

    /// Row `row` current and focused, as the keyboard leaves it: scrolled into view, at the
    /// view's bottom if it was below.
    pub(crate) fn current_focused(mut self, row: u32) -> Self {
        self.current = Some(row);
        self.render();
        let id = self.ui.find(&[KEY, TABLE_BODY, Key::index(u64::from(row))]);
        self.ui.set_focus(id);
        self.render();
        self
    }

    /// One event through the UI thread: dispatched, the current row its messages move, and a
    /// frame.
    pub(crate) fn step(&mut self, step: Step) {
        match step {
            Step::Input(input) => {
                self.ui.dispatch(input, &mut self.msgs);
            }
            Step::Realize(at) => {
                if let Some(table) = self.ui.find(&[KEY]) {
                    self.ui.realize(table, at);
                }
            }
        }
        for Msg(e) in self.msgs.drain(..) {
            if let TableEvent::Current(at) = e {
                self.current = Some(at);
            }
        }
        self.render();
    }

    /// The current row.
    pub(crate) fn current(&self) -> Option<u32> {
        self.current
    }
}
