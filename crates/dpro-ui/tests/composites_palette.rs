//! The command palette (TD-0009 A3.5 (3), A3.9), in its popup window over the composites page:
//! focus stays in the field; Up and Down move the highlight, wrapping; Enter sends `on_run` and
//! Esc the popup's `on_close`; a click on an option sends `on_pick`, and hovering one never moves
//! focus. A client sees a dialog whose Edit is `ControllerFor` the list, exactly one option
//! selected, positions that skip the headers, and a live status. At most 8 rows show, the rest
//! scrolled, and the page under it dims behind the scrim, except in high contrast.

#![expect(
    clippy::expect_used,
    reason = "test helpers: a node that isn't there fails the test"
)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use composites::{
    DISABLED, MAX_ROWS, Model, Msg, PALETTE, Run, Step, Window, popup_char, popup_key,
};
use dpro_ui::{
    Color, Input, Key, KeyCode, NodeId, SysColors, Theme, Tone,
    accesskit::{Live, Node, Role as AkRole, TreeUpdate},
    composite::{PALETTE_FIELD, PALETTE_LIST, PALETTE_STATUS},
    tokens::palette,
};

fn open() -> Run {
    Run::with(Model::new()).palette_open()
}

fn field(run: &Run) -> NodeId {
    run.part_in(Window::Popup, PALETTE_FIELD)
        .expect("the palette's field")
}

fn list(run: &Run) -> NodeId {
    run.part_in(Window::Popup, PALETTE_LIST)
        .expect("the palette's list")
}

/// Result `i`'s option.
fn option(run: &Run, i: u32) -> NodeId {
    let entry = run.model.over.rows[i as usize].entry;
    run.ui_of(Window::Popup)
        .children(list(run))
        .find(|&c| {
            run.ui_of(Window::Popup)
                .info(c)
                .is_some_and(|n| n.key == Key::index(u64::from(entry)))
        })
        .expect("the option")
}

/// The options a client sees in `tree`, under the list.
fn options<'t>(run: &Run, tree: &'t TreeUpdate) -> Vec<&'t Node> {
    let list = run.ak_in(Window::Popup, tree, list(run)).expect("the list");
    list.children()
        .iter()
        .filter_map(|id| tree.nodes.iter().find(|(ak, _)| ak == id))
        .map(|(_, n)| n)
        .filter(|n| n.role() == AkRole::ListBoxOption)
        .collect()
}

#[test]
fn it_opens_with_its_field_focused_and_every_result_up_to_50() {
    let run = open();
    assert_eq!(run.win.popup_open, Some(dpro_ui::PopupKind::Palette));
    assert_eq!(run.ui_of(Window::Popup).focus(), Some(field(&run)));
    assert_eq!(run.model.over.rows.len(), MAX_ROWS);
    assert_eq!(run.model.over.highlighted, Some(0), "a default selection");
}

#[test]
fn focus_stays_in_the_field() {
    let mut run = open();
    let field = field(&run);
    for s in [
        popup_char('c'),
        popup_key(KeyCode::Down),
        popup_key(KeyCode::Tab),
        popup_key(KeyCode::Up),
        popup_key(KeyCode::Home),
        popup_key(KeyCode::End),
    ] {
        run.step(s);
        assert_eq!(run.ui_of(Window::Popup).focus(), Some(field));
    }
    assert_eq!(run.model.over.query, "c");
}

#[test]
fn up_and_down_move_the_highlight_and_wrap() {
    let mut run = open();
    run.step(popup_key(KeyCode::Up));
    let last = u32::try_from(MAX_ROWS - 1).unwrap_or(0);
    assert_eq!(run.model.over.highlighted, Some(last), "Up from the first");
    assert_eq!(
        run.ui_of(Window::Popup)
            .info(option(&run, last))
            .and_then(|n| n.selected),
        Some(true)
    );
    run.step(popup_key(KeyCode::Down));
    assert_eq!(run.model.over.highlighted, Some(0), "Down from the last");
    run.step(popup_key(KeyCode::Down));
    assert_eq!(run.model.over.highlighted, Some(1));
    // The highlighted row is scrolled into view: the last one, after Up from the first.
    run.step(popup_key(KeyCode::Up));
    run.step(popup_key(KeyCode::Up));
    run.settle();
    let ui = run.ui_of(Window::Popup);
    let row = ui.info(option(&run, last)).expect("the last row").rect;
    let viewport = scroller(&run)
        .and_then(|s| ui.info(s))
        .expect("the list's viewport")
        .rect;
    assert!(
        row.y >= viewport.y - 0.5 && row.y + row.h <= viewport.y + viewport.h + 0.5,
        "{row:?} out of {viewport:?}"
    );
}

