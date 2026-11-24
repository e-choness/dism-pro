//! The dialog frame (TD-0009 A3.5 (13), A3.9), in a dialog popup over the composites page: focus
//! starts on `initial`; Esc sends `cancel` (the popup's `on_close`); Enter activates only the
//! focused button, as there's no default button; the footer stays visible at 225 % text, its body
//! scrolling; and a client sees a Window/IsDialog named by the title and described, whose
//! appearing raises `WindowOpened`.

#![expect(
    clippy::expect_used,
    reason = "test helpers: a node that isn't there fails the test"
)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;

use composites::{DIALOG, DIALOG_BUTTONS, Model, Msg, Run, Window, popup_key};
use dpro_ui::{
    Cx, FakeText, Key, KeyCode, Motion, Size, Ui,
    accesskit::Role as AkRole,
    composite::{DIALOG_BODY, DIALOG_FOOTER, DIALOG_TITLE, DialogFrame, Emphasis},
    tokens::dialog,
};

fn button(run: &Run, i: u64) -> dpro_ui::NodeId {
    let footer = run
        .part_in(Window::Popup, DIALOG_FOOTER)
        .expect("the footer");
    let ui = run.ui_of(Window::Popup);
    ui.children(footer)
        .find(|&c| ui.info(c).is_some_and(|n| n.key == Key::index(i)))
        .expect("the button")
}

#[test]
#[ignore = "slice 6c: lands with the code"]
fn focus_starts_on_the_initial_button() {
    let run = Run::with(Model::new()).dialog_open();
    assert_eq!(run.ui_of(Window::Popup).focus(), Some(button(&run, 0)));
    // A frame alone, starting on its second button (A8 starts on Deny).
    let mut ui = alone(1, Size { w: 440.0, h: 300.0 });
    ui.focus_first(true);
    let second = ui.find(&[Key::name("d")]).and_then(|_| {
        let footer = part(&ui, DIALOG_FOOTER)?;
        ui.children(footer).nth(1)
    });
    assert_eq!(ui.focus(), second);
}

#[test]
fn esc_sends_cancel() {
    let mut run = Run::with(Model::new()).dialog_open();
    assert_eq!(run.win.popup_open, Some(dpro_ui::PopupKind::Dialog));
    run.step(popup_key(KeyCode::Escape));
    assert_eq!(run.model.last, Some(Msg::DialogCancel));
    assert_eq!(run.win.popup_open, None);
    assert_eq!(run.model.over.ran, None);
}

#[test]
#[ignore = "slice 6c: lands with the code"]
fn enter_activates_only_the_focused_button() {
    let mut run = Run::with(Model::new()).dialog_open();
    // Focus on the body's text, nothing: no default button takes Enter.
    run.win.popup.set_focus(None);
    run.step(popup_key(KeyCode::Enter));
    assert!(run.model.over.dialog, "a stray Enter confirms nothing");
    run.win.popup.focus_first(true);
    run.step(popup_key(KeyCode::Tab));
    assert_eq!(run.ui_of(Window::Popup).focus(), Some(button(&run, 1)));
    run.step(popup_key(KeyCode::Enter));
    assert_eq!(run.model.over.ran, Some(Msg::DialogAnswer(1)));
    assert!(!run.model.over.dialog);
}

#[test]
#[ignore = "slice 6c: lands with the code"]
fn the_footer_stays_visible_at_225_percent() {
    // At 225 % in a short window, the body scrolls and the footer stays inside the window.
    let mut ui = alone(0, Size { w: 440.0, h: 260.0 });
    ui.set_text_scale(2.25);
    ui.layout(&mut FakeText);
    let footer = part(&ui, DIALOG_FOOTER).expect("the footer");
    let rect = ui.info(footer).expect("the footer").rect;
    assert!(
        rect.y + rect.h <= 260.0 + 0.5,
        "the footer at {rect:?} leaves the window"
    );
    let body = part(&ui, DIALOG_BODY).expect("the body");
    let body = ui.info(body).expect("the body");
    let scroll = body.scroll.expect("a viewport");
    assert!(scroll.content.h > body.rect.h, "the body scrolls");
    for b in ui.children(footer) {
        let r = ui.info(b).expect("a button").rect;
        assert!(r.h >= dialog::BUTTON_H - 0.5, "{r:?}");
    }
}

#[test]
fn a_client_sees_a_dialog_named_by_its_title() {
    let mut run = Run::accessible(Model::new()).dialog_open();
    let tree = run.a11y_tree_of(Window::Popup);
    let id = run.part_in(Window::Popup, DIALOG).expect("the dialog");
    let node = run.ak_in(Window::Popup, &tree, id).expect("listed");
    assert_eq!(node.role(), AkRole::Dialog, "UIA Window, IsDialog");
    assert_eq!(node.label(), Some("A job is still running"));
}

#[test]
#[ignore = "slice 6c: lands with the code"]
fn it_is_described_and_its_title_is_a_heading() {
    let mut run = Run::accessible(Model::new()).dialog_open();
    let tree = run.a11y_tree_of(Window::Popup);
    let id = run.part_in(Window::Popup, DIALOG).expect("the dialog");
    let node = run.ak_in(Window::Popup, &tree, id).expect("listed");
    assert_eq!(node.description(), Some("Cleanup is 40 % done."));
    let title = run.part_in(Window::Popup, DIALOG_TITLE).expect("the title");
    let title = run.ak_in(Window::Popup, &tree, title).expect("listed");
    assert_eq!(title.level(), Some(2));
    // Its buttons, in order, with the danger one last.
    let labels: Vec<&str> = (0..3)
        .map(|i| {
            run.ui_of(Window::Popup)
                .info(button(&run, i))
                .map_or("", |n| n.text)
        })
        .collect();
    assert_eq!(
        labels,
        DIALOG_BUTTONS.iter().map(|b| b.0).collect::<Vec<_>>()
    );
}

/// A dialog frame alone, in a window `size`, starting on button `initial`, with a long body.
fn alone(initial: u8, size: Size) -> Ui<u8> {
    let mut ui = Ui::new(size, 1.0, Motion::default());
    let buttons = [
        ("Deny", 1, Emphasis::Normal),
        ("Allow once", 2, Emphasis::Primary),
    ];
    let view = |cx: &mut Cx<'_, u8>| {
        cx.add_with(
            dpro_ui::Element::stack(Key::name("d")).style(dpro_ui::Style {
                grow: 1.0,
                ..dpro_ui::Style::default()
            }),
            |cx| {
                cx.dialog_frame(
                    Key::name("frame"),
                    &DialogFrame {
                        title: "Allow this agent to clean?",
                        description: None,
                        buttons: &buttons,
                        initial,
                        cancel: 0,
                    },
                    |cx| {
                        for i in 0..6 {
                            cx.add(
                                dpro_ui::Element::text(
                                    Key::index(i),
                                    "It asks to delete 1.2 GB of temporary files on C:.",
                                )
                                .text_style(dpro_ui::TextStyle {
                                    wrap: true,
                                    ..dpro_ui::TextStyle::default()
                                }),
                            );
                        }
                    },
                );
            },
        );
    };
    ui.build(view);
    ui.layout(&mut FakeText);
    ui
}

/// The first node keyed `key` in `ui`, breadth first.
fn part(ui: &Ui<u8>, key: Key) -> Option<dpro_ui::NodeId> {
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
