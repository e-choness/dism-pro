//! TD-0009 A3.5 (10), slice 6b: grouped and two-line rows on A2's table, written before the code
//! (A3.9, A3.7's U18).
//!
//! - Rows have one height and group headers another, so the rows in view are still arithmetic:
//!   row `i` is at `i × row_h + (g + 1) × group_h`, `g` its group (a binary search), and a header
//!   at `start_g × row_h + g × group_h`. Rows before the first group have no header.
//! - A two-line row is both lines, the gap between them and the row padding, at least 52 px; a
//!   header is its caption line and 8 px above and below, at least 32 px. The column header stays
//!   a one-line row high.
//! - A header isn't a row of the view: the keyboard passes over it, it can't be current or
//!   checked, and a click on it does nothing. Down and Up cross groups; Home and End go to the
//!   first and last rows, a group's first row revealed with its header; Page Up and Page Down
//!   move by the view's height.
//! - Accessibility (A3.10 item 5): a header is a grid row with one row header spanning every
//!   column, named by the group's words; it has no selection. The grid's row count and every
//!   row's index count the headers, and a client's realize and search speak in those indices.
//! - A two-line cell is named by its first line and described by its second; a badge cell is
//!   named by its word.
//! - Nodes stay O(visible): the same count for 100 or 100k rows and 1 or 1,000 groups (U18).
//!
//! Every test here waits on the code (slice 6b's MEASURE commit has the API only).

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers: a node that isn't there, or a table that doesn't settle, fails the test"
)]

use dpro_ui::{
    Align, Canvas, CaretStop, Cells, Column, ColumnId, Cx, Dim, Element, FakeText, GlyphSink, Icon,
    Input, Key, KeyCode, KeyEvent, Mods, Motion, NodeId, Order, Point, Rect, Role, Row, RowGroup,
    RowKey, RowLines, Size, Style, TABLE_BODY, Table, TableEvent, TextMetrics, TextStyle,
    TextSystem, Theme, Track, Tri, Ui,
    accesskit::{self, Action, NodeId as AkId, Role as AkRole},
    composite::BadgeTone,
    table_group,
};

const NAME: ColumnId = ColumnId(0);
const SIZE: ColumnId = ColumnId(1);
const STATE: ColumnId = ColumnId(2);

const COLUMNS: [Column<'static>; 3] = [
    Column {
        id: NAME,
        header: "Path",
        width: Track::Share(1.0),
        min: 120.0,
        align: Align::Start,
        mono: true,
        sort: Some(Order::Ascending),
        hideable: false,
    },
    Column {
        id: SIZE,
        header: "Size",
        width: Track::Fixed(90.0),
        min: 60.0,
        align: Align::End,
        mono: false,
        sort: None,
        hideable: true,
    },
    Column {
        id: STATE,
        header: "State",
        width: Track::Fixed(110.0),
        min: 60.0,
        align: Align::Start,
        mono: false,
        sort: None,
        hideable: true,
    },
];

/// 600 × 400 at scale 1: the column header is a one-line row high, the rows' view the 364 px
/// under it.
const WINDOW: Size = Size { w: 600.0, h: 400.0 };
const HEAD: f32 = 36.0;
const VIEW: f32 = WINDOW.h - HEAD;
/// A two-line row: a 16.25 px body line, the 2 px gap, a 15 px caption line and 9 px above and
/// below make 51.25, under the 52 px token (A3.5 (10)).
const ROW_TWO: f32 = 52.0;
/// A one-line row (A2.10 item 2).
const ROW_ONE: f32 = 36.0;
/// A group header: a 15 px caption line and 8 px above and below make 31, under the 32 px token.
const GROUP: f32 = 32.0;
/// A cell's text from its sides.
const PAD_X: f32 = 12.0;
const TABLE: Key = Key::name("table");
/// Groups at 0, 3, 10 and 25 of 40 rows: 3, 7, 15 and 15 rows.
const STARTS: [u32; 4] = [0, 3, 10, 25];

#[derive(Clone, Copy, Debug, PartialEq)]
struct Msg(TableEvent);

fn current(at: u32) -> Msg {
    Msg(TableEvent::Current(at))
}

/// A row's path, its first line.
fn name(r: u32) -> String {
    format!("C:\\Data\\Item {r:05}")
}

