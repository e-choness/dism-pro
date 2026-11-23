//! A synthetic page with slice 6a's composites (TD-0009 A3.7, A3.9), for its benches and tests.
//!
//! The Clean page as the composites would build it, in a scroll viewport that is the page's Main
//! landmark: a level-1 heading; a row of the 7 badges; a card grid of 4 summary cards (two
//! clickable, a static status, one loading); 7 category scan cards in their states (scanned,
//! mixed, scanning, not scanned, failed with Details, nothing to clean, busy); the batch bar; an
//! inline error panel; an info bar; and a settings group of 4 rows (a check box, a radio set, a
//! dropdown and a number field with a step).
//!
//! The target pill, palette, toast, scrim, dialog, drawer, slides, scopes, grouped rows and picker
//! are later slices' (6b–6e) and join this page with them.
//!
//! [`Run`] hosts it as the platform does: a `Ui`, a `FakeText`, the clock, and, when asked,
//! painting and an assistive technology listening. Shared by `ir.rs` and the composites tests.

#![allow(
    dead_code,
    reason = "each file that includes the fixture uses a part of it"
)]

use std::fmt::Write as _;

use dpro_ui::{
    Align, Axes, Bytes, Canvas, Cx, Element, FakeText, Input, Insets, InvariantFormats, Key,
    KeyCode, KeyEvent, Landmark, Live, Mods, Motion, Point, RectPx, Size, SizeUnits, Style,
    TextEdit, TextStyle, Theme, Tri, Ui,
    accesskit::{Node, NodeId as AkId, TreeUpdate},
    composite::{
        BadgeSpec, BadgeTone, BatchBar, CardValue, Emphasis, ErrorPanel, ErrorTone, InfoBar,
        InfoTone, LogExcerpt, ScanCard, ScanLabels, ScanSize, SettingsRow, StatusTone, SummaryCard,
    },
    tokens::{self, SPACE, font},
};

/// The window, as the shell fixture's.
pub(crate) const START: Size = Size {
    w: 1000.0,
    h: 700.0,
};
/// Narrow enough that the 4 summary cards wrap two to a line (4 × 220 px and 3 gaps don't fit
/// in 640 less the page's padding), and settings rows put their control under their text.
pub(crate) const NARROW: Size = Size { w: 640.0, h: 700.0 };
/// One frame at 60 Hz, rounded down to whole milliseconds.
pub(crate) const FRAME_MS: u64 = 16;

/// The page: a scroll viewport, the Main landmark, named by its title.
pub(crate) const PAGE: Key = Key::name("page");
pub(crate) const HEADING: Key = Key::name("heading");
pub(crate) const BADGES: Key = Key::name("badges");
pub(crate) const CARDS: Key = Key::name("cards");
pub(crate) const SCANS: Key = Key::name("scans");
pub(crate) const BATCH: Key = Key::name("batch");
pub(crate) const ERROR: Key = Key::name("error");
pub(crate) const INFO: Key = Key::name("info");
pub(crate) const SETTINGS: Key = Key::name("settings");
/// A settings row's control, under the row's control slot.
pub(crate) const CONTROL: Key = Key::name("control");

/// The page's title: its level-1 heading and its landmark's name.
pub(crate) const TITLE: &str = "Clean";

/// The badges row, one of each word (A3.5 (1)).
pub(crate) const BADGE_SPECS: [BadgeSpec<'static>; 7] = [
    BadgeSpec {
        text: "Undoable",
        tone: BadgeTone::Undoable,
        description: None,
    },
    BadgeSpec {
        text: "Undoable 4 days",
        tone: BadgeTone::Undoable,
        description: Some("Can be undone until 26 November."),
    },
    BadgeSpec {
        text: "Permanent",
        tone: BadgeTone::Permanent,
        description: Some("Deleted files can't be brought back."),
    },
    BadgeSpec {
        text: "Restart required",
        tone: BadgeTone::Caution,
        description: Some("Windows Update is waiting for a restart."),
    },
    BadgeSpec {
        text: "No restart needed",
        tone: BadgeTone::Neutral,
        description: None,
    },
    BadgeSpec {
        text: "Running",
        tone: BadgeTone::Info,
        description: None,
    },
    BadgeSpec {
        text: "Partially applied",
        tone: BadgeTone::Caution,
        description: Some("3 of 7 categories were cleaned."),
    },
];

