//! The batch selection bar (TD-0009 A3.5 (11)): while a table has checked rows, how many, how many
//! of them the filter hides, the page's note, its actions and Clear selection.
//!
//! It's a toolbar (A3.5 addition 8): one Tab stop after the table, Left and Right between its
//! buttons, named so a page can show a second one. It never applies: an action opens the drawer,
//! with a hold for danger (§5.2). The count isn't live, because each check box's toggle is already
//! announced.
//!
//! The bar is the inverse of the page (`InverseFill`, `InverseText`), its primary action
//! `InverseAccent`. The count sits in a slot [`batch::COUNT_W`] wide at 100 % text, so a count
//! that changes inside it damages the count alone, and the text beside it grows into the room the
//! buttons leave, so the buttons never move. Appearing or leaving reflows the table's column
//! once.

use crate::{
    Align, Axis, Cx, Dim, Element, Icon, Insets, Justify, Key, Look, NodeId, Radius, Size, Style,
    TextStyle, Tone,
    tokens::{SPACE, batch, font},
};

use super::{Emphasis, STRONG};

/// The count: `ui-batch-selected`.
pub const BATCH_COUNT: Key = Key::name("dpro-ui.batch.count");
/// `ui-batch-hidden`, when the filter hides some checked rows.
pub const BATCH_HIDDEN: Key = Key::name("dpro-ui.batch.hidden");
/// The page's note.
pub const BATCH_NOTE: Key = Key::name("dpro-ui.batch.note");
/// Clear selection, a glyph button at the end.
pub const BATCH_CLEAR: Key = Key::name("dpro-ui.batch.clear");

/// The count, the hidden count and the note, wrapping when they don't fit.
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
    /// The page's actions, at most three, each [`Emphasis::Primary`] or [`Emphasis::Normal`].
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
        debug_assert!(
            b.actions.len() <= MAX_ACTIONS,
            "a batch bar shows at most {MAX_ACTIONS} actions"
        );
        // The count's slot holds as many characters at any text size.
        let slot = batch::COUNT_W * self.ui.text_scale();
        Some(
            self.add_with(
                // The buttons go under the text when the slot and they don't fit on one line (a
                // narrow window at a large text size), so nothing overlaps. What decides it is
                // the slot, not the count, so a count change never moves them.
                Element::toolbar(key)
                    .style(Style {
                        axis: Axis::Row,
                        gap: SPACE[1],
                        align: Align::Center,
                        padding: Insets::xy(batch::PAD_X, batch::PAD_Y),
                        wrap: true,
                        ..Style::default()
                    })
                    .look(BAR)
                    .name(b.name),
                |cx| {
                    cx.add_with(
                        Element::stack(TEXT).style(Style {
                            axis: Axis::Row,
                            gap: SPACE[2],
                            align: Align::Center,
                            grow: 1.0,
                            min: Size { w: slot, h: 0.0 },
                            wrap: true,
                            ..Style::default()
                        }),
                        |cx| {
                            cx.add(
                                Element::text(BATCH_COUNT, b.selected)
                                    .text_style(STRONG)
                                    .style(Style {
                                        min: Size { w: slot, h: 0.0 },
                                        ..Style::default()
                                    })
                                    .look(INK),
                            );
                            if b.hidden > 0 {
                                cx.add(Element::text(BATCH_HIDDEN, b.hidden_text).look(INK));
                            }
                            if !b.note.is_empty() {
                                cx.add(
                                    Element::text(BATCH_NOTE, b.note)
                                        .text_style(PROSE)
                                        .look(INK),
                                );
                            }
                        },
                    );
                    for (i, (label, msg, emphasis)) in (0..).zip(b.actions.iter().take(MAX_ACTIONS))
                    {
                        let (look, text_style) = match emphasis {
                            Emphasis::Primary => (PRIMARY, STRONG),
                            Emphasis::Normal => (QUIET, TextStyle::default()),
                        };
                        cx.add(
                            Element::button(Key::index(i), label, msg.clone())
                                .style(BUTTON)
                                .text_style(text_style)
                                .look(look),
                        );
                    }
                    let (name, clear) = &b.clear;
                    cx.add_with(
                        // Named explicitly, so its glyph is presentational (A3.5 addition 9).
                        Element::button(BATCH_CLEAR, "", clear.clone())
                            .name(name)
                            .style(Style {
                                padding: Insets::default(),
                                align: Align::Center,
                                min: Size {
                                    w: batch::BUTTON_H,
                                    h: batch::BUTTON_H,
                                },
                                ..BUTTON
                            })
                            .look(QUIET),
                        |cx| {
                            cx.add(Element::glyph(CLEAR_GLYPH, Icon::Cancel).look(INK));
                        },
                    );
                },
            ),
        )
    }
}

/// The most actions a bar shows (A3.5 (11)).
const MAX_ACTIONS: usize = 3;

/// The bar: the inverse fill, rounded as a card. Its border is the fill's own color, so it
/// doesn't show, except in high contrast, where the fill is the window and every border
/// `WindowText` (A3.5 (11)).
const BAR: Look = Look {
    fill: Some(Tone::InverseFill),
    border: Some(Tone::InverseFill),
    radius: Radius::Card,
    ink: Tone::InverseText,
};

/// Text and glyphs on the bar.
const INK: Look = Look {
    ink: Tone::InverseText,
    ..Look::PLAIN
};

/// The primary action: the inverse accent, its text on it. Semibold.
const PRIMARY: Look = Look {
    fill: Some(Tone::InverseAccent),
    border: None,
    radius: Radius::Control,
    ink: Tone::OnInverseAccent,
};

/// Any other action, and Clear selection: only its text or glyph until hovered or pressed, whose
/// layer is the bar's ink (`Theme::layer`).
const QUIET: Look = Look {
    fill: None,
    border: None,
    radius: Radius::Control,
    ink: Tone::InverseText,
};

/// The note wraps when the bar is narrow.
const PROSE: TextStyle = TextStyle {
    size: font::BODY,
    weight: font::REGULAR,
    mono: false,
    wrap: true,
};

/// A button: at least [`batch::BUTTON_H`] high, its text [`batch::PAD_X`] from its sides.
const BUTTON: Style = Style {
    axis: Axis::Column,
    gap: 0.0,
    padding: Insets::xy(batch::PAD_X, 0.0),
    align: Align::Stretch,
    justify: Justify::Center,
    grow: 0.0,
    width: Dim::Auto,
    height: Dim::Auto,
    min: Size {
        w: 0.0,
        h: batch::BUTTON_H,
    },
    max: Size::INFINITE,
    wrap: false,
};