/// A row's evidence, its second line.
fn evidence(r: u32) -> String {
    format!("Found under key {r}")
}

/// `s` for the test's life: the groups borrow their words.
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Leftovers' page: `rows` rows of a path, its evidence, a size and a state badge, in groups
/// starting at `starts`.
struct Page {
    rows: u32,
    lines: RowLines,
    groups: Vec<RowGroup<'static>>,
    current: Option<u32>,
    names: Vec<String>,
    evidence: Vec<String>,
    sizes: Vec<String>,
}

impl Page {
    fn new(rows: u32, starts: &[u32]) -> Self {
        let groups = starts
            .iter()
            .enumerate()
            .map(|(g, &start)| {
                let end = starts.get(g + 1).copied().unwrap_or(rows);
                let n = end - start;
                RowGroup {
                    start,
                    label: leak(format!("Group {g}")),
                    count: leak(format!("{n} items")),
                    name: leak(format!("Group {g}, {n} items")),
                }
            })
            .collect();
        Self {
            rows,
            lines: RowLines::Two,
            groups,
            current: None,
            names: (0..rows).map(name).collect(),
            evidence: (0..rows).map(evidence).collect(),
            sizes: (0..rows).map(|r| format!("{} KB", r % 1000)).collect(),
        }
    }

    fn view(&self, cx: &mut Cx<'_, Msg>) {
        let table = Table {
            rows: self.rows,
            lines: self.lines,
            groups: &self.groups,
            columns: &COLUMNS,
            widths: &[],
            hidden: &[],
            sorted: None,
            sort_status: ["Sorted ascending", "Sorted descending"],
            checks: Some(("Select", Tri::Off)),
            current: self.current,
            status: None,
            context: false,
            on: Msg,
        };
        cx.table(
            Element::table(TABLE)
                .style(Style {
                    height: Dim::Fixed(WINDOW.h),
                    ..Style::default()
                })
                .name("Leftovers"),
            &table,
            |at, cells: &mut Cells<'_>| {
                let i = us(at);
                cells.two(&self.names[i], &self.evidence[i]);
                cells.push(&self.sizes[i]);
                if at % 2 == 0 {
                    cells.badge("Undoable", BadgeTone::Undoable);
                } else {
                    cells.badge("Permanent", BadgeTone::Permanent);
                }
                Row {
                    key: RowKey(u64::from(at)),
                    check: Some(Tri::Off),
                    locked: "",
                }
            },
        );
    }
}

/// What a table's rows' view holds, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Row(u32),
    Head(u32),
}

/// A Ui and its page, framed as the host frames them.
struct Run {
    ui: Ui<Msg>,
    page: Page,
    /// The group starts the page was made with.
    starts: Vec<u32>,
}

impl Run {
    fn new(page: Page) -> Self {
        Self::scaled(page, 1.0)
    }

    fn scaled(page: Page, scale: f32) -> Self {
        let starts = page.groups.iter().map(|g| g.start).collect();
        let mut run = Self {
            ui: Ui::new(WINDOW, scale, Motion::REDUCED),
            page,
            starts,
        };
        run.ui.set_active(true);
        run.frame();
        run
    }

    /// The view built and laid out, and again if the table asks.
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

    /// `inputs`, the current row they move applied to the page, then a frame. Returns the
    /// messages.
    fn send_all(&mut self, inputs: &[Input]) -> Vec<Msg> {
        let mut out = Vec::new();
        for &input in inputs {
            self.ui.dispatch(input, &mut out);
        }
        for m in &out {
            if let Msg(TableEvent::Current(at)) = *m {
                self.page.current = Some(at);
            }
        }
        self.frame();
        out
    }

