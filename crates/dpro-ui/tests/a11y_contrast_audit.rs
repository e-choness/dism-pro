//! The high-contrast audit (TD-0009 A4.5, A4.9; slice 7d). Every screen the fixtures reach,
//! painted in high contrast, keeps Microsoft's contrast-theme rules and WCAG 2.2's 1.4.1 and
//! 1.4.11:
//! 1. every pixel is one of the 8 system colors, or an antialiased edge between them;
//! 2. every text run and glyph is a documented pair: `WindowText`, `GrayText` or `HotLight` on
//!    `Window`, `HighlightText` on `Highlight`, `ButtonText` on `ButtonFace`, and the one listed
//!    exception, `Window` on a `GrayText` fill (a disabled box's mark);
//! 3. `GrayText` shows only inside disabled UI, and a disabled control's own text is `GrayText`,
//!    so disabled reads apart from secondary text;
//! 4. `HotLight` shows nowhere until a link role exists;
//! 5. the focus ring is `Highlight`, `stroke::FOCUS` wide and whole, on every Tab stop;
//! 6. popups have a `WindowText` border `stroke::CONTRAST` wide (`stroke.contrast`);
//! 7. hover and press show an outline and nothing else, a selection is a `Highlight` fill, and a
//!    checked box or a picked radio button shows its mark: nothing is told by color alone.
//!
//! The screens: the shell's table page (a row current, a category checked), its drawer with Hold
//! to apply half held, a summary page and Settings; the shell's row menu, column menu, Start page
//! list (also scrolled in a short window, its bar shown) and a tooltip; the composites page,
//! Details open, and its palette, running job's toast and dialog windows (slice 6c); the Preview
//! drawer as a pane and as a sheet (slice 6d); and every standard control, enabled and disabled.
//! A5's and A6's fixtures join as they land.
//!
//! How text is found: a screen is painted twice, with `FakeText` and with its metrics but no ink.
//! Where the two frames differ is text or a glyph, and the second frame says what is under it.
//! Rules 2 to 7 paint with the RGB cube's eight corners as the system colors, so no blend of two
//! (an antialiased edge) is a third; rule 1 paints with eight near-black colors instead, so any
//! blend of them stays near black and any other color stands out.

#![allow(missing_docs)]
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a window or a node that isn't there fails the audit"
)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;
#[path = "../benches/fixtures/drawer.rs"]
mod drawer_page;
#[path = "../benches/fixtures/shell.rs"]
mod shell;

mod audit;

use audit::{
    Blank, Gallery, Screen, Window, owners, paint, paint_in, painted, px_f, report, whole,
};
use dpro_ui::{
    Color, Element, FakeText, Input, Key, KeyCode, Mods, Motion, NodeId, Point, RectPx, Role, Size,
    SysColors, Theme, Tri, Ui, tokens,
};

/// The system colors rules 2 to 7 paint with: the RGB cube's corners, none between two others.
const ID: SysColors = SysColors {
    window: Color::hex(0x00_0000),
    window_text: Color::hex(0xFF_FFFF),
    highlight: Color::hex(0x00_FFFF),
    highlight_text: Color::hex(0xFF_0000),
    hot_light: Color::hex(0xFF_FF00),
    gray_text: Color::hex(0x00_FF00),
    btn_face: Color::hex(0x00_00FF),
    btn_text: Color::hex(0xFF_00FF),
};

/// The system colors rule 1 paints with: blends of them stay in `(0, 0, 1..=8)`.
const NEAR: SysColors = SysColors {
    window: Color::hex(0x00_0001),
    window_text: Color::hex(0x00_0002),
    highlight: Color::hex(0x00_0003),
    highlight_text: Color::hex(0x00_0004),
    hot_light: Color::hex(0x00_0005),
    gray_text: Color::hex(0x00_0006),
    btn_face: Color::hex(0x00_0007),
    btn_text: Color::hex(0x00_0008),
};

fn argb(c: Color) -> u32 {
    0xFF00_0000 | u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
}

fn name(p: u32) -> &'static str {
    let s = ID;
    [
        (s.window, "Window"),
        (s.window_text, "WindowText"),
        (s.highlight, "Highlight"),
        (s.highlight_text, "HighlightText"),
        (s.hot_light, "HotLight"),
        (s.gray_text, "GrayText"),
        (s.btn_face, "ButtonFace"),
        (s.btn_text, "ButtonText"),
    ]
    .into_iter()
    .find(|&(c, _)| argb(c) == p)
    .map_or("a blend", |(_, n)| n)
}

