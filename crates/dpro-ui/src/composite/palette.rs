//! The command palette's view (TD-0009 A3.5 (3)): a dialog with a search field, the results it
//! drives, a status line and the keys' hints, built in a [`PopupKind::Palette`](crate::PopupKind)
//! popup's view.
//!
//! Focus stays in the field: its step moves the highlight (the model's), Enter sends `on_run`, and
//! the results are a [suggestion list](crate::Element::suggestions) whose options never take focus.
//! A client sees a dialog holding an Edit that is `ControllerFor` the list box, exactly one option
//! selected (the highlighted one), each placed among the options alone, and a polite live status
//! while there is one. The highlighted row is scrolled into view; at most
//! [`ROWS`](crate::tokens::palette::ROWS) rows show at 100 % text, the rest scrolled.

use crate::{
    Align, Axes, Axis, Cx, Element, Icon, Insets, Key, Live, Look, NodeId, Radius, Size, Style,
    TextEdit, TextStyle, Tone, Ui,
    tokens::{SPACE, font, palette},
};

use super::{BadgeSpec, SECONDARY, SMALL};

/// The search field.
pub const PALETTE_FIELD: Key = Key::name("dpro-ui.palette.field");
/// The results: a list of suggestions the field drives, in a vertical viewport.
pub const PALETTE_LIST: Key = Key::name("dpro-ui.palette.list");
/// The status line, a polite live region: "No commands match …".
pub const PALETTE_STATUS: Key = Key::name("dpro-ui.palette.status");

const FIELD_ROW: Key = Key::name("dpro-ui.palette.field-row");
const SEARCH: Key = Key::name("dpro-ui.palette.search");
const ESC: Key = Key::name("dpro-ui.palette.esc");
const VIEWPORT: Key = Key::name("dpro-ui.palette.viewport");
const HINTS: Key = Key::name("dpro-ui.palette.hints");
const LABEL: Key = Key::name("dpro-ui.palette.label");
const DETAIL: Key = Key::name("dpro-ui.palette.detail");
const BADGE: Key = Key::name("dpro-ui.palette.badge");
const ACCEL: Key = Key::name("dpro-ui.palette.accel");

/// A header's key: above every entry's (`Key::index` of the entry), so the two never meet.
const HEADER: u64 = 1 << 32;

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
    /// The command palette: the field row, the results under their headers, the status while
    /// there is one, and the hints.
    pub fn palette(&mut self, key: Key, p: &PaletteView<'_, M>) -> NodeId {
        let mut wired = (None, None);
        let id = self.add_with(
            Element::dialog(key, p.name)
                .style(Style {
                    padding: Insets::xy(0.0, SPACE[1]),
                    ..Style::default()
                })
                // A transient surface's border: 2 px of `WindowText` in high contrast (A4.4).
                .look(Look {
                    fill: Some(Tone::Surface),
                    border: Some(Tone::TransientBorder),
                    radius: Radius::Drawer,
                    ink: Tone::Text,
                }),
            |cx| {
                wired.0 = field_row(cx, p);
                cx.add_with(
                    Element::scroll(VIEWPORT, Axes::Y).style(Style {
                        padding: Insets::xy(SPACE[1], 0.0),
                        max: Size {
                            w: f32::INFINITY,
                            h: f32::from(palette::ROWS) * palette::ROW_H,
                        },
                        ..Style::default()
                    }),
                    |cx| {
                        let list = Element::suggestions(PALETTE_LIST).name(p.results);
                        wired.1 = Some(cx.add_with(list, |cx| results(cx, p)));
                    },
                );
                if !p.status.is_empty() {
                    cx.add(
                        Element::text(PALETTE_STATUS, p.status)
                            .text_style(SMALL)
                            .look(SECONDARY)
                            .style(Style {
                                padding: Insets::xy(palette::PAD_X, palette::PAD_Y),
                                ..Style::default()
                            })
                            .live(Live::Polite),
                    );
                }
                cx.add_with(
                    Element::stack(HINTS).style(Style {
                        axis: Axis::Row,
                        gap: SPACE[3],
                        padding: Insets::xy(palette::PAD_X, palette::PAD_Y),
                        wrap: true,
                        ..Style::default()
                    }),
                    |cx| {
                        for (i, hint) in (0..).zip(p.hints) {
                            cx.add(
                                Element::text(Key::index(i), hint)
                                    .text_style(SMALL)
                                    .look(SECONDARY),
                            );
                        }
                    },
                );
            },
        );
        if let (Some(field), Some(list)) = wired {
            self.ui.set_controls(field, list);
        }
        id
    }
}