    /// A key pressed and released.
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
        self.send_all(&[
            Input::Press {
                at,
                mods: Mods::default(),
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
    fn row(&self, r: u32) -> Option<NodeId> {
        self.ui.find(&[TABLE, TABLE_BODY, Key::index(u64::from(r))])
    }

    /// Group `g`'s header row, if it's built.
    fn header(&self, g: u32) -> Option<NodeId> {
        self.ui.find(&[TABLE, TABLE_BODY, table_group(g)])
    }

    /// Row `r`'s cell in column `col`.
    fn cell(&self, r: u32, col: ColumnId) -> NodeId {
        self.ui
            .find(&[
                TABLE,
                TABLE_BODY,
                Key::index(u64::from(r)),
                Key::index(u64::from(col.0)),
            ])
            .expect("the cell")
    }

    fn rect(&self, id: NodeId) -> Rect {
        self.ui.info(id).expect("a live node").rect
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

    /// The rows scrolled to `y` logical pixels down, and framed.
    fn scroll_to(&mut self, y: f32) {
        let body = self.body();
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a test's offsets are small"
        )]
        let px = (y * self.ui.scale()).round() as i64;
        self.ui.scroll_to(body, [0, px]);
        self.frame();
    }

    /// Row `r` current and focused, as the keyboard leaves it.
    fn focus_row(&mut self, r: u32) {
        self.page.current = Some(r);
        self.frame();
        let id = self.row(r);
        self.ui.set_focus(id);
        self.frame();
    }

    fn focused_row(&self) -> Option<u32> {
        let focus = self.ui.focus()?;
        (0..self.page.rows).find(|&r| self.row(r) == Some(focus))
    }

    /// What the rows' view holds, in order. For small tables: it looks every key up.
    fn built(&self) -> Vec<Item> {
        let keys: Vec<Key> = self
            .ui
            .children(self.body())
            .map(|id| self.ui.info(id).expect("a node").key)
            .collect();
        let groups = u32::try_from(self.starts.len()).expect("groups");
        keys.iter()
            .map(|&k| {
                (0..self.page.rows)
                    .find(|&r| Key::index(u64::from(r)) == k)
                    .map(Item::Row)
                    .or_else(|| (0..groups).find(|&g| table_group(g) == k).map(Item::Head))
                    .expect("a row's or a header's key")
            })
            .collect()
    }

    // ---- the arithmetic, worked out here on its own -----------------------------------------

    /// How many groups start at or before row `r`: its header and those above it.
    fn groups_through(&self, r: u32) -> u32 {
        u32::try_from(self.starts.iter().filter(|&&s| s <= r).count()).expect("groups")
    }

    fn row_top(&self, r: u32) -> f32 {
        f(r) * ROW_TWO + f(self.groups_through(r)) * GROUP
    }

    fn head_top(&self, g: u32) -> f32 {
        f(self.starts[us(g)]) * ROW_TWO + f(g) * GROUP
    }

    /// Row `r`'s index as a client counts it: the headers above it counted.
    fn grid_row(&self, r: u32) -> u32 {
        r + self.groups_through(r)
    }

    fn content(&self) -> f32 {
        f(self.page.rows) * ROW_TWO + f(u32::try_from(self.starts.len()).expect("groups")) * GROUP
    }

    /// Everything that's at least partly between `lo` and `hi`, in order.
    fn items_between(&self, lo: f32, hi: f32) -> Vec<Item> {
        let mut items: Vec<(f32, Item)> = (0..self.page.rows)
            .map(|r| (self.row_top(r), ROW_TWO, Item::Row(r)))
            .chain(
                (0..u32::try_from(self.starts.len()).expect("groups"))
                    .map(|g| (self.head_top(g), GROUP, Item::Head(g))),
            )
            .filter(|&(top, h, _)| top < hi && top + h > lo)
            .map(|(top, _, item)| (top, item))
            .collect();
        items.sort_by(|a, b| a.0.total_cmp(&b.0));
        items.into_iter().map(|(_, item)| item).collect()
    }
}

#[allow(clippy::cast_precision_loss, reason = "a test's rows")]
fn f(n: u32) -> f32 {
    n as f32
}

fn ak(ui: &Ui<Msg>, tree: &accesskit::TreeUpdate, id: NodeId) -> AkId {
    tree.nodes
        .iter()
        .map(|(k, _)| *k)
        .find(|k| ui.a11y_node(*k) == Some(id))
        .expect("in the tree")
}

fn node<'t>(tree: &'t accesskit::TreeUpdate, ui: &Ui<Msg>, id: NodeId) -> &'t accesskit::Node {
    let k = ak(ui, tree, id);
    by_id(tree, k)
}

fn by_id(tree: &accesskit::TreeUpdate, k: AkId) -> &accesskit::Node {
    &tree.nodes.iter().find(|(n, _)| *n == k).expect("a node").1
}

// ---- geometry --------------------------------------------------------------------------------

