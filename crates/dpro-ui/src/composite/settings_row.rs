//! Settings rows (TD-0009 A3.5 (15)): a label and a description at the start, a control at the end.
//!
//! The control is any existing role: a check box, a radio set, a dropdown, a field (with a step for
//! numbers, A3.5 addition 6) or a button. It's the Tab stop, named by the label and described by the
//! description, so the row's text is presentational (addition 9) and isn't read twice. A radio set
//! is one stop with arrows, as A1's radios are.
//!
//! A group is a level-2 heading over its rows, which share a card (`layer.card` under glass) with
//! hairlines between them. In a window narrower than [`settings::NARROW`] each row puts its
//! control under its text; in a wider one the control wraps under it only where the text would
//! get narrower than [`settings::TEXT_MIN_W`] (at a large text size). A value change damages its
//! control alone.

use crate::{
    Align, Axis, Cx, Dim, Element, Insets, Justify, Key, Look, NodeId, Size, Style, TextStyle,
    Tone,
    tokens::{SPACE, font, settings, stroke},
};

use super::{CARD, SECONDARY, STRONG};

/// A settings group's heading, a level-2 label.
pub const SETTINGS_HEADING: Key = Key::name("dpro-ui.settings.heading");
/// A row's label.
pub const SETTINGS_LABEL: Key = Key::name("dpro-ui.settings.label");
/// A row's description.
pub const SETTINGS_DESCRIPTION: Key = Key::name("dpro-ui.settings.description");

/// A group's rows, on their card.
const ROWS: Key = Key::name("dpro-ui.settings.rows");
/// The hairline above every row but its group's first, keyed by this and the row's key.
const DIVIDER: Key = Key::name("dpro-ui.settings.divider");
/// A row's text and its control's slot.
const TEXT: Key = Key::name("dpro-ui.settings.text");
const CONTROL: Key = Key::name("dpro-ui.settings.control");

/// A settings row's text, for [`Cx::settings_row`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsRow<'s> {
    /// It names the control.
    pub label: &'s str,
    /// It describes the control.
    pub description: Option<&'s str>,
    /// The text shows disabled when it isn't; the control's own state is the caller's.
    pub enabled: bool,
}

impl<M> Cx<'_, M> {
    /// A settings group: `heading` as a level-2 heading, then the rows `rows` builds, on a card.
    pub fn settings_group(
        &mut self,
        key: Key,
        heading: &str,
        rows: impl FnOnce(&mut Cx<'_, M>),
    ) -> NodeId {
        self.add_with(
            Element::stack(key).style(Style {
                gap: SPACE[2],
                ..Style::default()
            }),
            |cx| {
                cx.add(
                    Element::text(SETTINGS_HEADING, heading)
                        .text_style(STRONG)
                        .heading(2),
                );
                cx.add_with(Element::stack(ROWS).look(CARD), rows);
            },
        )
    }

    /// A settings row in a [group](Self::settings_group): `row`'s text, then the control
    /// `control` builds. Name the control by `row.label` and describe it by `row.description`;
    /// the row's text is left out of the accessibility tree. `key` is unique among the group's
    /// rows, as any key among siblings; a hairline before the row (but the first) takes a key
    /// made from it.
    pub fn settings_row(
        &mut self,
        key: Key,
        row: &SettingsRow<'_>,
        control: impl FnOnce(&mut Cx<'_, M>),
    ) -> NodeId {
        // Nothing has been built under the group's card yet this frame: this is its first row,
        // which has no hairline above it.
        let first = self.next == self.ui.nodes[self.parent as usize].first;
        if !first {
            // A sibling of the row, keyed from the row's key, so the row needs no wrapper: a
            // frame builds and lays out one node fewer per row.
            self.add(
                Element::stack(Key(key.0 ^ DIVIDER.0))
                    .style(Style {
                        height: Dim::Fixed(stroke::HAIRLINE),
                        ..Style::default()
                    })
                    .look(Look {
                        fill: Some(Tone::Border),
                        ..Look::PLAIN
                    }),
            );
        }
        let narrow = self.ui.viewport().w < settings::NARROW;
        let (line, text) = if narrow {
            (NARROW_LINE, Style::default())
        } else {
            (WIDE_LINE, WIDE_TEXT)
        };
        self.add_with(Element::stack(key).style(line), |cx| {
            // The control says these words (A3.5 addition 9).
            cx.add_with(Element::stack(TEXT).style(text).presentation(), |cx| {
                cx.add(
                    Element::text(SETTINGS_LABEL, row.label)
                        .text_style(LABEL)
                        .enabled(row.enabled),
                );
                if let Some(d) = row.description {
                    cx.add(
                        Element::text(SETTINGS_DESCRIPTION, d)
                            .text_style(DESCRIPTION)
                            .look(SECONDARY)
                            .enabled(row.enabled),
                    );
                }
            });
            // The control at its own width, at the start of its slot. The slot is there in
            // either layout, so the control keeps its node (and focus) across a resize.
            cx.add_with(
                Element::stack(CONTROL).style(Style {
                    align: Align::Start,
                    ..Style::default()
                }),
                control,
            );
        })
    }
}

/// A row in a wide window: its text, then its control at the end, the control wrapping under
/// the text where the text would get narrower than [`settings::TEXT_MIN_W`]. Padding 14 × 16, as a
/// scan card's.
const WIDE_LINE: Style = Style {
    axis: Axis::Row,
    gap: SPACE[3],
    padding: Insets::xy(settings::PAD_X, settings::PAD_Y),
    align: Align::Center,
    wrap: true,
    ..LINE_BASE
};

/// A row in a narrow window: its control under its text.
const NARROW_LINE: Style = Style {
    axis: Axis::Column,
    gap: SPACE[2],
    padding: Insets::xy(settings::PAD_X, settings::PAD_Y),
    align: Align::Stretch,
    wrap: false,
    ..LINE_BASE
};

/// The text beside a control takes what the control leaves, down to [`settings::TEXT_MIN_W`].
const WIDE_TEXT: Style = Style {
    grow: 1.0,
    min: Size {
        w: settings::TEXT_MIN_W,
        h: 0.0,
    },
    ..LINE_BASE
};

/// What the styles above don't set: [`Style::default`]'s, as a constant.
const LINE_BASE: Style = Style {
    axis: Axis::Column,
    gap: 0.0,
    padding: Insets::all(0.0),
    align: Align::Stretch,
    justify: Justify::Start,
    grow: 0.0,
    width: Dim::Auto,
    height: Dim::Auto,
    min: Size::ZERO,
    max: Size::INFINITE,
    wrap: false,
};

/// A row's label: body, wrapping.
const LABEL: TextStyle = TextStyle {
    size: font::BODY,
    weight: font::REGULAR,
    mono: false,
    wrap: true,
};

/// A row's description: caption, wrapping.
const DESCRIPTION: TextStyle = TextStyle {
    size: font::CAPTION,
    weight: font::REGULAR,
    mono: false,
    wrap: true,
};
