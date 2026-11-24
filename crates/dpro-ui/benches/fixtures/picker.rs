//! A synthetic page with slice 6e's in-app path picker (TD-0009 A3.5 (12), A3.7 U19, A3.9), for
//! its benches, tests and G1's probe.
//!
//! The page: a level-1 heading, "Browse…" (which opens the picker) and the path chosen. The picker
//! is a sheet over the page, which leaves the window while it's open, listing [`START`]: a folder
//! of [`ENTRIES`] subfolders, through a scripted [`Lister`] whose batches come one per wake
//! ([`Msg::Listed`]), as the platform's waker brings them. A folder ending in `missing` isn't
//! there, one ending in `notes.txt` isn't a folder, and a lister set to hang never answers.
//!
//! [`Run`] hosts it as the platform does: a `Ui`, a `FakeText`, the clock, and, when asked,
//! painting at a scale and an assistive technology listening.

#![allow(
    dead_code,
    reason = "each file that includes the fixture uses a part of it"
)]

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

use dpro_app::picker::{
    Activated, BATCH, Entry, FolderModel, ListBatch, ListError, ListRequest, Lister, Listing, Mode,
    Refused,
};
use dpro_ui::{
    Align, Canvas, Cells, Column, ColumnId, Cx, Element, FakeText, Input, Insets, Key, KeyCode,
    KeyEvent, Live, Look, Mods, Motion, Point, Row, RowKey, RowLines, Size, Status, Style, Table,
    TableEvent, TextEdit, TextStyle, Theme, Track, Ui, UnixMillis,
    accesskit::{Node, NodeId as AkId, TreeUpdate},
    composite::{ErrorPanel, ErrorTone, PathPicker},
    tokens::{self, SPACE, font},
};

/// The window, as the drawer page's.
pub(crate) const START_SIZE: Size = Size {
    w: 1000.0,
    h: 700.0,
};
/// One frame at 60 Hz, rounded down to whole milliseconds: the platform's frame timer.
pub(crate) const FRAME_MS: u64 = 16;
/// How many subfolders every listed folder has: U19's 10,000.
pub(crate) const ENTRIES: usize = 10_000;

/// The row the page and the sheet share.
pub(crate) const ROW: Key = Key::name("row");
/// The page, while the picker is closed.
pub(crate) const PAGE: Key = Key::name("page");
pub(crate) const HEADING: Key = Key::name("heading");
/// "Browse…": what opens the picker, the page's first control.
pub(crate) const OPEN: Key = Key::name("open");
/// The path chosen, under it.
pub(crate) const CHOSEN: Key = Key::name("chosen");
/// The picker, in the row.
pub(crate) const PICKER: Key = Key::name("picker");

/// The page's title.
pub(crate) const PAGE_TITLE: &str = "Capture image";
pub(crate) const BROWSE: &str = "Browse…";
pub(crate) const NOTHING_CHOSEN: &str = "No folder chosen";
/// The picker's title (`ui-picker-folder-title`): its name and its level-2 heading.
pub(crate) const TITLE: &str = "Choose a folder";
pub(crate) const PATH_LABEL: &str = "Path";
pub(crate) const UP_LABEL: &str = "Up one level";
pub(crate) const DRIVES_LABEL: &str = "Drive";
pub(crate) const SELECT: &str = "Select this folder";
pub(crate) const CANCEL: &str = "Cancel";
pub(crate) const CLOSE: &str = "Close";
pub(crate) const REGION: &str = "region";
pub(crate) const LOADING: &str = "Loading…";
pub(crate) const EMPTY: &str = "This folder has no subfolders.";
pub(crate) const DETAILS: &str = "Details";
pub(crate) const COPY: &str = "Copy details";

/// The table's columns: Name, then Date modified.
pub(crate) const COLUMNS: [Column<'static>; 2] = [
    Column {
        id: ColumnId(0),
        header: "Name",
        width: Track::Share(1.0),
        min: 160.0,
        align: Align::Start,
        mono: false,
        sort: None,
        hideable: false,
    },
    Column {
        id: ColumnId(1),
        header: "Date modified",
        width: Track::Fixed(160.0),
        min: 96.0,
        align: Align::Start,
        mono: false,
        sort: None,
        hideable: true,
    },
];

/// Every subfolder's last write: 2026-11-14.
const MODIFIED: UnixMillis = UnixMillis(1_794_700_800_000);