#[test]
fn two_line_rows_headers_and_the_column_header_have_their_heights() {
    let run = Run::new(Page::new(40, &STARTS));
    let body = run.rect(run.body());
    assert_eq!(body.y, HEAD, "the column header is a one-line row high");
    assert_eq!(run.scroll().content.h, run.content());
    let row = run.rect(run.row(3).expect("built"));
    assert_eq!(row.h, ROW_TWO);
    let head = run.rect(run.header(1).expect("built"));
    assert_eq!(head.h, GROUP);
    // The same rows on one line each.
    let mut page = Page::new(40, &STARTS);
    page.lines = RowLines::One;
    let run = Run::new(page);
    assert_eq!(run.rect(run.row(3).expect("built")).h, ROW_ONE);
    assert_eq!(
        run.scroll().content.h,
        40.0 * ROW_ONE + 4.0 * GROUP,
        "headers as high on one-line rows"
    );
}

/// A3.9: going from y to a row and back, at every group's edges. Each offset builds what's in
/// view and half a view either side, headers and rows, each where the arithmetic puts it.
#[test]
fn rows_and_headers_round_trip_through_y_at_every_group_edge() {
    let mut run = Run::new(Page::new(40, &STARTS));
    let max = run.content() - VIEW;
    let mut offsets: Vec<f32> = Vec::new();
    for item in run.items_between(0.0, f32::INFINITY) {
        let top = match item {
            Item::Row(r) => run.row_top(r),
            Item::Head(g) => run.head_top(g),
        };
        // The item's edge at the view's top, at the build's top (half a view above) and at its
        // bottom (a view and a half below).
        for edge in [top, top - VIEW / 2.0, top - VIEW - VIEW / 2.0] {
            offsets.extend([edge - 1.0, edge, edge + 1.0]);
        }
    }
    offsets.retain(|&y| (0.0..=max).contains(&y));
    for y in offsets {
        run.scroll_to(y);
        assert_eq!(run.offset(), y);
        let lo = (y - VIEW / 2.0).max(0.0);
        let hi = y + VIEW + VIEW / 2.0;
        assert_eq!(run.built(), run.items_between(lo, hi), "at {y}");
        let origin = run.rect(run.body()).y - y;
        for item in run.built() {
            let (id, top, h) = match item {
                Item::Row(r) => (run.row(r), run.row_top(r), ROW_TWO),
                Item::Head(g) => (run.header(g), run.head_top(g), GROUP),
            };
            let rect = run.rect(id.expect("built"));
            assert_eq!((rect.y - origin, rect.h), (top, h), "{item:?} at {y}");
        }
    }
}

/// Rows before the first group have no header.
#[test]
fn rows_before_the_first_group_have_no_header() {
    let run = Run::new(Page::new(20, &[4, 12]));
    let origin = run.rect(run.body()).y;
    assert_eq!(run.rect(run.row(0).expect("built")).y, origin);
    assert_eq!(
        run.rect(run.row(3).expect("built")).y,
        origin + 3.0 * ROW_TWO
    );
    let head = run.rect(run.header(0).expect("built"));
    assert_eq!(head.y, origin + 4.0 * ROW_TWO);
    assert_eq!(run.rect(run.row(4).expect("built")).y, head.bottom());
    assert_eq!(run.scroll().content.h, 20.0 * ROW_TWO + 2.0 * GROUP);
}

/// A new text size keeps the row at the top of the view there, groups and all.
#[test]
fn a_text_size_change_keeps_the_top_row() {
    let mut run = Run::new(Page::new(40, &STARTS));
    run.scroll_to(run.row_top(12));
    run.ui.set_text_scale(2.0);
    run.frame();
    // 26 px text in 32.5 px lines, a 2 px gap, 24 px captions in 30 px lines and 18 px of
    // padding: 82.5, 83 px. Headers: 30 and 16, 46 px. Row 12 is under 3 of them.
    assert_eq!(run.offset(), 12.0 * 83.0 + 3.0 * 46.0);
    let row = run.rect(run.row(12).expect("built"));
    assert_eq!(row.y, run.rect(run.body()).y);
    assert_eq!(row.h, 83.0);
    assert_eq!(run.rect(run.header(2).expect("built")).h, 46.0);
}

// ---- the keyboard ----------------------------------------------------------------------------