/// The scan cards' categories: name, description, and the risk badge if any.
pub(crate) const CATEGORIES: [(&str, &str, Option<BadgeSpec<'static>>); 7] = [
    (
        "Temporary files",
        "Files apps left in the temporary folders",
        None,
    ),
    (
        "Windows Update cache",
        "Downloaded updates already installed",
        Some(BadgeSpec {
            text: "Undoable",
            tone: BadgeTone::Undoable,
            description: None,
        }),
    ),
    (
        "Delivery Optimization",
        "Update files shared with other PCs",
        None,
    ),
    (
        "Thumbnails",
        "Pictures' previews, made again when needed",
        None,
    ),
    (
        "Error reports",
        "Reports already sent to Microsoft",
        Some(BadgeSpec {
            text: "Permanent",
            tone: BadgeTone::Permanent,
            description: Some("Deleted files can't be brought back."),
        }),
    ),
    ("Logs", "Setup and servicing logs", None),
    (
        "Recycle Bin",
        "Files you deleted",
        Some(BadgeSpec {
            text: "Permanent",
            tone: BadgeTone::Permanent,
            description: Some("Deleted files can't be brought back."),
        }),
    ),
];

/// The words every scan card shares.
pub(crate) const LABELS: ScanLabels<'static> = ScanLabels {
    unknown: "Not scanned",
    scanning: "Scanning",
    nothing: "Nothing to clean",
    failed: "Scan failed",
    details: "Details",
    separator: ". ",
};

/// Why a busy category's box is disabled (`ui-busy-target`).
pub(crate) const BUSY: &str = "A job is using this target";

/// The batch bar's actions.
pub(crate) const BATCH_ACTIONS: [(&str, Msg, Emphasis); 2] = [
    ("Clean selected", Msg::Batch(0), Emphasis::Primary),
    ("Review", Msg::Batch(1), Emphasis::Normal),
];

/// The error panel's log: 12 lines, of which Details shows the last 8.
pub(crate) const LOG: &str = "\
2026-11-22 09:14:02 Job 42 started: Clean, 7 categories
2026-11-22 09:14:02 Temporary files: 1,204 files
2026-11-22 09:14:03 Windows Update cache: 88 files
2026-11-22 09:14:03 Delivery Optimization: 12 files
2026-11-22 09:14:04 Thumbnails: in use by explorer.exe
2026-11-22 09:14:04 DeleteFileW failed: 0x80070020
2026-11-22 09:14:04 Error reports: 31 files
2026-11-22 09:14:05 Logs: 214 files
2026-11-22 09:14:05 Recycle Bin: locked by another process
2026-11-22 09:14:05 DeleteFileW failed: 0x80070020
2026-11-22 09:14:06 4 of 7 categories cleaned, 3 skipped
2026-11-22 09:14:06 Job 42 ended: partially applied";

/// The settings group's heading and rows.
pub(crate) const SETTINGS_HEADING: &str = "Cleanup";
pub(crate) const ROWS: [SettingsRow<'static>; 4] = [
    SettingsRow {
        label: "Empty the Recycle Bin",
        description: Some("Also removes files deleted less than 30 days ago"),
        enabled: true,
    },
    SettingsRow {
        label: "Scan when the page opens",
        description: None,
        enabled: true,
    },
    SettingsRow {
        label: "Keep logs for",
        description: Some("Older logs are cleaned with the Logs category"),
        enabled: true,
    },
    SettingsRow {
        label: "Days of restore points to keep",
        description: Some("From 1 to 90"),
        enabled: true,
    },
];
/// Row 1's radio set.
pub(crate) const WHEN: [&str; 3] = ["Always", "Once a day", "Never"];
/// Row 2's dropdown's values.
pub(crate) const KEEP: [&str; 3] = ["7 days", "30 days", "Forever"];