/// A root: a full path on Windows and on Linux (T1), where the benches run.
pub(crate) fn root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\")
    } else {
        PathBuf::from("/")
    }
}

/// The folder the picker opens on: `<root>/Images/Capture`.
pub(crate) fn start() -> PathBuf {
    root().join("Images").join("Capture")
}

/// Subfolder `i`'s name.
pub(crate) fn entry_name(i: usize) -> String {
    format!("Folder {i:05}")
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Msg {
    /// Browse…: the picker opens on [`start`].
    Open,
    /// Close, Cancel and Esc.
    Close,
    Path(TextEdit),
    /// Enter in the path field.
    Go,
    /// Up, Alt+Up, Backspace in the table.
    Up,
    Drives,
    Table(TableEvent),
    /// A wake: the lister has one more batch ready.
    Listed,
    Select,
    Details,
    Copy,
}

/// The scripted lister: each folder's subfolders in batches of [`BATCH`], one released per
/// [`Msg::Listed`].
#[derive(Debug)]
pub(crate) struct Scripted {
    /// Every request it was given, in order.
    pub(crate) started: Vec<ListRequest>,
    /// How many subfolders a folder has.
    pub(crate) count: usize,
    /// Its listings never answer.
    pub(crate) hang: bool,
    /// Batches released and not yet polled.
    ready: usize,
    queue: VecDeque<ListBatch>,
}

impl Scripted {
    pub(crate) fn new(count: usize) -> Self {
        Self {
            started: Vec::new(),
            count,
            hang: false,
            ready: 0,
            queue: VecDeque::new(),
        }
    }

    /// Releases the next batch for [`Lister::poll`].
    pub(crate) fn release(&mut self) {
        self.ready += 1;
    }

    fn listing(&self, path: &Path) -> Result<Vec<Entry>, ListError> {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        match name {
            "missing" => Err(ListError::NotFound),
            "notes.txt" => Err(ListError::NotFolder),
            "denied" => Err(ListError::Denied),
            _ => Ok((0..self.count)
                .map(|i| Entry {
                    name: entry_name(i).into(),
                    folder: true,
                    modified: Some(MODIFIED),
                })
                .collect()),
        }
    }
}

impl Lister for Scripted {
    fn start(&mut self, request: ListRequest) {
        self.queue.clear();
        self.ready = 0;
        if !self.hang {
            let generation = request.generation;
            match self.listing(&request.path) {
                Ok(entries) => {
                    let mut entries = entries.into_iter().peekable();
                    loop {
                        let batch: Vec<Entry> = entries.by_ref().take(BATCH).collect();
                        let last = entries.peek().is_none();
                        self.queue.push_back(ListBatch {
                            generation,
                            entries: batch,
                            end: last.then_some(Ok(())),
                        });
                        if last {
                            break;
                        }
                    }
                }
                Err(e) => self.queue.push_back(ListBatch {
                    generation,
                    entries: Vec::new(),
                    end: Some(Err(e)),
                }),
            }
        }
        self.started.push(request);
    }

    fn poll(&mut self) -> Option<ListBatch> {
        if self.ready == 0 {
            return None;
        }
        let batch = self.queue.pop_front()?;
        self.ready -= 1;
        Some(batch)
    }
}

/// The view-model.
#[derive(Debug)]
pub(crate) struct Model {
    pub(crate) open: bool,
    pub(crate) folder: FolderModel,
    pub(crate) lister: Scripted,
    pub(crate) chosen: Option<PathBuf>,
    /// The chosen path's text, for the page.
    chosen_text: String,
    pub(crate) drives_open: bool,
    pub(crate) details: bool,
    /// The path field's last refusal, until the next go.
    pub(crate) refused: Option<Refused>,
    /// The error panel's title, written when the error came: the view formats nothing.
    pub(crate) error_text: String,
    /// The drive the drive list shows.
    drive_text: String,
    pub(crate) last: Option<Msg>,
}

impl Model {
    /// A folder picker whose folders have [`ENTRIES`] subfolders.
    pub(crate) fn new() -> Self {
        Self::with_entries(ENTRIES)
    }

    pub(crate) fn with_entries(count: usize) -> Self {
        Self {
            open: false,
            folder: FolderModel::new(Mode::Folder),
            lister: Scripted::new(count),
            chosen: None,
            chosen_text: String::new(),
            drives_open: false,
            details: false,
            refused: None,
            error_text: String::new(),
            drive_text: root().to_string_lossy().into_owned(),
            last: None,
        }
    }

    pub(crate) fn update(&mut self, msg: Msg) {
        match &msg {
            Msg::Open => {
                self.open = true;
                self.refused = None;
                if let Ok(request) = self.folder.open(&start()) {
                    self.lister.start(request);
                }
            }
            Msg::Close => {
                self.open = false;
                self.drives_open = false;
            }
            Msg::Path(edit) => {
                edit.apply(self.folder.path_mut());
            }
            Msg::Go => match self.folder.go() {
                Ok(request) => {
                    self.refused = None;
                    self.lister.start(request);
                }
                Err(why) => {
                    self.refused = Some(why);
                    self.error_text = format!("“{}” isn't a full path.", self.folder.path());
                }
            },
            Msg::Up => {
                self.refused = None;
                if let Some(request) = self.folder.up() {
                    self.lister.start(request);
                }
            }
            Msg::Drives => self.drives_open = !self.drives_open,
            Msg::Table(TableEvent::Current(at)) => self.folder.table_mut().set_current(*at),
            Msg::Table(TableEvent::Activate(at)) => match self.folder.activate(*at) {
                Some(Activated::Open(request)) => self.lister.start(request),
                Some(Activated::Chose(path)) => self.choose(path),
                None => {}
            },
            Msg::Table(_) | Msg::Copy => {}
            Msg::Listed => {
                self.lister.release();
                while let Some(batch) = self.lister.poll() {
                    self.folder.accept(batch);
                }
                if let Listing::Failed(e) = self.folder.listing() {
                    self.error_text = match e {
                        ListError::NotFound => format!("“{}” doesn't exist.", self.folder.path()),
                        ListError::NotFolder => {
                            format!("“{}” isn't a folder.", self.folder.path())
                        }
                        ListError::Denied => format!(
                            "You don't have permission to open “{}”.",
                            self.folder.path()
                        ),
                        ListError::Failed(_) => format!("Couldn't list “{}”.", self.folder.path()),
                    };
                }
            }
            Msg::Select => {
                if let Some(path) = self.folder.choice() {
                    self.choose(path);
                }
            }
            Msg::Details => self.details = !self.details,
        }
        self.last = Some(msg);
    }

    fn choose(&mut self, path: PathBuf) {
        self.chosen_text = path.to_string_lossy().into_owned();
        self.chosen = Some(path);
        self.open = false;
    }

    /// The error panel's title, if the panel shows.
    fn error(&self) -> Option<&str> {
        let failed = matches!(self.folder.listing(), Listing::Failed(_));
        (failed || self.refused.is_some()).then_some(self.error_text.as_str())
    }
}

/// The page, or the picker over it as a sheet.
pub(crate) fn view(m: &Model, cx: &mut Cx<'_, Msg>) {
    cx.add_with(
        Element::stack(ROW).style(Style {
            grow: 1.0,
            ..Style::default()
        }),
        |cx| {
            if m.open {
                picker(m, cx);
            } else {
                page(m, cx);
            }
        },
    );
}

fn page(m: &Model, cx: &mut Cx<'_, Msg>) {
    cx.add_with(
        Element::stack(PAGE).style(Style {
            grow: 1.0,
            gap: SPACE[2],
            padding: Insets::all(SPACE[4]),
            align: Align::Start,
            ..Style::default()
        }),
        |cx| {
            cx.add(
                Element::text(HEADING, PAGE_TITLE)
                    .text_style(TextStyle {
                        size: font::TITLE,
                        weight: font::SEMIBOLD,
                        mono: false,
                        wrap: false,
                    })
                    .heading(1),
            );
            cx.add(
                Element::button(OPEN, BROWSE, Msg::Open)
                    .style(Style {
                        min: Size { w: 0.0, h: 32.0 },
                        padding: Insets::xy(SPACE[3], 0.0),
                        ..Style::default()
                    })
                    .look(Look::SECONDARY),
            );
            let chosen = if m.chosen.is_some() {
                m.chosen_text.as_str()
            } else {
                NOTHING_CHOSEN
            };
            cx.add(Element::text(CHOSEN, chosen));
        },
    );
}

fn picker(m: &Model, cx: &mut Cx<'_, Msg>) {
    let folder = &m.folder;
    let status = match folder.listing() {
        _ if !folder.is_empty() => None,
        Listing::Loading => Some(LOADING),
        Listing::Listed => Some(EMPTY),
        Listing::Idle | Listing::Failed(_) => None,
    }
    .map(|text| Status { text, action: None });
    let error = m.error().map(|title| ErrorPanel {
        tone: ErrorTone::Error,
        title,
        code: "",
        api: "",
        next: "",
        log: None,
        expanded: m.details,
        details: (DETAILS, Msg::Details),
        actions: [None, None],
        copy: (COPY, Msg::Copy),
        live: Live::Assertive,
    });
    let p = PathPicker {
        title: TITLE,
        close: || Msg::Close,
        close_label: CLOSE,
        path: (PATH_LABEL, folder.path(), Msg::Path),
        go: Msg::Go,
        up: (
            UP_LABEL,
            folder
                .folder()
                .is_some_and(|f| f.parent().is_some())
                .then_some(Msg::Up),
        ),
        drives: (DRIVES_LABEL, &m.drive_text, m.drives_open, Msg::Drives),
        error,
        table: Table {
            rows: folder.len(),
            lines: RowLines::One,
            groups: &[],
            columns: &COLUMNS,
            widths: &[],
            hidden: &[],
            sorted: None,
            sort_status: ["", ""],
            checks: None,
            current: folder.table().current_view(),
            status,
            context: false,
            on: Msg::Table,
        },
        cancel: CANCEL,
        select: (SELECT, folder.folder().is_some().then_some(Msg::Select)),
        region: REGION,
    };
    cx.path_picker(PICKER, &p, |at, cells: &mut Cells<'_>| {
        let source = folder
            .table()
            .view()
            .get(at as usize)
            .copied()
            .unwrap_or(at);
        if let Some(e) = folder.entry(source) {
            cells.push(&e.name);
            match e.modified {
                Some(t) => cells.date(t),
                None => cells.push(""),
            }
        }
        Row {
            key: RowKey(u64::from(source)),
            check: None,
            locked: "",
        }
    });
}

