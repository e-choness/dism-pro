//! A synthetic shell page for TD-0009 §7 U2, U3, U8 and U12, and S-01's scripted session.
//!
//! A header, a rail of 12 pages, a toolbar (the filter, two tabs and 4 category check boxes), and
//! a data table (TD-0009 A2, slice 5b1) of 8 columns over 1,000 data rows, 750 on the first tab;
//! the drawer adds 20 rows, a progress bar and its actions, Hold to apply among them. The table
//! builds the rows in view and half a view either side. §7 estimates a real page at about 600
//! nodes. Pages 1 to 10
//! show 8 summary cards instead of the toolbar and table, so switching pages replaces the content,
//! and Settings (the last) shows a radio group and the Start page dropdown.
//!
//! Popups (TD-0009 A1): a row's context menu (5 items and a separator, the last item disabled
//! with a tooltip saying why), the Start page dropdown's list, and tooltips on the header's
//! drawer button and on Hold to apply. [`Run`] hosts the popup's [`Ui`] as the platform does.
//!
//! Shared by `interact.rs`, `a11y.rs`, `ir.rs` and the tests that replay the session.

#![allow(
    dead_code,
    reason = "each file that includes the fixture uses a part of it"
)]

use std::sync::OnceLock;

use dpro_ui::{
    Align, Axis, Canvas, Cells, ClipboardRequest, Column, ColumnId, Cx, Dim, Element, FakeText,
    Input, Insets, Justify, Key, KeyCode, KeyEvent, Look, Mods, Motion, Order, Point, Popup,
    PopupKind, Radius, Row, RowKey, ScrollBar, Size, Status, Style, TABLE_BODY, Table, TableEvent,
    TextEdit, TextStyle, Theme, Tone, Track, Tri, Ui, accesskit::TreeUpdate, tokens,
};

const RAIL: u8 = 12;
/// The last page: a radio group instead of summary cards.
pub(crate) const SETTINGS: u8 = RAIL - 1;
const CELLS: usize = 8;
pub(crate) const DRAWER_ROWS: u64 = 20;
const CATEGORIES: u8 = 4;
const DATA: usize = 1_000;
/// The text typed into the filter: 10 keystrokes.
const TYPED: &str = "contoso up";
const PAGES: [&str; RAIL as usize] = [
    "Overview", "Clean", "Repair", "Packages", "Features", "Drivers", "Updates", "Apps", "Startup",
    "Services", "Images", "Settings",
];
/// The table's tabs: every fourth data row is a Store app (Tailspin's), the rest desktop programs.
const TABS: [&str; 2] = ["Desktop programs", "Store apps"];
/// Settings' radio group.
const THEMES: [&str; 3] = ["System", "Light", "Dark"];
/// Settings' Start page dropdown: the pages the app may open on.
pub(crate) const START_PAGES: [&str; 4] = ["Overview", "Clean", "Repair", "Packages"];
/// A row's context menu: each item's label, and whether it's enabled. A separator follows the
/// second. The first is a danger item: it opens the drawer, and never applies.
pub(crate) const ROW_MENU: [(&str, bool); 5] = [
    ("Uninstall", true),
    ("Open file location", true),
    ("Copy name", true),
    ("Copy ID", true),
    ("Properties", false),
];
/// What the toolbar says after the row menu's item `i` (the first opens the drawer instead).
const DONE: [&str; 5] = ["", "Opened file location", "Copied name", "Copied ID", ""];
/// The table's columns, one per cell: the fixed widths, then shares of what they leave.
const COLUMNS: [Column<'static>; CELLS] = [
    column_of(0, "Name", Track::Share(2.0), Some(Order::Ascending)),
    column_of(1, "Publisher", Track::Share(1.0), Some(Order::Ascending)),
    Column {
        mono: true,
        ..column_of(2, "Version", Track::Fixed(96.0), None)
    },
    Column {
        align: Align::End,
        ..column_of(3, "Size", Track::Fixed(80.0), Some(Order::Descending))
    },
    column_of(4, "Installed", Track::Fixed(96.0), Some(Order::Descending)),
    column_of(5, "Status", Track::Fixed(80.0), None),
    column_of(6, "Location", Track::Share(1.0), None),
    Column {
        mono: true,
        ..column_of(7, "ID", Track::Fixed(300.0), None)
    },
];

const fn column_of(
    id: u16,
    header: &'static str,
    width: Track,
    sort: Option<Order>,
) -> Column<'static> {
    Column {
        id: ColumnId(id),
        header,
        width,
        min: 64.0,
        align: Align::Start,
        mono: false,
        sort,
    }
}
/// Why Properties is disabled: its tooltip.
pub(crate) const PROPERTIES_TIP: &str = "Not available for this program";
/// The header's drawer button's tooltip.
pub(crate) const DRAWER_TIP: &str = "Every change before it applies";
/// One frame at 60 Hz, rounded down to whole milliseconds.
pub(crate) const FRAME_MS: u64 = 16;

pub(crate) const START: Size = Size {
    w: 1000.0,
    h: 700.0,
};

/// A path as long as `MAX_PATH`, as pasted into the filter.
pub(crate) const PATH: &str = "C:\\Program Files\\Contoso\\Updater Pro\\Components\\Shared\\\
Plugins\\Telemetry\\Collectors\\Network\\Diagnostics\\Adapters\\Wireless\\Profiles\\Enterprise\\\
Certificates\\Intermediate\\Revocation\\Cache\\Entries\\2026\\November\\Week45\\Session0001\\\
Snapshot\\contoso-updater-telemetry-network-adapter-profile.json";

/// The longest text a field holds (64 KiB), as pasted into the filter: the worst case of a paste.
pub(crate) fn long_text() -> &'static str {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| "contoso updater ".repeat(dpro_ui::MAX_FIELD_LEN / 16))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Msg {
    Page(u8),
    Category(u8),
    Drawer,
    /// Data row `r` becomes the table's current row.
    Row(u32),
    /// What the table's keys, clicks and client actions ask for.
    Table(TableEvent),
    /// The filter field changed.
    Filter(TextEdit),
    /// Clears the filter, as Esc in the field would (the app's accelerator, not the field's).
    ClearFilter,
    /// A table tab picked.
    Tab(u8),
    /// Settings' theme picked.
    Theme(u8),
    /// The apply job's progress, in percent, as the engine reports it.
    Progress(u8),
    /// Held to the end: the changes apply.
    Apply,
    /// Hold to apply invoked by assistive technology: the typed confirmation opens instead.
    ConfirmApply,
    /// A context request found row `r`: it's selected and its menu opens.
    RowMenu(u32),
    /// The row menu's item `i`, from [`ROW_MENU`].
    RowAction(u8),
    /// Settings' Start page dropdown asks for its list.
    OpenStartPage,
    /// A start page picked from the list, by its place in [`START_PAGES`].
    StartPage(u8),
    /// Up (`false`) or Down on the closed dropdown: the page before or after.
    StepStartPage(bool),
    /// The open popup closes: the host's `on_close`.
    ClosePopup,
}

