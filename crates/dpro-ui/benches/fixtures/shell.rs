//! A synthetic shell page for TD-0009 §7 U2, U3 and U8, and S-01's scripted session.
//!
//! 387 live nodes, or 488 with the drawer open: a header, a rail of 12 pages, a toolbar with 4
//! category toggles, a table of 40 rows × 8 cells over 1,000 data rows, and a drawer of 25 rows. §7
//! estimates a real page at about 600, so per-event costs there are about half again as high.
//! Shared by `interact.rs`, `ir.rs`, `tests/alloc_per_frame.rs` and `tests/paint_damage.rs`.

#![allow(
    dead_code,
    reason = "each file that includes the fixture uses a part of it"
)]

use dpro_ui::{
    Align, Axis, Canvas, Cx, Dim, Element, FakeText, Input, Insets, Justify, Key, KeyCode,
    KeyEvent, Look, Mods, Motion, Point, Radius, Size, Style, TextStyle, Theme, Tone, Tri, Ui,
};

const RAIL: u8 = 12;
const ROWS: usize = 40;
const CELLS: usize = 8;
const DRAWER_ROWS: u64 = 25;
const CATEGORIES: u8 = 4;
const DATA: usize = 1_000;
/// The text typed into the filter: 10 keystrokes.
const TYPED: &str = "contoso up";

pub(crate) const START: Size = Size {
    w: 1000.0,
    h: 700.0,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Msg {
    Page(u8),
    Category(u8),
    Drawer,
    Row(u32),
}

/// The page's view-model.
pub(crate) struct Shell {
    page: u8,
    selected: Option<u32>,
    categories: [bool; CATEGORIES as usize],
    drawer: bool,
    filter: String,
    /// Column 0 is the name the filter searches.
    cells: Vec<[String; CELLS]>,
    /// Data rows matching the filter, in order; the table shows the first [`ROWS`].
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
            Msg::Drawer => self.drawer = !self.drawer,
            Msg::Row(r) => self.selected = Some(r),
        }
    }

    fn refilter(&mut self) {
        self.visible.clear();
        let filter = self.filter.as_str();
        self.visible.extend(
            (0u32..)
                .zip(&self.cells)
                .filter(|(_, c)| contains_folded(&c[0], filter))
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
                .look(Look {
                    ink: Tone::OnAccent,
                    ..control(Some(Tone::Accent))
                }),
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
            content(s, cx);
            if s.drawer {
                drawer(s, cx);
            }
        },
    );
}

fn rail(s: &Shell, cx: &mut Cx<'_, Msg>) {
    const PAGES: [&str; RAIL as usize] = [
        "Overview", "Clean", "Repair", "Packages", "Features", "Drivers", "Updates", "Apps",
        "Startup", "Services", "Images", "Settings",
    ];
    cx.add_with(
        Element::stack(Key::name("rail"))
            .style(Style {
                width: Dim::Fixed(200.0),
                padding: Insets::all(8.0),
                align: Align::Stretch,
                ..column(4.0)
            })
            .look(Look {
                fill: Some(Tone::Subtle),
                ..Look::PLAIN
            }),
        |cx| {
            for (p, label) in (0..RAIL).zip(PAGES) {
                cx.add(
                    Element::button(Key::index(u64::from(p)), label, Msg::Page(p))
                        .style(Style {
                            height: Dim::Fixed(36.0),
                            padding: Insets::xy(12.0, 0.0),
                            ..Style::default()
                        })
                        .look(control((p == s.page).then_some(Tone::Selected)))
                        .enabled(p != s.page),
                );
            }
        },
    );
}

