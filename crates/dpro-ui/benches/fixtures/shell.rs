//! A synthetic shell page for TD-0009 §7 U2, U3, U8 and U12, and S-01's scripted session.
//!
//! A header, a rail of 12 pages, a toolbar (the filter, two tabs and 4 category check boxes), and
//! a table of 40 rows × 8 cells over 1,000 data rows; the drawer adds 20 rows, a progress bar and
//! its actions, Hold to apply among them. §7 estimates a real page at about 600 nodes. Pages 1 to
//! 10 show 8 summary cards instead of the toolbar and table, so switching pages replaces the
//! content, and Settings (the last) shows a radio group.
//!
//! Shared by `interact.rs`, `a11y.rs`, `ir.rs` and the tests that replay the session.

#![allow(
    dead_code,
    reason = "each file that includes the fixture uses a part of it"
)]

use std::sync::OnceLock;

use dpro_ui::{
    Align, Axis, Canvas, ClipboardRequest, Cx, Dim, Element, FakeText, Input, Insets, Justify, Key,
    KeyCode, KeyEvent, Look, Mods, Motion, Point, Radius, Size, Style, TextEdit, TextStyle, Theme,
    Tone, Tri, Ui, accesskit::TreeUpdate, tokens,
};

const RAIL: u8 = 12;
/// The last page: a radio group instead of summary cards.
pub(crate) const SETTINGS: u8 = RAIL - 1;
const ROWS: usize = 40;
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
    Row(u32),
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
}

/// The page's view-model.
pub(crate) struct Shell {
    page: u8,
    selected: Option<u32>,
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
    /// Column 0 is the name the filter searches.
    cells: Vec<[String; CELLS]>,
    /// Data rows on the tab and matching the filter, in order; the table shows the first
    /// [`ROWS`].
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
            Msg::Row(r) => self.selected = Some(r),
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
        }
    }

    /// How many times Hold to apply applied.
    pub(crate) fn applies(&self) -> u32 {
        self.applies
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
    }
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

