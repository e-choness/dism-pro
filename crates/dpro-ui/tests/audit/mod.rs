//! What the two accessibility audits share (TD-0009 A4.5, slice 7d): painting a window, its
//! nodes as painted, the screens the fixtures reach, and driving them.

#![allow(dead_code, reason = "each audit uses a part of it")]

use dpro_ui::{
    Canvas, CaretStop, Element, FakeText, GlyphSink, Icon, Input, Key, KeyCode, KeyEvent, Look,
    Mods, Motion, NodeId, Rect, RectPx, Role, Size, Style, TextMetrics, TextStyle, TextSystem,
    Theme, Tone, Tri, Ui,
};

use crate::{composites, shell};

/// `FakeText`'s metrics with no ink: the frame as it is under its text and glyphs.
pub(crate) struct Blank;

impl TextSystem for Blank {
    fn measure(&mut self, text: &str, style: &TextStyle, max_width: f32) -> TextMetrics {
        FakeText.measure(text, style, max_width)
    }

    fn draw(&mut self, _: &str, _: &TextStyle, _: Rect, _: f32, _: &mut dyn GlyphSink) {}

    fn icon(&mut self, _: Icon, _: Rect, _: f32, _: &mut dyn GlyphSink) {}

    fn caret_stops(&mut self, text: &str, style: &TextStyle, out: &mut Vec<CaretStop>) {
        FakeText.caret_stops(text, style, out);
    }
}

/// A window's pixels.
pub(crate) struct Frame {
    pub(crate) px: Vec<u32>,
    pub(crate) w: i32,
    pub(crate) h: i32,
}

impl Frame {
    pub(crate) fn at(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return 0;
        }
        self.px[usize::try_from(y * self.w + x).unwrap()]
    }

    pub(crate) fn bounds(&self) -> RectPx {
        RectPx {
            x0: 0,
            y0: 0,
            x1: self.w,
            y1: self.h,
        }
    }
}

/// `region` of `ui` painted in `theme` with `text`, the rest of the window left at 0.
pub(crate) fn paint_in<M>(
    ui: &Ui<M>,
    theme: &Theme,
    text: &mut dyn TextSystem,
    region: RectPx,
) -> Frame {
    let (w, h) = ui.pixel_size();
    let mut px = vec![0; usize::try_from(w * h).unwrap()];
    let mut canvas = Canvas::new(&mut px, w, h).expect("a window has pixels");
    ui.paint_region(&mut canvas, text, theme, region);
    Frame {
        px,
        w: i32::try_from(w).unwrap(),
        h: i32::try_from(h).unwrap(),
    }
}

pub(crate) fn paint<M>(ui: &Ui<M>, theme: &Theme, text: &mut dyn TextSystem) -> Frame {
    let (w, h) = ui.pixel_size();
    let all = RectPx {
        x0: 0,
        y0: 0,
        x1: i32::try_from(w).unwrap(),
        y1: i32::try_from(h).unwrap(),
    };
    paint_in(ui, theme, text, all)
}

/// A node as painted: its box, and the part of it its ancestors let show.
pub(crate) struct Painted {
    pub(crate) id: NodeId,
    pub(crate) role: Role,
    pub(crate) key: Key,
    pub(crate) text: String,
    pub(crate) enabled: bool,
    /// It or an ancestor is disabled.
    pub(crate) in_disabled: bool,
    pub(crate) check: Tri,
    pub(crate) selected: Option<bool>,
    pub(crate) rect: RectPx,
    pub(crate) shows: RectPx,
}

/// Every node of `ui` in paint order (tree order: parents under children), clipped as painting
/// clips them, to every ancestor.
pub(crate) fn painted<M>(ui: &Ui<M>) -> Vec<Painted> {
    fn walk<M>(
        ui: &Ui<M>,
        id: NodeId,
        parent: Option<usize>,
        clip: RectPx,
        out: &mut Vec<Painted>,
    ) {
        for c in ui.children(id) {
            let Some(n) = ui.info(c) else { continue };
            let rect = n.rect.snap(1.0);
            let shows = rect.intersect(clip);
            let in_disabled = !n.enabled || parent.is_some_and(|p| out[p].in_disabled);
            out.push(Painted {
                id: c,
                role: n.role,
                key: n.key,
                text: n.text.to_owned(),
                enabled: n.enabled,
                in_disabled,
                check: n.check,
                selected: n.selected,
                rect,
                shows,
            });
            let me = out.len() - 1;
            walk(ui, c, Some(me), shows, out);
        }
    }
    let (w, h) = ui.pixel_size();
    let all = RectPx {
        x0: 0,
        y0: 0,
        x1: i32::try_from(w).unwrap(),
        y1: i32::try_from(h).unwrap(),
    };
    let mut out = Vec::new();
    walk(ui, ui.root(), None, all, &mut out);
    out
}

