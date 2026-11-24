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
    Align, Axes, Bytes, Canvas, Cx, Dim, Element, FakeText, Input, Insets, InvariantFormats, Key,
    KeyCode, KeyEvent, Landmark, Live, Mods, Motion, Point, Popup, PopupKind, RectPx, Size,
    SizeUnits, Style, TextEdit, TextStyle, Theme, Tri, Ui,
    accesskit::{Node, NodeId as AkId, TreeUpdate},
    composite::{
        BadgeSpec, BadgeTone, BatchBar, CardValue, DialogFrame, Emphasis, ErrorPanel, ErrorTone,
        InfoBar, InfoTone, JobCard, LogExcerpt, PaletteRow, PaletteView, ScanCard, ScanLabels,
        ScanSize, SettingsRow, StatusTone, SummaryCard, Toast, ToastTone,
    },
    tokens::{self, SPACE, font},
};

/// The window, as the shell fixture's.
pub(crate) const START: Size = Size {
    w: 1000.0,
    h: 700.0,
};
/// Narrow enough that the 4 summary cards wrap two to a line (4 × 220 px and 3 gaps don't fit
/// in 640 less the page's padding). Settings rows still keep their control at the end: they put it
/// under their text below 600 px (`tokens::settings::NARROW`).
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
    /// A field's edit: the palette's while it's open (it takes activation, so its field is the
    /// one typed into), else the Days field's, which only its step changes. One variant for both
    /// (slice 6c): a second one holding a `TextEdit` costs `Msg` a tag, 8 bytes, and every
    /// composites bench a wider message to move.
    Edit(TextEdit),
    // Slice 6c.
    /// Ctrl+K: the palette opens with an empty query; and its `on_close` (Esc, a click outside).
    PaletteOpen,
    PaletteClose,
    /// The palette's field: Up (`true`) or Down, Enter, and a click on result `i`.
    PaletteStep(bool),
    PaletteRun,
    PalettePick(u32),
    /// A job starts (its toast shows, and its card), its progress in percent, its line brought up
    /// to date (at most every 2 s, §4.4), a second of it, and its end.
    JobStart,
    Progress(u32),
    JobLine,
    JobTick,
    JobEnd(ToastTone),
    /// The toast's Dismiss (its button, Esc, an ended toast's hide), and its actions.
    ToastDismiss,
    ToastView,
    ToastUndo,
    /// The job card's Cancel.
    JobCancel,
    /// The close-while-running dialog opens; one of its buttons; its cancel (Esc, a click
    /// outside).
    DialogOpen,
    DialogAnswer(u8),
    DialogCancel,
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
    /// Slice 6c: the palette, the job and the dialog.
    pub(crate) over: Overlay,
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
            over: Overlay::new(),
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
            | Msg::Keep => {}
            Msg::Edit(_) if !self.over.palette => {}
            // Slice 6c.
            Msg::PaletteOpen
            | Msg::PaletteClose
            | Msg::Edit(_)
            | Msg::PaletteStep(_)
            | Msg::PaletteRun
            | Msg::PalettePick(_)
            | Msg::JobStart
            | Msg::Progress(_)
            | Msg::JobLine
            | Msg::JobTick
            | Msg::JobEnd(_)
            | Msg::ToastDismiss
            | Msg::ToastView
            | Msg::ToastUndo
            | Msg::JobCancel
            | Msg::DialogOpen
            | Msg::DialogAnswer(_)
            | Msg::DialogCancel => self.over.update(&msg),
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
            // Slice 6c: a running job's card, as Activity's Running section shows it.
            if m.over.job.is_some_and(|j| j.tone == ToastTone::Running) {
                job_card(m, cx);
            }
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
            // A target at least 24 px high, as the radios are (the caller sizes its control).
            let mut el = Element::toggle(CONTROL, empty.label, state, Msg::Empty).style(Style {
                min: Size {
                    w: 0.0,
                    h: SPACE[5],
                },
                ..Style::default()
            });
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
            // A field is as wide as its view makes it.
            let mut el = Element::field(CONTROL, &m.days_text, Msg::Edit)
                .name(days.label)
                .step(Msg::Days)
                .style(Style {
                    width: Dim::Fixed(SPACE[5] * 3.0),
                    padding: Insets::xy(SPACE[1], 0.0),
                    min: Size {
                        w: 0.0,
                        h: SPACE[5],
                    },
                    ..Style::default()
                });
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
    /// Slice 6c: input to the popup's window (the palette or the dialog), and to the toast's.
    Popup(Input),
    Toast(Input),
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
    /// Slice 6c: the popup's and the toast's windows.
    pub(crate) win: Windows,
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
            win: Windows::new(motion),
        };
        run.ui.set_active(true);
        run.ui
            .set_formats(InvariantFormats::new(SizeUnits::new(UNITS)));
        run.ui.set_text_scale(text_scale);
        run.win.popup.set_text_scale(text_scale);
        run.win.toast.set_text_scale(text_scale);
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
                self.win.tick(self.now, &mut self.msgs);
            }
            Step::TextScale(t) => {
                self.ui.set_text_scale(t);
                self.win.popup.set_text_scale(t);
                self.win.toast.set_text_scale(t);
            }
            Step::Popup(input) => self.popup_input(input),
            Step::Toast(input) => {
                if self.win.toast_open {
                    self.win.toast.dispatch(input, &mut self.msgs);
                }
            }
        }
        for m in self.msgs.drain(..) {
            self.model.update(m);
        }
        self.frame();
    }

    /// Ticks 60 Hz frames until the UI stops asking for them.
    pub(crate) fn settle(&mut self) {
        while self.ui.wants_frame() || self.win.wants_frame() {
            self.step(Step::Tick(FRAME_MS));
        }
    }

    /// How long until the platform's one-shot wake is due, if it is: the caret's next blink, a
    /// tooltip's next change, or an ended toast's hide.
    pub(crate) fn wake(&mut self) -> Option<u64> {
        [
            self.ui.caret_timer(self.now),
            self.ui.tooltip_timer(self.now),
            self.win
                .popup_open
                .and_then(|_| self.win.popup.caret_timer(self.now)),
            self.win
                .toast_open
                .then(|| self.win.toast.expiry_timer(self.now))
                .flatten(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// What the platform does with input to the popup's window: Esc sends its `on_close`; the
    /// palette's and the dialog's messages run as they come (they close themselves).
    fn popup_input(&mut self, input: Input) {
        let Some(open) = popup(&self.model) else {
            return;
        };
        if let Input::Key(k) = input
            && k.down
            && k.key == KeyCode::Escape
        {
            self.msgs.push(open.on_close);
            return;
        }
        self.win.popup.dispatch(input, &mut self.msgs);
    }

    fn frame(&mut self) {
        // The scrim shows over the page while the palette or the dialog is open (A3.5 addition
        // 13).
        self.ui.set_scrim(popup(&self.model).is_some());
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
        self.win.frame(
            &self.model,
            &mut self.text,
            self.theme.as_ref(),
            self.take_a11y,
        );
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

// ---- Slice 6c: the palette, a job's toast and card, and the dialog (A3.7) -------------------

/// The palette's popup, the toast and the dialog's popup; the job card is on the page.
pub(crate) const PALETTE: Key = Key::name("palette");
pub(crate) const TOAST: Key = Key::name("toast");
pub(crate) const JOB: Key = Key::name("job");
pub(crate) const DIALOG: Key = Key::name("dialog");
/// The dialog's body text.
pub(crate) const DIALOG_TEXT: Key = Key::name("dialog-text");

/// The palette's entries as the app's registry ranks them (A5 owns the real ones): a command,
/// where it is, and its keys. Pages, then commands, then settings.
pub(crate) const ENTRIES: [(&str, &str, &str); 64] = [
    ("Home", "Page", "Ctrl+1"),
    ("Clean", "Page", "Ctrl+2"),
    ("Optimize", "Page", "Ctrl+3"),
    ("Drivers", "Page", "Ctrl+4"),
    ("Updates", "Page", "Ctrl+5"),
    ("Apps", "Page", "Ctrl+6"),
    ("Features", "Page", ""),
    ("Startup", "Page", ""),
    ("Services", "Page", ""),
    ("Activity", "Page", "Ctrl+0"),
    ("Images", "Page", ""),
    ("Boot entries", "Page", ""),
    ("Restore points", "Page", ""),
    ("Hosts file", "Page", ""),
    ("File associations", "Page", ""),
    ("Settings", "Page", "Ctrl+,"),
    ("Scan for junk files", "Clean", "F5"),
    ("Clean selected categories", "Clean", "Ctrl+Enter"),
    ("Empty the Recycle Bin", "Clean", ""),
    ("Clean the Windows Update cache", "Clean", ""),
    ("Delete error reports", "Clean", ""),
    ("Clean thumbnails", "Clean", ""),
    ("Clean Delivery Optimization files", "Clean", ""),
    ("Analyze the component store", "Optimize", ""),
    ("Clean up the component store", "Optimize", ""),
    ("Reset the base of superseded components", "Optimize", ""),
    ("Export drivers", "Drivers", ""),
    ("Remove old driver packages", "Drivers", ""),
    ("Add a driver", "Drivers", ""),
    ("Check for updates", "Updates", ""),
    ("Hide an update", "Updates", ""),
    ("Uninstall an update", "Updates", ""),
    ("Uninstall an app", "Apps", ""),
    ("Remove a provisioned app", "Apps", ""),
    ("Turn a feature on or off", "Features", ""),
    ("Disable a startup item", "Startup", ""),
    ("Change the startup type of a service", "Services", ""),
    ("Undo the last job", "Activity", "Ctrl+Z"),
    ("Open the job log", "Activity", ""),
    ("Mount an image", "Images", ""),
    ("Unmount an image", "Images", ""),
    ("Capture an image", "Images", ""),
    ("Change the target", "Target", "Ctrl+T"),
    ("Open an offline Windows", "Target", ""),
    ("Create a restore point", "Restore points", ""),
    ("Edit the hosts file", "Hosts file", ""),
    ("Copy diagnostics", "Help", ""),
    ("Check for a new version", "Help", ""),
    ("Theme", "Appearance", ""),
    ("Glass effects", "Appearance", ""),
    ("Text size", "Appearance", ""),
    ("Language", "Language", ""),
    ("Keep logs for", "Cleanup", ""),
    ("Empty the Recycle Bin when cleaning", "Cleanup", ""),
    ("Scan when the page opens", "Cleanup", ""),
    ("Restore point before each job", "Safety", ""),
    ("Undo history", "Safety", ""),
    ("Confirm permanent changes", "Safety", ""),
    ("Agent access", "Agents", ""),
    ("Command palette shortcut", "Keyboard", ""),
    ("Show accelerator keys", "Keyboard", ""),
    ("Telemetry", "Privacy", ""),
    ("Update channel", "Updates", ""),
    ("Reset all settings", "Advanced", ""),
];
/// The areas: each one's first entry and its header.
pub(crate) const AREAS: [(u32, &str); 3] = [(0, "Pages"), (16, "Commands"), (48, "Settings")];
/// At most this many results (A5's `palette::MAX_ROWS`).
pub(crate) const MAX_ROWS: usize = 50;
/// The entries that can't be undone carry a badge.
const PERMANENT: [u32; 2] = [20, 25];
/// The entry disabled while a job holds its target.
pub(crate) const DISABLED: u32 = 24;
/// The job the toast and the card show.
pub(crate) const JOB_NAME: &str = "Cleanup";
/// The dialog's buttons: none is the default; focus starts on the first.
pub(crate) const DIALOG_BUTTONS: [(&str, Msg, Emphasis); 3] = [
    ("Keep running", Msg::DialogAnswer(0), Emphasis::Normal),
    (
        "Close when it's done",
        Msg::DialogAnswer(1),
        Emphasis::Primary,
    ),
    (
        "Cancel the job and close",
        Msg::DialogAnswer(2),
        Emphasis::Danger,
    ),
];

/// A job, as the toast and the card show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Job {
    /// How far it has come, in percent.
    pub(crate) percent: u16,
    pub(crate) tone: ToastTone,
    /// Its toast shows.
    pub(crate) toast: bool,
    pub(crate) cancelling: bool,
    pub(crate) seconds: u32,
}

/// What shows over the page and in its windows: the palette, a job, the dialog.
pub(crate) struct Overlay {
    pub(crate) palette: bool,
    pub(crate) query: String,
    pub(crate) rows: Vec<PaletteRow<'static>>,
    pub(crate) groups: Vec<(u32, &'static str)>,
    pub(crate) highlighted: Option<u32>,
    pub(crate) status: String,
    pub(crate) job: Option<Job>,
    /// The job's line and elapsed time, formatted on change.
    pub(crate) line: String,
    pub(crate) elapsed: String,
    pub(crate) dialog: bool,
    /// What the palette, the toast or the dialog last ran.
    pub(crate) ran: Option<Msg>,
}

impl Overlay {
    fn new() -> Self {
        let mut o = Self {
            palette: false,
            query: String::with_capacity(64),
            rows: Vec::with_capacity(MAX_ROWS),
            groups: Vec::with_capacity(AREAS.len()),
            highlighted: None,
            status: String::with_capacity(128),
            job: None,
            line: String::with_capacity(96),
            elapsed: String::with_capacity(16),
            dialog: false,
            ran: None,
        };
        o.refilter();
        o
    }

    /// Whether the toast shows.
    pub(crate) fn toast(&self) -> bool {
        self.job.is_some_and(|j| j.toast)
    }

    fn update(&mut self, msg: &Msg) {
        match msg {
            Msg::PaletteOpen => {
                self.palette = true;
                self.query.clear();
                self.refilter();
            }
            Msg::PaletteClose => self.palette = false,
            Msg::Edit(edit) => {
                if edit.apply(&mut self.query) {
                    self.refilter();
                }
            }
            Msg::PaletteStep(up) => {
                let n = u32::try_from(self.rows.len()).unwrap_or(0);
                if n > 0 {
                    let h = self.highlighted.unwrap_or(0);
                    self.highlighted = Some(if *up { (h + n - 1) % n } else { (h + 1) % n });
                }
            }
            Msg::PaletteRun => {
                if let Some(h) = self.highlighted {
                    self.run(h);
                }
            }
            Msg::PalettePick(i) => self.run(*i),
            Msg::JobStart => {
                self.job = Some(Job {
                    percent: 0,
                    tone: ToastTone::Running,
                    toast: true,
                    cancelling: false,
                    seconds: 0,
                });
                self.format_job();
            }
            Msg::Progress(p) => {
                if let Some(j) = &mut self.job {
                    j.percent = u16::try_from((*p).min(100)).unwrap_or(100);
                }
            }
            Msg::JobLine | Msg::JobTick | Msg::JobEnd(_) | Msg::JobCancel => {
                if let Some(j) = &mut self.job {
                    match msg {
                        Msg::JobTick => j.seconds += 1,
                        Msg::JobEnd(tone) => {
                            j.tone = *tone;
                            j.toast = true;
                        }
                        Msg::JobCancel => j.cancelling = true,
                        _ => {}
                    }
                }
                self.format_job();
            }
            Msg::ToastDismiss => {
                if let Some(j) = &mut self.job {
                    j.toast = false;
                    // An ended job is in Activity's history; a running one goes on.
                    if j.tone != ToastTone::Running {
                        self.job = None;
                    }
                }
            }
            Msg::ToastView | Msg::ToastUndo => self.ran = Some(msg.clone()),
            Msg::DialogOpen => self.dialog = true,
            Msg::DialogAnswer(_) => {
                self.ran = Some(msg.clone());
                self.dialog = false;
            }
            Msg::DialogCancel => self.dialog = false,
            _ => {}
        }
    }

    /// Result `i` runs, if it's enabled: the palette closes.
    fn run(&mut self, i: u32) {
        if self.rows.get(i as usize).is_some_and(|r| r.enabled) {
            self.ran = Some(Msg::PalettePick(i));
            self.palette = false;
        }
    }

    /// The results for the query: the entries whose label holds it (ASCII case aside), at most
    /// [`MAX_ROWS`], each area under its header; the first highlighted; "no matches" if none.
    fn refilter(&mut self) {
        self.rows.clear();
        self.groups.clear();
        for (entry, (label, detail, accel)) in (0..).zip(ENTRIES) {
            if self.rows.len() == MAX_ROWS {
                break;
            }
            if !holds(label, &self.query) {
                continue;
            }
            let at = u32::try_from(self.rows.len()).unwrap_or(u32::MAX);
            if let Some(&(_, header)) = AREAS.iter().rev().find(|(first, _)| entry >= *first)
                && self.groups.last().is_none_or(|&(_, h)| h != header)
            {
                self.groups.push((at, header));
            }
            self.rows.push(PaletteRow {
                entry,
                label,
                detail,
                accel,
                badge: PERMANENT.contains(&entry).then_some(BadgeSpec {
                    text: "Permanent",
                    tone: BadgeTone::Permanent,
                    description: Some("Can't be undone."),
                }),
                enabled: entry != DISABLED,
                reason: if entry == DISABLED { BUSY } else { "" },
            });
        }
        self.highlighted = (!self.rows.is_empty()).then_some(0);
        self.status.clear();
        if self.rows.is_empty() {
            let _ = write!(
                self.status,
                "No commands match \u{201c}{}\u{201d}.",
                self.query
            );
        }
    }

    /// The job's line and elapsed time, into their buffers.
    fn format_job(&mut self) {
        let Some(j) = self.job else {
            return;
        };
        self.line.clear();
        let _ = match j.tone {
            ToastTone::Running => write!(self.line, "{JOB_NAME}: {} %", j.percent),
            ToastTone::Done => write!(self.line, "{JOB_NAME} finished."),
            ToastTone::Warning => write!(
                self.line,
                "{JOB_NAME} finished. Restart Windows to complete it."
            ),
            ToastTone::Failed => {
                write!(self.line, "{JOB_NAME} failed. 4 of 7 changes were applied.")
            }
        };
        self.elapsed.clear();
        let _ = write!(self.elapsed, "{}:{:02}", j.seconds / 60, j.seconds % 60);
    }
}

/// Whether `hay` holds `needle`, ASCII case aside, without allocating.
fn holds(hay: &str, needle: &str) -> bool {
    needle.is_empty()
        || hay
            .as_bytes()
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// The model's popup: the palette or the dialog.
pub(crate) fn popup(m: &Model) -> Option<Popup<Msg>> {
    if m.over.palette {
        Some(Popup {
            kind: PopupKind::Palette,
            on_close: Msg::PaletteClose,
        })
    } else if m.over.dialog {
        Some(Popup {
            kind: PopupKind::Dialog,
            on_close: Msg::DialogCancel,
        })
    } else {
        None
    }
}

/// What the popup's window shows.
pub(crate) fn popup_view(m: &Model, cx: &mut Cx<'_, Msg>) {
    if m.over.palette {
        palette_view(m, cx);
    } else if m.over.dialog {
        dialog_view(cx);
    }
}

/// The palette's view.
pub(crate) fn palette_view(m: &Model, cx: &mut Cx<'_, Msg>) {
    let o = &m.over;
    cx.palette(
        PALETTE,
        &PaletteView {
            name: "Command palette",
            field: "Search commands",
            placeholder: "Type a command, page or setting",
            results: "Commands",
            esc: "Esc",
            query: &o.query,
            on_edit: Msg::Edit,
            on_step: Msg::PaletteStep,
            on_run: Msg::PaletteRun,
            on_pick: Msg::PalettePick,
            rows: &o.rows,
            groups: &o.groups,
            highlighted: o.highlighted,
            status: &o.status,
            hints: ["\u{2191}\u{2193} to move", "Enter to open", "Esc to close"],
        },
    );
}

/// The toast's view, while it shows.
pub(crate) fn toast_view(m: &Model, cx: &mut Cx<'_, Msg>) {
    let Some(j) = m.over.job.filter(|j| j.toast) else {
        return;
    };
    let ended = j.tone != ToastTone::Running;
    cx.toast(
        TOAST,
        &Toast {
            tone: j.tone,
            region: "Job status",
            title: JOB_NAME,
            line: &m.over.line,
            progress: (!ended).then(|| f32::from(j.percent) / 100.0),
            actions: [
                ended.then_some(("View", Msg::ToastView)),
                (j.tone == ToastTone::Done).then_some(("Undo\u{2026}", Msg::ToastUndo)),
            ],
            dismiss: ("Dismiss", Msg::ToastDismiss),
        },
    );
}

/// The running job's card. Its line is live only while the toast doesn't show (one live view of
/// a job, A3.5 (7)).
fn job_card(m: &Model, cx: &mut Cx<'_, Msg>) {
    let Some(j) = m.over.job else {
        return;
    };
    cx.job_card(
        JOB,
        &JobCard {
            title: JOB_NAME,
            elapsed: &m.over.elapsed,
            progress: f32::from(j.percent) / 100.0,
            line: &m.over.line,
            live: !j.toast,
            cancel: Some((
                if j.cancelling {
                    "Cancelling\u{2026}"
                } else {
                    "Cancel"
                },
                Msg::JobCancel,
            )),
            cancelling: j.cancelling,
        },
    );
}

/// The close-while-running dialog (A5's `ui-close-running`).
pub(crate) fn dialog_view(cx: &mut Cx<'_, Msg>) {
    cx.dialog_frame(
        DIALOG,
        &DialogFrame {
            title: "A job is still running",
            description: Some("Cleanup is 40 % done."),
            buttons: &DIALOG_BUTTONS,
            initial: 0,
            cancel: Msg::DialogCancel,
        },
        |cx| {
            cx.add(
                Element::text(
                    DIALOG_TEXT,
                    "Closing now stops it after the current change. The changes it made stay, \
                     and Activity can undo them.",
                )
                .text_style(TextStyle {
                    wrap: true,
                    ..TextStyle::default()
                }),
            );
        },
    );
}

/// Which window a test or a bench looks at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Window {
    Page,
    Popup,
    Toast,
}

/// The windows over the page, as the platform hosts them: the model's popup (the palette or the
/// dialog) and its toast, each a `Ui` of its own at the width the host gives it.
pub(crate) struct Windows {
    pub(crate) popup: Ui<Msg>,
    pub(crate) popup_open: Option<PopupKind>,
    pub(crate) toast: Ui<Msg>,
    pub(crate) toast_open: bool,
    popup_pixels: Vec<u32>,
    toast_pixels: Vec<u32>,
    /// Each window's last frame's damage.
    pub(crate) popup_damage: Vec<RectPx>,
    pub(crate) toast_damage: Vec<RectPx>,
}

impl Windows {
    fn new(motion: Motion) -> Self {
        let mut popup = Ui::new(Size { w: 0.0, h: 0.0 }, 1.0, motion);
        let mut toast = Ui::new(Size { w: 0.0, h: 0.0 }, 1.0, motion);
        popup.set_formats(InvariantFormats::new(SizeUnits::new(UNITS)));
        toast.set_formats(InvariantFormats::new(SizeUnits::new(UNITS)));
        Self {
            popup,
            popup_open: None,
            toast,
            toast_open: false,
            popup_pixels: Vec::new(),
            toast_pixels: Vec::new(),
            popup_damage: Vec::with_capacity(64),
            toast_damage: Vec::with_capacity(16),
        }
    }

    fn tick(&mut self, now: u64, out: &mut Vec<Msg>) {
        if self.popup_open.is_some() {
            self.popup.tick(now, out);
        }
        if self.toast_open {
            self.toast.tick(now, out);
        }
    }

    fn wants_frame(&self) -> bool {
        (self.popup_open.is_some() && self.popup.wants_frame())
            || (self.toast_open && self.toast.wants_frame())
    }

    /// Opens, rebuilds or hides each window to match the model, as the host does after the main
    /// view: built, sized to its content at its width, laid out, focused when it opens (the
    /// popup), painted when frames are, and its update taken while a client listens.
    fn frame(&mut self, model: &Model, text: &mut FakeText, theme: Option<&Theme>, a11y: bool) {
        let want = popup(model).map(|p| p.kind);
        if want != self.popup_open && self.popup_open.is_some() {
            hide(&mut self.popup);
            self.popup.build(|_| {});
            self.popup.clear_damage();
            self.popup_open = None;
        }
        if let Some(kind) = want {
            let opening = self.popup_open.is_none();
            let w = if kind == PopupKind::Palette {
                tokens::palette::WIDTH
            } else {
                tokens::dialog::WIDTH
            };
            self.popup.build(|cx| popup_view(model, cx));
            show(&mut self.popup, text, w, opening, a11y);
            if opening {
                self.popup.set_active(true);
                self.popup.focus_first(true);
            }
            self.popup_open = Some(kind);
            finish(
                &mut self.popup,
                &mut self.popup_pixels,
                &mut self.popup_damage,
                text,
                theme,
                a11y,
            );
        }
        let toast = model.over.toast();
        if !toast && self.toast_open {
            hide(&mut self.toast);
            // The next toast is a new one: its tree, and its hide wake, start again.
            self.toast.build(|_| {});
            self.toast.clear_damage();
            self.toast_open = false;
        }
        if toast {
            let opening = !self.toast_open;
            self.toast.build(|cx| toast_view(model, cx));
            show(&mut self.toast, text, tokens::toast::WIDTH, opening, a11y);
            self.toast_open = true;
            finish(
                &mut self.toast,
                &mut self.toast_pixels,
                &mut self.toast_damage,
                text,
                theme,
                a11y,
            );
        }
    }
}

/// A window's frame after its build: sized to its content at `w` in whole pixels, as the platform
/// sizes the window (scale 1), laid out, and, when it opens, all of it damaged and its whole tree
/// served to a client listening.
fn show(ui: &mut Ui<Msg>, text: &mut FakeText, w: f32, opening: bool, a11y: bool) {
    let size = ui.natural_size(text, w);
    ui.set_viewport(Size {
        w,
        h: size.h.ceil(),
    });
    ui.layout(text);
    if opening {
        ui.repaint_all();
        if a11y {
            drop(ui.a11y_tree());
        }
    }
}

/// A window's damage painted when frames are, kept for the tests, then cleared; and its update
/// taken while a client listens.
fn finish(
    ui: &mut Ui<Msg>,
    pixels: &mut Vec<u32>,
    damage: &mut Vec<RectPx>,
    text: &mut FakeText,
    theme: Option<&Theme>,
    a11y: bool,
) {
    if let Some(theme) = theme {
        let (w, h) = ui.pixel_size();
        pixels.resize(usize::try_from(u64::from(w) * u64::from(h)).unwrap_or(0), 0);
        if let Some(mut canvas) = Canvas::new(pixels, w, h) {
            ui.paint(&mut canvas, text, theme);
        }
    }
    damage.clear();
    damage.extend_from_slice(ui.damage());
    ui.clear_damage();
    if a11y {
        drop(ui.a11y_update());
    }
}

/// A window hidden: its press, hover and focus go.
fn hide(ui: &mut Ui<Msg>) {
    ui.cancel_press();
    let mut none = Vec::new();
    ui.dispatch(Input::PointerLeave, &mut none);
    ui.set_focus(None);
    ui.set_active(false);
}

impl Run {
    /// The `Ui` of window `w`.
    pub(crate) fn ui_of(&self, w: Window) -> &Ui<Msg> {
        match w {
            Window::Page => &self.ui,
            Window::Popup => &self.win.popup,
            Window::Toast => &self.win.toast,
        }
    }

    /// The accessibility update window `w`'s frames left since the last one taken.
    pub(crate) fn a11y_update_of(&mut self, w: Window) -> Option<TreeUpdate> {
        match w {
            Window::Page => self.ui.a11y_update(),
            Window::Popup => self.win.popup.a11y_update(),
            Window::Toast => self.win.toast.a11y_update(),
        }
    }

    /// Window `w`'s whole tree, as a client that starts listening gets it.
    pub(crate) fn a11y_tree_of(&mut self, w: Window) -> TreeUpdate {
        match w {
            Window::Page => self.ui.a11y_tree(),
            Window::Popup => self.win.popup.a11y_tree(),
            Window::Toast => self.win.toast.a11y_tree(),
        }
    }

    /// The first node keyed `key` in window `w`, breadth first from its root.
    pub(crate) fn part_in(&self, w: Window, key: Key) -> Option<dpro_ui::NodeId> {
        let ui = self.ui_of(w);
        let mut queue = std::collections::VecDeque::from([ui.root()]);
        while let Some(id) = queue.pop_front() {
            for c in ui.children(id) {
                if ui.info(c).is_some_and(|n| n.key == key) {
                    return Some(c);
                }
                queue.push_back(c);
            }
        }
        None
    }

    /// The center of node `id` in window `w`.
    pub(crate) fn center_in(&self, w: Window, id: dpro_ui::NodeId) -> Point {
        self.ui_of(w)
            .info(id)
            .map_or(Point::default(), |n| n.rect.center())
    }

    /// Node `id` of window `w` as a client sees it in `tree`, if it's there.
    pub(crate) fn ak_in<'t>(
        &self,
        w: Window,
        tree: &'t TreeUpdate,
        id: dpro_ui::NodeId,
    ) -> Option<&'t Node> {
        let ui = self.ui_of(w);
        tree.nodes
            .iter()
            .find(|(ak, _)| ui.a11y_node(*ak) == Some(id))
            .map(|(_, node)| node)
    }

    /// The main window's last painted pixel at (2, 2): the page's background, in its padding.
    pub(crate) fn page_pixel(&self) -> u32 {
        let (w, _) = self.ui.pixel_size();
        self.pixels
            .get(usize::try_from(2 * w + 2).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or(0)
    }

    /// The palette opened, settled: its field focused, the 50 first entries listed, the first
    /// highlighted.
    pub(crate) fn palette_open(mut self) -> Self {
        self.step(Step::Update(Msg::PaletteOpen));
        self.settle();
        self
    }

    /// The dialog opened, settled.
    pub(crate) fn dialog_open(mut self) -> Self {
        self.step(Step::Update(Msg::DialogOpen));
        self.settle();
        self
    }

    /// A job started and at 40 %, its toast and its card showing, settled.
    pub(crate) fn job_running(mut self) -> Self {
        for s in [
            Step::Update(Msg::JobStart),
            Step::Update(Msg::Progress(40)),
            Step::Update(Msg::JobLine),
        ] {
            self.step(s);
        }
        self.settle();
        self
    }

    /// Steps that warm slice 6c's composites, and end where they started: the palette opened,
    /// typed into (to no results and back), its highlight moved both ways and closed with Esc; a
    /// job started, its progress, line and time, a cancel asked for, its end and its toast
    /// dismissed; the dialog opened, Tab, and closed with Esc.
    pub(crate) fn overlay_session() -> Vec<Step> {
        vec![
            Step::Update(Msg::PaletteOpen),
            popup_char('s'),
            popup_char('e'),
            popup_key(KeyCode::Down),
            popup_key(KeyCode::Down),
            popup_key(KeyCode::Up),
            popup_char('q'),
            popup_char('q'),
            popup_key(KeyCode::Backspace),
            popup_key(KeyCode::Backspace),
            popup_key(KeyCode::Backspace),
            popup_key(KeyCode::Backspace),
            popup_key(KeyCode::Up),
            popup_key(KeyCode::Escape),
            Step::Update(Msg::JobStart),
            Step::Update(Msg::Progress(10)),
            Step::Update(Msg::Progress(20)),
            Step::Update(Msg::JobLine),
            Step::Update(Msg::JobTick),
            Step::Update(Msg::JobCancel),
            Step::Update(Msg::JobEnd(ToastTone::Done)),
            Step::Update(Msg::ToastDismiss),
            Step::Update(Msg::DialogOpen),
            popup_key(KeyCode::Tab),
            popup_key(KeyCode::Escape),
        ]
    }
}

/// A character typed in the popup, as a step.
pub(crate) fn popup_char(c: char) -> Step {
    Step::Popup(Input::Char(c))
}

/// A key pressed in the popup, as a step.
pub(crate) fn popup_key(key: KeyCode) -> Step {
    Step::Popup(Input::Key(KeyEvent {
        key,
        down: true,
        mods: Mods::default(),
    }))
}