/// The size units' labels, as the app's catalog gives them.
const UNITS: [&str; 7] = ["{} B", "{} KB", "{} MB", "{} GB", "{} TB", "{} PB", "{} EB"];

/// What the page's controls and the engine send.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Msg {
    /// A clickable summary card.
    Open(u8),
    /// A scan card's box.
    Toggle(u8),
    /// A failed scan's Details.
    Details(u8),
    /// The table's checked rows, for the batch bar.
    Count(u32),
    /// Of them, those the filter hides.
    Hidden(u32),
    /// A category's size arrives from its scan.
    Size(u8, u64),
    /// A category's scan starts again.
    Scanning(u8),
    /// A category's scan fails.
    Failed(u8),
    /// A batch action.
    Batch(u8),
    /// The batch bar's Clear selection.
    Clear,
    /// The error panel's Details.
    Expand,
    Retry,
    OpenLog,
    Copy,
    /// The info bar's action, its Dismiss, and the engine showing it again.
    InfoAction,
    Dismiss,
    ShowInfo,
    /// Settings: the check box, the radio set, the dropdown, the field's step and its edits.
    Empty,
    When(u8),
    Keep,
    Days(bool),
    Edit(TextEdit),
}

/// What a scan card shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Scan {
    pub(crate) check: Tri,
    pub(crate) size: ScanSize,
}

/// The page's view-model.
#[allow(
    clippy::struct_excessive_bools,
    reason = "a view-model's independent switches, as a page has"
)]
pub(crate) struct Model {
    pub(crate) count: u32,
    pub(crate) hidden: u32,
    /// "{count} selected" and "{hidden} hidden by the filter", formatted on change.
    selected: String,
    hidden_text: String,
    pub(crate) scans: [Scan; 7],
    /// Scan card 6's category is busy: a job holds its target.
    pub(crate) busy: bool,
    pub(crate) expanded: bool,
    pub(crate) info: bool,
    pub(crate) empty: bool,
    pub(crate) when: u8,
    pub(crate) keep: u8,
    pub(crate) days: u32,
    days_text: String,
    /// The last message a test may look for: what a card opened, a batch action, …
    pub(crate) last: Option<Msg>,
}

impl Model {
    pub(crate) fn new() -> Self {
        let mut m = Self {
            count: 3,
            hidden: 1,
            selected: String::with_capacity(32),
            hidden_text: String::with_capacity(48),
            scans: [
                Scan {
                    check: Tri::On,
                    size: ScanSize::Bytes(Bytes(1_288_490_189)),
                },
                Scan {
                    check: Tri::Mixed,
                    size: ScanSize::Bytes(Bytes(3_650_722_201)),
                },
                Scan {
                    check: Tri::Off,
                    size: ScanSize::Scanning,
                },
                Scan {
                    check: Tri::Off,
                    size: ScanSize::Unknown,
                },
                Scan {
                    check: Tri::Off,
                    size: ScanSize::Failed,
                },
                Scan {
                    check: Tri::Off,
                    size: ScanSize::Bytes(Bytes(0)),
                },
                Scan {
                    check: Tri::On,
                    size: ScanSize::Bytes(Bytes(671_088_640)),
                },
            ],
            busy: true,
            expanded: false,
            info: true,
            empty: true,
            when: 1,
            keep: 1,
            days: 30,
            days_text: String::with_capacity(8),
            last: None,
        };
        m.format();
        m
    }

    /// The counts' and the field's text, into their buffers: nothing allocates once they're big
    /// enough.
    fn format(&mut self) {
        self.selected.clear();
        let _ = write!(self.selected, "{} selected", self.count);
        self.hidden_text.clear();
        let _ = write!(self.hidden_text, "{} hidden by the filter", self.hidden);
        self.days_text.clear();
        let _ = write!(self.days_text, "{}", self.days);
    }

