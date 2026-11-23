//! The info bar (TD-0009 A3.5 (14)): a page-level message with an optional action and Dismiss. Read
//! only, the Optimize page's warnings, a disconnected source.
//!
//! A client sees a group named by the title. The message is live when the bar appears on its own:
//! polite, or assertive for an error (A4.4); nothing is live when the user's action brought it,
//! since that moves focus.

use crate::{
    Align, Axis, Cx, Element, Icon, Insets, Key, Live, Look, NodeId, Radius, Style, TextStyle,
    Tone,
    tokens::{SPACE, font},
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
    pub fn info_bar(&mut self, key: Key, b: &InfoBar<'_, M>) -> NodeId {
        let (fill, border, icon) = match b.tone {
            InfoTone::Info => (Tone::Selected, Tone::Accent, Icon::Info),
            InfoTone::Warning => (Tone::Subtle, Tone::Warn, Icon::Warning),
            InfoTone::Error => (Tone::PermanentFill, Tone::Danger, Icon::ErrorBadge),
        };
        self.add_with(
            Element::stack(key)
                .style(Style {
                    axis: Axis::Row,
                    gap: SPACE[2],
                    align: Align::Center,
                    padding: Insets::xy(PAD_X, PAD_Y),
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
                    ink: border,
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
                    cx.add(Element::button(INFO_ACTION, label, msg.clone()).look(Look::SECONDARY));
                }
                if let Some((name, msg)) = &b.dismiss {
                    cx.add_with(
                        // Named explicitly, so its glyph is presentational (A3.5 addition 9).
                        Element::button(INFO_DISMISS, "", msg.clone())
                            .name(name)
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

/// Padding 12 × 16 (A3.5 (14)).
const PAD_X: f32 = 16.0;
const PAD_Y: f32 = 12.0;

/// The message wraps.
const MESSAGE: TextStyle = TextStyle {
    size: font::BODY,
    weight: font::REGULAR,
    mono: false,
    wrap: true,
};
