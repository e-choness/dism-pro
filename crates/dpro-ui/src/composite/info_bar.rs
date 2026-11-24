//! The info bar (TD-0009 A3.5 (14)): a page-level message with an optional action and Dismiss. Read
//! only, the Optimize page's warnings, a disconnected source.
//!
//! A client sees a group named by the title. The message is live when the bar appears on its own:
//! polite, or assertive for an error (A4.4); nothing is live when the user's action brought it,
//! since that moves focus.

use crate::{
    Align, Axis, Cx, Element, Icon, Insets, Justify, Key, Live, Look, NodeId, Radius, Size, Style,
    TextStyle, Tone,
    tokens::{SPACE, font, info},
};

use super::STRONG;

/// The bar's glyph: [`Icon::Info`], [`Icon::Warning`] or [`Icon::ErrorBadge`].
pub const INFO_GLYPH: Key = Key::name("dpro-ui.info.glyph");
/// The title, in body semibold.
pub const INFO_TITLE: Key = Key::name("dpro-ui.info.title");
/// The message, on one wrapping line after the title.
pub const INFO_MESSAGE: Key = Key::name("dpro-ui.info.message");
/// The action button.
pub const INFO_ACTION: Key = Key::name("dpro-ui.info.action");
/// Dismiss, a glyph button at the end.
pub const INFO_DISMISS: Key = Key::name("dpro-ui.info.dismiss");

const DISMISS_GLYPH: Key = Key::name("dpro-ui.info.dismiss.glyph");

/// What an info bar says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InfoTone {
    Info,
    Warning,
    Error,
}

/// An info bar, as a view describes it for [`Cx::info_bar`].
#[derive(Clone, Debug)]
pub struct InfoBar<'s, M> {
    pub tone: InfoTone,
    /// It names the bar.
    pub title: &'s str,
    pub message: &'s str,
    pub action: Option<(&'s str, M)>,
    /// Dismiss's name (`ui-infobar-dismiss`) and message; `None` for a bar that stays.
    pub dismiss: Option<(&'s str, M)>,
    /// [`Live::Polite`] (or [`Live::Assertive`] for an error) when the bar came unbidden;
    /// [`Live::Off`] when the user's action brought it.
    pub live: Live,
}

impl<M: Clone> Cx<'_, M> {
    /// An info bar. The action and Dismiss are Tab stops in visual order; the bar isn't.
    ///
    /// The bar is a row that wraps: where the window is too narrow for the title, the action and
    /// Dismiss beside the message (225 % text in a narrow window), they go on a line of their own,
    /// at its end, instead of past the bar's end. The message takes what's left of its line and
    /// wraps in it. Coming and going reflows the page once; nothing in it animates.
    pub fn info_bar(&mut self, key: Key, b: &InfoBar<'_, M>) -> NodeId {
        let (fill, border, glyph, icon) = colors(b.tone);
        self.add_with(
            Element::stack(key)
                .style(Style {
                    axis: Axis::Row,
                    gap: SPACE[2],
                    align: Align::Center,
                    // The message grows, so this only places a line of the action and Dismiss.
                    justify: Justify::End,
                    padding: Insets::xy(info::PAD_X, info::PAD_Y),
                    wrap: true,
                    ..Style::default()
                })
                .look(Look {
                    fill: Some(fill),
                    border: Some(border),
                    radius: Radius::Card,
                    ink: Tone::Text,
                })
                .name(b.title),
            |cx| {
                cx.add(Element::glyph(INFO_GLYPH, icon).look(Look {
                    ink: glyph,
                    ..Look::PLAIN
                }));
                cx.add(Element::text(INFO_TITLE, b.title).text_style(STRONG));
                cx.add(
                    Element::text(INFO_MESSAGE, b.message)
                        .text_style(MESSAGE)
                        .style(Style {
                            grow: 1.0,
                            ..Style::default()
                        })
                        .live(b.live),
                );
                if let Some((label, msg)) = &b.action {
                    cx.add(
                        Element::button(INFO_ACTION, label, msg.clone())
                            .style(Style {
                                min: Size {
                                    w: 0.0,
                                    h: info::BUTTON_H,
                                },
                                padding: Insets::xy(SPACE[2], 0.0),
                                ..Style::default()
                            })
                            .look(Look::SECONDARY),
                    );
                }
                if let Some((name, msg)) = &b.dismiss {
                    cx.add_with(
                        // Named explicitly, so its glyph is presentational (A3.5 addition 9). A
                        // square, its glyph in the middle.
                        Element::button(INFO_DISMISS, "", msg.clone())
                            .name(name)
                            .style(Style {
                                align: Align::Center,
                                justify: Justify::Center,
                                min: Size {
                                    w: info::BUTTON_H,
                                    h: info::BUTTON_H,
                                },
                                ..Style::default()
                            })
                            .look(Look::SUBTLE),
                        |cx| {
                            cx.add(Element::glyph(DISMISS_GLYPH, Icon::Cancel));
                        },
                    );
                }
            },
        )
    }
}

/// `tone`'s tint, border, glyph ink and glyph (A3.5 (14), decision #45): warnings and errors
/// reuse the error panel's tints and borders, and the glyph takes the status color. High contrast
/// has no tints: the tint is the window, the border window text.
const fn colors(tone: InfoTone) -> (Tone, Tone, Tone, Icon) {
    match tone {
        InfoTone::Info => (Tone::InfoFill, Tone::InfoBorder, Tone::Accent, Icon::Info),
        InfoTone::Warning => (Tone::WarnFill, Tone::WarnBorder, Tone::Warn, Icon::Warning),
        InfoTone::Error => (
            Tone::ErrorFill,
            Tone::DangerBorder,
            Tone::Danger,
            Icon::ErrorBadge,
        ),
    }
}

/// The message wraps.
const MESSAGE: TextStyle = TextStyle {
    size: font::BODY,
    weight: font::REGULAR,
    mono: false,
    wrap: true,
};