/// The field row: the Search glyph, the field, and the Esc keycap. Returns the field.
fn field_row<M: Clone>(cx: &mut Cx<'_, M>, p: &PaletteView<'_, M>) -> Option<NodeId> {
    let mut field = None;
    cx.add_with(
        Element::stack(FIELD_ROW).style(Style {
            axis: Axis::Row,
            gap: SPACE[2],
            align: Align::Center,
            padding: Insets::xy(palette::PAD_X, palette::PAD_Y),
            ..Style::default()
        }),
        |cx| {
            cx.add(Element::glyph(SEARCH, Icon::Search).look(SECONDARY));
            field = Some(
                cx.add(
                    Element::field(PALETTE_FIELD, p.query, p.on_edit)
                        .name(p.field)
                        .placeholder(p.placeholder)
                        .step(p.on_step)
                        .submit(p.on_run.clone())
                        .text_style(FIELD)
                        .style(Style {
                            grow: 1.0,
                            min: Size {
                                w: 0.0,
                                h: SPACE[5],
                            },
                            ..Style::default()
                        }),
                ),
            );
            cx.add(
                Element::text(ESC, p.esc)
                    .text_style(KEYCAP)
                    .style(Style {
                        padding: Insets::xy(SPACE[1], 0.0),
                        ..Style::default()
                    })
                    .look(Look {
                        fill: None,
                        border: Some(Tone::Border),
                        radius: Radius::Small,
                        ink: Tone::TextSecondary,
                    }),
            );
        },
    );
    field
}

/// The results, each area under its header.
fn results<M: Clone>(cx: &mut Cx<'_, M>, p: &PaletteView<'_, M>) {
    let mut groups = p.groups.iter().peekable();
    for (i, row) in (0..).zip(p.rows) {
        if let Some(&&(first, header)) = groups.peek()
            && first == i
        {
            groups.next();
            cx.add(
                Element::text(Key::index(HEADER | u64::from(first)), header)
                    .text_style(HEADER_TEXT)
                    .look(SECONDARY)
                    .style(Style {
                        padding: Insets {
                            left: palette::ROW_PAD,
                            top: SPACE[2],
                            right: palette::ROW_PAD,
                            bottom: SPACE[1],
                        },
                        ..Style::default()
                    }),
            );
        }
        let highlighted = p.highlighted == Some(i);
        let detail = if row.enabled { row.detail } else { row.reason };
        let mut el = Element::option(
            Key::index(u64::from(row.entry)),
            row.label,
            highlighted,
            (p.on_pick)(i),
        )
        .enabled(row.enabled)
        .describe(detail)
        .style(Style {
            axis: Axis::Row,
            gap: SPACE[2],
            align: Align::Center,
            padding: Insets::xy(palette::ROW_PAD, palette::ROW_PAD),
            min: Size {
                w: 0.0,
                h: palette::ROW_H,
            },
            ..Style::default()
        });
        if !row.accel.is_empty() {
            el = el.shortcut(row.accel);
        }
        el = el.look(Look {
            fill: highlighted.then_some(Tone::Selected),
            border: None,
            radius: Radius::Control,
            ink: Tone::Text,
        });
        cx.add_with(el, |cx| {
            // The label, or its detail if it has one, pushes the badge and the keys to the end.
            cx.add(Element::text(LABEL, row.label).style(Style {
                grow: if detail.is_empty() { 1.0 } else { 0.0 },
                ..Style::default()
            }));
            if !detail.is_empty() {
                cx.add(
                    Element::text(DETAIL, detail)
                        .text_style(SMALL)
                        .look(SECONDARY)
                        .style(Style {
                            grow: 1.0,
                            ..Style::default()
                        }),
                );
            }
            if let Some(b) = row.badge {
                cx.add(Element::badge(BADGE, b.text, b.tone));
            }
            if !row.accel.is_empty() {
                cx.add(
                    Element::text(ACCEL, row.accel)
                        .text_style(KEYCAP)
                        .look(SECONDARY),
                );
            }
        });
    }
}

impl<M> Ui<M> {
    /// Field `field` drives node `list` (UI Automation's `ControllerFor`): the palette's field
    /// and its results. A client sees it with the field.
    fn set_controls(&mut self, field: NodeId, list: NodeId) {
        let (Some(i), Some(l)) = (self.index(field), self.index(list)) else {
            return;
        };
        let f = self.nodes[i as usize].field;
        if let Some(slot) = self.fields.get_mut(f as usize)
            && slot.controls != l
        {
            slot.controls = l;
            self.a11y_touch(i);
        }
    }
}

/// The field: subtitle text.
const FIELD: TextStyle = TextStyle {
    size: font::SUBTITLE,
    weight: font::REGULAR,
    mono: false,
    wrap: false,
};

/// A keycap's, and a result's keys: caption text in mono.
const KEYCAP: TextStyle = TextStyle {
    size: font::CAPTION,
    weight: font::REGULAR,
    mono: true,
    wrap: false,
};

/// An area's header: caption text, semibold.
const HEADER_TEXT: TextStyle = TextStyle {
    size: font::CAPTION,
    weight: font::SEMIBOLD,
    mono: false,
    wrap: false,
};