/// A control `height` high, `pad` from its text at each side (doc 05: 30, 36 or 40 px).
fn sized(height: f32, pad: f32) -> Style {
    Style {
        height: Dim::Fixed(height),
        padding: Insets::xy(pad, 0.0),
        ..Style::default()
    }
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
            height: Dim::Fixed(48.0),
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
                .look(Look::PRIMARY),
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
    let selection = if s.selected.is_some() {
        "1 selected"
    } else {
        ""
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
            cx.add_with(
                Element::stack(Key::name("table"))
                    .style(Style {
                        grow: 1.0,
                        padding: Insets::all(4.0),
                        align: Align::Stretch,
                        ..column(0.0)
                    })
                    .look(card(Radius::Card))
                    .name("Programs"),
                |cx| {
                    for &r in s.visible.iter().take(ROWS) {
                        let cells = &s.cells[r as usize];
                        let selected = s.selected == Some(r);
                        cx.add_with(
                            Element::button(Key::index(u64::from(r)), &cells[0], Msg::Row(r))
                                .style(Style {
                                    height: Dim::Fixed(32.0),
                                    padding: Insets::xy(8.0, 0.0),
                                    ..row(12.0)
                                })
                                .look(control(selected.then_some(Tone::Selected)))
                                .selected(selected),
                            |cx| {
                                for (c, text) in (0..).zip(cells) {
                                    let (style, tone) = match c {
                                        0 => (BODY, Tone::Text),
                                        2 | 7 => (MONO, Tone::TextSecondary),
                                        _ => (BODY, Tone::TextSecondary),
                                    };
                                    cx.add(
                                        Element::text(Key::index(c), text)
                                            .text_style(style)
                                            .style(Style {
                                                grow: 1.0,
                                                ..Style::default()
                                            })
                                            .look(ink(tone)),
                                    );
                                }
                            },
                        );
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
                            .look(Look::DANGER),
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
                            height: Dim::Fixed(56.0),
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

/// Settings › Appearance's theme: a radio group, one Tab stop, the arrow keys pick (§4.5).
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
        },
    );
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
}

/// A key pressed with `mods`, as a step.
pub(crate) fn key(key: KeyCode, down: bool, mods: Mods) -> Step {
    Step::Input(Input::Key(KeyEvent { key, down, mods }))
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
}

impl Run {
    /// Frames end at layout and damage (U2).
    pub(crate) fn with(shell: Shell) -> Self {
        Self::new(shell, 1.0, None)
    }

    /// Frames are painted too, at `scale` in `theme` (U3).
    pub(crate) fn painted(shell: Shell, scale: f32, theme: Theme) -> Self {
        Self::new(shell, scale, Some(theme))
    }

    /// Frames end at layout, and an assistive technology listens (U12): the first frame's whole
    /// tree is served, then every frame's changes are taken.
    pub(crate) fn accessible(shell: Shell) -> Self {
        let mut run = Self::new(shell, 1.0, None);
        drop(run.ui.a11y_tree());
        run.take_a11y = true;
        run
    }

    fn new(shell: Shell, scale: f32, theme: Option<Theme>) -> Self {
        let mut run = Self {
            ui: Ui::new(
                START,
                scale,
                Motion {
                    hover_ms: tokens::motion::HOVER_MS,
                },
            ),
            shell,
            text: FakeText,
            msgs: Vec::with_capacity(4),
            theme,
            pixels: Vec::new(),
            now: 0,
            take_a11y: false,
            clipboard: String::with_capacity(64),
        };
        // The window has keyboard focus, as a window the user works in does.
        run.ui.set_active(true);
        run.frame();
        run
    }

    /// One event through the whole UI-thread pipeline (TD-0009 §5.2): dispatch, the clipboard
    /// request it made if any, update, view, reconcile, layout, damage, and raster when painted.
    pub(crate) fn step(&mut self, step: Step) {
        match step {
            Step::Input(input) => {
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
                // A hold held to the end sends its message from here.
                self.ui.tick(self.now, &mut self.msgs);
            }
            Step::Update(msg) => self.msgs.push(msg),
        }
        for m in self.msgs.drain(..) {
            self.shell.update(m);
        }
        // The caret timer, as the platform runs it after every event and on its wake: the caret
        // blinks if it's due, before the frame paints.
        let _ = self.ui.caret_timer(self.now);
        self.frame();
    }

    /// Ticks 60 Hz frames until the UI stops asking for them: hover fades end, and a hold still
    /// pressed runs to its end.
    pub(crate) fn settle(&mut self) {
        while self.ui.wants_frame() {
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
        let shell = &self.shell;
        self.ui.build(|cx| view(shell, cx));
        self.ui.layout(&mut self.text);
        if let Some(theme) = &self.theme {
            let (w, h) = self.ui.pixel_size();
            // Within capacity after warm-up: a resize back and forth doesn't allocate.
            self.pixels.resize(area(w, h), 0);
            if let Some(mut canvas) = Canvas::new(&mut self.pixels, w, h) {
                self.ui.paint(&mut canvas, &mut self.text, theme);
            }
        }
        self.ui.clear_damage();
        if self.take_a11y {
            drop(self.ui.a11y_update());
        }
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

    /// TD-0009 §7's session: a hover sweep over the rail, Tab through the shell, a click into the
    /// filter, typing 10 characters into it and clearing it with Ctrl+A and Backspace, opening the
    /// drawer, pressing Hold to apply for three frames and letting go early, two progress reports,
    /// closing the drawer, toggling a category on and off, clicking the second tab and going back
    /// with Left, and a resize from 1000 to 1400 px and back. It ends where it started, so it can
    /// repeat.
    pub(crate) fn session(&self) -> Vec<Step> {
        let plain = |code, down| key(code, down, Mods::default());
        let body = [Key::name("body")];
        let mut steps = Vec::new();
        for p in 0..u64::from(RAIL) {
            let at = self.center(&[body[0], Key::name("rail"), Key::index(p)]);
            steps.push(Step::Input(Input::PointerMove(at)));
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