#[test]
fn enter_sends_on_run_and_esc_sends_on_close() {
    let mut run = open();
    run.step(popup_key(KeyCode::Down));
    run.step(popup_key(KeyCode::Enter));
    assert_eq!(run.model.over.ran, Some(Msg::PalettePick(1)));
    assert!(!run.model.over.palette, "running a command closes it");
    assert_eq!(run.win.popup_open, None);
    let mut run = open();
    run.step(popup_key(KeyCode::Escape));
    assert_eq!(run.model.last, Some(Msg::PaletteClose));
    assert_eq!(run.model.over.ran, None);
    assert_eq!(run.win.popup_open, None);
}

#[test]
fn a_disabled_result_can_be_highlighted_but_enter_does_nothing() {
    let mut run = open();
    for _ in 0..DISABLED {
        run.step(popup_key(KeyCode::Down));
    }
    assert_eq!(run.model.over.highlighted, Some(DISABLED));
    run.step(popup_key(KeyCode::Enter));
    assert!(run.model.over.palette, "still open");
    assert_eq!(run.model.over.ran, None);
}

#[test]
fn a_click_on_an_option_picks_it_and_hover_never_moves_focus() {
    let mut run = open();
    let field = field(&run);
    let third = option(&run, 2);
    let at = run.center_in(Window::Popup, third);
    run.step(Step::Popup(Input::PointerMove(at)));
    run.settle();
    assert_eq!(run.ui_of(Window::Popup).focus(), Some(field), "hover");
    assert!(
        run.ui_of(Window::Popup)
            .info(third)
            .is_some_and(|n| n.hovered)
    );
    run.step(Step::Popup(Input::PointerDown(at)));
    assert_eq!(run.ui_of(Window::Popup).focus(), Some(field), "press");
    run.step(Step::Popup(Input::PointerUp(at)));
    assert_eq!(run.model.last, Some(Msg::PalettePick(2)));
    assert_eq!(run.model.over.ran, Some(Msg::PalettePick(2)));
}

#[test]
fn typing_filters_and_no_match_says_so() {
    let mut run = open();
    for c in "zzz".chars() {
        run.step(popup_char(c));
    }
    assert_eq!(run.model.over.rows.len(), 0);
    assert_eq!(run.model.over.highlighted, None);
    assert_eq!(
        run.model.over.status,
        "No commands match \u{201c}zzz\u{201d}."
    );
}

#[test]
fn a_client_sees_a_dialog_named_by_the_palette() {
    let mut run = Run::accessible(Model::new()).palette_open();
    let tree = run.a11y_tree_of(Window::Popup);
    let dialog = run.part_in(Window::Popup, PALETTE).expect("the palette");
    let node = run
        .ak_in(Window::Popup, &tree, dialog)
        .expect("in the tree");
    assert_eq!(node.role(), AkRole::Dialog);
    assert_eq!(node.label(), Some("Command palette"));
    let edit = run
        .ak_in(Window::Popup, &tree, field(&run))
        .expect("the field");
    assert_eq!(edit.role(), AkRole::TextInput);
    assert_eq!(edit.label(), Some("Search commands"));
}

#[test]
fn the_field_is_controller_for_the_list() {
    let mut run = Run::accessible(Model::new()).palette_open();
    let tree = run.a11y_tree_of(Window::Popup);
    let edit = run
        .ak_in(Window::Popup, &tree, field(&run))
        .expect("the field");
    let list_id = tree
        .nodes
        .iter()
        .find(|(ak, _)| run.ui_of(Window::Popup).a11y_node(*ak) == Some(list(&run)))
        .map(|(ak, _)| *ak)
        .expect("the list");
    assert_eq!(edit.controls(), &[list_id]);
    let list = run.ak_in(Window::Popup, &tree, list(&run)).expect("listed");
    assert_eq!(list.role(), AkRole::ListBox);
    assert_eq!(list.label(), Some("Commands"));
}