/// A3.9: Down and Up cross groups, past the headers between them; Home and End go to the first
/// and last rows.
#[test]
fn down_and_up_cross_groups_past_their_headers() {
    let mut run = Run::new(Page::new(40, &STARTS));
    run.focus_row(2);
    assert_eq!(run.key(KeyCode::Down), [current(3)]);
    assert_eq!(
        run.focused_row(),
        Some(3),
        "focus follows, past group 1's header"
    );
    assert_eq!(run.key(KeyCode::Up), [current(2)]);
    assert_eq!(run.focused_row(), Some(2));

    // Row 9 at the view's bottom; Down reveals row 10 under group 2's header.
    run.focus_row(9);
    assert_eq!(run.offset(), run.row_top(9) + ROW_TWO - VIEW);
    assert_eq!(run.key(KeyCode::Down), [current(10)]);
    assert_eq!(run.offset(), run.row_top(10) + ROW_TWO - VIEW);

    assert_eq!(run.key(KeyCode::End), [current(39)]);
    assert_eq!(run.offset(), run.content() - VIEW);
    assert_eq!(run.key(KeyCode::Down), [], "the last row");
    assert_eq!(run.key(KeyCode::Home), [current(0)]);
    assert_eq!(run.offset(), 0.0, "group 0's header shows above row 0");
    assert_eq!(run.key(KeyCode::Up), [], "the first row");

    // Up into a group's first row from below reveals its header with it.
    run.scroll_to(run.row_top(26));
    run.focus_row(26);
    assert_eq!(run.key(KeyCode::Up), [current(25)]);
    assert_eq!(run.offset(), run.head_top(3));
}

/// Page Down and Page Up move by the view's height: a view's rows less one, as A2's do, in
/// pixels, so the headers in between count.
#[test]
fn page_down_and_up_move_by_the_view_s_height() {
    let mut run = Run::new(Page::new(40, &STARTS));
    run.focus_row(0);
    // 364 px hold 7 rows of 52: a page is 6 rows' 312 px. Row 0 is at 32, so the last row
    // starting by 344 is row 5 (324): rows 3 to 5 are under group 1's header.
    assert_eq!(run.key(KeyCode::PageDown), [current(5)]);
    // 324 + 312 = 636: row 10 (616), under group 2's header.
    assert_eq!(run.key(KeyCode::PageDown), [current(10)]);
    // 616 − 312 = 304: the first row from there is row 5 (324).
    assert_eq!(run.key(KeyCode::PageUp), [current(5)]);
    assert_eq!(run.key(KeyCode::PageUp), [current(0)]);
}

/// A header isn't a row of the view: a click on it does nothing.
#[test]
fn a_click_on_a_header_does_nothing() {
    let mut run = Run::new(Page::new(40, &STARTS));
    let head = run.header(1).expect("built");
    assert_eq!(run.click(run.rect(head).center()), []);
    assert_eq!(run.page.current, None);
    let info = run.ui.info(head).expect("a header");
    assert_eq!(info.selected, None, "never selected");
}

// ---- looks -----------------------------------------------------------------------------------

/// A header: its label at its start in caption semibold, its count at its end in caption, both
/// secondary (A3.5 (10), A3.10 item 10: no case transform).
#[test]
fn a_header_shows_its_label_and_count() {
    let run = Run::new(Page::new(40, &STARTS));
    let head = run.header(1).expect("built");
    let rect = run.rect(head);
    let words: Vec<(&str, TextStyle, Rect)> = descendants(&run.ui, head)
        .into_iter()
        .filter_map(|id| run.ui.info(id))
        .filter(|n| n.role == Role::Label)
        .map(|n| (n.text, n.text_style, n.rect))
        .collect();
    assert_eq!(words.len(), 2, "{words:?}");
    let (label, label_style, label_rect) = words[0];
    let (count, count_style, count_rect) = words[1];
    assert_eq!((label, count), ("Group 1", "7 items"));
    assert_eq!((label_style.size, label_style.weight), (12.0, 600));
    assert_eq!((count_style.size, count_style.weight), (12.0, 400));
    assert_eq!(label_rect.x, rect.x + PAD_X);
    assert_eq!(
        count_rect.right(),
        rect.right() - PAD_X,
        "the count at the end"
    );
}

/// Every node under `id`, depth first.
fn descendants(ui: &Ui<Msg>, id: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    for c in ui.children(id) {
        out.push(c);
        out.extend(descendants(ui, c));
    }
    out
}