    pub(crate) fn update(&mut self, msg: Msg) {
        match &msg {
            Msg::Count(n) => self.count = *n,
            Msg::Hidden(n) => self.hidden = *n,
            Msg::Clear => self.count = 0,
            Msg::Toggle(i) => {
                if let Some(s) = self.scans.get_mut(usize::from(*i)) {
                    s.check = if s.check == Tri::On {
                        Tri::Off
                    } else {
                        Tri::On
                    };
                }
            }
            Msg::Size(i, b) => self.set(*i, ScanSize::Bytes(Bytes(*b))),
            Msg::Scanning(i) => self.set(*i, ScanSize::Scanning),
            Msg::Failed(i) => self.set(*i, ScanSize::Failed),
            Msg::Expand => self.expanded = !self.expanded,
            Msg::Dismiss => self.info = false,
            Msg::ShowInfo => self.info = true,
            Msg::Empty => self.empty = !self.empty,
            Msg::When(w) => self.when = *w,
            Msg::Days(up) => {
                self.days = if *up {
                    (self.days + 1).min(90)
                } else {
                    self.days.saturating_sub(1).max(1)
                };
            }
            Msg::Open(_)
            | Msg::Details(_)
            | Msg::Batch(_)
            | Msg::Retry
            | Msg::OpenLog
            | Msg::Copy
            | Msg::InfoAction
            | Msg::Keep
            | Msg::Edit(_) => {}
        }
        self.format();
        self.last = Some(msg);
    }

    fn set(&mut self, i: u8, size: ScanSize) {
        if let Some(s) = self.scans.get_mut(usize::from(i)) {
            s.size = size;
        }
    }
}

/// The page.
pub(crate) fn view(m: &Model, cx: &mut Cx<'_, Msg>) {
    cx.add_with(
        Element::scroll(PAGE, Axes::Y)
            .style(Style {
                grow: 1.0,
                padding: Insets::all(SPACE[4]),
                gap: SPACE[4],
                align: Align::Stretch,
                ..Style::default()
            })
            .name(TITLE)
            .landmark(Landmark::Main),
        |cx| {
            cx.add(
                Element::text(HEADING, TITLE)
                    .text_style(TextStyle {
                        size: font::TITLE,
                        weight: font::SEMIBOLD,
                        ..TextStyle::default()
                    })
                    .heading(1),
            );
            cx.badges(BADGES, &BADGE_SPECS);
            cards(cx);
            scans(m, cx);
            cx.batch_bar(
                BATCH,
                &BatchBar {
                    count: m.count,
                    hidden: m.hidden,
                    selected: &m.selected,
                    hidden_text: &m.hidden_text,
                    note: "Categories are cleaned one at a time",
                    actions: &BATCH_ACTIONS,
                    clear: ("Clear selection", Msg::Clear),
                    name: "Selection",
                },
            );
            cx.error_panel(
                ERROR,
                &ErrorPanel {
                    tone: ErrorTone::Partial,
                    title: "3 of 7 categories couldn't be cleaned",
                    code: "Code 0x80070020",
                    api: "DeleteFileW",
                    next: "Close the programs using these files, then try again.",
                    log: Some(LogExcerpt {
                        source: "C:\\ProgramData\\DISM Pro\\logs\\job-42.log",
                        text: LOG,
                    }),
                    expanded: m.expanded,
                    details: ("Details", Msg::Expand),
                    actions: [
                        Some(("Try again", Msg::Retry)),
                        Some(("Open log", Msg::OpenLog)),
                    ],
                    copy: ("Copy details", Msg::Copy),
                    live: Live::Assertive,
                },
            );
            if m.info {
                cx.info_bar(
                    INFO,
                    &InfoBar {
                        tone: InfoTone::Warning,
                        title: "Windows Update is running",
                        message: "Its cache can't be cleaned until it finishes.",
                        action: Some(("Open Windows Update", Msg::InfoAction)),
                        dismiss: Some(("Dismiss", Msg::Dismiss)),
                        live: Live::Polite,
                    },
                );
            }
            settings(m, cx);
        },
    );
}