/// A step of a script.
#[derive(Clone, Debug)]
pub(crate) enum Step {
    Input(Input),
    /// A message straight to the view-model.
    Update(Msg),
    Resize(Size),
    /// The clock moves on by this many milliseconds: a frame.
    Tick(u64),
}

/// A key pressed, as a step.
pub(crate) fn key(key: KeyCode, mods: Mods) -> Step {
    Step::Input(Input::Key(KeyEvent {
        key,
        down: true,
        mods,
    }))
}

/// A key let go, as a step.
pub(crate) fn key_up(key: KeyCode) -> Step {
    Step::Input(Input::Key(KeyEvent {
        key,
        down: false,
        mods: Mods::default(),
    }))
}

/// The page, its Ui and text system, laid out at [`START_SIZE`] when created; painted too, when
/// made by [`Run::painted`].
pub(crate) struct Run {
    pub(crate) ui: Ui<Msg>,
    pub(crate) model: Model,
    text: FakeText,
    msgs: Vec<Msg>,
    theme: Option<Theme>,
    pixels: Vec<u32>,
    now: u64,
    /// Each frame takes the accessibility update, as the platform does while a client listens.
    pub(crate) take_a11y: bool,
    /// Frames run since the run started.
    pub(crate) frames: u64,
}

impl Run {
    /// Frames end at layout and damage.
    pub(crate) fn with(model: Model) -> Self {
        Self::new(model, None, 1.0, Motion::STANDARD)
    }