/// `FakeText` that records what it draws.
#[derive(Default)]
struct Recorder(Vec<(String, TextStyle, Rect)>);

impl TextSystem for Recorder {
    fn measure(&mut self, text: &str, style: &TextStyle, max_width: f32) -> TextMetrics {
        FakeText.measure(text, style, max_width)
    }
    fn draw(
        &mut self,
        text: &str,
        style: &TextStyle,
        rect: Rect,
        scale: f32,
        sink: &mut dyn GlyphSink,
    ) {
        self.0.push((text.to_owned(), *style, rect));
        FakeText.draw(text, style, rect, scale, sink);
    }
    fn icon(&mut self, icon: Icon, rect: Rect, scale: f32, sink: &mut dyn GlyphSink) {
        FakeText.icon(icon, rect, scale, sink);
    }
    fn caret_stops(&mut self, text: &str, style: &TextStyle, out: &mut Vec<CaretStop>) {
        FakeText.caret_stops(text, style, out);
    }
}

/// A two-line cell draws its first line in its column's face and its second under it, the gap
/// apart, in caption text; the two are centred in the row together.
#[test]
fn a_two_line_cell_draws_its_second_line_under_its_first() {
    let mut run = Run::new(Page::new(40, &STARTS));
    let (w, h) = run.ui.pixel_size();
    let mut px = vec![0; usize::try_from(w * h).expect("pixels")];
    let mut canvas = Canvas::new(&mut px, w, h).expect("a canvas");
    let mut text = Recorder::default();
    run.ui.repaint_all();
    run.ui.paint(&mut canvas, &mut text, &Theme::LIGHT);
    let drawn = |s: &str| {
        text.0
            .iter()
            .find(|(t, ..)| t == s)
            .map_or_else(|| panic!("{s} drawn"), |&(_, style, rect)| (style, rect))
    };
    let (first, first_at) = drawn(&name(4));
    let (second, second_at) = drawn(&evidence(4));
    assert!(first.mono, "the path in its column's face");
    assert_eq!((first.size, second.size), (13.0, 12.0));
    assert!(!second.mono);
    assert_eq!(second_at.x, first_at.x);
    assert_eq!(second_at.y, first_at.y + 16.25 + 2.0);
    let row = run.rect(run.row(4).expect("built"));
    let block = 16.25 + 2.0 + 15.0;
    assert_eq!(first_at.y, row.y + (ROW_TWO - block) / 2.0);
}

/// A3.5 (10): a two-line cell is named by its first line, which names its row, and described by
/// its second. On one-line rows the second line is left out.
#[test]
fn a_two_line_cell_is_named_by_its_first_line_and_described_by_its_second() {
    let mut run = Run::new(Page::new(40, &STARTS));
    let tree = run.ui.a11y_tree();
    let row = node(&tree, &run.ui, run.row(4).expect("built"));
    assert_eq!(row.label(), Some(name(4).as_str()));
    let cell = node(&tree, &run.ui, run.cell(4, NAME));
    assert_eq!(cell.role(), AkRole::RowHeader);
    assert_eq!(cell.label(), Some(name(4).as_str()));
    assert_eq!(cell.description(), Some(evidence(4).as_str()));

    let mut page = Page::new(40, &STARTS);
    page.lines = RowLines::One;
    let mut run = Run::new(page);
    let tree = run.ui.a11y_tree();
    let cell = node(&tree, &run.ui, run.cell(4, NAME));
    assert_eq!(cell.label(), Some(name(4).as_str()));
    assert_eq!(cell.description(), None);
}

/// A badge cell holds the badge, centred in the row, and is named by its word; the badge itself
/// is the cell's to say.
#[test]
fn a_badge_cell_holds_its_badge_and_is_named_by_its_word() {
    let mut run = Run::new(Page::new(40, &STARTS));
    let cell = run.cell(4, STATE);
    let badge = run.ui.children(cell).next().expect("its badge");
    let info = run.ui.info(badge).expect("a badge");
    assert_eq!(info.text, "Undoable");
    assert_eq!((info.text_style.size, info.text_style.weight), (12.0, 600));
    let (c, b) = (run.rect(cell), info.rect);
    assert_eq!(b.x, c.x + PAD_X);
    // 15 px of caption and 3 px above and below.
    assert_eq!((b.y, b.h), (c.y + (ROW_TWO - 21.0) / 2.0, 21.0));
    let tree = run.ui.a11y_tree();
    let named = node(&tree, &run.ui, cell);
    assert_eq!(named.role(), AkRole::Cell);
    assert_eq!(named.label(), Some("Undoable"));
    assert!(
        named.children().is_empty(),
        "the badge is the cell's to say"
    );
    let odd = node(&tree, &run.ui, run.cell(5, STATE));
    assert_eq!(odd.label(), Some("Permanent"));
}

