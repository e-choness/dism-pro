//! The batch selection bar (TD-0009 A3.5 (11)): while a table has checked rows, how many, how many
//! of them the filter hides, the page's note, its actions and Clear selection.
//!
//! It's a toolbar (A3.5 addition 8): one Tab stop after the table, Left and Right between its
//! buttons. It never applies: an action opens the drawer, with a hold for danger (§5.2). The count
//! isn't live, because each check box's toggle is already announced.

use crate::{
    Align, Axis, Cx, Dim, Element, Icon, Insets, Justify, Key, Look, NodeId, Radius, Size, Style,
    Tone, tokens::SPACE,
};

use super::{Emphasis, button_look};

/// The count: `ui-batch-selected`.
pub const BATCH_COUNT: Key = Key::name("dpro-ui.batch.count");
/// `ui-batch-hidden`, when the filter hides some checked rows.
pub const BATCH_HIDDEN: Key = Key::name("dpro-ui.batch.hidden");
/// The page's note.
pub const BATCH_NOTE: Key = Key::name("dpro-ui.batch.note");
/// Clear selection, a glyph button at the end.
pub const BATCH_CLEAR: Key = Key::name("dpro-ui.batch.clear");

const TEXT: Key = Key::name("dpro-ui.batch.text");
const CLEAR_GLYPH: Key = Key::name("dpro-ui.batch.clear.glyph");

/// A batch bar, as a view describes it for [`Cx::batch_bar`].
#[derive(Clone, Debug)]
pub struct BatchBar<'s, M> {
    /// The checked rows; the bar shows only while it's above 0.
    pub count: u32,
    /// The checked rows the filter hides (`TableModel::hidden_selected`).
    pub hidden: u32,
    /// `ui-batch-selected`, resolved with `count`.
    pub selected: &'s str,
    /// `ui-batch-hidden`, resolved with `hidden`; shown while `hidden > 0`.
    pub hidden_text: &'s str,
    /// The page's sentence ("uninstalls run one at a time"); empty for none.
    pub note: &'s str,
    /// At most three.
    pub actions: &'s [(&'s str, M, Emphasis)],
    /// Clear selection's name (`ui-batch-clear`) and message.
    pub clear: (&'s str, M),
    /// `ui-batch-region`: the toolbar's name, since a page can show a second toolbar.
    pub name: &'s str,
}

impl<M: Clone> Cx<'_, M> {
    /// The batch bar, or nothing while `count` is 0. A count change damages the count alone; the
    /// buttons keep their place.
    pub fn batch_bar(&mut self, key: Key, b: &BatchBar<'_, M>) -> Option<NodeId> {
        if b.count == 0 {
            return None;
        }
        Some(
            self.add_with(
                Element::toolbar(key)
                    .style(Style {
                        axis: Axis::Row,
                        gap: SPACE[2],
                        align: Align::Center,
                        padding: Insets::xy(PAD_X, PAD_Y),
                        ..Style::default()
                    })
                    .look(Look {
                        fill: Some(Tone::Text),
                        border: None,
                        radius: Radius::Card,
                        ink: Tone::Surface,
                    })
                    .name(b.name),
                |cx| {
                    cx.add_with(
                        Element::stack(TEXT).style(Style {
                            axis: Axis::Row,
                            gap: SPACE[1],
                            grow: 1.0,
                            align: Align::Center,
                            ..Style::default()
                        }),
                        |cx| {
                            cx.add(Element::text(BATCH_COUNT, b.selected));
                            if b.hidden > 0 {
                                cx.add(Element::text(BATCH_HIDDEN, b.hidden_text));
                            }
                            if !b.note.is_empty() {
                                cx.add(Element::text(BATCH_NOTE, b.note));
                            }
                        },
                    );
                    for (i, (label, msg, emphasis)) in (0..).zip(b.actions.iter().take(MAX_ACTIONS))
                    {
                        cx.add(
                            Element::button(Key::index(i), label, msg.clone())
                                .style(BUTTON)
                                .look(button_look(*emphasis)),
                        );
                    }
                    let (name, clear) = &b.clear;
                    cx.add_with(
                        // Named explicitly, so its glyph is presentational (A3.5 addition 9).
                        Element::button(BATCH_CLEAR, "", clear.clone())
                            .name(name)
                            .style(Style {
                                padding: Insets::default(),
                                min: Size {
                                    w: BUTTON_H,
                                    h: BUTTON_H,
                                },
                                ..BUTTON
                            })
                            .look(Look::SUBTLE),
                        |cx| {
                            cx.add(Element::glyph(CLEAR_GLYPH, Icon::Cancel));
                        },
                    );
                },
            ),
        )
    }
}

/// The most actions a bar shows (A3.5 (11)).
const MAX_ACTIONS: usize = 3;
/// Padding 10 × 14 and 34 px buttons (A3.5 (11)), taller when the text size needs it.
const PAD_X: f32 = 14.0;
const PAD_Y: f32 = 10.0;
const BUTTON_H: f32 = 34.0;
const BUTTON: Style = Style {
    axis: Axis::Column,
    gap: 0.0,
    padding: Insets::xy(PAD_X, 0.0),
    align: Align::Stretch,
    justify: Justify::Center,
    grow: 0.0,
    width: Dim::Auto,
    height: Dim::Auto,
    min: Size {
        w: 0.0,
        h: BUTTON_H,
    },
    max: Size::INFINITE,
    wrap: false,
};