    /// Frames are painted too, at scale 1 in `theme`.
    pub(crate) fn painted(model: Model, theme: Theme) -> Self {
        Self::new(model, Some(theme), 1.0, Motion::STANDARD)
    }

    /// An assistive technology listens: the first frame's whole tree is served, then every
    /// frame's changes are taken.
    pub(crate) fn accessible(model: Model) -> Self {
        let mut run = Self::new(model, None, 1.0, Motion::STANDARD);
        drop(run.ui.a11y_tree());
        run.take_a11y = true;
        run
    }

    fn new(model: Model, theme: Option<Theme>, scale: f32, motion: Motion) -> Self {
        let mut run = Self {
            ui: Ui::new(START_SIZE, scale, motion),
            model,
            text: FakeText,
            msgs: Vec::with_capacity(4),
            theme,
            pixels: Vec::new(),
            now: 0,
            take_a11y: false,
            frames: 0,
        };
        run.ui.set_active(true);
        run.frame();
        run
    }

    /// One event through the whole pipeline: dispatch, update, view, reconcile, layout (again if
    /// the table asks, as the platform does), damage, and raster when painted.
    pub(crate) fn step(&mut self, step: Step) {
        match step {
            Step::Input(input) => {
                self.ui.dispatch(input, &mut self.msgs);
            }
            Step::Update(msg) => self.msgs.push(msg),
            Step::Resize(size) => self.ui.set_viewport(size),
            Step::Tick(ms) => {
                self.now += ms;
                self.ui.tick(self.now, &mut self.msgs);
            }
        }
        for m in self.msgs.drain(..) {
            self.model.update(m);
        }
        self.frame();
    }