/// For each pixel, the last node in paint order that shows there: the one whose pixel it is.
pub(crate) fn owners(nodes: &[Painted], frame: &Frame) -> Vec<Option<u32>> {
    let mut own = vec![None; frame.px.len()];
    for (k, n) in nodes.iter().enumerate() {
        let r = n.shows.intersect(frame.bounds());
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                own[usize::try_from(y * frame.w + x).unwrap()] = Some(u32::try_from(k).unwrap());
            }
        }
    }
    own
}

/// A screen: a window as the host shows it, and how to drive it.
pub(crate) trait Screen {
    type Msg: Clone;
    fn ui(&self) -> &Ui<Self::Msg>;
    fn ui_mut(&mut self) -> &mut Ui<Self::Msg>;
    /// One input through the host's whole pipeline, then frames until nothing animates.
    fn input(&mut self, input: Input);
    /// Nothing hovered, pressed or lit: the pointer gone.
    fn rest(&mut self) {
        self.input(Input::PointerLeave);
    }
    /// How many messages `input` sends from the window, which the host then doesn't get.
    fn sends(&mut self, input: Input) -> usize {
        let mut out = Vec::new();
        self.ui_mut().dispatch(input, &mut out);
        out.len()
    }
}

impl Screen for shell::Run {
    type Msg = shell::Msg;
    fn ui(&self) -> &Ui<shell::Msg> {
        &self.ui
    }
    fn ui_mut(&mut self) -> &mut Ui<shell::Msg> {
        &mut self.ui
    }
    fn input(&mut self, input: Input) {
        self.step(shell::Step::Input(input));
        self.settle();
    }
}

impl Screen for composites::Run {
    type Msg = composites::Msg;
    fn ui(&self) -> &Ui<composites::Msg> {
        &self.ui
    }
    fn ui_mut(&mut self) -> &mut Ui<composites::Msg> {
        &mut self.ui
    }
    fn input(&mut self, input: Input) {
        self.step(composites::Step::Input(input));
        self.settle();
    }
}

/// A popup's or a tooltip's window, as the host sizes and shows it.
pub(crate) struct Window(pub(crate) Ui<shell::Msg>);

impl Screen for Window {
    type Msg = shell::Msg;
    fn ui(&self) -> &Ui<shell::Msg> {
        &self.0
    }
    fn ui_mut(&mut self) -> &mut Ui<shell::Msg> {
        &mut self.0
    }
    fn input(&mut self, input: Input) {
        let mut out = Vec::new();
        self.0.dispatch(input, &mut out);
        self.0.layout(&mut FakeText);
        let mut now = 0;
        while self.0.wants_frame() {
            now += 16;
            self.0.tick(now, &mut out);
        }
    }
    /// In a menu, hovering an item focuses it, which lights it: the light goes too.
    fn rest(&mut self) {
        self.input(Input::PointerLeave);
        self.0.set_focus(None);
    }
}

/// Every standard control, a row of each kind enabled and the same disabled (A4.5's "disabled"
/// state, which the fixtures show only on a menu item and two scan cards).
pub(crate) struct Gallery {
    pub(crate) ui: Ui<u8>,
}

impl Gallery {
    const SIZE: Size = Size { w: 960.0, h: 420.0 };

    pub(crate) fn new() -> Self {
        let mut ui = Ui::new(Self::SIZE, 1.0, Motion::default());
        ui.set_active(true);
        ui.build(gallery);
        ui.layout(&mut FakeText);
        Self { ui }
    }
}

impl Screen for Gallery {
    type Msg = u8;
    fn ui(&self) -> &Ui<u8> {
        &self.ui
    }
    fn ui_mut(&mut self) -> &mut Ui<u8> {
        &mut self.ui
    }
    fn input(&mut self, input: Input) {
        let mut out = Vec::new();
        self.ui.dispatch(input, &mut out);
        self.ui.build(gallery);
        self.ui.layout(&mut FakeText);
    }
}

