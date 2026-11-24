//! A synthetic page with slice 6d's drawer (TD-0009 A3.5 (6), A3.7, A3.9), for its benches, tests
//! and G1's probe.
//!
//! The Clean page in a scroll viewport that is the page's Main landmark: a level-1 heading, a
//! line, "Review changes" (which opens the drawer), "Scan again" and 8 category lines. Beside it,
//! while it's
//! open, the Preview drawer: a summary, a badge, 20 changes as check boxes in a section, the
//! Permanent callout and the command line in a code block, and Cancel with a hold to delete. As a
//! sheet ([`drawer_is_sheet`], below 940 px) the page leaves the window while it's open.
//!
//! [`Run`] hosts it as the platform does: a `Ui`, a `FakeText`, the clock, and, when asked,
//! painting at a scale and an assistive technology listening. It counts the frames whose layout
//! had work, so the tests can pin "lay out once, slide the paint" (U16).

#![allow(
    dead_code,
    reason = "each file that includes the fixture uses a part of it"
)]

use dpro_ui::{
    Align, Axes, Axis, Canvas, Cx, Element, FakeText, Input, Insets, Key, KeyCode, KeyEvent,
    Landmark, Look, Mods, Motion, Point, RectPx, Size, Style, TextStyle, Theme, Tri, Ui,
    accesskit::{Node, NodeId as AkId, TreeUpdate},
    composite::{
        BadgeSpec, BadgeTone, Callout, DrawerFooter, DrawerFrame, DrawerPrimary, drawer_is_sheet,
    },
    tokens::{self, SPACE, font},
};

/// The window, as the shell fixture's: the drawer is a pane beside the page.
pub(crate) const START: Size = Size {
    w: 1000.0,
    h: 700.0,
};
/// Narrow enough that the page beside a pane would be under 480 px: the drawer is a sheet.
pub(crate) const NARROW: Size = Size { w: 800.0, h: 700.0 };
/// One frame at 60 Hz, rounded down to whole milliseconds: the platform's frame timer.
pub(crate) const FRAME_MS: u64 = 16;

/// The row the page and the drawer share.
pub(crate) const ROW: Key = Key::name("row");
/// The page: a scroll viewport, the Main landmark, named by its title.
pub(crate) const PAGE: Key = Key::name("page");
pub(crate) const HEADING: Key = Key::name("heading");
pub(crate) const LEAD: Key = Key::name("lead");
/// "Review changes": what opens the drawer.
pub(crate) const OPEN: Key = Key::name("open");
/// "Scan again", after it: another control on the page.
pub(crate) const SCAN: Key = Key::name("scan");
pub(crate) const LINES: Key = Key::name("lines");
/// The drawer, at the row's end.
pub(crate) const DRAWER: Key = Key::name("drawer");
/// The body's parts.
pub(crate) const CHANGES: Key = Key::name("changes");
pub(crate) const CALLOUT: Key = Key::name("callout");
pub(crate) const COMMAND: Key = Key::name("command");
pub(crate) const CODE: Key = Key::name("code");
pub(crate) const NOTHING: Key = Key::name("nothing");

/// The page's title: its level-1 heading and its landmark's name.
pub(crate) const TITLE: &str = "Clean";
/// The drawer's title: its name and its level-2 heading.
pub(crate) const DRAWER_TITLE: &str = "Preview";
/// The opener's label.
pub(crate) const REVIEW: &str = "Review changes";
/// The drawer's landmark kind (`a11y-landmark-region`, resolved).
pub(crate) const REGION: &str = "region";
pub(crate) const CLOSE: &str = "Close";
pub(crate) const CANCEL: &str = "Cancel";
pub(crate) const HOLD: &str = "Hold to delete";
pub(crate) const COPY: &str = "Copy command";
pub(crate) const COMMAND_LINE: &str = "dismpro clean --category temp --category wu-cache --category \
    thumbnails --older-than 7d --yes";

/// The changes, one check box each: more than the body shows at 700 px, so it scrolls.
pub(crate) const ITEMS: [&str; 20] = [
    "Temporary files",
    "Windows Update cache",
    "Delivery Optimization files",
    "Thumbnails",
    "Error reports",
    "Setup logs",
    "Recycle Bin",
    "Browser caches",
    "Font cache",
    "DirectX shader cache",
    "Old Windows installation",
    "Upgrade logs",
    "Memory dumps",
    "Defender scan history",
    "Retail demo content",
    "Language resources",
    "Offline web pages",
    "Downloaded program files",
    "System queued reports",
    "Feedback diagnostics",
];

/// The page's category lines, under the opener.
const CATEGORIES: [(&str, &str); 8] = [
    ("Temporary files", "1.2 GB"),
    ("Windows Update cache", "3.4 GB"),
    ("Delivery Optimization", "860 MB"),
    ("Thumbnails", "120 MB"),
    ("Error reports", "45 MB"),
    ("Logs", "310 MB"),
    ("Recycle Bin", "2.1 GB"),
    ("Browser caches", "640 MB"),
];

