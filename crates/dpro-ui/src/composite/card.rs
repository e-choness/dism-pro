//! Summary cards and category scan cards (TD-0009 A3.5 (4), (5)).
//!
//! - A **summary card** (Overview, artboard 01): a label, a large metric or a status dot with a
//!   word, and a detail line. A clickable one is a button named by its label and described by
//!   "{value}. {detail}", its parts presentational; a static one is a group named by its label,
//!   which is a level-2 heading. [`Cx::card_grid`] wraps cards at [`card::MIN_W`] each.
//! - A **category scan card** (Clean, artboard 02): a check box named by the category, holding its
//!   name and description, and described by "{description}. {size}. {badge}"; a risk badge; the
//!   size in a fixed slot at the end, so a size update while scanning damages that slot alone,
//!   with a 2 px bar under it while its category scans; and Details when the scan failed.
//!
//!   A3.5 (5) draws the bar along the card's bottom. That needs a row inside the card for the
//!   parts and a bar node beside it in every card, 14 more nodes on the composites page:
//!   measured, +11.6 % on its whole accessibility tree and +6.6 to +8.3 % on each frame that
//!   builds it. In the slot it's one node while a category scans, and starting or ending a scan
//!   still repaints the slot alone (decision #41).
//!
//! Sizes are written by the window's formats, as a table's size cells are, and every description
//! is assembled in one kept buffer, so a warm page builds its cards without allocating.

use std::{mem, rc::Rc};

use dpro_app::table::Tri;
use dpro_core::units::Bytes;

use crate::{
    Align, Axis, Cx, Dim, Element, Insets, Key, Look, NodeId, Radius, Size, Style, TextStyle, Tone,
    tokens::{SPACE, card, font, scan},
};

use super::{BadgeSpec, CARD, SECONDARY, SMALL, STRONG, join};

/// A summary card's label.
pub const CARD_LABEL: Key = Key::name("dpro-ui.card.label");
/// Its metric, or its status (the dot and the word).
pub const CARD_VALUE: Key = Key::name("dpro-ui.card.value");
/// Its detail line.
pub const CARD_DETAIL: Key = Key::name("dpro-ui.card.detail");
/// Its delta, beside the detail.
pub const CARD_DELTA: Key = Key::name("dpro-ui.card.delta");
/// A scan card's check box, which holds its name and description.
pub const SCAN_CHECK: Key = Key::name("dpro-ui.scan.check");
/// The category's name, in the check box.
pub const SCAN_NAME: Key = Key::name("dpro-ui.scan.name");
/// The category's description, in the check box.
pub const SCAN_DESCRIPTION: Key = Key::name("dpro-ui.scan.description");
/// The risk badge.
pub const SCAN_BADGE: Key = Key::name("dpro-ui.scan.badge");
/// The size's fixed slot.
pub const SCAN_SIZE: Key = Key::name("dpro-ui.scan.size");
/// Details, when the scan failed.
pub const SCAN_DETAILS: Key = Key::name("dpro-ui.scan.details");

/// The size slot's text.
const SIZE_TEXT: Key = Key::name("dpro-ui.scan.size.text");
/// The scan's bar under the size, while the category scans.
const SCAN_BAR: Key = Key::name("dpro-ui.scan.bar");
/// A status card's dot and word.
const DOT: Key = Key::name("dpro-ui.card.dot");
const WORD: Key = Key::name("dpro-ui.card.word");

/// How a status or a delta reads: its dot's or its text's color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatusTone {
    Ok,
    Warn,
    Danger,
}