/// Which popup the shell has open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Open {
    None,
    /// Row `r`'s context menu.
    RowMenu(u32),
    /// The Start page dropdown's list.
    StartPages,
}

/// The page's view-model.
pub(crate) struct Shell {
    page: u8,
    /// The current data row, and its place in the table.
    selected: Option<u32>,
    current: Option<u32>,
    /// The checked data rows, and how many.
    checked: Vec<bool>,
    checked_count: usize,
    /// The table's sort: the rows in data order, or reversed by the column's header.
    sorted: Option<(ColumnId, Order)>,
    categories: [bool; CATEGORIES as usize],
    drawer: bool,
    filter: String,
    tab: u8,
    theme: u8,
    progress: u8,
    confirming: bool,
    applied: bool,
    /// How many times Hold to apply applied.
    applies: u32,
    /// The popup open.
    open: Open,
    /// The row menu's item last invoked, as the toolbar says.
    done: Option<u8>,
    /// Settings' Start page, in [`START_PAGES`].
    start_page: u8,
    /// Column 0 is the name the filter searches.
    cells: Vec<[String; CELLS]>,
    /// Data rows on the tab and matching the filter, in the table's order.
    visible: Vec<u32>,
}

impl Shell {
    pub(crate) fn new() -> Self {
        const VENDORS: [&str; 4] = ["Contoso", "Fabrikam", "Northwind", "Tailspin"];
        const PRODUCTS: [&str; 4] = ["Updater Pro", "Toolbox", "Media Suite", "Backup Agent"];
        let cells = (0..DATA)
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
            .collect();
        let mut s = Self {
            page: 0,
            selected: None,
            current: None,
            checked: vec![false; DATA],
            checked_count: 0,
            sorted: None,
            categories: [false; CATEGORIES as usize],
            drawer: false,
            // Room for the typed text, so typing doesn't allocate (U8).
            filter: String::with_capacity(64),
            tab: 0,
            theme: 0,
            progress: 0,
            confirming: false,
            applied: false,
            applies: 0,
            open: Open::None,
            done: None,
            start_page: 0,
            cells,
            visible: Vec::with_capacity(DATA),
        };
        s.refilter();
        s
    }

    pub(crate) fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Page(p) => self.page = p,
            Msg::Category(c) => {
                if let Some(on) = self.categories.get_mut(usize::from(c)) {
                    *on = !*on;
                }
            }
            Msg::Drawer => {
                self.drawer = !self.drawer;
                self.confirming = false;
            }
            Msg::Row(r) => self.select(Some(r)),
            Msg::Table(e) => self.table(e),
            Msg::Filter(edit) => {
                if edit.apply(&mut self.filter) {
                    self.refilter();
                }
            }
            Msg::ClearFilter => {
                self.filter.clear();
                self.refilter();
            }
            Msg::Tab(t) => {
                if self.tab != t {
                    self.tab = t;
                    self.refilter();
                }
            }
            Msg::Theme(t) => self.theme = t,
            Msg::Progress(p) => self.progress = p.min(100),
            Msg::Apply => {
                self.applies += 1;
                self.applied = true;
                self.confirming = false;
                self.progress = 100;
            }
            Msg::ConfirmApply => self.confirming = true,
            Msg::RowMenu(r) => {
                self.select(Some(r));
                self.open = Open::RowMenu(r);
            }
            // A danger item opens the drawer: a menu never applies (A1.10 item 3).
            Msg::RowAction(0) => {
                self.drawer = true;
                self.confirming = false;
            }
            Msg::RowAction(i) => self.done = Some(i),
            Msg::OpenStartPage => self.open = Open::StartPages,
            Msg::StartPage(p) => self.start_page = p.min(last(&START_PAGES)),
            Msg::StepStartPage(down) => {
                self.start_page = if down {
                    (self.start_page + 1).min(last(&START_PAGES))
                } else {
                    self.start_page.saturating_sub(1)
                };
            }
            Msg::ClosePopup => self.open = Open::None,
        }
    }

    /// How many times Hold to apply applied.
    pub(crate) fn applies(&self) -> u32 {
        self.applies
    }

    /// The row menu's item last invoked.
    pub(crate) fn done(&self) -> Option<u8> {
        self.done
    }

    /// Settings' Start page, in [`START_PAGES`].
    pub(crate) fn start_page(&self) -> u8 {
        self.start_page
    }

    /// Whether the drawer is open.
    pub(crate) fn drawer(&self) -> bool {
        self.drawer
    }

    /// The selected row.
    pub(crate) fn selected(&self) -> Option<u32> {
        self.selected
    }

    /// The menu or list open (`Model::popup`).
    pub(crate) fn popup(&self) -> Option<Popup<Msg>> {
        let kind = match self.open {
            Open::None => return None,
            Open::RowMenu(_) => PopupKind::Menu,
            Open::StartPages => PopupKind::List,
        };
        Some(Popup {
            kind,
            on_close: Msg::ClosePopup,
        })
    }

    /// The message opening the context menu of the node keyed `key`: a shown row's
    /// (`Model::context_menu`).
    pub(crate) fn context_menu(&self, key: Key) -> Option<Msg> {
        self.visible
            .iter()
            .find(|&&r| Key::index(u64::from(r)) == key)
            .map(|&r| Msg::RowMenu(r))
    }

    fn refilter(&mut self) {
        self.visible.clear();
        let (filter, store) = (self.filter.as_str(), self.tab == 1);
        self.visible.extend(
            (0u32..)
                .zip(&self.cells)
                .filter(|(i, c)| (i % 4 == 3) == store && contains_folded(&c[0], filter))
                .map(|(i, _)| i),
        );
        if self.sorted.is_some_and(|(_, o)| o == Order::Descending) {
            self.visible.reverse();
        }
        self.select(self.selected);
    }

    /// Data row `r` (or none) becomes current, at its place in the table if it's there.
    fn select(&mut self, r: Option<u32>) {
        self.selected = r;
        self.current = r
            .and_then(|r| self.visible.iter().position(|&v| v == r))
            .and_then(|at| u32::try_from(at).ok());
    }

    /// The table's event: the fixture's checks toggle (Shift's range is the model's, not
    /// measured here), and a sort reverses the rows.
    fn table(&mut self, e: TableEvent) {
        let row = |at: u32| self.visible.get(at as usize).copied();
        match e {
            TableEvent::Current(at) => self.select(row(at)),
            TableEvent::Check(at, _) => {
                if let Some(r) = row(at) {
                    let on = &mut self.checked[r as usize];
                    *on = !*on;
                    if *on {
                        self.checked_count += 1;
                    } else {
                        self.checked_count -= 1;
                    }
                    self.select(Some(r));
                }
            }
            TableEvent::CheckAll => {
                let on = self.checked_count < DATA;
                self.checked.fill(on);
                self.checked_count = if on { DATA } else { 0 };
            }
            TableEvent::Activate(at) => {
                self.select(row(at));
                self.drawer = true;
            }
            TableEvent::Sort(col) => {
                let order = match self.sorted {
                    Some((c, Order::Ascending)) if c == col => Order::Descending,
                    _ => Order::Ascending,
                };
                let reversed = self.sorted.is_some_and(|(_, o)| o == Order::Descending);
                if reversed != (order == Order::Descending) {
                    self.visible.reverse();
                }
                self.sorted = Some((col, order));
                self.select(self.selected);
            }
        }
    }

    /// The check column header's box.
    fn all_checked(&self) -> Tri {
        match self.checked_count {
            0 => Tri::Off,
            DATA => Tri::On,
            _ => Tri::Mixed,
        }
    }
}