const BADGES: [BadgeSpec<'static>; 2] = [
    BadgeSpec {
        text: "Permanent",
        tone: BadgeTone::Permanent,
        description: Some("Deleted files can't be brought back."),
    },
    BadgeSpec {
        text: "No restart needed",
        tone: BadgeTone::Neutral,
        description: None,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Msg {
    Open,
    Close,
    Cancel,
    /// The hold came to its end.
    Delete,
    /// UI Automation's Invoke on the hold: the typed confirmation.
    Confirm,
    Copy,
    Check(u8),
    Scan,
}

/// The view-model.
#[derive(Clone, Debug)]
pub(crate) struct Model {
    pub(crate) open: bool,
    pub(crate) checked: [bool; ITEMS.len()],
    /// The body holds nothing to focus: one line of text.
    pub(crate) empty: bool,
    pub(crate) copies: u32,
    pub(crate) last: Option<Msg>,
}

impl Model {
    pub(crate) fn new() -> Self {
        Self {
            open: false,
            checked: [true; ITEMS.len()],
            empty: false,
            copies: 0,
            last: None,
        }
    }

    pub(crate) fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Open => self.open = true,
            Msg::Close | Msg::Cancel => self.open = false,
            Msg::Copy => self.copies += 1,
            Msg::Delete | Msg::Confirm | Msg::Scan => {}
            Msg::Check(i) => {
                if let Some(c) = self.checked.get_mut(usize::from(i)) {
                    *c = !*c;
                }
            }
        }
        self.last = Some(msg);
    }
}

/// The page beside the drawer, or the drawer alone as a sheet.
pub(crate) fn view(m: &Model, cx: &mut Cx<'_, Msg>) {
    let sheet = drawer_is_sheet(cx.viewport().w);
    cx.add_with(
        Element::stack(ROW).style(Style {
            axis: Axis::Row,
            grow: 1.0,
            align: Align::Stretch,
            ..Style::default()
        }),
        |cx| {
            if !(m.open && sheet) {
                page(cx);
            }
            if m.open {
                let frame = DrawerFrame {
                    title: DRAWER_TITLE,
                    summary: Some(("2.3 GB", "to free · 20 items")),
                    badges: &BADGES,
                    close: || Msg::Close,
                    close_label: CLOSE,
                    footer: DrawerFooter {
                        cancel: (CANCEL, Msg::Cancel),
                        primary: DrawerPrimary::Hold(HOLD, || Msg::Delete, || Msg::Confirm),
                        applying: false,
                    },
                    sheet,
                    region: REGION,
                };
                cx.drawer(DRAWER, &frame, |cx| body(m, cx));
            }
        },
    );
}

fn page(cx: &mut Cx<'_, Msg>) {
    cx.add_with(
        Element::scroll(PAGE, Axes::Y)
            .style(Style {
                grow: 1.0,
                gap: SPACE[2],
                padding: Insets::all(SPACE[4]),
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
                        mono: false,
                        wrap: false,
                    })
                    .heading(1),
            );
            cx.add(Element::text(LEAD, "8 categories, 8.6 GB to free"));
            cx.add(
                Element::button(OPEN, REVIEW, Msg::Open)
                    .style(Style {
                        min: Size { w: 0.0, h: 32.0 },
                        padding: Insets::xy(SPACE[3], 0.0),
                        ..Style::default()
                    })
                    .look(Look::PRIMARY),
            );
            cx.add(
                Element::button(SCAN, "Scan again", Msg::Scan)
                    .style(Style {
                        min: Size { w: 0.0, h: 32.0 },
                        padding: Insets::xy(SPACE[3], 0.0),
                        ..Style::default()
                    })
                    .look(Look::SECONDARY),
            );
            cx.add_with(
                Element::stack(LINES).style(Style {
                    gap: SPACE[1],
                    ..Style::default()
                }),
                |cx| {
                    for (i, (what, size)) in (0..).zip(CATEGORIES) {
                        cx.add_with(
                            Element::stack(Key::index(i)).style(Style {
                                axis: Axis::Row,
                                gap: SPACE[2],
                                ..Style::default()
                            }),
                            |cx| {
                                cx.add(Element::text(Key::name("what"), what).style(Style {
                                    grow: 1.0,
                                    ..Style::default()
                                }));
                                cx.add(Element::text(Key::name("size"), size));
                            },
                        );
                    }
                },
            );
        },
    );
}

fn body(m: &Model, cx: &mut Cx<'_, Msg>) {
    if m.empty {
        cx.add(Element::text(NOTHING, "Nothing here can be changed."));
        return;
    }
    cx.drawer_section(CHANGES, "Changes", |cx| {
        for (i, name) in (0u8..).zip(ITEMS) {
            let on = m.checked[usize::from(i)];
            cx.add(
                Element::toggle(
                    Key::index(u64::from(i)),
                    name,
                    if on { Tri::On } else { Tri::Off },
                    Msg::Check(i),
                )
                .style(Style {
                    min: Size { w: 0.0, h: 32.0 },
                    ..Style::default()
                }),
            );
        }
    });
    cx.callout(
        CALLOUT,
        &Callout {
            title: "Permanent",
            text: "Deleted files can't be brought back. Nothing here goes to the Recycle Bin.",
        },
    );
    cx.drawer_section(COMMAND, "Command", |cx| {
        cx.code_block(CODE, COMMAND_LINE, (COPY, Msg::Copy));
    });
}

