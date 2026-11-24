//! The in-app path picker (TD-0009 A3.5 (12)): a folder's rows in a sheet over the page, where
//! Windows has no file dialog (`WinPE`) or the dialog failed.
//!
//! The picker is a [scope](crate::Element::scope) named by its title, in the drawer frame's sheet
//! mode: Esc closes it, and focus goes back to what opened it. Opening it moves focus to the path
//! field. Under the header:
//! - **the path row**: Up (a glyph button; also Alt+Up anywhere in the picker, and Backspace in
//!   the table), the drive list (a combo box the model fills from
//!   `dpro_win::gui::file_dialog::drives`) and the path field, where a pasted path, a `\\?\` path
//!   or a UNC one goes on Enter;
//! - **the error panel**, when the folder couldn't be listed or the typed path is wrong
//!   (`ui-picker-not-found`, `ui-picker-not-folder`, `ui-picker-refused`, ...);
//! - **the rows**: an A2 [table](crate::Cx::table) of Name and Date modified, folders first, which
//!   scrolls itself; its status line says `ui-loading` while the first batch hasn't come, and
//!   `ui-picker-empty-folders` or `ui-picker-empty-files` for an empty folder. Enter on a folder
//!   opens it;
//! - **the footer**: Cancel, and Select (`ui-picker-select-folder` or `ui-picker-select-file`),
//!   disabled while there's nothing to choose.
//!
//! The model is `dpro_app::picker::FolderModel`, and its listing runs off the UI thread: the
//! picker answers keys, Up and Cancel while a folder that never answers is still listing (A3.8).
//!
//! A client sees a group named by the title, a level-2 heading; the path field (an edit with a
//! value, named `ui-picker-path`); Up, a button named `ui-picker-up`, its glyph presentational;
//! the drive combo box (`ui-picker-drives`); the grid; Cancel and Select.
//!
//! Slice 6e's MEASURE stub: a plain sheet that builds every part, without the drawer frame's
//! header and slide (whose body scrolls: the table scrolls itself), without Alt+Up and Backspace,
//! and without moving focus to the path field on open. The tests that need those are ignored
//! until the code lands.

use crate::{
    Align, Axis, Cells, Cx, Element, Icon, Insets, Key, Landmark, Look, NodeId, Row, Size, Style,
    Table, TextEdit, TextStyle, Tone,
    composite::ErrorPanel,
    tokens::{SPACE, font, picker},
};

use super::STRONG;

/// The title, a level-2 heading.
pub const PICKER_TITLE: Key = Key::name("dpro-ui.picker.title");
/// Close, in the header's corner.
pub const PICKER_CLOSE: Key = Key::name("dpro-ui.picker.close");
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
/// The footer.
pub const PICKER_FOOTER: Key = Key::name("dpro-ui.picker.footer");
/// Cancel, in the footer.
pub const PICKER_CANCEL: Key = Key::name("dpro-ui.picker.cancel");
/// Select, at the footer's end.
pub const PICKER_SELECT: Key = Key::name("dpro-ui.picker.select");

const HEADER: Key = Key::name("dpro-ui.picker.header");
const ERROR_ROW: Key = Key::name("dpro-ui.picker.error-row");
const TABLE_ROW: Key = Key::name("dpro-ui.picker.table-row");
const CLOSE_GLYPH: Key = Key::name("dpro-ui.picker.close.glyph");
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
    /// Up's name (`ui-picker-up`), and what it sends; `None` at a root, where it's disabled.
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
    /// The in-app path picker, a sheet that fills its parent (no narrower than
    /// [`drawer::MIN_W`](crate::tokens::drawer::MIN_W)). `rows(index, cells)` gives the row at
    /// an index in the view's order, as [`Cx::table`]'s does.
    pub fn path_picker<'r>(
        &mut self,
        key: Key,
        p: &PathPicker<'_, M>,
        rows: impl FnMut(u32, &mut Cells<'_>) -> Row<'r>,
    ) -> NodeId {
        self.add_with(
            Element::scope(key)
                .on_escape(p.close)
                .style(Style {
                    grow: 1.0,
                    min: Size {
                        w: crate::tokens::drawer::MIN_W,
                        h: 0.0,
                    },
                    ..Style::default()
                })
                .look(SHEET)
                .name(p.title)
                .landmark(Landmark::Region { kind: p.region }),
            |cx| {
                header(cx, p);
                path_row(cx, p);
                if let Some(error) = &p.error {
                    cx.add_with(
                        Element::stack(ERROR_ROW).style(Style {
                            padding: Insets {
                                left: picker::PAD_X,
                                top: 0.0,
                                right: picker::PAD_X,
                                bottom: picker::GAP,
                            },
                            ..Style::default()
                        }),
                        |cx| {
                            cx.error_panel(PICKER_ERROR, error);
                        },
                    );
                }
                cx.add_with(
                    Element::stack(TABLE_ROW).style(Style {
                        grow: 1.0,
                        padding: Insets::xy(picker::PAD_X, 0.0),
                        ..Style::default()
                    }),
                    |cx| {
                        cx.table(
                            Element::table(PICKER_TABLE).style(Style {
                                grow: 1.0,
                                ..Style::default()
                            }),
                            &p.table,
                            rows,
                        );
                    },
                );
                footer(cx, p);
            },
        )
    }
}

