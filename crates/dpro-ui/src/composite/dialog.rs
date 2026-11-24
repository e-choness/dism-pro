//! The dialog frame (TD-0009 A3.5 (13)): a title, a body that scrolls, and up to three buttons at
//! its bottom end, with no default button. In a [`PopupKind::Dialog`](crate::PopupKind) popup
//! over the owner, or as the root of a window of its own.
//!
//! The MEASURE commit's stub: the API, and the dialog holding its body alone.

use crate::{Cx, Element, Key, NodeId};

use super::Emphasis;

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
    /// What Esc and the window's close send.
    pub cancel: M,
}

impl<M: Clone> Cx<'_, M> {
    /// A dialog frame around `body`.
    pub fn dialog_frame(
        &mut self,
        key: Key,
        d: &DialogFrame<'_, M>,
        body: impl FnOnce(&mut Cx<'_, M>),
    ) -> NodeId {
        self.add_with(Element::dialog(key, d.title), |cx| {
            cx.add_with(Element::stack(DIALOG_BODY), body);
        })
    }
}
