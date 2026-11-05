//! A synthetic shell page for TD-0009 §7 U2 and U8, and S-01's scripted session.
//!
//! About 600 nodes, the size §7 gives for a real page: a header, a rail of 12 pages, a toolbar with
//! 4 category toggles, a table of 40 rows × 8 cells over 1,000 data rows, and a drawer of 25 rows.
//! Shared by `interact.rs`, `ir.rs` and `tests/alloc_per_frame.rs`.

use dpro_ui::{
    Align, Axis, Cx, Dim, Element, FakeText, Input, Insets, Justify, Key, KeyCode, KeyEvent, Mods,
    Motion, Point, Size, Style, TextStyle, Tri, Ui,
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
            cx.add(Element::text(Key::name("target"), "This PC (online)"));
            cx.add(Element::button(
                Key::name("drawer"),
                if s.drawer {
                    "Hide changes"
                } else {
                    "Review changes"
                },
                Msg::Drawer,
            ));
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
        Element::stack(Key::name("rail")).style(Style {
            width: Dim::Fixed(200.0),
            padding: Insets::all(8.0),
            align: Align::Stretch,
            ..column(4.0)
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
                let shown = if s.filter.is_empty() {
                    "Filter"
                } else {
                    &s.filter
                };
                cx.add(Element::text(Key::name("filter"), shown).style(Style {
                    width: Dim::Fixed(240.0),
                    ..Style::default()
                }));
                let selection = if s.selected.is_some() {
                    "1 selected"
                } else {
                    ""
                };
                cx.add(Element::text(Key::name("selection"), selection));
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
                Element::stack(Key::name("table")).style(Style {
                    grow: 1.0,
                    align: Align::Stretch,
                    ..column(0.0)
                }),
                |cx| {
                    for &r in s.visible.iter().take(ROWS) {
                        let cells = &s.cells[r as usize];
                        cx.add_with(
                            Element::button(Key::index(u64::from(r)), &cells[0], Msg::Row(r))
                                .style(Style {
                                    height: Dim::Fixed(32.0),
                                    padding: Insets::xy(8.0, 0.0),
                                    ..row(12.0)
                                }),
                            |cx| {
                                for (c, text) in (0..).zip(cells) {
                                    let style = if c == 2 || c == 7 { MONO } else { BODY };
                                    cx.add(
                                        Element::text(Key::index(c), text).text_style(style).style(
                                            Style {
                                                grow: 1.0,
                                                ..Style::default()
                                            },
                                        ),
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
        Element::stack(Key::name("drawer")).style(Style {
            width: Dim::Fixed(320.0),
            padding: Insets::all(16.0),
            align: Align::Stretch,
            ..column(6.0)
        }),
        |cx| {
            for i in 0..DRAWER_ROWS {
                let cells = &s.cells[usize::try_from(i).unwrap_or(0)];
                cx.add_with(Element::stack(Key::index(i)).style(row(8.0)), |cx| {
                    cx.add(Element::text(Key::name("what"), &cells[0]));
                    cx.add(Element::text(Key::name("size"), &cells[3]));
                    cx.add(Element::text(Key::name("undo"), "Undoable"));
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
}

/// A shell, its Ui and text system, laid out at [`START`] when created.
pub(crate) struct Run {
    pub(crate) ui: Ui<Msg>,
    pub(crate) shell: Shell,
    text: FakeText,
    msgs: Vec<Msg>,
}

impl Run {
    pub(crate) fn with(shell: Shell) -> Self {
        let mut run = Self {
            ui: Ui::new(START, 1.0, Motion { hover_ms: 120 }),
            shell,
            text: FakeText,
            msgs: Vec::with_capacity(4),
        };
        run.frame();
        run
    }

    /// One event through the whole UI-thread pipeline (TD-0009 §5.2), without raster: dispatch,
    /// update, view, reconcile, layout, damage.
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
        }
        self.frame();
    }

    fn frame(&mut self) {
        let shell = &self.shell;
        self.ui.build(|cx| view(shell, cx));
        self.ui.layout(&mut self.text);
        self.ui.clear_damage();
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