/// The card grid: two clickable metrics, a static status and one loading.
fn cards(cx: &mut Cx<'_, Msg>) {
    cx.card_grid(CARDS, |cx| {
        let cards = [
            SummaryCard {
                label: "Space to free",
                value: CardValue::Metric("5.6 GB"),
                detail: "Across 4 categories",
                delta: Some(("1.1 GB more than last week", StatusTone::Warn)),
                open: Some(Msg::Open(0)),
                separator: ". ",
            },
            SummaryCard {
                label: "System health",
                value: CardValue::Status("Healthy", StatusTone::Ok),
                detail: "No restart pending",
                delta: None,
                open: None,
                separator: ". ",
            },
            SummaryCard {
                label: "Last cleanup",
                value: CardValue::Metric("4 days ago"),
                detail: "2.3 GB freed",
                delta: None,
                open: Some(Msg::Open(2)),
                separator: ". ",
            },
            SummaryCard {
                label: "Undo history",
                value: CardValue::Loading,
                detail: "",
                delta: None,
                open: None,
                separator: ". ",
            },
        ];
        for (i, c) in (0..).zip(&cards) {
            cx.summary_card(Key::index(i), c);
        }
    });
}

/// The 7 scan cards, in a column.
fn scans(m: &Model, cx: &mut Cx<'_, Msg>) {
    cx.add_with(
        Element::stack(SCANS).style(Style {
            gap: SPACE[2],
            align: Align::Stretch,
            ..Style::default()
        }),
        |cx| {
            for (i, ((name, description, badge), scan)) in
                (0..).zip(CATEGORIES.iter().zip(&m.scans))
            {
                cx.scan_card(
                    Key::index(u64::from(i)),
                    &ScanCard {
                        check: scan.check,
                        name,
                        description,
                        badge: *badge,
                        size: scan.size,
                        busy: (m.busy && i == 6).then_some(BUSY),
                        toggle: Msg::Toggle(i),
                        details: (scan.size == ScanSize::Failed).then_some(Msg::Details(i)),
                        labels: &LABELS,
                    },
                );
            }
        },
    );
}

/// The settings group: a check box, a radio set, a dropdown, a number field.
fn settings(m: &Model, cx: &mut Cx<'_, Msg>) {
    cx.settings_group(SETTINGS, SETTINGS_HEADING, |cx| {
        let [empty, when, keep, days] = &ROWS;
        cx.settings_row(Key::index(0), empty, |cx| {
            let state = if m.empty { Tri::On } else { Tri::Off };
            let mut el = Element::toggle(CONTROL, empty.label, state, Msg::Empty);
            if let Some(d) = empty.description {
                el = el.describe(d);
            }
            cx.add(el);
        });
        cx.settings_row(Key::index(1), when, |cx| {
            cx.add_with(
                Element::radios(CONTROL).name(when.label).style(Style {
                    gap: SPACE[1],
                    ..Style::default()
                }),
                |cx| {
                    for (w, label) in (0..).zip(WHEN) {
                        cx.add(
                            Element::radio(
                                Key::index(u64::from(w)),
                                label,
                                m.when == w,
                                Msg::When(w),
                            )
                            .style(Style {
                                min: Size {
                                    w: 0.0,
                                    h: SPACE[5],
                                },
                                ..Style::default()
                            }),
                        );
                    }
                },
            );
        });
        cx.settings_row(Key::index(2), keep, |cx| {
            let mut el = Element::combo(
                CONTROL,
                keep.label,
                KEEP[usize::from(m.keep)],
                false,
                Msg::Keep,
            );
            if let Some(d) = keep.description {
                el = el.describe(d);
            }
            cx.add(el);
        });
        cx.settings_row(Key::index(3), days, |cx| {
            let mut el = Element::field(CONTROL, &m.days_text, Msg::Edit)
                .name(days.label)
                .step(Msg::Days);
            if let Some(d) = days.description {
                el = el.describe(d);
            }
            cx.add(el);
        });
    });
}

/// A step of a script.
#[derive(Clone, Debug)]
pub(crate) enum Step {
    Input(Input),
    /// A message straight to the view-model, as from the engine or the table's checks.
    Update(Msg),
    Resize(Size),
    /// The clock moves on by this many milliseconds.
    Tick(u64),
    /// Windows' text size changed, as a factor: 1 to 2.25.
    TextScale(f32),
}