/// What a screen breaks of rules 1 to 4, described: its every pixel, text run and `GrayText`.
fn colors_and_pairs<M>(ui: &Ui<M>) -> Vec<String> {
    let mut bad = Vec::new();
    // Rule 1: blends of the near palette stay near black.
    let near = paint(ui, &Theme::high_contrast(NEAR), &mut FakeText);
    let strays = near
        .px
        .iter()
        .filter(|&&p| (p >> 8) & 0xFFFF != 0 || !(1..=8).contains(&(p & 0xFF)))
        .count();
    if strays > 0 {
        bad.push(format!("rule 1: {strays} pixels aren't system colors"));
    }
    let theme = Theme::high_contrast(ID);
    let full = paint(ui, &theme, &mut FakeText);
    let under = paint(ui, &theme, &mut Blank);
    let nodes = painted(ui);
    let own = owners(&nodes, &full);
    let s = ID;
    let pairs = [
        (s.window_text, s.window),
        (s.gray_text, s.window),
        (s.hot_light, s.window),
        (s.highlight_text, s.highlight),
        (s.btn_text, s.btn_face),
        // A disabled box's mark: GrayText's partner on its fill.
        (s.window, s.gray_text),
    ]
    .map(|(ink, bg)| (argb(ink), argb(bg)));
    let (gray, hot) = (argb(s.gray_text), argb(s.hot_light));
    // One finding per rule and node, at the first pixel that shows it.
    let mut seen = Vec::new();
    let w = usize::try_from(full.w).unwrap();
    for (k, (&ink, &bg)) in full.px.iter().zip(&under.px).enumerate() {
        let owner = own[k].map(|o| &nodes[o as usize]);
        let mut note = |rule: &str, what: String| {
            let once = (rule.to_owned(), what, own[k]);
            if !seen.contains(&once) {
                let who = owner.map_or_else(String::new, |n| {
                    format!(" in {:?} {:?} {:?}", n.role, n.key, n.text)
                });
                bad.push(format!("{rule}: {}{who} at ({}, {})", once.1, k % w, k / w));
                seen.push(once);
            }
        };
        if ink != bg && !pairs.contains(&(ink, bg)) {
            note("rule 2", format!("{} on {}", name(ink), name(bg)));
        }
        if ink == gray && !owner.is_some_and(|n| n.in_disabled) {
            note("rule 3", "GrayText outside disabled UI".to_owned());
        }
        if ink != bg
            && owner.is_some_and(|n| !n.enabled)
            && ink != gray
            && (ink, bg) != (argb(s.window), gray)
        {
            note(
                "rule 3",
                format!("disabled text in {} on {}", name(ink), name(bg)),
            );
        }
        if ink == hot {
            note("rule 4", "HotLight".to_owned());
        }
    }
    bad
}

/// Rule 7's marks, at rest: a selected item is a `Highlight` fill; a checked box shows its mark
/// (`HighlightText`, or `Window` on its `GrayText` fill when disabled), an unchecked one none; a
/// picked radio button its dot.
fn marks<M>(ui: &Ui<M>) -> Vec<String> {
    let theme = Theme::high_contrast(ID);
    let full = paint(ui, &theme, &mut FakeText);
    let under = paint(ui, &theme, &mut Blank);
    let s = ID;
    let mut bad = Vec::new();
    let any = |r: RectPx, f: &dyn Fn(i32, i32) -> bool| {
        (r.y0..r.y1).any(|y| (r.x0..r.x1).any(|x| f(x, y)))
    };
    for n in painted(ui) {
        let r = n.shows;
        if r.is_empty() {
            continue;
        }
        let selectable = matches!(
            n.role,
            Role::Button | Role::Tab | Role::MenuItem | Role::Row
        );
        if selectable && n.selected == Some(true) && n.enabled {
            let mid = i32::midpoint(n.rect.y0, n.rect.y1);
            let p = full.at(n.rect.x0 + 3, mid);
            if p != argb(s.highlight) {
                bad.push(format!(
                    "rule 7: selected {:?} {:?} is {}, not Highlight",
                    n.role,
                    n.text,
                    name(p)
                ));
            }
        }
        // A box or a radio button its viewport cuts (the last one a scrolled list shows) is
        // judged where it shows whole: a sliver of it may hold no part of its mark.
        if matches!(n.role, Role::CheckBox | Role::Radio) && r != n.rect {
            continue;
        }
        let mark = |x, y| full.at(x, y) == argb(s.highlight_text);
        let disabled_mark =
            |x, y| full.at(x, y) == argb(s.window) && under.at(x, y) == argb(s.gray_text);
        match (n.role, n.check, n.enabled) {
            (Role::CheckBox, Tri::On | Tri::Mixed, true) if !any(r, &mark) => {
                bad.push(format!("rule 7: checked {:?} shows no mark", n.text));
            }
            (Role::CheckBox, Tri::On, false) if !any(r, &disabled_mark) => {
                bad.push(format!(
                    "rule 7: disabled checked {:?} shows no mark",
                    n.text
                ));
            }
            (Role::CheckBox, Tri::Off, _) if any(r, &mark) => {
                bad.push(format!("rule 7: unchecked {:?} shows a mark", n.text));
            }
            _ => {}
        }
        if n.role == Role::Radio && n.enabled && (n.selected == Some(true)) != any(r, &mark) {
            bad.push(format!(
                "rule 7: radio {:?} picked {:?} and its dot don't agree",
                n.text, n.selected
            ));
        }
    }
    bad
}

