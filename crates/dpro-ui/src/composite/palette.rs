//! The command palette's view (TD-0009 A3.5 (3)): a dialog with a search field, the results it
//! drives, a status line and the keys' hints, built in a [`PopupKind::Palette`](crate::PopupKind)
//! popup's view.
//!
//! The MEASURE commit's stub: the API, and a dialog holding the field alone.

use crate::{Cx, Element, Key, NodeId, TextEdit};

use super::BadgeSpec;

/// The search field.
pub const PALETTE_FIELD: Key = Key::name("dpro-ui.palette.field");
/// The results: a list of suggestions the field drives, in a vertical viewport.
pub const PALETTE_LIST: Key = Key::name("dpro-ui.palette.list");
/// The status line, a polite live region: "No commands match …".
pub const PALETTE_STATUS: Key = Key::name("dpro-ui.palette.status");

/// One result of the palette, as the app maps a ranked entry to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteRow<'s> {
    /// The entry's id: the row's key, so a result that stays keeps its node.
    pub entry: u32,
    /// The command's name, which names the option.
    pub label: &'s str,
    /// Where it leads or what it does, after the label in secondary text; empty for none.
    pub detail: &'s str,
    /// Its keys ("Ctrl+Shift+P"), at the end in mono; empty for none.
    pub accel: &'s str,
    pub badge: Option<BadgeSpec<'s>>,
    /// A disabled result can be highlighted; Enter does nothing, and `reason` says why.
    pub enabled: bool,
    /// Why it's disabled (`ui-busy-target`), as its detail and description.
    pub reason: &'s str,
}

/// The palette, as a view describes it for [`Cx::palette`].
#[derive(Clone, Debug)]
pub struct PaletteView<'s, M> {
    /// The dialog's name (`ui-palette-name`).
    pub name: &'s str,
    /// The field's name (`ui-palette-field`) and what it shows while empty
    /// (`ui-palette-placeholder`).
    pub field: &'s str,
    pub placeholder: &'s str,
    /// The results' name (`ui-palette-results`).
    pub results: &'s str,
    /// The Esc keycap's text (`i18n-key-esc`).
    pub esc: &'s str,
    pub query: &'s str,
    pub on_edit: fn(TextEdit) -> M,
    /// Up (`true`, the result before) and Down (`false`), as a field's step sends them: the
    /// model moves the highlight, wrapping.
    pub on_step: fn(bool) -> M,
    /// Enter: the highlighted result runs, if it's enabled.
    pub on_run: M,
    /// A click on result `i` (its place in `rows`).
    pub on_pick: fn(u32) -> M,
    pub rows: &'s [PaletteRow<'s>],
    /// The first row of each area and its header, in order.
    pub groups: &'s [(u32, &'s str)],
    /// The highlighted row, by its place in `rows`.
    pub highlighted: Option<u32>,
    /// The status line (`ui-palette-none`); empty for none.
    pub status: &'s str,
    /// The keys' hints (`ui-palette-hint-move`, `-open`, `-close`).
    pub hints: [&'s str; 3],
}

impl<M: Clone> Cx<'_, M> {
    /// The command palette.
    pub fn palette(&mut self, key: Key, p: &PaletteView<'_, M>) -> NodeId {
        self.add_with(Element::dialog(key, p.name), |cx| {
            cx.add(
                Element::field(PALETTE_FIELD, p.query, p.on_edit)
                    .name(p.field)
                    .placeholder(p.placeholder)
                    .step(p.on_step)
                    .submit(p.on_run.clone()),
            );
        })
    }
}