/// The header: the title and Close.
fn header<M: Clone>(cx: &mut Cx<'_, M>, p: &PathPicker<'_, M>) {
    cx.add_with(
        Element::stack(HEADER).style(Style {
            axis: Axis::Row,
            gap: SPACE[2],
            align: Align::Start,
            padding: Insets {
                left: picker::PAD_X,
                top: picker::HEADER_TOP,
                right: picker::PAD_X,
                bottom: picker::GAP,
            },
            ..Style::default()
        }),
        |cx| {
            cx.add(
                Element::text(PICKER_TITLE, p.title)
                    .text_style(TITLE)
                    .style(Style {
                        grow: 1.0,
                        ..Style::default()
                    })
                    .heading(2),
            );
            cx.add_with(
                Element::button(PICKER_CLOSE, "", (p.close)())
                    .name(p.close_label)
                    .style(square(picker::CLOSE))
                    .look(Look::SUBTLE),
                |cx| {
                    cx.add(Element::glyph(CLOSE_GLYPH, Icon::Cancel));
                },
            );
        },
    );
}

/// Up, the drive list and the path field.
fn path_row<M: Clone>(cx: &mut Cx<'_, M>, p: &PathPicker<'_, M>) {
    cx.add_with(
        Element::stack(PICKER_PATH_ROW).style(Style {
            axis: Axis::Row,
            gap: SPACE[1],
            align: Align::Center,
            padding: Insets {
                left: picker::PAD_X,
                top: 0.0,
                right: picker::PAD_X,
                bottom: picker::GAP,
            },
            ..Style::default()
        }),
        |cx| {
            let (up_label, up) = &p.up;
            // A disabled button still needs a message; it never sends it.
            let up_msg = up.clone().unwrap_or_else(|| (p.close)());
            cx.add_with(
                Element::button(PICKER_UP, "", up_msg)
                    .name(up_label)
                    .enabled(up.is_some())
                    .style(square(picker::CONTROL_H))
                    .look(Look::SUBTLE),
                |cx| {
                    cx.add(Element::glyph(UP_GLYPH, Icon::Up));
                },
            );
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
            cx.add(
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
            );
        },
    );
}

/// Cancel, then Select, at the footer's end.
fn footer<M: Clone>(cx: &mut Cx<'_, M>, p: &PathPicker<'_, M>) {
    let button = Style {
        min: Size {
            w: 0.0,
            h: picker::BUTTON_H,
        },
        padding: Insets::xy(SPACE[3], 0.0),
        ..Style::default()
    };
    cx.add_with(
        Element::stack(PICKER_FOOTER).style(Style {
            axis: Axis::Row,
            gap: SPACE[1],
            align: Align::Center,
            justify: crate::Justify::End,
            padding: Insets::xy(picker::PAD_X, picker::FOOTER_PAD_Y),
            wrap: true,
            ..Style::default()
        }),
        |cx| {
            cx.add(
                Element::button(PICKER_CANCEL, p.cancel, (p.close)())
                    .style(button)
                    .look(Look::SECONDARY),
            );
            let (label, select) = &p.select;
            let msg = select.clone().unwrap_or_else(|| (p.close)());
            cx.add(
                Element::button(PICKER_SELECT, label, msg)
                    .enabled(select.is_some())
                    .style(button)
                    .text_style(STRONG)
                    .look(Look::PRIMARY),
            );
        },
    );
}

fn square(side: f32) -> Style {
    Style {
        align: Align::Center,
        justify: crate::Justify::Center,
        min: Size { w: side, h: side },
        ..Style::default()
    }
}

/// The sheet: the surface in a hairline, square, as the drawer's pane is.
const SHEET: Look = Look {
    fill: Some(Tone::Surface),
    border: Some(Tone::TransientBorder),
    radius: crate::Radius::None,
    ink: Tone::Text,
};

/// The title: title text, semibold, wrapping.
const TITLE: TextStyle = TextStyle {
    size: font::TITLE,
    weight: font::SEMIBOLD,
    mono: false,
    wrap: true,
};