/// Rule 7's hover and press: each target hovered, then each plain button pressed, changes only
/// its own pixels, and only to an outline (`Highlight`, or `HighlightText` on a highlight fill).
fn outlines<S: Screen>(screen: &mut S) -> Vec<String> {
    let theme = Theme::high_contrast(ID);
    let outline = [argb(ID.highlight), argb(ID.highlight_text)];
    let mut bad = Vec::new();
    let targets: Vec<(NodeId, Role, String, RectPx, Key)> = painted(screen.ui())
        .into_iter()
        .filter(|n| {
            n.enabled
                && !n.shows.is_empty()
                && matches!(
                    n.role,
                    Role::Button
                        | Role::Tab
                        | Role::ComboBox
                        | Role::MenuItem
                        | Role::Row
                        | Role::CheckBox
                        | Role::Radio
                        | Role::ColumnHeader
                )
        })
        .map(|n| (n.id, n.role, n.text, n.shows, n.key))
        .collect();
    screen.rest();
    for (id, role, text, shows, key) in targets {
        if screen.ui().info(id).is_none() {
            continue;
        }
        // The middle of the part that shows, at scale 1.
        let center = Point {
            x: px_f(i32::midpoint(shows.x0, shows.x1)),
            y: px_f(i32::midpoint(shows.y0, shows.y1)),
        };
        let region = RectPx {
            x0: shows.x0 - 2,
            y0: shows.y0 - 2,
            x1: shows.x1 + 2,
            y1: shows.y1 + 2,
        };
        let rest = paint_in(screen.ui(), &theme, &mut FakeText, region);
        let mut states = vec![("hovered", Input::PointerMove(center))];
        // A hold button's press is the hold's own state; a press on anything else that picks
        // (tabs, rows, radios, boxes) changes more than its outline.
        if role == Role::Button && key != Key::name("apply") {
            states.push(("pressed", Input::PointerDown(center)));
        }
        for (state, input) in states {
            screen.input(input);
            let now = paint_in(screen.ui(), &theme, &mut FakeText, region);
            let mut changed = 0;
            let mut wrong = None;
            for y in region.y0..region.y1 {
                for x in region.x0..region.x1 {
                    let (a, b) = (rest.at(x, y), now.at(x, y));
                    if a != b {
                        changed += 1;
                        // An outline's rounded corners blend it into what was there.
                        let drawn = outline.contains(&b)
                            || (name(b) == "a blend" && outline.iter().any(|&o| between(b, a, o)));
                        if !drawn || !shows.contains_px(x, y) {
                            wrong.get_or_insert((x, y, b));
                        }
                    }
                }
            }
            if let Some((x, y, p)) = wrong {
                bad.push(format!(
                    "rule 7: {state} {role:?} {text:?} paints {} at ({x}, {y})",
                    name(p)
                ));
            }
            // A column header that doesn't sort takes focus only, and shows no hover.
            if changed == 0 && role != Role::ColumnHeader {
                bad.push(format!("rule 7: {state} {role:?} {text:?} shows nothing"));
            }
        }
        screen.rest();
    }
    bad
}