/// A summary card's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardValue<'s> {
    /// Not known yet: a skeleton where the metric goes.
    Loading,
    /// A large figure ("41.2 GB"), in display type.
    Metric(&'s str),
    /// A word with a dot in its tone ("Healthy"), in title type.
    Status(&'s str, StatusTone),
}

/// A summary card, as a view describes it for [`Cx::summary_card`].
#[derive(Clone, Debug)]
pub struct SummaryCard<'s, M> {
    pub label: &'s str,
    pub value: CardValue<'s>,
    /// The line under the value; on an error, the error's line (the page shows the panel).
    pub detail: &'s str,
    /// Doc 05's "metric + delta": its text and how it reads.
    pub delta: Option<(&'s str, StatusTone)>,
    /// A clickable card's message, which opens its page; `None` for a static card.
    pub open: Option<M>,
    /// What joins the value and the detail in a clickable card's description:
    /// `ui-description-separator` (". " in en-US).
    pub separator: &'s str,
}

/// A scan card's size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanSize {
    /// Not scanned: `ui-size-unknown`.
    Unknown,
    /// Scanning: `ui-scan-scanning`, and a bar along the card's bottom.
    Scanning,
    /// Scanned, in the window's formats; 0 is "Nothing to clean", and the box is disabled.
    Bytes(Bytes),
    /// The scan failed: `ui-scan-failed`, and Details opens the error panel.
    Failed,
}

/// The words every scan card on a page shares, from the app's catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanLabels<'s> {
    /// `ui-size-unknown`.
    pub unknown: &'s str,
    /// `ui-scan-scanning`.
    pub scanning: &'s str,
    /// `ui-scan-nothing`.
    pub nothing: &'s str,
    /// `ui-scan-failed`.
    pub failed: &'s str,
    /// `ui-scan-details`.
    pub details: &'s str,
    /// What joins the check box's description, "{description}. {size}. {badge}":
    /// `ui-description-separator`.
    pub separator: &'s str,
}

/// A category scan card, as a view describes it for [`Cx::scan_card`].
#[derive(Clone, Debug)]
pub struct ScanCard<'s, M> {
    /// Mixed when the user has unchecked some of the category's items.
    pub check: Tri,
    /// The category, which names the check box.
    pub name: &'s str,
    pub description: &'s str,
    /// The risk badge (its levels and words come from the rule packs, `clean-`).
    pub badge: Option<BadgeSpec<'s>>,
    pub size: ScanSize,
    /// While a job holds the target: why the box is disabled (`ui-busy-target`).
    pub busy: Option<&'s str>,
    pub toggle: M,
    /// Details' message, when the scan failed.
    pub details: Option<M>,
    pub labels: &'s ScanLabels<'s>,
}