/// The last place in `items`.
fn last(items: &[&str]) -> u8 {
    u8::try_from(items.len().saturating_sub(1)).unwrap_or(u8::MAX)
}

/// ASCII case-insensitive containment without allocating (the real table folds once, TD-0009 §5.1).
fn contains_folded(hay: &str, needle: &str) -> bool {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    n.is_empty() || h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

fn column(gap: f32) -> Style {
    Style {
        axis: Axis::Column,
        gap,
        ..Style::default()
    }
}

fn row(gap: f32) -> Style {
    Style {
        axis: Axis::Row,
        gap,
        align: Align::Center,
        ..Style::default()
    }
}

/// A control at least `height` high, `pad` from its text at each side (doc 05: 30, 36 or 40 px).
/// At least, not exactly: larger text (Windows' text size, up to 225 %) makes it taller.
fn sized(height: f32, pad: f32) -> Style {
    Style {
        min: at_least(height),
        padding: Insets::xy(pad, 0.0),
        ..Style::default()
    }
}

/// No narrower than its content, and at least `height` high: what the canvas's fixed heights
/// become, so that text scaled up reflows instead of clipping.
fn at_least(height: f32) -> Size {
    Size { w: 0.0, h: height }
}

/// A card: surface, hairline border, rounded by `radius`.
fn card(radius: Radius) -> Look {
    Look {
        fill: Some(Tone::Surface),
        border: Some(Tone::Border),
        radius,
        ..Look::PLAIN
    }
}

fn ink(tone: Tone) -> Look {
    Look {
        ink: tone,
        ..Look::PLAIN
    }
}

/// A control-rounded button look, `fill`ed when given.
fn control(fill: Option<Tone>) -> Look {
    Look {
        fill,
        radius: Radius::Control,
        ..Look::PLAIN
    }
}

const BODY: TextStyle = TextStyle {
    size: 14.0,
    weight: 400,
    mono: false,
    wrap: false,
};
const STRONG: TextStyle = TextStyle {
    weight: 600,
    ..BODY
};
const TITLE: TextStyle = TextStyle {
    size: 20.0,
    weight: 600,
    ..BODY
};
const MONO: TextStyle = TextStyle { mono: true, ..BODY };

/// The whole page; the root's children.
pub(crate) fn view(s: &Shell, cx: &mut Cx<'_, Msg>) {
    cx.add_with(
        Element::stack(Key::name("header")).style(Style {
            padding: Insets::xy(16.0, 8.0),
            min: at_least(48.0),
            justify: Justify::SpaceBetween,
            ..row(12.0)
        }),
        |cx| {
            cx.add(Element::text(Key::name("title"), "DISM Pro").text_style(TITLE));
            cx.add(Element::text(Key::name("target"), "This PC (online)").look(ink(Tone::Accent)));
            cx.add(
                Element::button(
                    Key::name("drawer"),
                    if s.drawer {
                        "Hide changes"
                    } else {
                        "Review changes"
                    },
                    Msg::Drawer,
                )
                .style(sized(32.0, 16.0))
                .text_style(STRONG)
                .look(Look::PRIMARY)
                .tooltip(DRAWER_TIP),
            );
        },
    );
    cx.add_with(
        Element::stack(Key::name("body")).style(Style {
            grow: 1.0,
            align: Align::Stretch,
            ..row(0.0)
        }),
        |cx| {
            rail(s, cx);
            match s.page {
                0 => content(s, cx),
                SETTINGS => settings(s, cx),
                _ => summary(s, cx),
            }
            if s.drawer {
                drawer(s, cx);
            }
        },
    );
}

/// A list: one Tab stop, the arrow keys move through its pages, Enter or Space opens one.
fn rail(s: &Shell, cx: &mut Cx<'_, Msg>) {
    cx.add_with(
        Element::list(Key::name("rail"))
            .style(Style {
                width: Dim::Fixed(200.0),
                padding: Insets::all(8.0),
                align: Align::Stretch,
                ..column(4.0)
            })
            .look(Look {
                fill: Some(Tone::Subtle),
                ..Look::PLAIN
            })
            .name("Pages"),
        |cx| {
            for (p, label) in (0..RAIL).zip(PAGES) {
                cx.add(
                    Element::button(Key::index(u64::from(p)), label, Msg::Page(p))
                        .style(sized(36.0, 12.0))
                        .look(control((p == s.page).then_some(Tone::Selected)))
                        .selected(p == s.page),
                );
            }
        },
    );
}

/// The table page's toolbar: the filter, the program types, the selection and the categories.
fn toolbar(s: &Shell, cx: &mut Cx<'_, Msg>) {
    const NAMES: [&str; CATEGORIES as usize] = ["Apps", "Games", "Tools", "System"];
    cx.add(
        Element::field(Key::name("filter"), &s.filter, Msg::Filter)
            .name("Filter rows")
            .placeholder("Filter")
            .style(Style {
                width: Dim::Fixed(240.0),
                padding: Insets::xy(8.0, 4.0),
                ..Style::default()
            })
            .look(card(Radius::Control)),
    );
    // Artboard 08's segmented tabs: the arrow keys pick, one Tab stop.
    cx.add_with(
        Element::tabs(Key::name("tabs"))
            .name("Program types")
            .style(Style {
                padding: Insets::all(3.0),
                ..row(2.0)
            })
            .look(Look::TABS),
        |cx| {
            for (t, label) in (0..).zip(TABS) {
                let on = s.tab == t;
                cx.add(
                    Element::tab(Key::index(u64::from(t)), label, on, Msg::Tab(t))
                        .style(sized(30.0, 12.0))
                        .text_style(if on { STRONG } else { BODY })
                        .look(if on { Look::TAB_SELECTED } else { Look::TAB }),
                );
            }
        },
    );
    let selection = match (s.done, s.selected) {
        (Some(i), _) => DONE[usize::from(i)],
        (None, Some(_)) => "1 selected",
        (None, None) => "",
    };
    cx.add(Element::text(Key::name("selection"), selection).look(ink(Tone::TextSecondary)));
    for (c, name) in (0..CATEGORIES).zip(NAMES) {
        let on = if s.categories[usize::from(c)] {
            Tri::On
        } else {
            Tri::Off
        };
        cx.add(
            Element::toggle(Key::index(u64::from(c)), name, on, Msg::Category(c))
                .style(sized(32.0, 4.0)),
        );
    }
}

fn content(s: &Shell, cx: &mut Cx<'_, Msg>) {
    cx.add_with(
        Element::stack(Key::name("content")).style(Style {
            grow: 1.0,
            padding: Insets::all(16.0),
            align: Align::Stretch,
            ..column(8.0)
        }),
        |cx| {
            cx.add_with(Element::stack(Key::name("toolbar")).style(row(8.0)), |cx| {
                toolbar(s, cx);
            });
            let table = Table {
                rows: u32::try_from(s.visible.len()).unwrap_or(u32::MAX),
                columns: &COLUMNS,
                sorted: s.sorted,
                checks: Some(("Select", s.all_checked())),
                current: s.current,
                status: s.visible.is_empty().then_some(Status {
                    text: "No programs match the filter",
                    action: Some(("Clear filter", Msg::ClearFilter)),
                }),
                context: true,
                on: Msg::Table,
            };
            cx.table(
                Element::table(Key::name("table"))
                    .style(Style {
                        grow: 1.0,
                        ..column(0.0)
                    })
                    .look(card(Radius::Card))
                    .name("Programs"),
                &table,
                |at, cells: &mut Cells<'_>| {
                    let r = s.visible[at as usize];
                    for text in &s.cells[r as usize] {
                        cells.push(text);
                    }
                    Row {
                        key: RowKey(u64::from(r)),
                        check: Some(if s.checked[r as usize] {
                            Tri::On
                        } else {
                            Tri::Off
                        }),
                        locked: "",
                    }
                },
            );
        },
    );
}