/// Whether pixel `p` is `a`, `b` or a blend of them: each channel between theirs.
fn between(p: u32, a: u32, b: u32) -> bool {
    (0..3).all(|k| {
        let c = |v: u32| (v >> (8 * k)) & 0xFF;
        (c(a).min(c(b))..=c(a).max(c(b))).contains(&c(p))
    })
}

/// Rule 5: Tab from nothing focused until focus comes back; at each stop, the ring: `Highlight`,
/// or `HighlightText` where the fill around the node is `Highlight` (a box in a selected row).
/// A table's row, which never scrolls across, has its ring round the part that shows.
fn rings<S: Screen>(screen: &mut S) -> Vec<String> {
    let theme = Theme::high_contrast(ID);
    let wide = whole(tokens::stroke::FOCUS);
    let mut bad = Vec::new();
    let mut stops = Vec::new();
    for _ in 0..200 {
        screen.input(audit::tab(false));
        let Some(focus) = screen.ui().focus() else {
            bad.push("rule 5: Tab focused nothing".to_owned());
            break;
        };
        if stops.contains(&focus) {
            break;
        }
        stops.push(focus);
        let n = screen.ui().info(focus).unwrap();
        let mut r = n.rect.snap(1.0);
        if n.role == Role::Row {
            let shows = painted(screen.ui()).into_iter().find(|p| p.id == focus);
            if let Some(s) = shows {
                (r.x0, r.x1) = (s.shows.x0, s.shows.x1);
            }
        }
        let region = RectPx {
            x0: r.x0 - 1,
            y0: r.y0 - 1,
            x1: r.x1 + 1,
            y1: r.y1 + 1,
        };
        let f = paint_in(screen.ui(), &theme, &mut FakeText, region);
        let (mx, my) = (i32::midpoint(r.x0, r.x1), i32::midpoint(r.y0, r.y1));
        let around = paint_in(screen.ui(), &theme, &mut Blank, region).at(r.x0 - 1, my);
        let ring = if around == argb(ID.highlight) {
            argb(ID.highlight_text)
        } else {
            argb(ID.highlight)
        };
        let mut sides = Vec::new();
        for d in 0..wide {
            for (side, x, y) in [
                ("left", r.x0 + d, my),
                ("right", r.x1 - 1 - d, my),
                ("top", mx, r.y0 + d),
                ("bottom", mx, r.y1 - 1 - d),
            ] {
                if f.at(x, y) != ring && !sides.contains(&side) {
                    sides.push(side);
                }
            }
        }
        if !sides.is_empty() || !f.bounds().contains(r) {
            bad.push(format!(
                "rule 5: {:?} {:?}'s ring isn't whole: {sides:?} of {r:?}",
                n.role, n.text
            ));
        }
    }
    bad
}

/// Rule 6: a popup's edge is `WindowText`, `stroke::CONTRAST` wide, all round.
fn border<M>(ui: &Ui<M>) -> Vec<String> {
    let f = paint(ui, &Theme::high_contrast(ID), &mut FakeText);
    let wide = whole(tokens::stroke::CONTRAST);
    let edge = argb(ID.window_text);
    let (mx, my) = (f.w / 2, f.h / 2);
    let mut bad = Vec::new();
    for d in 0..wide {
        for (side, x, y) in [
            ("left", d, my),
            ("right", f.w - 1 - d, my),
            ("top", mx, d),
            ("bottom", mx, f.h - 1 - d),
        ] {
            let p = f.at(x, y);
            if p != edge {
                bad.push(format!("rule 6: {side} edge {d} px in is {}", name(p)));
            }
        }
    }
    bad
}