    /// Ticks 60 Hz frames until the UI stops asking for them; returns how many it took.
    pub(crate) fn settle(&mut self) -> u64 {
        let mut ticks = 0;
        while self.ui.wants_frame() {
            self.step(Step::Tick(FRAME_MS));
            ticks += 1;
        }
        ticks
    }

    /// Wakes until the listing ends (or 100 wakes): every batch taken.
    pub(crate) fn list_all(&mut self) {
        for _ in 0..100 {
            if self.model.folder.listing() != Listing::Loading {
                break;
            }
            self.step(Step::Update(Msg::Listed));
        }
    }

    /// How long until the platform's one-shot wake is due, if it is: the caret's next blink or a
    /// tooltip's next change.
    pub(crate) fn wake(&mut self) -> Option<u64> {
        [
            self.ui.caret_timer(self.now),
            self.ui.tooltip_timer(self.now),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    fn frame(&mut self) {
        // As the platform's: again if the table asks (its first build guesses its rows).
        for _ in 0..3 {
            let model = &self.model;
            self.ui.build(|cx| view(model, cx));
            self.ui.layout(&mut self.text);
            self.ui.take_messages(&mut self.msgs);
            let fitted = !self.msgs.is_empty();
            for m in self.msgs.drain(..) {
                self.model.update(m);
            }
            if !fitted && !self.ui.wants_build() {
                break;
            }
        }
        self.frames += 1;
        if let Some(theme) = &self.theme {
            let (w, h) = self.ui.pixel_size();
            self.pixels
                .resize(usize::try_from(u64::from(w) * u64::from(h)).unwrap_or(0), 0);
            if let Some(mut canvas) = Canvas::new(&mut self.pixels, w, h) {
                self.ui.paint(&mut canvas, &mut self.text, theme);
            }
        }
        self.ui.clear_damage();
        if self.take_a11y {
            drop(self.ui.a11y_update());
        }
    }

    /// The node at `path` from the root.
    pub(crate) fn find(&self, path: &[Key]) -> Option<dpro_ui::NodeId> {
        self.ui.find(path)
    }

    /// The picker, if it's open.
    pub(crate) fn picker(&self) -> Option<dpro_ui::NodeId> {
        self.ui.find(&[ROW, PICKER])
    }

    /// "Browse…", while the page shows.
    pub(crate) fn opener(&self) -> Option<dpro_ui::NodeId> {
        self.ui.find(&[ROW, PAGE, OPEN])
    }

    /// The first node keyed `key` under the picker, breadth first: one of its parts.
    pub(crate) fn part(&self, key: Key) -> Option<dpro_ui::NodeId> {
        let mut queue = std::collections::VecDeque::from([self.picker()?]);
        while let Some(id) = queue.pop_front() {
            for c in self.ui.children(id) {
                if self.ui.info(c).is_some_and(|n| n.key == key) {
                    return Some(c);
                }
                queue.push_back(c);
            }
        }
        None
    }

    /// Whether node `id` is `ancestor` or under it.
    pub(crate) fn is_under(&self, id: dpro_ui::NodeId, ancestor: dpro_ui::NodeId) -> bool {
        id == ancestor || self.ui.children(ancestor).any(|c| self.is_under(id, c))
    }

    /// The center of node `id`.
    pub(crate) fn center(&self, id: dpro_ui::NodeId) -> Point {
        self.ui
            .info(id)
            .map_or(Point::default(), |n| n.rect.center())
    }

    /// Node `id` as a client sees it in `tree`, if it's there.
    pub(crate) fn ak<'t>(&self, tree: &'t TreeUpdate, id: dpro_ui::NodeId) -> Option<&'t Node> {
        tree.nodes
            .iter()
            .find(|(ak, _)| self.ui.a11y_node(*ak) == Some(id))
            .map(|(_, node)| node)
    }

