//! The keyboard-only audit (TD-0009 A4.5, A4.9; slice 7d). On every screen the fixtures reach,
//! with the keyboard alone (WCAG 2.2's 2.1.1, 2.1.2, 2.4.3, 2.4.7, 2.4.11 and 1.4.11):
//! 1. Tab from nothing focused visits every stop once and comes back to the first, within *N* + 1
//!    presses (no trap), and Shift+Tab visits the same stops the other way round;
//! 2. within each region (the header, the rail, the page, the drawer), the stops follow reading
//!    order: none reads before one Tab reached ahead of it;
//! 3. every enabled control that shows is a Tab stop, or reachable by the arrow keys from one (a
//!    group's items, a table's rows and column headers), and every node with a context menu
//!    opens it with Shift+F10;
//! 4. after every key the focus is visible and unclipped (a table's row, which never scrolls
//!    across, may be cut at the table's sides), and its ring stands out from what's around it, at
//!    least 3:1 in light and dark (high contrast's is the contrast audit's rule 5);
//! 5. Esc sends no message from any stop (it never cancels: TD-0009 §4.3), and closes the
//!    topmost popup, giving focus back to what opened it;
//! 6. disabled controls aren't Tab stops, and a focus parked on a control that went disabled
//!    leaves on Tab and Shift+Tab (parking is slice 7c's: this holds with it or without);
//! 7. targets are 24 × 24, which `targets.rs` checks on every fixture.
//!
//! The screens are the contrast audit's: the shell's table page, its drawer open, a summary page
//! and Settings; the composites page, Details open; and every standard control, enabled and
//! disabled. The popups' own keys are `popups.rs`'s. A5's and A6's fixtures join as they land.

#![allow(missing_docs)]
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a window or a node that isn't there fails the audit"
)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;
#[path = "../benches/fixtures/shell.rs"]
mod shell;

mod audit;

use audit::{Blank, Gallery, Painted, Screen, key, paint_in, painted, report, tab};
use dpro_ui::{
    Element, FakeText, Input, Key, KeyCode, Mods, Motion, NodeId, RectPx, Role, Size, Theme, Ui,
};

/// More Tab presses than any screen has stops: a walk this long is trapped.
const LIMIT: usize = 300;

/// WCAG 2.2's least contrast for a focus indicator (1.4.11).
const RING_CONTRAST: f64 = 3.0;