/// The shell's popups as their windows show them: the row menu and the column menu opened from
/// the keyboard, the Start page list, the same list scrolled in a window shorter than its
/// options with its bar shown, and the drawer button's tooltip.
fn popups() -> Vec<(&'static str, Ui<shell::Msg>)> {
    use shell::{Run, Shell, Step};
    let take = |run: Run| run.popup;
    let rows = Run::with(Shell::new()).menu_open();
    let columns = Run::with(Shell::new()).column_menu_open();
    // The list opens from its dropdown, which anchors it: F4.
    let open_list = || {
        let mut list = audit::settings_page();
        let combo = list
            .ui
            .find(&Run::START_COMBO)
            .expect("Settings has the dropdown");
        list.ui.set_focus(Some(combo));
        list.step(Step::Input(audit::key(KeyCode::F4, Mods::default())));
        list.settle();
        assert!(list.popup_open, "F4 opens the list");
        list
    };
    let list = open_list();
    // What scrolls, and the bar over it, stay inside the border, which high contrast draws
    // wider than a hairline.
    let mut short = take(open_list());
    short.set_auto_hide_scroll_bars(false);
    short.set_viewport(Size {
        h: 64.0,
        ..short.viewport()
    });
    short.layout(&mut FakeText);
    short.dispatch(audit::key(KeyCode::End, Mods::default()), &mut Vec::new());
    short.layout(&mut FakeText);
    let options = short.find(&[Key::name("start-pages")]).expect("the list");
    let scroll = short
        .info(options)
        .unwrap()
        .scroll
        .expect("the list scrolls");
    assert!(scroll.offset.y > 0.0, "End scrolls the list: {scroll:?}");
    let mut tip = Ui::new(Size { w: 0.0, h: 0.0 }, 1.0, Motion::default());
    tip.build(|cx| {
        cx.add(Element::tooltip_text(Key::name("tip"), shell::DRAWER_TIP));
    });
    let size = tip.natural_size(&mut FakeText, shell::START.w);
    tip.set_viewport(size);
    tip.layout(&mut FakeText);
    vec![
        ("the row menu", take(rows)),
        ("the column menu", take(columns)),
        ("the Start page list", take(list)),
        (
            "the Start page list, scrolled to its end in a short window",
            short,
        ),
        ("a tooltip", tip),
    ]
}

/// The shell's screens at rest, and the drawer's with Hold to apply half held.
fn shell_screens() -> Vec<(&'static str, shell::Run)> {
    use shell::{Run, Step};
    let mut screens: Vec<_> = audit::SHELL
        .iter()
        .map(|&(name, make)| (name, make()))
        .collect();
    let mut holding = audit::drawer_page();
    let at = holding.center(&Run::APPLY);
    holding.step(Step::Input(Input::PointerMove(at)));
    holding.settle();
    holding.step(Step::Input(Input::PointerDown(at)));
    holding.step(Step::Tick(0));
    holding.step(Step::Tick(u64::from(tokens::motion::HOLD_MS) / 2));
    screens.push(("the drawer, Hold to apply half held", holding));
    screens
}

/// Slice 6c's windows as they show: the palette, a running job's toast and the dialog. Each is a
/// transient surface, so rule 6 holds them to a popup's border (A4.4).
fn composite_windows() -> Vec<(&'static str, Ui<composites::Msg>)> {
    use composites::{Model, Run};
    vec![
        (
            "the palette",
            Run::with(Model::new()).palette_open().win.popup,
        ),
        (
            "a running job's toast",
            Run::with(Model::new()).job_running().win.toast,
        ),
        (
            "the dialog",
            Run::with(Model::new()).dialog_open().win.popup,
        ),
    ]
}

impl Screen for drawer_page::Run {
    type Msg = drawer_page::Msg;
    fn ui(&self) -> &Ui<drawer_page::Msg> {
        &self.ui
    }
    fn ui_mut(&mut self) -> &mut Ui<drawer_page::Msg> {
        &mut self.ui
    }
    fn input(&mut self, input: Input) {
        self.step(drawer_page::Step::Input(input));
        self.settle();
    }
}

/// Slice 6d's Preview drawer, open: as a pane beside the page, and as a sheet over it in a
/// narrow window.
fn drawer_screens() -> Vec<(&'static str, drawer_page::Run)> {
    use drawer_page::{Model, Msg, NARROW, Run, START, Step};
    let open = |size| {
        let mut run = Run::with(Model::new()).warmed();
        run.step(Step::Resize(size));
        run.step(Step::Update(Msg::Open));
        run.settle();
        assert!(run.drawer().is_some(), "the drawer is open");
        run
    };
    vec![
        ("the Preview drawer, a pane", open(START)),
        ("the Preview drawer, a sheet", open(NARROW)),
    ]
}

fn composites_screens() -> Vec<(&'static str, composites::Run)> {
    audit::COMPOSITES
        .iter()
        .map(|&(name, make)| (name, make()))
        .collect()
}