/// The Preview drawer: the changes, the apply job's progress, and its actions. Hold to apply is a
/// danger button: held for `motion.hold` it applies, and UI Automation's Invoke asks for the typed
/// confirmation instead (TD-0009 §5.2, §8).
fn drawer(s: &Shell, cx: &mut Cx<'_, Msg>) {
    cx.add_with(
        Element::stack(Key::name("drawer"))
            .style(Style {
                width: Dim::Fixed(320.0),
                padding: Insets::all(16.0),
                align: Align::Stretch,
                ..column(6.0)
            })
            .look(card(Radius::Drawer))
            .name("Changes"),
        |cx| {
            for i in 0..DRAWER_ROWS {
                let cells = &s.cells[usize::try_from(i).unwrap_or(0)];
                cx.add_with(Element::stack(Key::index(i)).style(row(8.0)), |cx| {
                    cx.add(Element::text(Key::name("what"), &cells[0]));
                    cx.add(
                        Element::text(Key::name("size"), &cells[3]).look(ink(Tone::TextSecondary)),
                    );
                    cx.add(Element::text(Key::name("undo"), "Undoable").look(ink(Tone::Ok)));
                });
            }
            cx.add(
                Element::progress(Key::name("progress"), f32::from(s.progress) / 100.0)
                    .name("Applying")
                    .style(Style {
                        height: Dim::Fixed(6.0),
                        ..Style::default()
                    }),
            );
            let status = if s.applied {
                "Applied"
            } else if s.confirming {
                "Type 20 to confirm"
            } else {
                ""
            };
            cx.add(Element::text(Key::name("status"), status).look(ink(Tone::TextSecondary)));
            cx.add_with(
                Element::stack(Key::name("actions")).style(Style {
                    justify: Justify::End,
                    ..row(8.0)
                }),
                |cx| {
                    cx.add(
                        Element::button(Key::name("cancel"), "Cancel", Msg::Drawer)
                            .style(sized(36.0, 14.0))
                            .look(Look::SECONDARY),
                    );
                    cx.add(
                        Element::button(Key::name("apply"), "Hold to apply", Msg::Apply)
                            .hold(|| Msg::ConfirmApply)
                            .style(sized(36.0, 16.0))
                            .text_style(STRONG)
                            .look(Look::DANGER)
                            .tooltip("Hold until the bar fills to apply"),
                    );
                },
            );
        },
    );
}

/// Every page but the first and Settings: a heading and 8 summary cards (artboard 02).
fn summary(s: &Shell, cx: &mut Cx<'_, Msg>) {
    const CARDS: [(&str, &str); 8] = [
        ("Temporary files", "1.2 GB"),
        ("Windows Update cache", "3.4 GB"),
        ("Delivery Optimization", "860 MB"),
        ("Thumbnails", "120 MB"),
        ("Error reports", "45 MB"),
        ("Logs", "310 MB"),
        ("Recycle Bin", "2.1 GB"),
        ("Browser caches", "640 MB"),
    ];
    let title = PAGES.get(usize::from(s.page)).copied().unwrap_or_default();
    cx.add_with(
        Element::stack(Key::name("summary"))
            .style(Style {
                grow: 1.0,
                padding: Insets::all(16.0),
                align: Align::Stretch,
                ..column(8.0)
            })
            .name(title),
        |cx| {
            cx.add(Element::text(Key::name("heading"), title).text_style(TITLE));
            for (i, (what, size)) in (0..).zip(CARDS) {
                cx.add_with(
                    Element::stack(Key::index(i))
                        .style(Style {
                            min: at_least(56.0),
                            padding: Insets::xy(12.0, 8.0),
                            ..row(12.0)
                        })
                        .look(card(Radius::Card)),
                    |cx| {
                        cx.add(Element::text(Key::name("what"), what).style(Style {
                            grow: 1.0,
                            ..Style::default()
                        }));
                        cx.add(
                            Element::text(Key::name("size"), size).look(ink(Tone::TextSecondary)),
                        );
                        cx.add(
                            Element::button(Key::name("review"), "Review", Msg::Drawer)
                                .style(sized(30.0, 12.0))
                                .look(Look::SECONDARY),
                        );
                    },
                );
            }
        },
    );
}

