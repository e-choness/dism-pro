//! The in-app path picker (TD-0009 A3.5 (12)): a folder's rows in a sheet over the page, where
//! Windows has no file dialog (`WinPE`) or the dialog failed.
//!
//! The picker is the [drawer frame](crate::Cx::drawer) in its sheet mode, a
//! [scope](crate::Element::scope) named by its title: Esc closes it, and focus goes back to what
//! opened it. Its body doesn't scroll (the table scrolls itself), and opening it moves focus to
//! the path field. The body holds:
//! - **the path row**: Up (a glyph button; also Alt+Up anywhere in the picker, and Backspace in
//!   the table), the drive list (a combo box the model fills from
//!   `dpro_win::gui::file_dialog::drives`) and the path field, where a pasted path, a `\\?\` path
//!   or a UNC one goes on Enter;
//! - **the error panel**, when the folder couldn't be listed or the typed path is wrong
//!   (`ui-picker-not-found`, `ui-picker-not-folder`, `ui-picker-refused`, ...);
//! - **the rows**: an A2 [table](crate::Cx::table) of Name and Date modified, folders first, which
//!   scrolls itself; its status line says `ui-loading` while the first batch hasn't come, and
//!   `ui-picker-empty-folders` or `ui-picker-empty-files` for an empty folder. Enter on a folder
//!   opens it.
//!
//! The footer is the drawer's: Cancel, and Select (`ui-picker-select-folder` or
//! `ui-picker-select-file`), disabled while there's nothing to choose.
//!
//! The model is `dpro_app::picker::FolderModel`, and its listing runs off the UI thread: the
//! picker answers keys, Up and Cancel while a folder that never answers is still listing (A3.8).
//!
//! A client sees a region landmark (UI Automation's Group) named by the title, which is a level-2
//! heading; Close; the path field (an edit with a value, named `ui-picker-path`); Up, a button
//! named `ui-picker-up`, its glyph presentational; the drive combo box (`ui-picker-drives`); the
//! grid; Cancel and Select.

use crate::{
    Align, Axis, Cells, Cx, Element, Icon, Justify, Key, Look, NodeId, Row, Size, Style, Table,
    TextEdit,
    composite::{
        DRAWER_CANCEL, DRAWER_CLOSE, DRAWER_FOOTER, DRAWER_PRIMARY, DRAWER_TITLE, DrawerFooter,
        DrawerFrame, DrawerPrimary, ErrorPanel,
    },
    tokens::{SPACE, picker},
    tree::NIL,
};

use super::drawer::Body;

/// The title, a level-2 heading: the drawer frame's.
pub const PICKER_TITLE: Key = DRAWER_TITLE;
/// Close, in the header's corner: the drawer frame's.
pub const PICKER_CLOSE: Key = DRAWER_CLOSE;
/// The path row: Up, the drive list and the path field.
pub const PICKER_PATH_ROW: Key = Key::name("dpro-ui.picker.path-row");
/// Up one level, a glyph button.
pub const PICKER_UP: Key = Key::name("dpro-ui.picker.up");
/// The drive list, a combo box.
pub const PICKER_DRIVES: Key = Key::name("dpro-ui.picker.drives");
/// The path field.
pub const PICKER_PATH: Key = Key::name("dpro-ui.picker.path");
/// The error panel, when there's one.
pub const PICKER_ERROR: Key = Key::name("dpro-ui.picker.error");
/// The rows' table.
pub const PICKER_TABLE: Key = Key::name("dpro-ui.picker.table");
/// The footer: the drawer frame's.
pub const PICKER_FOOTER: Key = DRAWER_FOOTER;
/// Cancel, in the footer: the drawer frame's.
pub const PICKER_CANCEL: Key = DRAWER_CANCEL;
/// Select, at the footer's end: the drawer frame's primary action.
pub const PICKER_SELECT: Key = DRAWER_PRIMARY;

const UP_GLYPH: Key = Key::name("dpro-ui.picker.up.glyph");