fn content(s: &Shell, cx: &mut Cx<'_, Msg>) {
    const NAMES: [&str; CATEGORIES as usize] = ["Apps", "Games", "Tools", "System"];
    cx.add_with(
        Element::stack(Key::name("content")).style(Style {
            grow: 1.0,
            padding: Insets::all(16.0),
            align: Align::Stretch,
            ..column(8.0)
        }),
        |cx| {
            cx.add_with(Element::stack(Key::name("toolbar")).style(row(8.0)), |cx| {
                let (shown, tone) = if s.filter.is_empty() {
                    ("Filter", Tone::TextSecondary)
                } else {
                    (s.filter.as_str(), Tone::Text)
                };
                cx.add(
                    Element::text(Key::name("filter"), shown)
                        .style(Style {
                            width: Dim::Fixed(240.0),
                            padding: Insets::xy(8.0, 4.0),
                            ..Style::default()
                        })
                        .look(Look {
                            ink: tone,
                            ..card(Radius::Control)
                        }),
                );
                let selection = if s.selected.is_some() {
                    "1 selected"
                } else {
                    ""
                };
                cx.add(
                    Element::text(Key::name("selection"), selection).look(ink(Tone::TextSecondary)),
                );
                for (c, name) in (0..CATEGORIES).zip(NAMES) {
                    let on = if s.categories[usize::from(c)] {
                        Tri::On
                    } else {
                        Tri::Off
                    };
                    cx.add(Element::toggle(
                        Key::index(u64::from(c)),
                        name,
                        on,
                        Msg::Category(c),
                    ));
                }
            });
            cx.add_with(
                Element::stack(Key::name("table"))
                    .style(Style {
                        grow: 1.0,
                        padding: Insets::all(4.0),
                        align: Align::Stretch,
                        ..column(0.0)
                    })
                    .look(card(Radius::Card)),
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
                                .look(control(selected.then_some(Tone::Selected))),
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

fn drawer(s: &Shell, cx: &mut Cx<'_, Msg>) {
    cx.add_with(
        Element::stack(Key::name("drawer"))
            .style(Style {
                width: Dim::Fixed(320.0),
                padding: Insets::all(16.0),
                align: Align::Stretch,
                ..column(6.0)
            })
            .look(card(Radius::Drawer)),
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
        },
    );
}

/// One step of the scripted session.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Step {
    Input(Input),
    Type(char),
    ClearFilter,
    Resize(Size),
    /// The animation clock moves on by this many milliseconds.
    Tick(u64),
}

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
    /// The animation clock, in milliseconds.
    now: u64,
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

    fn new(shell: Shell, scale: f32, theme: Option<Theme>) -> Self {
        let mut run = Self {
            ui: Ui::new(START, scale, Motion { hover_ms: 120 }),
            shell,
            text: FakeText,
            msgs: Vec::with_capacity(4),
            theme,
            pixels: Vec::new(),
            now: 0,
        };
        run.frame();
        run
    }

    /// One event through the whole UI-thread pipeline (TD-0009 §5.2): dispatch, update, view,
    /// reconcile, layout, damage, and raster when painted.
    pub(crate) fn step(&mut self, step: Step) {
        match step {
            Step::Input(input) => {
                self.ui.dispatch(input, &mut self.msgs);
                for m in self.msgs.drain(..) {
                    self.shell.update(m);
                }
            }
            Step::Type(c) => {
                self.shell.filter.push(c);
                self.shell.refilter();
            }
            Step::ClearFilter => {
                self.shell.filter.clear();
                self.shell.refilter();
            }
            Step::Resize(size) => self.ui.set_viewport(size),
            Step::Tick(ms) => {
                self.now += ms;
                self.ui.tick(self.now);
            }
        }
        self.frame();
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
    fn center(&self, path: &[Key]) -> Point {
        self.ui
            .find(path)
            .and_then(|id| self.ui.info(id))
            .map_or(Point::default(), |n| n.rect.center())
    }

    /// TD-0009 §7's session: a hover sweep over the rail, Tab through the shell, typing 10
    /// characters into the filter, opening and closing the drawer, toggling a category on and off,
    /// and a resize from 1000 to 1400 px and back. It ends where it started, so it can repeat.
    pub(crate) fn session(&self) -> Vec<Step> {
        let click = |p: Point| {
            [
                Step::Input(Input::PointerDown(p)),
                Step::Input(Input::PointerUp(p)),
            ]
        };
        let key = |code, down| {
            Step::Input(Input::Key(KeyEvent {
                key: code,
                down,
                mods: Mods::default(),
            }))
        };
        let body = [Key::name("body")];
        let mut steps = Vec::new();
        for p in 0..u64::from(RAIL) {
            let at = self.center(&[body[0], Key::name("rail"), Key::index(p)]);
            steps.push(Step::Input(Input::PointerMove(at)));
        }
        steps.push(Step::Input(Input::PointerLeave));
        for _ in 0..20 {
            steps.push(key(KeyCode::Tab, true));
            steps.push(key(KeyCode::Tab, false));
        }
        steps.extend(TYPED.chars().map(Step::Type));
        steps.push(Step::ClearFilter);
        let drawer = self.center(&[Key::name("header"), Key::name("drawer")]);
        steps.extend(click(drawer));
        steps.extend(click(drawer));
        let category = self.center(&[
            body[0],
            Key::name("content"),
            Key::name("toolbar"),
            Key::index(0),
        ]);
        steps.extend(click(category));
        steps.extend(click(category));
        for w in [1080.0, 1160.0, 1240.0, 1320.0, 1400.0, 1000.0] {
            steps.push(Step::Resize(Size { w, h: START.h }));
        }
        steps
    }
}

/// Pixels in a `w × h` canvas.
fn area(w: u32, h: u32) -> usize {
    usize::try_from(u64::from(w) * u64::from(h)).unwrap_or(usize::MAX)
}