impl<M: Clone> Cx<'_, M> {
    /// A summary card: clickable (a Tab stop; Enter or Space sends `open`) or static. Its height
    /// is its lines', so a value change damages the value alone.
    pub fn summary_card(&mut self, key: Key, c: &SummaryCard<'_, M>) -> NodeId {
        let value = match c.value {
            CardValue::Loading => "",
            CardValue::Metric(t) | CardValue::Status(t, _) => t,
        };
        let style = Style {
            padding: Insets::all(card::PADDING),
            gap: SPACE[0],
            grow: 1.0,
            min: Size {
                w: card::MIN_W,
                h: 0.0,
            },
            ..Style::default()
        };
        let clickable = c.open.is_some();
        let parts = |cx: &mut Cx<'_, M>| {
            let mut label = Element::text(CARD_LABEL, c.label)
                .text_style(SMALL)
                .look(SECONDARY);
            if !clickable {
                label = label.heading(2);
            }
            cx.add(label);
            card_value(cx, c.value);
            if !c.detail.is_empty() {
                cx.add(
                    Element::text(CARD_DETAIL, c.detail)
                        .text_style(SMALL)
                        .look(SECONDARY),
                );
            }
            if let Some((delta, tone)) = c.delta {
                cx.add(
                    Element::text(CARD_DELTA, delta)
                        .text_style(SMALL)
                        .look(Look {
                            ink: status(tone),
                            ..Look::PLAIN
                        }),
                );
            }
        };
        if let Some(open) = &c.open {
            let mut description = mem::take(&mut self.ui.cell_text);
            join(&mut description, c.separator, &[value, c.detail]);
            let id = self.add_with(
                // Named explicitly, so its parts are presentational (A3.5 addition 9).
                Element::button(key, "", open.clone())
                    .name(c.label)
                    .style(style)
                    .look(CARD)
                    .describe(&description),
                parts,
            );
            self.ui.cell_text = description;
            id
        } else {
            self.add_with(
                Element::stack(key).style(style).look(CARD).name(c.label),
                parts,
            )
        }
    }

    /// A grid of cards: a row that wraps (A3.5 addition 3), each card at least
    /// [`card::MIN_W`] wide, 16 px apart. `cards` builds them.
    pub fn card_grid(&mut self, key: Key, cards: impl FnOnce(&mut Cx<'_, M>)) -> NodeId {
        self.add_with(
            Element::stack(key).style(Style {
                axis: Axis::Row,
                gap: SPACE[3],
                wrap: true,
                ..Style::default()
            }),
            cards,
        )
    }

    /// A category scan card. The check box is a Tab stop and Space toggles it; Details is the
    /// next. Clicks on the name and the description toggle the box; on the badge and the size
    /// they do nothing.
    pub fn scan_card(&mut self, key: Key, c: &ScanCard<'_, M>) -> NodeId {
        let formats = Rc::clone(&self.ui.formats.0);
        let labels = c.labels;
        // The size, as its slot shows it and as the description says it.
        let push_size = |out: &mut String| match c.size {
            ScanSize::Unknown => out.push_str(labels.unknown),
            ScanSize::Scanning => out.push_str(labels.scanning),
            ScanSize::Failed => out.push_str(labels.failed),
            ScanSize::Bytes(Bytes(0)) => out.push_str(labels.nothing),
            ScanSize::Bytes(b) => formats.size(b, out),
        };
        let mut text = mem::take(&mut self.ui.cell_text);
        text.clear();
        push_size(&mut text);
        let size_end = text.len();
        // "{description}. {size}. {badge}", after the size in the same buffer.
        if !c.description.is_empty() {
            text.push_str(c.description);
            text.push_str(labels.separator);
        }
        push_size(&mut text);
        if let Some(b) = c.badge {
            text.push_str(labels.separator);
            text.push_str(b.text);
        }
        let (size, description) = text.split_at(size_end);
        // The size's slot is a size's width at the user's text size, so sizes arriving while the
        // category scans keep it as it is; a longer word ("Nothing to clean") widens it.
        let size_w = scan::SIZE_W * self.ui.text_scale;
        let id = self.add_with(
            Element::stack(key)
                .style(Style {
                    axis: Axis::Row,
                    gap: scan::GAP,
                    align: Align::Center,
                    padding: Insets::xy(scan::PAD_X, scan::PAD_Y),
                    ..Style::default()
                })
                .look(CARD),
            |cx| scan_parts(cx, c, size, description, size_w),
        );
        self.ui.cell_text = text;
        id
    }
}