/// The in-app picker, as a view describes it for [`Cx::path_picker`].
#[derive(Clone, Debug)]
pub struct PathPicker<'s, M> {
    /// `ui-picker-folder-title` or `ui-picker-file-title`: it names the sheet, and heads it.
    pub title: &'s str,
    /// What Close, Cancel and Esc send.
    pub close: fn() -> M,
    /// Close's name: `ui-drawer-close`.
    pub close_label: &'s str,
    /// The path field's name (`ui-picker-path`), its text, and its edits.
    pub path: (&'s str, &'s str, fn(TextEdit) -> M),
    /// What Enter in the path field sends: go where it says.
    pub go: M,
    /// Up's name (`ui-picker-up`), and what it, Alt+Up and Backspace in the table send; `None`
    /// at a root, where it's disabled and those keys go on.
    pub up: (&'s str, Option<M>),
    /// The drive list's name (`ui-picker-drives`), the drive shown (`C:\`), whether its list is
    /// open, and what opens or closes it. The list itself is the model's popup.
    pub drives: (&'s str, &'s str, bool, M),
    /// Why the folder couldn't be listed, or the typed path is wrong.
    pub error: Option<ErrorPanel<'s, M>>,
    /// The rows: Name and Date modified, folders first; the status line while there are none.
    pub table: Table<'s, M>,
    /// Cancel's label (`ui-drawer-cancel`).
    pub cancel: &'s str,
    /// Select's label (`ui-picker-select-folder` or `ui-picker-select-file`) and message; `None`
    /// while there's nothing to choose.
    pub select: (&'s str, Option<M>),
    /// The landmark's kind, for a client: `a11y-landmark-region`, resolved.
    pub region: &'s str,
}

impl<M: Clone> Cx<'_, M> {
    /// The in-app path picker: the drawer frame as a sheet that fills its parent (no narrower
    /// than [`drawer::MIN_W`](crate::tokens::drawer::MIN_W)), sliding in as the drawer does.
    /// `rows(index, cells)` gives the row at an index in the view's order, as [`Cx::table`]'s
    /// does.
    ///
    /// Opening it moves focus to the path field. Esc, Close and Cancel send
    /// [`close`](PathPicker::close), and when the sheet goes, focus goes back to what opened it,
    /// found again by its keys though the page built it anew (the scope's
    /// [`FocusMemo`](crate::FocusMemo), A4.4). Opening a folder empties the rows while it
    /// loads: focus waits on the rows' viewport and moves onto the first row as it comes.
    /// Alt+Up anywhere in it, and Backspace on its rows or the empty viewport, press Up;
    /// Backspace in the path field edits the path.
    pub fn path_picker<'r>(
        &mut self,
        key: Key,
        p: &PathPicker<'_, M>,
        rows: impl FnMut(u32, &mut Cells<'_>) -> Row<'r>,
    ) -> NodeId {
        let (select_label, select) = (p.select.0, &p.select.1);
        let frame = DrawerFrame {
            title: p.title,
            summary: None,
            badges: &[],
            close: p.close,
            close_label: p.close_label,
            footer: DrawerFooter {
                cancel: (p.cancel, (p.close)()),
                // A disabled button still needs a message; it never sends it.
                primary: DrawerPrimary::Apply(
                    select_label,
                    select.clone().unwrap_or_else(|| (p.close)()),
                ),
                // Nothing to choose yet: the footer's disabled primary.
                applying: select.is_none(),
            },
            sheet: true,
            region: p.region,
        };
        let mut up = NIL;
        let id = self.drawer_with(key, &frame, Body::Fill, |cx| {
            let (up_node, path) = path_row(cx, p);
            up = up_node;
            if let Some(error) = &p.error {
                cx.error_panel(PICKER_ERROR, error);
            }
            cx.table(
                Element::table(PICKER_TABLE).style(Style {
                    grow: 1.0,
                    ..Style::default()
                }),
                &p.table,
                rows,
            );
            path
        });
        self.ui.set_scope_up(id.index, up);
        id
    }
}

/// Up, the drive list and the path field; returns Up's node and the field's.
fn path_row<M: Clone>(cx: &mut Cx<'_, M>, p: &PathPicker<'_, M>) -> (u32, u32) {
    let (mut up_node, mut path_node) = (NIL, NIL);
    cx.add_with(
        Element::stack(PICKER_PATH_ROW).style(Style {
            axis: Axis::Row,
            gap: SPACE[1],
            align: Align::Center,
            ..Style::default()
        }),
        |cx| {
            let (up_label, up) = &p.up;
            // A disabled button still needs a message; it never sends it.
            let up_msg = up.clone().unwrap_or_else(|| (p.close)());
            up_node = cx
                .add_with(
                    // Named explicitly, so its glyph is presentational (A3.5 addition 9).
                    Element::button(PICKER_UP, "", up_msg)
                        .name(up_label)
                        .enabled(up.is_some())
                        .style(Style {
                            align: Align::Center,
                            justify: Justify::Center,
                            min: Size {
                                w: picker::CONTROL_H,
                                h: picker::CONTROL_H,
                            },
                            ..Style::default()
                        })
                        .look(Look::SUBTLE),
                    |cx| {
                        cx.add(Element::glyph(UP_GLYPH, Icon::Up));
                    },
                )
                .index;
            let (drives_label, drive, open, msg) = &p.drives;
            cx.add(
                Element::combo(PICKER_DRIVES, drives_label, drive, *open, msg.clone()).style(
                    Style {
                        min: Size {
                            w: picker::DRIVES_W,
                            h: picker::CONTROL_H,
                        },
                        ..Style::default()
                    },
                ),
            );
            let (path_label, text, on_edit) = p.path;
            path_node = cx
                .add(
                    Element::field(PICKER_PATH, text, on_edit)
                        .name(path_label)
                        .submit(p.go.clone())
                        .style(Style {
                            grow: 1.0,
                            min: Size {
                                w: 0.0,
                                h: picker::CONTROL_H,
                            },
                            ..Style::default()
                        }),
                )
                .index;
        },
    );
    (up_node, path_node)
}