/// The controls, enabled then disabled: a row of buttons, a row of marks, a row of inputs each.
fn gallery(cx: &mut dpro_ui::Cx<'_, u8>) {
    for (k, enabled) in [(0, true), (1, false)] {
        cx.add_with(
            Element::stack(Key::index(k)).style(Style {
                gap: 4.0,
                ..Style::default()
            }),
            |cx| {
                cx.add_with(Element::stack(Key::name("buttons")).style(row()), |cx| {
                    buttons(cx, enabled);
                });
                cx.add_with(Element::stack(Key::name("marks")).style(row()), |cx| {
                    marked(cx, enabled);
                });
                cx.add_with(Element::stack(Key::name("inputs")).style(row()), |cx| {
                    inputs(cx, enabled);
                });
            },
        );
    }
}

fn row() -> Style {
    Style {
        axis: dpro_ui::Axis::Row,
        gap: 8.0,
        align: dpro_ui::Align::Center,
        padding: dpro_ui::Insets::all(8.0),
        ..Style::default()
    }
}

/// A button's style: a target 32 px high, its text 12 px in.
fn control() -> Style {
    Style {
        min: Size { w: 0.0, h: 32.0 },
        padding: dpro_ui::Insets::xy(12.0, 0.0),
        ..Style::default()
    }
}

/// Each button look, and a dropdown.
fn buttons(cx: &mut dpro_ui::Cx<'_, u8>, enabled: bool) {
    let looks = [
        ("Primary", Look::PRIMARY),
        ("Secondary", Look::SECONDARY),
        ("Danger", Look::DANGER),
        ("Outlined", Look::DANGER_OUTLINE),
        ("Subtle", Look::SUBTLE),
    ];
    for (i, (label, look)) in (0..).zip(looks) {
        cx.add(
            Element::button(Key::index(i), label, 1)
                .style(control())
                .look(look)
                .enabled(enabled),
        );
    }
    cx.add(Element::combo(Key::name("combo"), "Keep", "30 days", false, 2).enabled(enabled));
}

/// Tabs (one picked), check boxes off, on and mixed, and radio buttons (one picked).
fn marked(cx: &mut dpro_ui::Cx<'_, u8>, enabled: bool) {
    cx.add_with(
        Element::tabs(Key::name("tabs"))
            .look(Look::TABS)
            .style(Style {
                padding: dpro_ui::Insets::all(3.0),
                gap: 2.0,
                ..row()
            }),
        |cx| {
            for (t, label) in [(0, "Picked"), (1, "Not picked")] {
                let on = t == 0;
                cx.add(
                    Element::tab(Key::index(t), label, on, 3)
                        .style(control())
                        .look(if on { Look::TAB_SELECTED } else { Look::TAB })
                        .enabled(enabled),
                );
            }
        },
    );
    for (i, state) in (10..).zip([Tri::Off, Tri::On, Tri::Mixed]) {
        cx.add(Element::toggle(Key::index(i), "Box", state, 4).enabled(enabled));
    }
    cx.add_with(Element::radios(Key::name("radios")).name("Radios"), |cx| {
        for (r, on) in [(0, true), (1, false)] {
            cx.add(Element::radio(Key::index(r), "Radio", on, 5).enabled(enabled));
        }
    });
}

/// A field with its placeholder, one with text, a label in each text tone, and a progress bar.
fn inputs(cx: &mut dpro_ui::Cx<'_, u8>, enabled: bool) {
    let field = Style {
        width: dpro_ui::Dim::Fixed(160.0),
        padding: dpro_ui::Insets::xy(8.0, 4.0),
        ..Style::default()
    };
    let boxed = Look {
        fill: Some(Tone::Surface),
        border: Some(Tone::Border),
        radius: dpro_ui::Radius::Control,
        ..Look::PLAIN
    };
    cx.add(
        Element::field(Key::name("empty"), "", |_| 6)
            .name("Filter")
            .placeholder("Filter")
            .style(field)
            .look(boxed)
            .enabled(enabled),
    );
    cx.add(
        Element::field(Key::name("full"), "Contoso", |_| 6)
            .name("Name")
            .style(field)
            .look(boxed)
            .enabled(enabled),
    );
    for (i, tone) in (20..).zip([Tone::Text, Tone::TextSecondary, Tone::Accent]) {
        cx.add(
            Element::text(Key::index(i), "Label")
                .look(Look {
                    ink: tone,
                    ..Look::PLAIN
                })
                .enabled(enabled),
        );
    }
    cx.add(
        Element::progress(Key::name("progress"), 0.5)
            .name("Progress")
            .style(Style {
                width: dpro_ui::Dim::Fixed(120.0),
                height: dpro_ui::Dim::Fixed(6.0),
                ..Style::default()
            }),
    );
}

