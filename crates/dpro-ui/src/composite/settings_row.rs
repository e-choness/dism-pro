//! Settings rows (TD-0009 A3.5 (15)): a label and a description at the start, a control at the end.
//!
//! The control is any existing role: a check box, a radio set, a dropdown, a field (with a step for
//! numbers, A3.5 addition 6) or a button. It's the Tab stop, named by the label and described by the
//! description, so the row's text isn't read twice (rule 9). On a narrow page the control wraps
//! under the text.

use crate::{Align, Axis, Cx, Element, Insets, Key, NodeId, Size, Style, tokens::SPACE};

use super::{CARD, SECONDARY, SMALL};

/// A settings group's heading, a level-2 label.
pub const SETTINGS_HEADING: Key = Key::name("dpro-ui.settings.heading");
/// A row's label.
pub const SETTINGS_LABEL: Key = Key::name("dpro-ui.settings.label");
/// A row's description.
pub const SETTINGS_DESCRIPTION: Key = Key::name("dpro-ui.settings.description");

const ROWS: Key = Key::name("dpro-ui.settings.rows");
const TEXT: Key = Key::name("dpro-ui.settings.text");
const CONTROL: Key = Key::name("dpro-ui.settings.control");

/// A settings row's text, for [`Cx::settings_row`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsRow<'s> {
    /// It names the control.
    pub label: &'s str,
    /// It describes the control.
    pub description: Option<&'s str>,
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
                        .text_style(super::STRONG)
                        .heading(2),
                );
                cx.add_with(Element::stack(ROWS).look(CARD), rows);
            },
        )
    }

    /// A settings row: `row`'s text, then the control `control` builds. Name the control by
    /// `row.label` and describe it by `row.description`.
    pub fn settings_row(
        &mut self,
        key: Key,
        row: &SettingsRow<'_>,
        control: impl FnOnce(&mut Cx<'_, M>),
    ) -> NodeId {
        self.add_with(
            Element::stack(key).style(Style {
                axis: Axis::Row,
                gap: SPACE[3],
                align: Align::Center,
                padding: Insets::xy(PAD_X, PAD_Y),
                wrap: true,
                ..Style::default()
            }),
            |cx| {
                cx.add_with(
                    Element::stack(TEXT).style(Style {
                        grow: 1.0,
                        gap: SPACE[0],
                        min: Size {
                            w: TEXT_MIN_W,
                            h: 0.0,
                        },
                        ..Style::default()
                    }),
                    |cx| {
                        cx.add(Element::text(SETTINGS_LABEL, row.label).enabled(row.enabled));
                        if let Some(d) = row.description {
                            cx.add(
                                Element::text(SETTINGS_DESCRIPTION, d)
                                    .text_style(SMALL)
                                    .look(SECONDARY)
                                    .enabled(row.enabled),
                            );
                        }
                    },
                );
                cx.add_with(Element::stack(CONTROL), control);
            },
        )
    }
}

/// Padding 14 × 16, as a scan card's (A3.5 (15)).
const PAD_X: f32 = 16.0;
const PAD_Y: f32 = 14.0;
/// The text's narrowest before the control wraps under it.
const TEXT_MIN_W: f32 = 240.0;