/// A step of a script.
#[derive(Clone, Copy, Debug)]
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
    /// Frames run since the run started: each step's build, layout and damage counts one.
    pub(crate) frames: u64,
    /// Of those, the frames whose layout had work ([`Ui::needs_layout`]).
    pub(crate) layouts: u64,
}

impl Run {
    /// Frames end at layout and damage, with the motion tokens' durations.
    pub(crate) fn with(model: Model) -> Self {
        Self::new(model, None, 1.0, Motion::STANDARD)
    }

    /// Frames end at layout and damage, with animations off.
    pub(crate) fn reduced(model: Model) -> Self {
        Self::new(model, None, 1.0, Motion::REDUCED)
    }

    /// Frames are painted too, at scale 1 in `theme`.
    pub(crate) fn painted(model: Model, theme: Theme) -> Self {
        Self::new(model, Some(theme), 1.0, Motion::STANDARD)
    }

    /// Frames are painted too, at `scale` in the light theme.
    pub(crate) fn painted_at(model: Model, scale: f32) -> Self {
        Self::new(model, Some(Theme::LIGHT), scale, Motion::STANDARD)
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
            ui: Ui::new(START, scale, motion),
            model,
            text: FakeText,
            msgs: Vec::with_capacity(4),
            theme,
            pixels: Vec::new(),
            now: 0,
            take_a11y: false,
            damage: Vec::with_capacity(64),
            frames: 0,
            layouts: 0,
        };
        run.ui.set_active(true);
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
        if self.ui.needs_layout() {
            self.layouts += 1;
        }
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

    /// The last painted frame's pixel at `(x, y)` in physical pixels, as `0xAARRGGBB`.
    pub(crate) fn pixel(&self, x: u32, y: u32) -> u32 {
        let (w, _) = self.ui.pixel_size();
        self.pixels
            .get(usize::try_from(u64::from(y) * u64::from(w) + u64::from(x)).unwrap_or(0))
            .copied()
            .unwrap_or(0)
    }

    /// Repaints the whole window into the pixels, whatever the damage, as after a theme switch.
    pub(crate) fn repaint(&mut self) {
        self.ui.repaint_all();
        self.frame();
    }

    /// The node at `path` from the root.
    pub(crate) fn find(&self, path: &[Key]) -> Option<dpro_ui::NodeId> {
        self.ui.find(path)
    }

    /// The drawer, if it's open.
    pub(crate) fn drawer(&self) -> Option<dpro_ui::NodeId> {
        self.ui.find(&[ROW, DRAWER])
    }

    /// "Review changes", while the page shows.
    pub(crate) fn opener(&self) -> Option<dpro_ui::NodeId> {
        self.ui.find(&[ROW, PAGE, OPEN])
    }

    /// The first node keyed `key` under the drawer, breadth first: one of its parts.
    pub(crate) fn part(&self, key: Key) -> Option<dpro_ui::NodeId> {
        self.part_of(self.drawer()?, key)
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

    /// Steps that warm the drawer: open, slide, Tab through it, check a change, scroll the body,
    /// close; open as a sheet and close; then the window back, settled, closed.
    pub(crate) fn session() -> Vec<Step> {
        let mut steps = vec![Step::Update(Msg::Open)];
        steps.extend(ticks());
        for _ in 0..6 {
            steps.push(key(KeyCode::Tab, Mods::default()));
        }
        steps.extend([
            key(KeyCode::Space, Mods::default()),
            key_up(KeyCode::Space),
            key(KeyCode::PageDown, Mods::default()),
            key(KeyCode::Escape, Mods::default()),
            Step::Update(Msg::Close),
            Step::Resize(NARROW),
            Step::Update(Msg::Open),
        ]);
        steps.extend(ticks());
        steps.extend([
            Step::Update(Msg::Close),
            Step::Resize(START),
            Step::Update(Msg::Open),
        ]);
        steps.extend(ticks());
        steps.extend([
            Step::Update(Msg::Close),
            Step::Input(Input::PointerMove(Point { x: -1.0, y: -1.0 })),
            Step::Tick(u64::from(tokens::motion::HOVER_MS) + FRAME_MS),
        ]);
        steps
    }

    /// `run` after the session, settled, the drawer closed.
    pub(crate) fn warmed(mut self) -> Self {
        for s in Self::session() {
            self.step(s);
        }
        self.settle();
        self
    }
}

/// A slide's frames and one more: what a slide can take.
fn ticks() -> impl Iterator<Item = Step> {
    let n = u64::from(tokens::motion::DRAWER_MS).div_ceil(FRAME_MS) + 1;
    (0..n).map(|_| Step::Tick(FRAME_MS))
}