/// A key pressed, as a step.
pub(crate) fn key(key: KeyCode, mods: Mods) -> Step {
    Step::Input(Input::Key(KeyEvent {
        key,
        down: true,
        mods,
    }))
}

/// A key let go, as a step: Space activates a button or a check box on its release.
pub(crate) fn key_up(key: KeyCode) -> Step {
    Step::Input(Input::Key(KeyEvent {
        key,
        down: false,
        mods: Mods::default(),
    }))
}

/// The page, its Ui and text system, laid out at [`START`] when created; painted too, when made by
/// [`Run::painted`].
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
    /// The last frame's damage, kept for the tests before it's cleared.
    pub(crate) damage: Vec<RectPx>,
    /// Layouts run since the run started: a frame's build and layout counts one.
    pub(crate) frames: u64,
}

impl Run {
    /// Frames end at layout and damage.
    pub(crate) fn with(model: Model) -> Self {
        Self::new(model, None, 1.0)
    }

    /// Frames end at layout and damage, with Windows' text size at `text_scale` from the first
    /// frame.
    pub(crate) fn text_scaled(model: Model, text_scale: f32) -> Self {
        Self::new(model, None, text_scale)
    }

    /// Frames are painted too, at scale 1 in `theme`.
    pub(crate) fn painted(model: Model, theme: Theme) -> Self {
        Self::new(model, Some(theme), 1.0)
    }

    /// An assistive technology listens: the first frame's whole tree is served, then every
    /// frame's changes are taken.
    pub(crate) fn accessible(model: Model) -> Self {
        let mut run = Self::new(model, None, 1.0);
        drop(run.ui.a11y_tree());
        run.take_a11y = true;
        run
    }

    fn new(model: Model, theme: Option<Theme>, text_scale: f32) -> Self {
        let motion = Motion {
            hover_ms: tokens::motion::HOVER_MS,
        };
        let mut run = Self {
            ui: Ui::new(START, 1.0, motion),
            model,
            text: FakeText,
            msgs: Vec::with_capacity(4),
            theme,
            pixels: Vec::new(),
            now: 0,
            take_a11y: false,
            damage: Vec::with_capacity(256),
            frames: 0,
        };
        run.ui.set_active(true);
        run.ui
            .set_formats(InvariantFormats::new(SizeUnits::new(UNITS)));
        run.ui.set_text_scale(text_scale);
        run.frame();
        run
    }