/// Settings › Appearance's theme: a radio group, one Tab stop, the arrow keys pick (§4.5). Then
/// the Start page: a dropdown (A1).
fn settings(s: &Shell, cx: &mut Cx<'_, Msg>) {
    cx.add_with(
        Element::stack(Key::name("settings"))
            .style(Style {
                grow: 1.0,
                padding: Insets::all(16.0),
                align: Align::Start,
                ..column(8.0)
            })
            .name("Settings"),
        |cx| {
            cx.add(Element::text(Key::name("heading"), "Appearance").text_style(TITLE));
            cx.add_with(
                Element::radios(Key::name("theme"))
                    .name("Theme")
                    .style(column(4.0)),
                |cx| {
                    for (t, label) in (0..).zip(THEMES) {
                        cx.add(
                            Element::radio(
                                Key::index(u64::from(t)),
                                label,
                                s.theme == t,
                                Msg::Theme(t),
                            )
                            .style(sized(32.0, 4.0)),
                        );
                    }
                },
            );
            let page = START_PAGES[usize::from(s.start_page)];
            cx.add(
                Element::combo(
                    Key::name("start"),
                    "Start page",
                    page,
                    s.open == Open::StartPages,
                    Msg::OpenStartPage,
                )
                .step(Msg::StepStartPage),
            );
        },
    );
}

/// What the open popup shows (`Model::popup_view`): the row's menu, or the Start page list.
pub(crate) fn popup_view(s: &Shell, cx: &mut Cx<'_, Msg>) {
    if let Open::RowMenu(_) = s.open {
        cx.add_with(Element::menu(Key::name("row-menu")).name("Row"), |cx| {
            for (i, (label, enabled)) in (0..).zip(ROW_MENU) {
                let mut item =
                    Element::menu_item(Key::index(u64::from(i)), label, Msg::RowAction(i))
                        .enabled(enabled);
                if i == 0 {
                    item = item.look(Look {
                        ink: Tone::Danger,
                        radius: Radius::Small,
                        ..Look::PLAIN
                    });
                }
                if !enabled {
                    item = item.tooltip(PROPERTIES_TIP);
                }
                cx.add(item);
                if i == 1 {
                    cx.add(Element::separator(Key::name("sep")));
                }
            }
        });
    } else if s.open == Open::StartPages {
        cx.add_with(
            Element::options(Key::name("start-pages")).name("Start page"),
            |cx| {
                for (p, label) in (0..).zip(START_PAGES) {
                    let on = p == s.start_page;
                    cx.add(
                        Element::option(Key::index(u64::from(p)), label, on, Msg::StartPage(p))
                            .look(Look {
                                fill: on.then_some(Tone::Selected),
                                radius: Radius::Small,
                                ..Look::PLAIN
                            }),
                    );
                }
            },
        );
    }
}

/// One step of the scripted session.
#[derive(Clone, Debug)]
pub(crate) enum Step {
    Input(Input),
    /// A typed character (`WM_CHAR`), to whatever has focus.
    Type(char),
    /// Text from the clipboard, pasted into the focused field.
    Paste(&'static str),
    Resize(Size),
    /// The clock moves on by this many milliseconds: animations and holds advance and the caret
    /// blinks, as on the platform's frame and caret timers.
    Tick(u64),
    /// A message straight to the view-model, as from a keyboard accelerator, the engine or
    /// assistive technology.
    Update(Msg),
    /// Windows' text size changed (Settings › Accessibility › Text size), as a factor: 1 to 2.25.
    TextScale(f32),
    /// Input to the open popup's window (a menu or the dropdown's list), as the platform routes
    /// it there.
    Popup(Input),
    /// The table's row at this place built and scrolled into view, as UI Automation's realizer
    /// asks for it (TD-0009 A2.5).
    Realize(u32),
}

/// A key pressed with `mods`, as a step.
pub(crate) fn key(key: KeyCode, down: bool, mods: Mods) -> Step {
    Step::Input(Input::Key(KeyEvent { key, down, mods }))
}

/// A key pressed with `mods` in the open popup's window, as a step.
pub(crate) fn popup_key(key: KeyCode, down: bool, mods: Mods) -> Step {
    Step::Popup(Input::Key(KeyEvent { key, down, mods }))
}

/// Ctrl held.
pub(crate) const CTRL: Mods = Mods {
    shift: false,
    ctrl: true,
    alt: false,
    win: false,
};

/// A shell, its Ui and text system, laid out at [`START`] when created; painted too, when made by
/// [`Run::painted`].
pub(crate) struct Run {
    pub(crate) ui: Ui<Msg>,
    pub(crate) shell: Shell,
    text: FakeText,
    msgs: Vec<Msg>,
    /// What frames are painted in; `None` stops at layout and damage.
    theme: Option<Theme>,
    /// The canvas's pixels, window-sized, kept between frames like a DIB section.
    pixels: Vec<u32>,
    /// The clock, in milliseconds.
    now: u64,
    /// Each frame takes the accessibility update, as the platform does while a client listens.
    /// Off for the frame a U12 bench measures the update of.
    pub(crate) take_a11y: bool,
    /// The clipboard, as the platform's copy, cut and paste would use it.
    pub(crate) clipboard: String,
    /// The open popup's UI (a menu or the dropdown's list), which the platform hosts in a window
    /// of its own, and whether that window shows.
    pub(crate) popup: Ui<Msg>,
    pub(crate) popup_open: bool,
    /// The popup's pixels, when frames are painted.
    popup_pixels: Vec<u32>,
}

impl Run {
    /// Frames end at layout and damage (U2).
    pub(crate) fn with(shell: Shell) -> Self {
        Self::new(shell, 1.0, None, 1.0)
    }

    /// Frames end at layout and damage, with Windows' text size at `text_scale` (1 to 2.25) from
    /// the first frame.
    pub(crate) fn text_scaled(shell: Shell, text_scale: f32) -> Self {
        Self::new(shell, 1.0, None, text_scale)
    }

    /// Frames are painted too, at `scale` in `theme` (U3).
    pub(crate) fn painted(shell: Shell, scale: f32, theme: Theme) -> Self {
        Self::new(shell, scale, Some(theme), 1.0)
    }

    /// Painted, with the text size at `text_scale` from the first frame.
    pub(crate) fn painted_text_scaled(shell: Shell, theme: Theme, text_scale: f32) -> Self {
        Self::new(shell, 1.0, Some(theme), text_scale)
    }

    /// Frames end at layout, and an assistive technology listens (U12): the first frame's whole
    /// tree is served, then every frame's changes are taken.
    pub(crate) fn accessible(shell: Shell) -> Self {
        let mut run = Self::new(shell, 1.0, None, 1.0);
        drop(run.ui.a11y_tree());
        run.take_a11y = true;
        run
    }