/// Rules 1 to 4 on every screen at rest, the popups and the controls included.
#[test]
fn every_pixel_and_text_run_is_a_system_pair() {
    let mut bad = Vec::new();
    for (name, run) in shell_screens() {
        bad.extend(audit::on(name, colors_and_pairs(&run.ui)));
    }
    for (name, ui) in popups() {
        bad.extend(audit::on(name, colors_and_pairs(&ui)));
    }
    for (name, run) in composites_screens() {
        bad.extend(audit::on(name, colors_and_pairs(&run.ui)));
    }
    for (name, ui) in composite_windows() {
        bad.extend(audit::on(name, colors_and_pairs(&ui)));
    }
    for (name, run) in drawer_screens() {
        bad.extend(audit::on(name, colors_and_pairs(&run.ui)));
    }
    bad.extend(audit::on(
        "the controls",
        colors_and_pairs(&Gallery::new().ui),
    ));
    report("high contrast", &bad);
}

/// Rule 5 on every screen's Tab stops.
#[test]
fn the_focus_ring_is_highlight_and_whole_on_every_stop() {
    let mut bad = Vec::new();
    for (name, mut run) in shell_screens() {
        bad.extend(audit::on(name, rings(&mut run)));
    }
    for (name, mut run) in composites_screens() {
        bad.extend(audit::on(name, rings(&mut run)));
    }
    for (name, mut run) in drawer_screens() {
        bad.extend(audit::on(name, rings(&mut run)));
    }
    bad.extend(audit::on("the controls", rings(&mut Gallery::new())));
    report("focus rings", &bad);
}

/// Rule 6 on every popup the shell opens, and slice 6c's windows.
#[test]
fn popups_have_a_contrast_border() {
    let mut bad = Vec::new();
    for (name, ui) in popups() {
        bad.extend(audit::on(name, border(&ui)));
    }
    for (name, ui) in composite_windows() {
        bad.extend(audit::on(name, border(&ui)));
    }
    report("popup borders", &bad);
}

/// Rule 6 on the Preview drawer, a transient surface (A4.4): its edge beside the page is
/// `WindowText`, `stroke::CONTRAST` wide, as a pane and as a sheet.
#[test]
fn the_drawer_has_a_contrast_border() {
    let edge = argb(ID.window_text);
    let mut bad = Vec::new();
    for (screen, run) in drawer_screens() {
        let f = paint(&run.ui, &Theme::high_contrast(ID), &mut FakeText);
        let r = run.ui.info(run.drawer().unwrap()).unwrap().rect.snap(1.0);
        let y = i32::midpoint(r.y0, r.y1);
        for d in 0..whole(tokens::stroke::CONTRAST) {
            let p = f.at(r.x0 + d, y);
            if p != edge {
                bad.push(format!(
                    "{screen}: rule 6: the start edge {d} px in is {}",
                    name(p)
                ));
            }
        }
    }
    report("the drawer's border", &bad);
}

/// Rule 7's marks on every screen, the popups and the controls included.
#[test]
fn selection_is_highlight_and_checks_show_a_mark() {
    let mut bad = Vec::new();
    for (name, run) in shell_screens() {
        bad.extend(audit::on(name, marks(&run.ui)));
    }
    for (name, ui) in popups() {
        bad.extend(audit::on(name, marks(&ui)));
    }
    for (name, run) in composites_screens() {
        bad.extend(audit::on(name, marks(&run.ui)));
    }
    for (name, ui) in composite_windows() {
        bad.extend(audit::on(name, marks(&ui)));
    }
    for (name, run) in drawer_screens() {
        bad.extend(audit::on(name, marks(&run.ui)));
    }
    bad.extend(audit::on("the controls", marks(&Gallery::new().ui)));
    report("marks", &bad);
}

/// Rule 7's hover and press, on every target of the table page, the composites page, the
/// controls and the row menu.
#[test]
fn hover_and_press_show_only_an_outline() {
    let mut bad = Vec::new();
    bad.extend(audit::on(
        "the table page",
        outlines(&mut audit::table_page()),
    ));
    bad.extend(audit::on(
        "the composites page",
        outlines(&mut audit::composites_page()),
    ));
    bad.extend(audit::on("the controls", outlines(&mut Gallery::new())));
    let menu = shell::Run::with(shell::Shell::new()).menu_open();
    bad.extend(audit::on("the row menu", outlines(&mut Window(menu.popup))));
    report("hover and press", &bad);
}