    /// One event through the whole pipeline: dispatch, update, view, reconcile, layout, damage,
    /// and raster when painted.
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
            Step::TextScale(t) => self.ui.set_text_scale(t),
        }
        for m in self.msgs.drain(..) {
            self.model.update(m);
        }
        self.frame();
    }

    /// Ticks 60 Hz frames until the UI stops asking for them.
    pub(crate) fn settle(&mut self) {
        while self.ui.wants_frame() {
            self.step(Step::Tick(FRAME_MS));
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
        let model = &self.model;
        self.ui.build(|cx| view(model, cx));
        self.ui.layout(&mut self.text);
        self.frames += 1;
        if let Some(theme) = &self.theme {
            let (w, h) = self.ui.pixel_size();
            self.pixels
                .resize(usize::try_from(u64::from(w) * u64::from(h)).unwrap_or(0), 0);
            if let Some(mut canvas) = Canvas::new(&mut self.pixels, w, h) {
                self.ui.paint(&mut canvas, &mut self.text, theme);
            }
        }
        self.damage.clear();
        self.damage.extend_from_slice(self.ui.damage());
        self.ui.clear_damage();
        if self.take_a11y {
            drop(self.ui.a11y_update());
        }
    }

    /// The node at `path` from the root.
    pub(crate) fn find(&self, path: &[Key]) -> Option<dpro_ui::NodeId> {
        self.ui.find(path)
    }

    /// The center of the node at `path`.
    pub(crate) fn center(&self, path: &[Key]) -> Point {
        self.ui
            .find(path)
            .and_then(|id| self.ui.info(id))
            .map_or(Point::default(), |n| n.rect.center())
    }

    /// The first node keyed `key` under the node at `path`, breadth first: a composite's part,
    /// wherever the composite nests it.
    pub(crate) fn part(&self, path: &[Key], key: Key) -> Option<dpro_ui::NodeId> {
        self.part_of(self.ui.find(path)?, key)
    }

    /// The first node keyed `key` under node `root`, breadth first.
    pub(crate) fn part_of(&self, root: dpro_ui::NodeId, key: Key) -> Option<dpro_ui::NodeId> {
        let mut queue = std::collections::VecDeque::from([root]);
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

    /// Node `id` as a client sees it in `tree`, if it's there.
    pub(crate) fn ak<'t>(&self, tree: &'t TreeUpdate, id: dpro_ui::NodeId) -> Option<&'t Node> {
        tree.nodes
            .iter()
            .find(|(ak, _)| self.ui.a11y_node(*ak) == Some(id))
            .map(|(_, node)| node)
    }

    /// The AccessKit id of node `id` in `tree`, if it's there.
    pub(crate) fn ak_id(&self, tree: &TreeUpdate, id: dpro_ui::NodeId) -> Option<AkId> {
        tree.nodes
            .iter()
            .find(|(ak, _)| self.ui.a11y_node(*ak) == Some(id))
            .map(|(ak, _)| *ak)
    }

    /// The nodes of `tree` under `root` (itself included) in tree order, depth first.
    pub(crate) fn walk(tree: &TreeUpdate, root: AkId) -> Vec<(AkId, &Node)> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if let Some((_, node)) = tree.nodes.iter().find(|(ak, _)| *ak == id) {
                out.push((id, node));
                stack.extend(node.children().iter().rev());
            }
        }
        out
    }

    /// The whole tree's nodes in tree order, from its root.
    pub(crate) fn tree_order(tree: &TreeUpdate) -> Vec<(AkId, &Node)> {
        tree.tree
            .as_ref()
            .map_or_else(Vec::new, |t| Self::walk(tree, t.root))
    }

    /// Steps that warm every composite: counts up and down to 0 and back, sizes arriving, a scan
    /// failing and starting again, Details open and shut, the info bar gone and back, every
    /// setting changed and back, a resize narrow and back, and Tab through the page.
    pub(crate) fn session() -> Vec<Step> {
        let mut steps = vec![
            Step::Update(Msg::Count(4)),
            Step::Update(Msg::Count(12)),
            Step::Update(Msg::Hidden(0)),
            Step::Update(Msg::Hidden(2)),
            Step::Update(Msg::Clear),
            Step::Update(Msg::Count(1)),
            Step::Update(Msg::Size(2, 104_857_600)),
            Step::Update(Msg::Size(0, 1_395_864_371)),
            Step::Update(Msg::Failed(3)),
            Step::Update(Msg::Scanning(3)),
            Step::Update(Msg::Expand),
            Step::Update(Msg::Expand),
            Step::Update(Msg::Dismiss),
            Step::Update(Msg::ShowInfo),
            Step::Update(Msg::Empty),
            Step::Update(Msg::Empty),
            Step::Update(Msg::When(2)),
            Step::Update(Msg::When(1)),
            Step::Update(Msg::Days(true)),
            Step::Update(Msg::Days(false)),
            Step::Resize(NARROW),
            Step::Resize(START),
            Step::TextScale(2.25),
            Step::TextScale(1.0),
        ];
        for _ in 0..12 {
            steps.push(key(KeyCode::Tab, Mods::default()));
        }
        steps.push(Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 })));
        // The state the benches start from: 3 checked, 1 hidden, scan 2 back to scanning, scan 0
        // at 1.2 GB.
        steps.extend([
            Step::Update(Msg::Count(3)),
            Step::Update(Msg::Hidden(1)),
            Step::Update(Msg::Scanning(2)),
            Step::Update(Msg::Size(0, 1_288_490_189)),
            Step::Tick(u64::from(tokens::motion::HOVER_MS) + FRAME_MS),
        ]);
        steps
    }

    /// `run` after the session, settled.
    pub(crate) fn warmed(mut self) -> Self {
        for s in Self::session() {
            self.step(s);
        }
        self.settle();
        self
    }
}