    /// The whole tree's nodes, in no order: what a client can reach.
    pub(crate) fn nodes(tree: &TreeUpdate) -> impl Iterator<Item = (AkId, &Node)> {
        tree.nodes.iter().map(|(id, n)| (*id, n))
    }

    /// The last painted frame's damage was cleared; the pixel at `(x, y)` in physical pixels.
    pub(crate) fn pixel(&self, x: u32, y: u32) -> u32 {
        let (w, _) = self.ui.pixel_size();
        self.pixels
            .get(usize::try_from(u64::from(y) * u64::from(w) + u64::from(x)).unwrap_or(0))
            .copied()
            .unwrap_or(0)
    }

    /// Steady frames on an open, listed picker (U8): the current row moved, a page down, the
    /// end and back, a wheel notch, the pointer over the rows and gone, its hover faded. Nothing
    /// is listed, so the model allocates nothing either.
    pub(crate) fn steady() -> Vec<Step> {
        let none = Mods::default();
        vec![
            Step::Update(Msg::Table(TableEvent::Current(0))),
            key(KeyCode::Down, none),
            key(KeyCode::PageDown, none),
            key(KeyCode::End, none),
            key(KeyCode::Home, none),
            Step::Input(Input::Wheel {
                at: Point { x: 500.0, y: 400.0 },
                delta: -120,
                horizontal: false,
            }),
            Step::Input(Input::PointerMove(Point { x: 500.0, y: 300.0 })),
            Step::Tick(u64::from(tokens::motion::HOVER_MS) + FRAME_MS),
            Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 })),
            Step::Tick(u64::from(tokens::motion::HOVER_MS) + FRAME_MS),
        ]
    }

    /// Steps that warm the picker: open it and list its folder, move through the rows, page down
    /// and back, scroll, open a folder and list it, Up, close; then open and close again, the
    /// pointer gone and its hover faded.
    pub(crate) fn session() -> Vec<Step> {
        let none = Mods::default();
        let mut steps = vec![Step::Update(Msg::Open)];
        steps.extend((0..ENTRIES.div_ceil(BATCH)).map(|_| Step::Update(Msg::Listed)));
        steps.extend([
            Step::Update(Msg::Table(TableEvent::Current(0))),
            key(KeyCode::Down, none),
            key(KeyCode::PageDown, none),
            key(KeyCode::End, none),
            key(KeyCode::Home, none),
            Step::Input(Input::Wheel {
                at: Point { x: 500.0, y: 400.0 },
                delta: -120,
                horizontal: false,
            }),
            Step::Update(Msg::Table(TableEvent::Activate(3))),
        ]);
        steps.extend((0..ENTRIES.div_ceil(BATCH)).map(|_| Step::Update(Msg::Listed)));
        steps.extend([
            Step::Update(Msg::Up),
            Step::Update(Msg::Close),
            Step::Update(Msg::Open),
        ]);
        steps.extend((0..ENTRIES.div_ceil(BATCH)).map(|_| Step::Update(Msg::Listed)));
        steps.extend([
            Step::Update(Msg::Close),
            Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 })),
            Step::Tick(u64::from(tokens::motion::HOVER_MS) + FRAME_MS),
        ]);
        steps
    }

    /// `run` after the session, settled, the picker closed.
    pub(crate) fn warmed(mut self) -> Self {
        for s in Self::session() {
            self.step(s);
        }
        self.settle();
        self
    }

    /// The picker open on [`start`] with its first batch requested and not yet come: what U19's
    /// bench starts from (the next [`Msg::Listed`] brings the first screen of rows).
    pub(crate) fn waiting(model: Model, theme: Option<Theme>) -> Self {
        let mut run = match theme {
            Some(theme) => Self::painted(model, theme),
            None => Self::with(model),
        };
        run.step(Step::Update(Msg::Open));
        run.settle();
        run
    }
}