    fn new(shell: Shell, scale: f32, theme: Option<Theme>, text_scale: f32) -> Self {
        let motion = Motion {
            hover_ms: tokens::motion::HOVER_MS,
        };
        let mut run = Self {
            ui: Ui::new(START, scale, motion),
            shell,
            text: FakeText,
            msgs: Vec::with_capacity(4),
            theme,
            pixels: Vec::new(),
            now: 0,
            take_a11y: false,
            clipboard: String::with_capacity(64),
            popup: Ui::new(Size { w: 0.0, h: 0.0 }, scale, motion),
            popup_open: false,
            popup_pixels: Vec::new(),
        };
        // The window has keyboard focus, as a window the user works in does.
        run.ui.set_active(true);
        run.ui.set_text_scale(text_scale);
        run.popup.set_text_scale(text_scale);
        run.frame();
        run
    }

    /// One event through the whole UI-thread pipeline (TD-0009 §5.2): dispatch, the clipboard
    /// request it made if any, update, view, reconcile, layout, damage, and raster when painted.
    pub(crate) fn step(&mut self, step: Step) {
        match step {
            Step::Input(input) => {
                // A wheel over the window closes its popup, as its anchor may move (A2.4).
                if matches!(input, Input::Wheel { .. })
                    && self.popup_open
                    && let Some(open) = self.shell.popup()
                {
                    self.msgs.push(open.on_close);
                }
                self.ui.dispatch(input, &mut self.msgs);
                self.clipboard();
            }
            Step::Type(c) => {
                self.ui.dispatch(Input::Char(c), &mut self.msgs);
            }
            Step::Paste(text) => {
                self.ui.paste(text, &mut self.msgs);
            }
            Step::Resize(size) => self.ui.set_viewport(size),
            Step::Tick(ms) => {
                self.now += ms;
                // A hold held to the end sends its message from here; a popup's hover eases.
                self.ui.tick(self.now, &mut self.msgs);
                if self.popup_open {
                    self.popup.tick(self.now, &mut self.msgs);
                }
            }
            Step::Update(msg) => self.msgs.push(msg),
            Step::TextScale(t) => {
                self.ui.set_text_scale(t);
                self.popup.set_text_scale(t);
            }
            Step::Popup(input) => self.popup_input(input),
            Step::Realize(at) => {
                if let Some(table) = self.ui.find(&Self::TABLE) {
                    self.ui.realize(table, at);
                }
            }
        }
        for m in self.msgs.drain(..) {
            self.shell.update(m);
        }
        // The caret and tooltip timers, as the platform runs them after every event and on its
        // wake: the caret blinks if it's due, before the frame paints, and a tip due shows.
        let _ = self.wake();
        self.frame();
    }

    /// What the platform does with input to the popup's window: Esc (and F4 or Alt+Up in a
    /// list) closes it, and an item's message closes it first, then runs.
    fn popup_input(&mut self, input: Input) {
        let Some(open) = self.shell.popup() else {
            return;
        };
        if let Input::Key(k) = input
            && k.down
            && (k.key == KeyCode::Escape
                || (open.kind == PopupKind::List
                    && ((k.key == KeyCode::F4 && !k.mods.alt)
                        || (k.mods.alt && k.key == KeyCode::Up))))
        {
            self.msgs.push(open.on_close);
            return;
        }
        let before = self.msgs.len();
        self.popup.dispatch(input, &mut self.msgs);
        if self.msgs.len() > before {
            self.msgs.insert(before, open.on_close);
        }
    }