/// WCAG 2.2 relative luminance of a painted pixel.
fn luminance(p: u32) -> f64 {
    let lin = |shift: u32| {
        let s = f64::from((p >> shift) & 0xFF) / 255.0;
        if s <= 0.040_45 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(16) + 0.7152 * lin(8) + 0.0722 * lin(0)
}

fn contrast(a: u32, b: u32) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// The node `id` as painted now.
fn node<M>(ui: &Ui<M>, id: NodeId) -> Option<Painted> {
    painted(ui).into_iter().find(|n| n.id == id)
}

/// The keys from the root to node `id`: what names it across two builds of the same screen.
fn path_of<M>(ui: &Ui<M>, id: NodeId) -> Option<Vec<Key>> {
    fn walk<M>(ui: &Ui<M>, at: NodeId, id: NodeId, path: &mut Vec<Key>) -> bool {
        for c in ui.children(at) {
            let Some(n) = ui.info(c) else { continue };
            path.push(n.key);
            if c == id || walk(ui, c, id, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut path = Vec::new();
    walk(ui, ui.root(), id, &mut path).then_some(path)
}

/// What a node is, for a finding.
fn who<M>(ui: &Ui<M>, id: NodeId) -> String {
    ui.info(id).map_or_else(
        || "a node gone".to_owned(),
        |n| format!("{:?} {:?}", n.role, n.text),
    )
}

/// Rule 4 after a key: the focus shows its ring, wholly in view (a row may be cut at its
/// table's sides).
fn shows_focus<S: Screen>(screen: &S, after: &str, bad: &mut Vec<String>) {
    let ui = screen.ui();
    let Some(focus) = ui.focus() else {
        return;
    };
    if !ui.focus_visible() {
        bad.push(format!(
            "rule 4: after {after}, {}'s ring is hidden",
            who(ui, focus)
        ));
        return;
    }
    let Some(n) = node(ui, focus) else {
        return;
    };
    let whole = if n.role == Role::Row {
        n.shows.y0 == n.rect.y0 && n.shows.y1 == n.rect.y1 && n.shows.x1 > n.shows.x0
    } else {
        n.shows == n.rect
    };
    if !whole {
        bad.push(format!(
            "rule 4: after {after}, {} shows {:?} of {:?}",
            who(ui, focus),
            n.shows,
            n.rect
        ));
    }
}

/// Rules 1, 4 and 6 on the Tab walk: the stops in order, from nothing focused.
fn walk<S: Screen>(screen: &mut S, bad: &mut Vec<String>) -> Vec<NodeId> {
    let mut stops = Vec::new();
    for _ in 0..LIMIT {
        screen.input(tab(false));
        let Some(focus) = screen.ui().focus() else {
            bad.push("rule 1: Tab focused nothing".to_owned());
            return stops;
        };
        shows_focus(screen, "Tab", bad);
        if let Some(at) = stops.iter().position(|&s| s == focus) {
            if at != 0 {
                bad.push(format!(
                    "rule 1: Tab is trapped: it came back to stop {at} of {}, not the first",
                    stops.len()
                ));
            }
            return stops;
        }
        if node(screen.ui(), focus).is_some_and(|n| n.in_disabled) {
            bad.push(format!(
                "rule 6: Tab stops on {}, which is disabled",
                who(screen.ui(), focus)
            ));
        }
        stops.push(focus);
    }
    bad.push(format!(
        "rule 1: {LIMIT} Tabs never came back to the first stop"
    ));
    stops
}

/// Whether `a` and `b` are the same Tab stop: the same node, or items of one group (siblings of
/// one role), which Tab enters at its first item and Shift+Tab at its last unless one is picked.
fn same_stop<M>(ui: &Ui<M>, a: NodeId, b: NodeId) -> bool {
    let item = |id| Some((ui.info(id)?.role, path_of(ui, id)?));
    a == b
        || matches!((item(a), item(b)), (Some((ra, pa)), Some((rb, pb)))
            if ra == rb && pa[..pa.len() - 1] == pb[..pb.len() - 1])
}

/// Rule 1's way back: from the first stop, Shift+Tab visits the stops the other way round.
fn walk_back<S: Screen>(screen: &mut S, stops: &[NodeId], bad: &mut Vec<String>) {
    let mut back = Vec::new();
    for _ in 0..stops.len() {
        screen.input(tab(true));
        shows_focus(screen, "Shift+Tab", bad);
        back.extend(screen.ui().focus());
    }
    // From the first stop, Shift+Tab goes to the last, and comes back to the first at the end.
    let want: Vec<NodeId> = stops[1..]
        .iter()
        .rev()
        .copied()
        .chain(stops.first().copied())
        .collect();
    let ui = screen.ui();
    if back.len() != want.len() || back.iter().zip(&want).any(|(&b, &w)| !same_stop(ui, b, w)) {
        let names = |ids: &[NodeId]| -> Vec<String> {
            ids.iter().map(|&id| who(screen.ui(), id)).collect()
        };
        bad.push(format!(
            "rule 1: Shift+Tab visits {:?}, not Tab's stops reversed {:?}",
            names(&back),
            names(&want)
        ));
    }
}

/// The region a node is in: the header, the rail, the page or the drawer (the shell's `body`
/// splits into the last three), or the window's one region.
fn region(path: &[Key]) -> &[Key] {
    let depth = if path.first() == Some(&Key::name("body")) {
        2
    } else {
        1
    };
    &path[..depth.min(path.len())]
}

/// Whether `b` reads before `a`: wholly above it, or beside it on its line and wholly left.
fn reads_before(b: RectPx, a: RectPx) -> bool {
    b.y1 <= a.y0 || (b.y0 < a.y1 && a.y0 < b.y1 && b.x1 <= a.x0)
}

/// Rule 2: within each region, no stop reads before one Tab reached ahead of it. Each stop as it
/// was when Tab reached it, since reaching one may scroll the page.
fn reading_order(seen: &[(Vec<Key>, RectPx, String)]) -> Vec<String> {
    let mut bad = Vec::new();
    for (i, (pa, ra, na)) in seen.iter().enumerate() {
        for (pb, rb, nb) in &seen[i + 1..] {
            if region(pa) == region(pb) && reads_before(*rb, *ra) {
                bad.push(format!(
                    "rule 2: Tab reaches {nb} after {na}, which it reads after"
                ));
            }
        }
    }
    bad
}

/// The enabled controls that show, by path: what rule 3 must reach.
fn targets<M>(ui: &Ui<M>) -> Vec<Vec<Key>> {
    painted(ui)
        .into_iter()
        .filter(|n| {
            !n.in_disabled
                && !n.shows.is_empty()
                && matches!(
                    n.role,
                    Role::Button
                        | Role::CheckBox
                        | Role::Field
                        | Role::Tab
                        | Role::Radio
                        | Role::ComboBox
                        | Role::Row
                        | Role::ColumnHeader
                )
        })
        .filter_map(|n| path_of(ui, n.id))
        .collect()
}

/// Rule 3's arrows, and rule 4 after each: from each stop of a fresh screen, each arrow key held
/// down past the group's end, then the other way back past its start (Up from a table's first row
/// goes to its column headers, which Right and Left then cross). Returns the paths reached.
fn arrows<S: Screen>(
    make: &dyn Fn() -> S,
    stops: usize,
    want: usize,
    bad: &mut Vec<String>,
) -> Vec<Vec<Key>> {
    let n = want + 2;
    let script = [
        (KeyCode::Down, n),
        (KeyCode::Up, 2 * n),
        (KeyCode::Right, n),
        (KeyCode::Left, 2 * n),
        (KeyCode::End, 1),
        (KeyCode::Home, 1),
    ];
    let mut reached = Vec::new();
    for k in 0..stops {
        let mut screen = make();
        for _ in 0..=k {
            screen.input(tab(false));
        }
        for (code, times) in script {
            for _ in 0..times {
                let from = screen.ui().focus();
                screen.input(key(code, Mods::default()));
                shows_focus(&screen, &format!("{code:?}"), bad);
                let to = screen.ui().focus();
                if let Some(path) = to.and_then(|id| path_of(screen.ui(), id))
                    && !reached.contains(&path)
                {
                    reached.push(path);
                }
                if to == from {
                    break;
                }
            }
        }
    }
    reached
}

/// Rules 1 to 6 (but 5's popups) on a screen, made fresh by `make` as often as rule 3 needs.
fn audit<S: Screen>(make: &dyn Fn() -> S) -> Vec<String> {
    let mut bad = Vec::new();
    let want = targets(make().ui());
    let mut screen = make();
    let stops = walk(&mut screen, &mut bad);
    // Rule 2 on one layout, the walk's last: reaching a stop may scroll the page.
    let seen: Vec<_> = stops
        .iter()
        .filter_map(|&id| {
            let ui = screen.ui();
            Some((path_of(ui, id)?, ui.info(id)?.rect.snap(1.0), who(ui, id)))
        })
        .collect();
    bad.extend(reading_order(&seen));
    walk_back(&mut screen, &stops, &mut bad);
    // Rule 5: Esc on each stop sends nothing.
    let mut screen = make();
    for _ in 0..stops.len() {
        screen.input(tab(false));
        let Some(focus) = screen.ui().focus() else {
            break;
        };
        let sent = screen.sends(key(KeyCode::Escape, Mods::default()));
        if sent > 0 {
            bad.push(format!(
                "rule 5: Esc on {} sends {sent} messages",
                who(screen.ui(), focus)
            ));
        }
    }
    // Rule 3: what neither Tab nor the arrows reach.
    let mut reached = arrows(make, stops.len(), want.len(), &mut bad);
    let screen = make();
    reached.extend(stops.iter().filter_map(|&id| path_of(screen.ui(), id)));
    for path in want.iter().filter(|p| !reached.contains(p)) {
        let id = screen.ui().find(path).expect("a target is there");
        bad.push(format!("rule 3: nothing reaches {}", who(screen.ui(), id)));
    }
    bad
}

/// Rule 4's contrast: at each Tab stop, in light and dark, the ring's outer pixels against those
/// just outside it, on each side the window shows.
fn ring_contrast<S: Screen>(screen: &mut S) -> Vec<String> {
    let mut bad = Vec::new();
    let mut stops = Vec::new();
    for _ in 0..LIMIT {
        screen.input(tab(false));
        let Some(focus) = screen.ui().focus() else {
            break;
        };
        if stops.contains(&focus) {
            break;
        }
        stops.push(focus);
        let Some(n) = node(screen.ui(), focus) else {
            continue;
        };
        let r = n.shows;
        let region = RectPx {
            x0: r.x0 - 1,
            y0: r.y0 - 1,
            x1: r.x1 + 1,
            y1: r.y1 + 1,
        };
        let (mx, my) = (i32::midpoint(r.x0, r.x1), i32::midpoint(r.y0, r.y1));
        for (scheme, theme) in [("light", Theme::LIGHT), ("dark", Theme::DARK)] {
            let f = paint_in(screen.ui(), &theme, &mut Blank, region);
            for (side, ring, out) in [
                ("left", (r.x0, my), (r.x0 - 1, my)),
                ("right", (r.x1 - 1, my), (r.x1, my)),
                ("top", (mx, r.y0), (mx, r.y0 - 1)),
                ("bottom", (mx, r.y1 - 1), (mx, r.y1)),
            ] {
                if !f.bounds().contains_px(out.0, out.1) {
                    continue;
                }
                let c = contrast(f.at(ring.0, ring.1), f.at(out.0, out.1));
                if c < RING_CONTRAST {
                    bad.push(format!(
                        "rule 4: in {scheme}, {}'s ring is {c:.2}:1 against what's {side} of it",
                        who(screen.ui(), focus)
                    ));
                }
            }
        }
    }
    bad
}

/// Rules 1 to 6 on the shell's screens.
#[test]
fn the_shell_works_from_the_keyboard_alone() {
    let mut bad = Vec::new();
    for (name, make) in audit::SHELL {
        bad.extend(audit::on(name, audit(&make)));
    }
    report("the shell by keyboard", &bad);
}

/// Rules 1 to 6 on the composites' screens.
#[test]
fn the_composites_work_from_the_keyboard_alone() {
    let mut bad = Vec::new();
    for (name, make) in audit::COMPOSITES {
        bad.extend(audit::on(name, audit(&make)));
    }
    report("the composites by keyboard", &bad);
}

/// Rules 1 to 6 on every standard control, enabled and disabled: the disabled are no stops.
#[test]
fn the_controls_work_from_the_keyboard_alone() {
    report("the controls by keyboard", &audit(&Gallery::new));
}

/// Rule 4's contrast on every screen's stops: the batch bar's ring was the one that blended in
/// (A3's as-built note).
#[test]
fn every_focus_ring_stands_out_in_light_and_dark() {
    let mut bad = Vec::new();
    for (name, make) in audit::SHELL {
        bad.extend(audit::on(name, ring_contrast(&mut make())));
    }
    for (name, make) in audit::COMPOSITES {
        bad.extend(audit::on(name, ring_contrast(&mut make())));
    }
    bad.extend(audit::on(
        "the controls",
        ring_contrast(&mut Gallery::new()),
    ));
    report("focus ring contrast", &bad);
}

/// Rule 3's context menus: a row's and every column header's open with Shift+F10, which the
/// platform sends as a context request for the focused node.
#[test]
fn shift_f10_opens_every_context_menu() {
    let mut bad = Vec::new();
    let probe = audit::table_page();
    let mut nodes: Vec<Vec<Key>> = painted(&probe.ui)
        .into_iter()
        .filter(|n| n.role == Role::ColumnHeader && !n.shows.is_empty())
        .filter_map(|n| path_of(&probe.ui, n.id))
        .collect();
    nodes.push(shell::Run::row(4).to_vec());
    for path in nodes {
        let mut run = audit::table_page();
        let id = run.ui.find(&path).expect("it shows");
        run.ui.set_focus(Some(id));
        run.input(Input::ContextMenu(None));
        if !run.popup_open {
            bad.push(format!(
                "rule 3: Shift+F10 on {} opens nothing",
                who(&run.ui, id)
            ));
        }
    }
    report("Shift+F10", &bad);
}

/// Rule 5's popups: Esc closes the row menu, the column menu and the Start page list, gives focus
/// back to what opened each, and sends nothing else: nothing applies, the drawer stays shut.
#[test]
fn esc_closes_the_topmost_popup_and_cancels_nothing() {
    use shell::{Run, Shell, Step};
    let mut bad = Vec::new();
    let list = || {
        let mut run = audit::settings_page();
        let combo = run
            .ui
            .find(&Run::START_COMBO)
            .expect("Settings has the dropdown");
        run.ui.set_focus(Some(combo));
        run.input(key(KeyCode::F4, Mods::default()));
        run
    };
    let opened: [(&str, Run, Vec<Key>); 3] = [
        (
            "the row menu",
            Run::with(Shell::new()).menu_open(),
            Run::row(4).to_vec(),
        ),
        (
            "the column menu",
            Run::with(Shell::new()).column_menu_open(),
            Run::column_header(1).to_vec(),
        ),
        ("the Start page list", list(), Run::START_COMBO.to_vec()),
    ];
    for (name, mut run, anchor) in opened {
        assert!(run.popup_open, "{name} opens");
        run.step(Step::Popup(key(KeyCode::Escape, Mods::default())));
        run.settle();
        if run.popup_open {
            bad.push(format!("rule 5: Esc leaves {name} open"));
        }
        if run.ui.focus() != run.ui.find(&anchor) {
            bad.push(format!(
                "rule 5: Esc on {name} doesn't give focus back to its anchor"
            ));
        }
        if run.shell.applies() > 0 || run.shell.drawer() {
            bad.push(format!("rule 5: Esc on {name} did more than close it"));
        }
    }
    report("Esc", &bad);
}

/// Rule 6's parked focus: focus on a button that then goes disabled leaves it on Tab and on
/// Shift+Tab, for an enabled control, whether the focus stayed parked (slice 7c) or moved on.
#[test]
fn a_parked_focus_leaves_on_tab() {
    fn view(cx: &mut dpro_ui::Cx<'_, u8>, middle: bool) {
        for (k, label) in [(0u64, "Before"), (1, "Parked"), (2, "After")] {
            cx.add(Element::button(Key::index(k), label, 1).enabled(k != 1 || middle));
        }
    }
    let mut bad = Vec::new();
    for back in [false, true] {
        let mut ui = Ui::new(Size { w: 400.0, h: 100.0 }, 1.0, Motion::default());
        ui.set_active(true);
        ui.build(|cx| view(cx, true));
        ui.layout(&mut FakeText);
        let parked = ui.find(&[Key::index(1)]).expect("it's there");
        ui.set_focus(Some(parked));
        ui.build(|cx| view(cx, false));
        ui.layout(&mut FakeText);
        let mut out = Vec::new();
        ui.dispatch(tab(back), &mut out);
        ui.layout(&mut FakeText);
        let to = ui.focus();
        let enabled = to.and_then(|id| ui.info(id)).is_some_and(|n| n.enabled);
        if to == Some(parked) || !enabled {
            let keys = if back { "Shift+Tab" } else { "Tab" };
            bad.push(format!(
                "rule 6: {keys} from the parked focus goes to {}",
                to.map_or_else(|| "nothing".to_owned(), |id| who(&ui, id))
            ));
        }
    }
    report("parked focus", &bad);
}
