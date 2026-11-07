//! TD-0009 §7 U1: S-01's fling script on the product table (A2.7).
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

#![allow(
    dead_code,
    reason = "each file that includes the fixture uses a part of it"
)]

use dpro_ui::{
    Align, Canvas, Cells, Column, ColumnId, Cx, Dim, Element, FakeText, Key, Motion, NodeId, Order,
    Row, RowKey, Size, Style, TABLE_BODY, Table, TableEvent, Theme, Track, Tri, Ui,
    accesskit::TreeUpdate,
};

/// U1's table.
pub(crate) const ROWS: u32 = 100_000;
pub(crate) const WINDOW: Size = Size {
    w: 1000.0,
    h: 700.0,
};
const CELLS: usize = 8;
const KEY: Key = Key::name("table");

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

/// The table's events; the script sends none.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Msg(pub(crate) TableEvent);

/// The rows' text, made once.
pub(crate) struct Data {
    cells: Vec<[String; CELLS]>,
    checked: Vec<bool>,
}

impl Data {
    pub(crate) fn new() -> Self {
        const VENDORS: [&str; 4] = ["Contoso", "Fabrikam", "Northwind", "Tailspin"];
        const PRODUCTS: [&str; 4] = ["Updater Pro", "Toolbox", "Media Suite", "Backup Agent"];
        let n = ROWS as usize;
        Self {
            cells: (0..n)
                .map(|i| {
                    [
                        format!("{} {} {i}", VENDORS[i % 4], PRODUCTS[(i / 4) % 4]),
                        format!("{} Corporation", VENDORS[i % 4]),
                        format!("{}.{}.{}", i % 13, i % 100, i % 997),
                        format!("{} MB", (i * 37) % 4096),
                        format!("2026-{:02}-{:02}", i % 12 + 1, i % 28 + 1),
                        "Installed".to_owned(),
                        format!("C:\\Program Files\\App {i}"),
                        format!("{{{i:08X}-0000-0000-0000-000000000000}}"),
                    ]
                })
                .collect(),
            checked: (0..n).map(|i| i % 7 == 0).collect(),
        }
    }
}

fn view(data: &Data, cx: &mut Cx<'_, Msg>) {
    let table = Table {
        rows: ROWS,
        columns: &COLUMNS,
        widths: &[],
        hidden: &[],
        sorted: None,
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
            for text in &data.cells[i] {
                cells.push(text);
            }
            Row {
                key: RowKey(u64::from(at)),
                check: Some(if data.checked[i] { Tri::On } else { Tri::Off }),
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
        let (w, h) = f.ui.pixel_size();
        f.pixels = vec![0; usize::try_from(u64::from(w) * u64::from(h)).unwrap_or(0)];
        f.frame(0.0);
        f
    }

    fn body(&self) -> Option<NodeId> {
        self.ui.find(&[KEY, TABLE_BODY])
    }

    /// The most the rows scroll, and a page: logical pixels.
    fn extent(&self) -> (f32, f32) {
        let Some((view, content, line)) = self.body().and_then(|id| {
            let n = self.ui.info(id)?;
            let s = n.scroll?;
            Some((n.rect.h, s.content.h, s.line.h))
        }) else {
            return (0.0, 0.0);
        };
        ((content - view).max(0.0), view - line)
    }

    /// S-01's script for this table.
    pub(crate) fn script(&self) -> Script {
        let (max, page) = self.extent();
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

    /// One frame: the rows at `offset` (logical), built, laid out and the window painted whole.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "an offset within the table's 8 M px"
    )]
    pub(crate) fn frame(&mut self, offset: f32) {
        if let Some(body) = self.body() {
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
