//! Badges (TD-0009 A3.5 (1)): small semibold labels that say what a change or a job is. Undoable,
//! Undoable 4 days, Permanent, Restart required, No restart needed, Partially applied, Running.
//!
//! dpro-app maps the domain to a tone and its words (`dpro_app::badges`); a badge renders whatever
//! it's given. It's text, named by its word and described by what the word means, and it isn't
//! focusable: a badge with an action is a button (§4.4). Color is never the only signal (WCAG
//! 1.4.1): the word always says the meaning.
//!
//! Each tone is one text and fill pair of the token table (decision #45), opaque under glass. High
//! contrast has no tints: the fill is the window and the word is window text, in a 1 px border of
//! the same color, so a badge still reads as one.

use crate::{
    Align, Axis, Cx, Element, Insets, Key, Look, NodeId, Radius, Style, TextStyle, Tone,
    tokens::{SPACE, badge, font},
};

use super::SMALL;

/// What a badge says, as a pair of text and fill colors (decision #45's token table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BadgeTone {
    /// Green: a change that can be undone.
    Undoable,
    /// Orange: one that can't, or whose undo expired.
    Permanent,
    /// Ink at 6 %: No restart needed.
    Neutral,
    /// Accent: a running job.
    Info,
    /// Amber: Restart required, Partially applied.
    Caution,
}

/// One badge of a [row of badges](Cx::badges).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadgeSpec<'s> {
    /// Its word, which is also its accessible name.
    pub text: &'s str,
    pub tone: BadgeTone,
    /// What the word means ("Can be undone until 14 March."), for assistive technology (UI
    /// Automation's `FullDescription`); `None` when the word says it all.
    pub description: Option<&'s str>,
}

impl<'s, M> Element<'s, M> {
    /// A badge: `text` in 12 px semibold, `tone`'s colors, padded 3 × 8 and slightly rounded. A
    /// label, so it isn't focusable; give it a [description](Self::describe) when the word needs
    /// one. Changing its text resizes it, which lays its row out once; changing its tone repaints
    /// it alone.
    #[must_use]
    pub fn badge(key: Key, text: &'s str, tone: BadgeTone) -> Self {
        Self::text(key, text)
            .style(Style {
                padding: Insets::xy(badge::PAD_X, badge::PAD_Y),
                ..Style::default()
            })
            .text_style(TextStyle {
                size: font::BADGE,
                weight: font::BADGE_WEIGHT,
                ..SMALL
            })
            .look(look(tone))
    }
}

impl<M: Clone> Cx<'_, M> {
    /// A wrapping row of badges, each described by its spec's description.
    pub fn badges(&mut self, key: Key, badges: &[BadgeSpec<'_>]) -> NodeId {
        self.add_with(
            Element::stack(key).style(Style {
                axis: Axis::Row,
                gap: SPACE[1],
                align: Align::Center,
                wrap: true,
                ..Style::default()
            }),
            |cx| {
                for (i, b) in (0..).zip(badges) {
                    let mut el = Element::badge(Key::index(i), b.text, b.tone);
                    if let Some(description) = b.description {
                        el = el.describe(description);
                    }
                    cx.add(el);
                }
            },
        )
    }
}

/// `tone`'s look: its fill and text pair from the token table (decision #45), slightly rounded.
///
/// The border is the fill's own tone. In light and dark it's the fill's color, so it doesn't
/// show; high contrast strokes every border in window text, which gives the badge its outline
/// there while the fill is the window.
const fn look(tone: BadgeTone) -> Look {
    let (fill, ink) = match tone {
        BadgeTone::Undoable => (Tone::UndoableFill, Tone::UndoableText),
        BadgeTone::Permanent => (Tone::PermanentFill, Tone::PermanentText),
        BadgeTone::Neutral => (Tone::NeutralFill, Tone::NeutralText),
        BadgeTone::Info => (Tone::RunningFill, Tone::RunningText),
        BadgeTone::Caution => (Tone::CautionFill, Tone::CautionText),
    };
    Look {
        fill: Some(fill),
        border: Some(fill),
        radius: Radius::Small,
        ink,
    }
}