#[test]
fn exactly_one_option_is_selected_and_positions_skip_the_headers() {
    let mut run = Run::accessible(Model::new()).palette_open();
    let tree = run.a11y_tree_of(Window::Popup);
    let found = options(&run, &tree);
    assert_eq!(found.len(), MAX_ROWS, "headers aren't options");
    let selected: Vec<usize> = (0..)
        .zip(&found)
        .filter(|(_, n)| n.is_selected() == Some(true))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(selected, [0]);
    // Zero-based, as AccessKit counts and slice 6a's suggestion lists place their options; the
    // set's size is the list's.
    for (i, n) in (0..).zip(&found) {
        assert_eq!(n.position_in_set(), Some(i));
    }
    let list = run
        .ak_in(Window::Popup, &tree, list(&run))
        .expect("the list");
    assert_eq!(list.size_of_set(), Some(MAX_ROWS));
    // An option is read once: its parts are presentational.
    assert!(found.iter().all(|n| n.children().is_empty()));
    // Down moves the selection: one option changes, in the update.
    run.step(popup_key(KeyCode::Down));
    let tree = run.a11y_tree_of(Window::Popup);
    let selected: Vec<usize> = (0..)
        .zip(options(&run, &tree))
        .filter(|(_, n)| n.is_selected() == Some(true))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(selected, [1]);
}

#[test]
fn the_status_is_a_polite_live_region() {
    let mut run = Run::accessible(Model::new()).palette_open();
    for c in "zzz".chars() {
        run.step(popup_char(c));
    }
    let tree = run.a11y_tree_of(Window::Popup);
    let status = run
        .part_in(Window::Popup, PALETTE_STATUS)
        .expect("the status");
    let node = run.ak_in(Window::Popup, &tree, status).expect("listed");
    assert_eq!(node.live(), Some(Live::Polite));
    assert_eq!(
        node.value().or(node.label()),
        Some("No commands match \u{201c}zzz\u{201d}.")
    );
}

/// The popup's list viewport: the scroll node around the list.
fn scroller(run: &Run) -> Option<NodeId> {
    let ui = run.ui_of(Window::Popup);
    let mut queue = std::collections::VecDeque::from([ui.root()]);
    while let Some(id) = queue.pop_front() {
        for c in ui.children(id) {
            if ui.info(c).is_some_and(|n| n.scroll.is_some()) {
                return Some(c);
            }
            queue.push_back(c);
        }
    }
    None
}

#[test]
fn at_most_8_rows_show_and_the_rest_scroll() {
    let run = open();
    let ui = run.ui_of(Window::Popup);
    let row = ui.info(option(&run, 0)).expect("a row").rect;
    assert!(row.h >= palette::ROW_H - 0.5, "{row:?}");
    let viewport = scroller(&run)
        .and_then(|s| ui.info(s))
        .expect("the list's viewport");
    let most = f32::from(palette::ROWS) * palette::ROW_H;
    assert!(
        viewport.rect.h <= most + 0.5,
        "{} > {most}",
        viewport.rect.h
    );
    assert!(
        viewport
            .scroll
            .is_some_and(|s| s.content.h > viewport.rect.h)
    );
    // The window is the palette's width.
    assert!((ui.viewport().w - palette::WIDTH).abs() < 0.5);
}

/// Each channel of `a` within 1 of `b`'s: blending rounds once per term.
fn near(a: u32, b: u32) -> bool {
    [16, 8, 0]
        .iter()
        .all(|s| ((a >> s) & 0xFF).abs_diff((b >> s) & 0xFF) <= 1)
}

fn px(c: Color) -> u32 {
    0xFF00_0000 | u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
}

#[test]
fn the_page_dims_under_the_scrim_except_in_high_contrast() {
    let sys = SysColors {
        window: Color::hex(0x00_0000),
        window_text: Color::hex(0xFF_FFFF),
        highlight: Color::hex(0x1A_EBFF),
        highlight_text: Color::hex(0x00_0000),
        hot_light: Color::hex(0xFF_FF00),
        gray_text: Color::hex(0x3F_F23F),
        btn_face: Color::hex(0x20_2040),
        btn_text: Color::hex(0xFF_FFFE),
    };
    for (theme, dims) in [
        (Theme::LIGHT, true),
        (Theme::DARK, true),
        (Theme::high_contrast(sys), false),
    ] {
        let mut run = Run::painted(Model::new(), theme);
        let before = run.page_pixel();
        run.step(Step::Update(Msg::PaletteOpen));
        assert!(run.ui.scrim(), "the host turns it on");
        let after = run.page_pixel();
        assert_eq!(after != before, dims, "{:?}", theme.scheme());
        if dims {
            let smoke = theme.color(Tone::Smoke).over(theme.color(Tone::App));
            assert!(near(after, px(smoke)), "{after:08x} {:?}", theme.scheme());
        }
        run.step(popup_key(KeyCode::Escape));
        assert!(!run.ui.scrim());
        assert_eq!(run.page_pixel(), before, "and back");
    }
}