/// A scan card's parts: the check box holding the name and the description, described by
/// `description`; the badge; `size` in its slot, at least `size_w` wide, with the scan's bar
/// under it while the category scans; and Details.
fn scan_parts<M: Clone>(
    cx: &mut Cx<'_, M>,
    c: &ScanCard<'_, M>,
    size: &str,
    description: &str,
    size_w: f32,
) {
    let nothing = c.size == ScanSize::Bytes(Bytes(0));
    // The toolkit puts the box at the check box's start and its lines after it (A3.5 addition 9).
    let mut check = Element::toggle(SCAN_CHECK, c.name, c.check, c.toggle.clone())
        .style(Style {
            grow: 1.0,
            ..Style::default()
        })
        .enabled(c.busy.is_none() && !nothing)
        .describe(description);
    if let Some(reason) = c.busy {
        check = check.tooltip(reason);
    }
    cx.add_with(check, |cx| {
        cx.add(Element::text(SCAN_NAME, c.name).text_style(STRONG));
        cx.add(
            Element::text(SCAN_DESCRIPTION, c.description)
                .text_style(SMALL)
                .look(SECONDARY),
        );
    });
    if let Some(b) = c.badge {
        let mut badge = Element::badge(SCAN_BADGE, b.text, b.tone);
        if let Some(d) = b.description {
            badge = badge.describe(d);
        }
        cx.add(badge);
    }
    cx.add_with(
        Element::stack(SCAN_SIZE).style(Style {
            align: Align::End,
            gap: SPACE[0],
            min: Size { w: size_w, h: 0.0 },
            ..Style::default()
        }),
        |cx| {
            cx.add(Element::text(SIZE_TEXT, size).look(SECONDARY));
            // The scan's bar, a size's width under "Scanning…": in the slot, so starting or
            // ending a scan repaints the slot alone, and a node only while the category scans.
            // The slot is shorter than the check box's two lines, so the card keeps its height.
            if c.size == ScanSize::Scanning {
                cx.add(
                    Element::stack(SCAN_BAR)
                        .style(Style {
                            width: Dim::Fixed(size_w),
                            height: Dim::Fixed(scan::BAR),
                            ..Style::default()
                        })
                        .look(Look {
                            fill: Some(Tone::Accent),
                            radius: Radius::Pill,
                            ..Look::PLAIN
                        }),
                );
            }
        },
    );
    if let Some(details) = &c.details {
        cx.add(
            Element::button(SCAN_DETAILS, c.labels.details, details.clone())
                .style(Style {
                    min: Size {
                        w: 0.0,
                        h: scan::DETAILS_H,
                    },
                    padding: Insets::xy(SPACE[2], 0.0),
                    ..Style::default()
                })
                .look(Look::SECONDARY),
        );
    }
}

/// A summary card's value: a skeleton, a metric, or a dot and a word.
fn card_value<M>(cx: &mut Cx<'_, M>, value: CardValue<'_>) {
    match value {
        // An empty label a metric's line high, so the metric that replaces it moves nothing: it
        // stretches across the card, and a client doesn't see it, as an empty label is hidden.
        // A quiet fill, not the hover layer: high contrast drops state layers, and shows this
        // as button face.
        CardValue::Loading => {
            cx.add(Element::text(CARD_VALUE, "").text_style(METRIC).look(Look {
                fill: Some(Tone::Subtle),
                radius: Radius::Small,
                ..Look::PLAIN
            }));
        }
        CardValue::Metric(metric) => {
            cx.add(Element::text(CARD_VALUE, metric).text_style(METRIC));
        }
        CardValue::Status(word, tone) => {
            cx.add_with(
                Element::stack(CARD_VALUE).style(Style {
                    axis: Axis::Row,
                    gap: SPACE[1],
                    align: Align::Center,
                    ..Style::default()
                }),
                |cx| {
                    cx.add(
                        Element::stack(DOT)
                            .style(Style {
                                width: Dim::Fixed(card::DOT),
                                height: Dim::Fixed(card::DOT),
                                ..Style::default()
                            })
                            .look(Look {
                                fill: Some(status(tone)),
                                radius: Radius::Pill,
                                ..Look::PLAIN
                            }),
                    );
                    cx.add(Element::text(WORD, word).text_style(TextStyle {
                        size: font::TITLE,
                        ..STRONG
                    }));
                },
            );
        }
    }
}

/// A metric: display type, semibold.
const METRIC: TextStyle = TextStyle {
    size: font::DISPLAY,
    ..STRONG
};

/// A status's color.
const fn status(tone: StatusTone) -> Tone {
    match tone {
        StatusTone::Ok => Tone::Ok,
        StatusTone::Warn => Tone::Warn,
        StatusTone::Danger => Tone::Danger,
    }
}