// ---- accessibility ---------------------------------------------------------------------------

/// A3.9: a header row is one row header spanning every column, named by the group's words, with
/// no selection; its label and count are its name's.
#[test]
fn a_header_is_a_row_of_one_spanning_unselectable_row_header() {
    let mut run = Run::new(Page::new(40, &STARTS));
    let tree = run.ui.a11y_tree();
    let ui = &run.ui;
    let head = node(&tree, ui, run.header(1).expect("built"));
    assert_eq!(head.role(), AkRole::Row);
    assert_eq!(
        head.row_index(),
        Some(4),
        "under rows 0 to 2 and two headers"
    );
    assert_eq!(head.is_selected(), None, "no SelectionItem");
    assert!(!head.supports_action(Action::Focus));
    assert_eq!(head.children().len(), 1);
    let cell = by_id(&tree, head.children()[0]);
    assert_eq!(cell.role(), AkRole::RowHeader);
    assert_eq!(cell.label(), Some("Group 1, 7 items"));
    assert_eq!(
        (cell.row_index(), cell.column_index(), cell.column_span()),
        (Some(4), Some(0), Some(4)),
        "the check column and three"
    );
    assert!(cell.children().is_empty(), "its words are its name");
    assert_eq!(
        node(&tree, ui, run.header(0).expect("built")).row_index(),
        Some(0)
    );
}

/// A3.9: the grid's row count, and every row's and cell's index, count the headers; a row's
/// place in the set doesn't.
#[test]
fn the_row_count_and_the_rows_indices_count_the_headers() {
    let mut run = Run::new(Page::new(40, &STARTS));
    let tree = run.ui.a11y_tree();
    let grid = node(&tree, &run.ui, run.table());
    assert_eq!((grid.row_count(), grid.column_count()), (Some(44), Some(4)));
    assert_eq!(grid.size_of_set(), Some(40), "the view's rows");
    assert_eq!(grid.scroll_y_max(), Some(f64::from(run.content() - VIEW)));
    let check = |run: &mut Run, rows: &[u32]| {
        let tree = run.ui.a11y_tree();
        let ui = &run.ui;
        for &r in rows {
            let row = node(&tree, ui, run.row(r).expect("built"));
            assert_eq!(row.row_index(), Some(us(run.grid_row(r))), "row {r}");
            assert_eq!(row.position_in_set(), Some(us(r)), "row {r}");
            let size = node(&tree, ui, run.cell(r, SIZE));
            assert_eq!(
                (size.row_index(), size.column_index()),
                (Some(us(run.grid_row(r))), Some(2))
            );
        }
    };
    check(&mut run, &[0, 2, 3, 9]);
    // Further down, under groups 2 and 3: from row 20, rows 16 to 29 are built.
    run.scroll_to(run.row_top(20));
    check(&mut run, &[16, 24, 25, 29]);
}