/// A pixel coordinate at scale 1 as a logical one.
pub(crate) fn px_f(v: i32) -> f32 {
    f32::from(i16::try_from(v).unwrap())
}

/// A stroke token in whole pixels at scale 1.
pub(crate) fn whole(w: f32) -> i32 {
    (1..=8)
        .find(|&k| (f32::from(u8::try_from(k).unwrap()) - w).abs() < 0.01)
        .expect("a stroke token is a few whole pixels")
}

/// A key pressed, with `mods`.
pub(crate) fn key(key: KeyCode, mods: Mods) -> Input {
    Input::Key(KeyEvent {
        key,
        down: true,
        mods,
    })
}

/// Tab, or Shift+Tab.
pub(crate) fn tab(back: bool) -> Input {
    key(
        KeyCode::Tab,
        Mods {
            shift: back,
            ..Mods::default()
        },
    )
}

/// The table page: row 4 current and a category checked.
///
/// The fixture's toolbar is one row that doesn't wrap, wider than [`shell::START`]: the table
/// page is 1100 px wide and the drawer's 1440, so the toolbar fits beside the drawer as the app's
/// will. The table's rows stay wider than the table, so a row's ring is checked where it's cut.
pub(crate) fn table_page() -> shell::Run {
    use shell::{Msg, Run, Shell, Step};
    let mut run = Run::with(Shell::new());
    run.step(wide(1100.0));
    run.step(Step::Update(Msg::Row(4)));
    run.step(Step::Update(Msg::Category(1)));
    run
}

/// The table page with the drawer open, 1440 px wide.
pub(crate) fn drawer_page() -> shell::Run {
    let mut run = shell::Run::with(shell::Shell::new());
    run.step(wide(1440.0));
    run.drawer_open()
}

/// A summary page.
pub(crate) fn summary_page() -> shell::Run {
    let mut run = shell::Run::with(shell::Shell::new());
    run.step(shell::Step::Update(shell::Msg::Page(1)));
    run
}

/// Settings.
pub(crate) fn settings_page() -> shell::Run {
    let mut run = shell::Run::with(shell::Shell::new());
    run.step(shell::Step::Update(shell::Msg::Page(shell::SETTINGS)));
    run
}

fn wide(w: f32) -> shell::Step {
    shell::Step::Resize(Size {
        w,
        h: shell::START.h,
    })
}

/// The composites page, Details closed.
pub(crate) fn composites_page() -> composites::Run {
    composites::Run::with(composites::Model::new())
}

/// The composites page, the error panel's Details open.
pub(crate) fn details_open() -> composites::Run {
    let mut run = composites_page();
    run.step(composites::Step::Update(composites::Msg::Expand));
    run
}

/// A screen's name, and how to make it fresh.
pub(crate) type Make<S> = (&'static str, fn() -> S);

/// The shell's screens, each as made fresh.
pub(crate) const SHELL: [Make<shell::Run>; 4] = [
    ("the table page", table_page),
    ("the drawer open", drawer_page),
    ("a summary page", summary_page),
    ("Settings", settings_page),
];

/// The composites' screens, each as made fresh.
pub(crate) const COMPOSITES: [Make<composites::Run>; 2] = [
    ("the composites page", composites_page),
    ("Details open", details_open),
];

/// Each finding, named by the screen it's on.
pub(crate) fn on(screen: &str, findings: Vec<String>) -> impl Iterator<Item = String> + '_ {
    findings.into_iter().map(move |f| format!("{screen}: {f}"))
}

/// Fails with every finding, if there are any.
pub(crate) fn report(what: &str, findings: &[String]) {
    assert!(findings.is_empty(), "{what}:\n{}", findings.join("\n"));
}