    /// How long until the platform's one-shot wake is due, if it is: the caret's next blink or
    /// a tooltip's next change, in the main window or the popup.
    pub(crate) fn wake(&mut self) -> Option<u64> {
        [
            self.ui.caret_timer(self.now),
            self.ui.tooltip_timer(self.now),
            self.popup.tooltip_timer(self.now),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// The clock, in milliseconds.
    pub(crate) fn now(&self) -> u64 {
        self.now
    }

    /// Ticks 60 Hz frames until the UI stops asking for them: hover fades end, and a hold still
    /// pressed runs to its end.
    pub(crate) fn settle(&mut self) {
        while self.ui.wants_frame() || (self.popup_open && self.popup.wants_frame()) {
            self.step(Step::Tick(FRAME_MS));
        }
    }

    /// What the platform does for a copy, cut or paste the Ui asked for, with `self.clipboard` as
    /// the clipboard.
    fn clipboard(&mut self) {
        match self.ui.take_clipboard() {
            Some(ClipboardRequest::Copy) => {
                self.clipboard.clear();
                self.clipboard.push_str(self.ui.selected_text());
            }
            Some(ClipboardRequest::Cut) => {
                self.clipboard.clear();
                self.clipboard.push_str(self.ui.selected_text());
                self.ui.cut_selection(&mut self.msgs);
            }
            Some(ClipboardRequest::Paste) => {
                self.ui.paste(&self.clipboard, &mut self.msgs);
            }
            None => {}
        }
    }

    /// How long until the caret timer wants to run again, if it does: the platform's one-shot
    /// timer.
    pub(crate) fn caret_wake(&mut self) -> Option<u64> {
        self.ui.caret_timer(self.now)
    }

    /// The window's damage repainted, as after a theme switch.
    pub(crate) fn repaint_all(&mut self) {
        self.ui.repaint_all();
        self.frame();
    }

    fn frame(&mut self) {
        // A context request asks the shell for its node's menu.
        if let Some(key) = self
            .ui
            .take_context()
            .and_then(|id| self.ui.info(id))
            .map(|n| n.key)
            && let Some(msg) = self.shell.context_menu(key)
        {
            self.shell.update(msg);
        }
        self.build();
        // A popup whose anchor went away closes (A1.8).
        if self.ui.popup_anchor().is_none()
            && let Some(open) = self.shell.popup()
        {
            self.shell.update(open.on_close);
            self.build();
        }
        self.sync_popup();
        if let Some(theme) = &self.theme {
            let (w, h) = self.ui.pixel_size();
            // Within capacity after warm-up: a resize back and forth doesn't allocate.
            self.pixels.resize(area(w, h), 0);
            if let Some(mut canvas) = Canvas::new(&mut self.pixels, w, h) {
                self.ui.paint(&mut canvas, &mut self.text, theme);
            }
            if self.popup_open {
                let (w, h) = self.popup.pixel_size();
                self.popup_pixels.resize(area(w, h), 0);
                if let Some(mut canvas) = Canvas::new(&mut self.popup_pixels, w, h) {
                    self.popup.paint(&mut canvas, &mut self.text, theme);
                }
            }
        }
        self.ui.clear_damage();
        self.popup.clear_damage();
        if self.take_a11y {
            drop(self.ui.a11y_update());
        }
    }

    /// The view built and laid out, and again if the table asks: its first build guesses its
    /// rows' view, and a reveal or a scroll may bring other rows into it (TD-0009 A2.5).
    fn build(&mut self) {
        let shell = &self.shell;
        for _ in 0..2 {
            self.ui.build(|cx| view(shell, cx));
            self.ui.layout(&mut self.text);
            if !self.ui.wants_build() {
                break;
            }
        }
    }

    /// Opens, rebuilds or hides the popup to match the shell's, as the platform does: a new one
    /// takes the activation from the main window, focuses its first item if the keyboard opened
    /// it (a list, its picked option, either way), and is sized to its content.
    fn sync_popup(&mut self) {
        let Some(open) = self.shell.popup() else {
            if std::mem::take(&mut self.popup_open) {
                self.popup.cancel_press();
                let mut none = Vec::new();
                self.popup.dispatch(Input::PointerLeave, &mut none);
                self.popup.set_focus(None);
                self.popup.set_active(false);
                // The activation comes back to the main window, its focus on the anchor.
                self.ui.set_active(true);
            }
            return;
        };
        let opening = !self.popup_open;
        let shell = &self.shell;
        self.popup.build(|cx| popup_view(shell, cx));
        let size = self.popup.natural_size(&mut self.text, START.w);
        self.popup.set_viewport(size);
        self.popup.layout(&mut self.text);
        if opening {
            let keyboard = self.ui.focus_visible();
            if keyboard || open.kind == PopupKind::List {
                self.popup.focus_first(keyboard);
            } else {
                self.popup.set_focus(None);
            }
            self.ui.set_active(false);
            self.popup.set_active(true);
            self.popup_open = true;
            self.popup.repaint_all();
        }
    }

    /// The popup's painted pixels, and its canvas's width.
    pub(crate) fn popup_pixels(&self) -> (&[u32], u32) {
        (&self.popup_pixels, self.popup.pixel_size().0)
    }

    /// The center of the popup's node at `path`.
    pub(crate) fn popup_center(&self, path: &[Key]) -> Point {
        self.popup
            .find(path)
            .and_then(|id| self.popup.info(id))
            .map_or(Point::default(), |n| n.rect.center())
    }

    /// The accessibility update the frames since the last one taken left, as the platform would
    /// take it.
    pub(crate) fn a11y_update(&mut self) -> Option<TreeUpdate> {
        self.ui.a11y_update()
    }

    /// The painted pixels, and the canvas's width.
    pub(crate) fn pixels(&self) -> (&[u32], u32) {
        (&self.pixels, self.ui.pixel_size().0)
    }

    /// The whole window painted from scratch into `out`, whatever the damage.
    pub(crate) fn paint_all_into(&mut self, out: &mut Vec<u32>) {
        let (w, h) = self.ui.pixel_size();
        out.clear();
        out.resize(area(w, h), 0);
        let theme = self.theme.unwrap_or(Theme::LIGHT);
        if let Some(mut canvas) = Canvas::new(out, w, h) {
            let all = canvas.bounds();
            self.ui
                .paint_region(&mut canvas, &mut self.text, &theme, all);
        }
    }

    /// The center of the node at `path`, from the current layout.
    pub(crate) fn center(&self, path: &[Key]) -> Point {
        self.ui
            .find(path)
            .and_then(|id| self.ui.info(id))
            .map_or(Point::default(), |n| n.rect.center())
    }

    /// The filter field's path from the root.
    pub(crate) const FILTER: [Key; 4] = [
        Key::name("body"),
        Key::name("content"),
        Key::name("toolbar"),
        Key::name("filter"),
    ];

    /// The drawer's Hold to apply button's path from the root, while the drawer is open.
    pub(crate) const APPLY: [Key; 4] = [
        Key::name("body"),
        Key::name("drawer"),
        Key::name("actions"),
        Key::name("apply"),
    ];

    /// Settings' Start page dropdown's path from the root, on the Settings page.
    pub(crate) const START_COMBO: [Key; 3] =
        [Key::name("body"), Key::name("settings"), Key::name("start")];

    /// The header's drawer button's path from the root.
    pub(crate) const DRAWER_BUTTON: [Key; 2] = [Key::name("header"), Key::name("drawer")];

    /// The table's path from the root, on the first page: the grid.
    pub(crate) const TABLE: [Key; 3] =
        [Key::name("body"), Key::name("content"), Key::name("table")];

    /// The table's rows' scroll viewport.
    pub(crate) const TABLE_ROWS: [Key; 4] = [
        Key::name("body"),
        Key::name("content"),
        Key::name("table"),
        TABLE_BODY,
    ];

    /// Data row `r`'s path from the root, while it's built on the first page.
    pub(crate) fn row(r: u64) -> [Key; 5] {
        [
            Key::name("body"),
            Key::name("content"),
            Key::name("table"),
            TABLE_BODY,
            Key::index(r),
        ]
    }

    /// The row menu's item `i`'s path in the popup's Ui ([`ROW_MENU`]).
    pub(crate) fn menu_item(i: u64) -> [Key; 2] {
        [Key::name("row-menu"), Key::index(i)]
    }

    /// The Start page list's option `p`'s path in the popup's Ui.
    pub(crate) fn start_option(p: u64) -> [Key; 2] {
        [Key::name("start-pages"), Key::index(p)]
    }

    /// Tab `t`'s path from the root.
    pub(crate) fn tab(t: u64) -> [Key; 5] {
        [
            Key::name("body"),
            Key::name("content"),
            Key::name("toolbar"),
            Key::name("tabs"),
            Key::index(t),
        ]
    }

    /// A click at the filter field's center: focus, and the caret where the click was.
    pub(crate) fn focus_filter(&self) -> [Step; 2] {
        click(self.center(&Self::FILTER))
    }

    /// Where Hold to apply is once the drawer opens: measured on a shell of its own, so `self`
    /// stays as it is.
    pub(crate) fn apply_button() -> Point {
        let mut open = Self::with(Shell::new());
        open.step(Step::Update(Msg::Drawer));
        open.center(&Self::APPLY)
    }

    /// The selected tab focused by a click, the hover fade it started over.
    pub(crate) fn in_tabs(mut self) -> Self {
        for s in click(self.center(&Self::tab(0))) {
            self.step(s);
        }
        self.settle();
        self
    }

    /// Row 4 (a desktop program the first page shows) focused, as the keyboard leaves it.
    pub(crate) fn row_focused(mut self) -> Self {
        let row = self.ui.find(&Self::row(4));
        self.ui.set_focus(row);
        self
    }

    /// The last row wholly in the table's view current and focused, as the keyboard leaves it:
    /// Down from it scrolls.
    pub(crate) fn bottom_row_current(mut self) -> Self {
        let Some(body) = self.ui.find(&Self::TABLE_ROWS) else {
            return self;
        };
        let end = self.ui.info(body).map_or(0.0, |n| n.rect.bottom());
        let last = self
            .ui
            .children(body)
            .filter(|&id| self.ui.info(id).is_some_and(|n| n.rect.bottom() <= end))
            .last();
        let key = last.and_then(|id| self.ui.info(id)).map(|n| n.key);
        let row = (0u32..)
            .take(DATA)
            .find(|&r| Some(Key::index(u64::from(r))) == key);
        if let Some(r) = row {
            self.step(Step::Update(Msg::Row(r)));
            let id = self.ui.find(&Self::row(u64::from(r)));
            self.ui.set_focus(id);
        }
        self
    }

    /// Row 4's menu open from the keyboard (Shift+F10), its first item focused, nothing
    /// animating.
    pub(crate) fn menu_open(self) -> Self {
        let mut run = self.row_focused();
        run.step(Step::Input(Input::ContextMenu(None)));
        run.settle();
        run
    }

    /// The pointer resting on the header's drawer button, its hover settled, its tip pending.
    pub(crate) fn tip_pending(mut self) -> Self {
        let at = self.center(&Self::DRAWER_BUTTON);
        self.step(Step::Input(Input::PointerMove(at)));
        self.settle();
        self
    }

    /// The drawer open, nothing animating.
    pub(crate) fn drawer_open(mut self) -> Self {
        self.step(Step::Update(Msg::Drawer));
        self.settle();
        self
    }

    /// The drawer open, the pointer resting on Hold to apply, then pressed: two frames into the
    /// hold, its clock running.
    pub(crate) fn holding(self) -> Self {
        let mut run = self.drawer_open();
        let at = run.center(&Self::APPLY);
        run.step(Step::Input(Input::PointerMove(at)));
        run.settle();
        run.step(Step::Input(Input::PointerDown(at)));
        run.step(Step::Tick(FRAME_MS));
        run.step(Step::Tick(FRAME_MS));
        run
    }

    /// The pointer resting on the table, its hover settled: where a wheel scrolls it.
    pub(crate) fn over_table(mut self) -> Self {
        let at = self.center(&Self::TABLE);
        self.step(Step::Input(Input::PointerMove(at)));
        self.settle();
        self
    }

    /// A notch of the wheel, `delta` (120 away from the user, -120 toward), at the table's center.
    pub(crate) fn table_wheel(&self, delta: i32) -> Step {
        wheel(self.center(&Self::TABLE), delta)
    }

    /// The table's vertical bar, if its content overflows.
    pub(crate) fn table_bar(&self) -> Option<ScrollBar> {
        let id = self.ui.find(&Self::TABLE_ROWS)?;
        self.ui.info(id)?.scroll?.bar_y
    }

    /// The pointer on the table's bar, so it's expanded, then pressed on its thumb: a thumb drag
    /// begun, nothing animating.
    pub(crate) fn thumb_grabbed(mut self) -> Self {
        let strip = self
            .table_bar()
            .map_or(Point::default(), |b| b.track.center());
        self.step(Step::Input(Input::PointerMove(strip)));
        self.settle();
        let thumb = self
            .table_bar()
            .map_or(Point::default(), |b| b.thumb.center());
        self.step(Step::Input(Input::PointerMove(thumb)));
        self.step(Step::Input(Input::PointerDown(thumb)));
        self.settle();
        self
    }

    /// While [`thumb_grabbed`](Self::thumb_grabbed), the pointer `dy` below where it pressed.
    pub(crate) fn thumb_moved(&self, dy: f32) -> Step {
        let at = self
            .table_bar()
            .map_or(Point::default(), |b| b.thumb.center());
        Step::Input(Input::PointerMove(Point { y: at.y + dy, ..at }))
    }

    /// TD-0009 §7's session: a hover sweep over the rail, two wheel notches down the table and two
    /// back (A2, slice 5a), Tab through the shell, a click into the filter, typing 10 characters
    /// into it and clearing it with Ctrl+A and Backspace, opening the drawer, pressing Hold to
    /// apply for three frames and letting go early, two progress reports, closing the drawer,
    /// toggling a category on and off, clicking the second tab and going back with Left, and a
    /// resize from 1000 to 1400 px and back. It ends where it started, so it can repeat.
    pub(crate) fn session(&self) -> Vec<Step> {
        let plain = |code, down| key(code, down, Mods::default());
        let body = [Key::name("body")];
        let mut steps = Vec::new();
        for p in 0..u64::from(RAIL) {
            let at = self.center(&[body[0], Key::name("rail"), Key::index(p)]);
            steps.push(Step::Input(Input::PointerMove(at)));
        }
        steps.push(Step::Input(Input::PointerLeave));
        let table = self.center(&Self::TABLE);
        steps.push(Step::Input(Input::PointerMove(table)));
        for delta in [-120, -120, 120, 120] {
            steps.push(wheel(table, delta));
        }
        steps.push(Step::Input(Input::PointerLeave));
        for _ in 0..20 {
            steps.push(plain(KeyCode::Tab, true));
            steps.push(plain(KeyCode::Tab, false));
        }
        steps.extend(self.focus_filter());
        steps.extend(TYPED.chars().map(Step::Type));
        steps.push(key(KeyCode::Other(u16::from(b'A')), true, CTRL));
        steps.push(key(KeyCode::Backspace, true, Mods::default()));
        let drawer = self.center(&[Key::name("header"), Key::name("drawer")]);
        steps.extend(click(drawer));
        let apply = Self::apply_button();
        steps.push(Step::Input(Input::PointerDown(apply)));
        steps.extend(std::iter::repeat_n(Step::Tick(FRAME_MS), 3));
        steps.push(Step::Input(Input::PointerUp(apply)));
        steps.push(Step::Update(Msg::Progress(41)));
        steps.push(Step::Update(Msg::Progress(0)));
        steps.extend(click(drawer));
        let category = self.center(&[
            body[0],
            Key::name("content"),
            Key::name("toolbar"),
            Key::index(0),
        ]);
        steps.extend(click(category));
        steps.extend(click(category));
        steps.extend(click(self.center(&Self::tab(1))));
        steps.push(plain(KeyCode::Left, true));
        steps.push(plain(KeyCode::Left, false));
        for w in [1080.0, 1160.0, 1240.0, 1320.0, 1400.0, 1000.0] {
            steps.push(Step::Resize(Size { w, h: START.h }));
        }
        steps
    }
}

/// A notch of the vertical wheel at `at`: `delta` 120 is away from the user (up the content), -120
/// toward.
pub(crate) fn wheel(at: Point, delta: i32) -> Step {
    Step::Input(Input::Wheel {
        at,
        delta,
        horizontal: false,
    })
}

/// A press and release at `p`.
pub(crate) fn click(p: Point) -> [Step; 2] {
    [
        Step::Input(Input::PointerDown(p)),
        Step::Input(Input::PointerUp(p)),
    ]
}

/// Pixels in a `w × h` canvas.
fn area(w: u32, h: u32) -> usize {
    usize::try_from(u64::from(w) * u64::from(h)).unwrap_or(usize::MAX)
}
