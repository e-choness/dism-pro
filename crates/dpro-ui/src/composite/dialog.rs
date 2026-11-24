//! The dialog frame (TD-0009 A3.5 (13)): a title, a body that scrolls, and up to three buttons at
//! its bottom end, with no default button. In a [`PopupKind::Dialog`](crate::PopupKind) popup
//! over the owner, or as the root of a window of its own.
//!
//! A client sees a window that is a dialog (addition 12), named by the title and described, whose
//! title is a heading at level 2. Focus starts on `initial` ([`Ui::focus_first`](crate::Ui)); Tab
//! moves through the body, then the footer; Enter activates only the focused button. The footer
//! is outside the body's viewport, so it stays in view at any text size: the frame grows to fill
//! what its parent gives it, and the body scrolls in what the title and footer leave.

use crate::{
    Axes, Axis, Cx, Element, Insets, Justify, Key, Look, NodeId, Radius, Size, Style, TextStyle,
    Tone,
    tokens::{SPACE, dialog, font},
};

use super::{Emphasis, button_look};

/// The title, a heading.
pub const DIALOG_TITLE: Key = Key::name("dpro-ui.dialog.title");
/// The body's vertical viewport.
pub const DIALOG_BODY: Key = Key::name("dpro-ui.dialog.body");
/// The footer's row of buttons, outside the body's viewport.
pub const DIALOG_FOOTER: Key = Key::name("dpro-ui.dialog.footer");

/// A dialog frame, as a view describes it for [`Cx::dialog_frame`].
#[derive(Clone, Debug)]
pub struct DialogFrame<'s, M> {
    /// It names the dialog.
    pub title: &'s str,
    /// What a screen reader reads with the title; `None` for none.
    pub description: Option<&'s str>,
    /// At most three buttons, in order: their text, message and emphasis.
    pub buttons: &'s [(&'s str, M, Emphasis)],
    /// The button focus starts on (A8's starts on Deny).
    pub initial: u8,
    /// What Esc and the window's close send: the caller passes it as its popup's
    /// [`on_close`](crate::Popup::on_close), or as its window's close.
    pub cancel: M,
}

impl<M: Clone> Cx<'_, M> {
    /// A dialog frame around `body`: the title, the body in a vertical viewport, and the footer's
    /// buttons (at most three), focus starting on the `initial` one.
    pub fn dialog_frame(
        &mut self,
        key: Key,
        d: &DialogFrame<'_, M>,
        body: impl FnOnce(&mut Cx<'_, M>),
    ) -> NodeId {
        let mut el = Element::dialog(key, d.title)
            .style(Style {
                gap: SPACE[3],
                padding: Insets::all(dialog::PADDING),
                grow: 1.0,
                ..Style::default()
            })
            // A transient surface's border: 2 px of `WindowText` in high contrast (A4.4).
            .look(Look {
                fill: Some(Tone::Surface),
                border: Some(Tone::TransientBorder),
                radius: Radius::Drawer,
                ink: Tone::Text,
            });
        if let Some(text) = d.description {
            el = el.describe(text);
        }
        let mut initial = None;
        let id = self.add_with(el, |cx| {
            cx.add(
                Element::text(DIALOG_TITLE, d.title)
                    .text_style(TITLE)
                    .heading(2),
            );
            cx.add_with(
                Element::scroll(DIALOG_BODY, Axes::Y).style(Style {
                    grow: 1.0,
                    ..Style::default()
                }),
                body,
            );
            cx.add_with(
                Element::stack(DIALOG_FOOTER).style(Style {
                    axis: Axis::Row,
                    gap: SPACE[2],
                    justify: Justify::End,
                    // With the frame's gap, the footer's 24 px above it.
                    padding: Insets {
                        top: dialog::FOOTER_GAP - SPACE[3],
                        ..Insets::default()
                    },
                    ..Style::default()
                }),
                |cx| {
                    for (i, (label, msg, emphasis)) in (0..).zip(d.buttons.iter().take(3)) {
                        let b = cx.add(
                            Element::button(Key::index(i), label, msg.clone())
                                .style(Style {
                                    min: Size {
                                        w: 0.0,
                                        h: dialog::BUTTON_H,
                                    },
                                    padding: Insets::xy(SPACE[3], 0.0),
                                    ..Style::default()
                                })
                                .look(button_look(*emphasis)),
                        );
                        if i == u64::from(d.initial) {
                            initial = Some(b);
                        }
                    }
                },
            );
        });
        self.ui.initial = initial;
        id
    }
}

/// The title: subtitle text, semibold.
const TITLE: TextStyle = TextStyle {
    size: font::SUBTITLE,
    weight: font::SEMIBOLD,
    mono: false,
    wrap: true,
};