/// A3.9: a client realizes a row by its index, headers counted; the row, under its group, is
/// built and scrolled into view. A header's index reveals the header, and a search by name
/// answers in the same indices.
#[test]
fn realizing_a_row_under_a_group_counts_the_headers() {
    let starts: Vec<u32> = (0..10).map(|g| g * 100).collect();
    let mut run = Run::new(Page::new(1_000, &starts));
    // Row 750 is under 8 headers.
    assert_eq!(run.grid_row(750), 758);
    assert!(run.ui.realize(run.table(), 758));
    run.frame();
    let row = run.rect(run.row(750).expect("realized"));
    let body = run.rect(run.body());
    assert_eq!(
        row.bottom(),
        body.bottom(),
        "scrolled into view, at the bottom"
    );
    let tree = run.ui.a11y_tree();
    let ak_row = node(&tree, &run.ui, run.row(750).expect("realized"));
    assert_eq!(ak_row.row_index(), Some(758));

    // Group 9's header is at 900 + 9.
    assert!(run.ui.realize(run.table(), 909));
    run.frame();
    let head = run.rect(run.header(9).expect("realized"));
    let body = run.rect(run.body());
    assert!(
        head.y >= body.y && head.bottom() <= body.bottom(),
        "{head:?}"
    );

    assert!(run.ui.realize(run.table(), 1_009), "the last row");
    assert!(!run.ui.realize(run.table(), 1_010), "past the end");

    // Row 420 is under 5 headers.
    assert!(run.ui.find_row(run.table(), None, &name(420)));
    run.frame();
    assert_eq!(run.ui.take_found(run.table()), Some(425));
    assert!(run.row(420).is_some(), "realized");
    // After group 5's header (500 + 5), the next row named row 500's is row 500 itself (506).
    assert!(run.ui.find_row(run.table(), Some(505), &name(500)));
    run.frame();
    assert_eq!(run.ui.take_found(run.table()), Some(506));
    assert_eq!(run.grid_row(500), 506);
    assert!(run.ui.find_row(run.table(), Some(506), &name(500)));
    run.frame();
    assert_eq!(run.ui.take_found(run.table()), None);
}

// ---- nodes and idling ------------------------------------------------------------------------

/// U18: the same nodes for 100 or 100k rows, in 1 or 1,000 groups.
#[test]
fn nodes_stay_as_many_for_100k_rows_in_1000_groups() {
    let thousand: Vec<u32> = (0..1_000).map(|g| g * 100).collect();
    let mut small = Run::new(Page::new(100, &[0]));
    let mut large = Run::new(Page::new(100_000, &thousand));
    let mut one = Run::new(Page::new(100_000, &[0]));
    // The grid, the header and its 4 column headers, and the rows' view; group 0's header (a
    // row, its row header, the label and the count); and 10 rows (32 + 10 × 52 = 552 px reach
    // past the 546 built), each a row, its check cell, 3 cells and the badge.
    assert_eq!(small.ui.len(), 7 + 4 + 10 * 6);
    assert_eq!(large.ui.len(), small.ui.len());
    assert_eq!(one.ui.len(), small.ui.len());
    let nodes = small.ui.a11y_tree().nodes.len();
    assert_eq!(large.ui.a11y_tree().nodes.len(), nodes);
    assert_eq!(one.ui.a11y_tree().nodes.len(), nodes);
    // At the far end too, where no header is in range in either.
    large.page.current = Some(99_999);
    large.frame();
    one.page.current = Some(99_999);
    one.frame();
    assert_eq!(large.ui.len(), one.ui.len());
    assert!(large.ui.len() <= small.ui.len() + 6, "O(visible)");
}

/// U6: moving across groups, to the ends and realizing a far row each settle at once, and an
/// idle frame then finds nothing to do.
#[test]
fn every_move_settles_at_once() {
    let thousand: Vec<u32> = (0..1_000).map(|g| g * 100).collect();
    let mut run = Run::new(Page::new(100_000, &thousand));
    drop(run.ui.a11y_tree());
    run.focus_row(99);
    drop(run.ui.a11y_update());
    let idle = |run: &mut Run, when: &str| {
        assert!(!run.ui.wants_frame(), "{when}: asks for frames");
        assert!(!run.ui.wants_build(), "{when}: asks for a build");
        // The presenter took the move's frame, and a client its update.
        drop(run.ui.a11y_update());
        run.ui.clear_damage();
        let page = &run.page;
        run.ui.build(|cx| page.view(cx));
        run.ui.layout(&mut FakeText);
        assert!(
            run.ui.damage().is_empty(),
            "{when}: an idle frame damaged {:?}",
            run.ui.damage()
        );
        assert!(
            run.ui.a11y_update().is_none(),
            "{when}: an idle frame changed the tree"
        );
        assert!(
            !run.ui.wants_build(),
            "{when}: an idle frame asked for a build"
        );
    };
    for key in [
        KeyCode::Down,
        KeyCode::Up,
        KeyCode::PageDown,
        KeyCode::End,
        KeyCode::Home,
    ] {
        run.key(key);
        idle(&mut run, &format!("{key:?}"));
    }
    assert!(run.ui.realize(run.table(), 75_757));
    run.frame();
    idle(&mut run, "realize");
}

fn us(n: u32) -> usize {
    usize::try_from(n).expect("a test's index")
}
