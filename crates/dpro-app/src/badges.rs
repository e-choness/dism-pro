//! Badges' tone and words, from the domain (TD-0009 A3.5 (1)): pure, so the toolkit's
//! `BadgeTone` and the app's catalog stay on their sides. The view resolves [`Text::id`] and
//! [`Description::id`] with the arguments each variant carries, and maps [`Tone`] to the toolkit's
//! badge tone one to one.
//!
//! | `Reversibility` | Tone | Text | Description |
//! |---|---|---|---|
//! | `Reversible` | Undoable | `badge-undoable` | none |
//! | `ReversibleUntil { until }` | Undoable | `ui-badge-undoable-days`, days = ⌈(until − now) / 1 day⌉ | `ui-badge-undoable-until-desc` |
//! | `ReversibleUntil`, deadline passed | Permanent | `ui-badge-expired` | none |
//! | `ReversibleVia { inverse }` | Undoable | `badge-undoable` | `ui-badge-undoable-via-desc` |
//! | `Irreversible { reason }` | Permanent | `badge-permanent` | `ui-badge-permanent-desc` |
//!
//! Restart, from `Change::reboot_required` or `PlanSummary::reboot_required`: Caution with
//! `ui-badge-restart`, described by a `RebootReason`'s message, or Neutral with
//! `ui-badge-no-restart`. Job states: Info with `ui-badge-running`, Caution with
//! `ui-badge-partial`.
//!
//! The view passes the time it builds at as `now`, so a badge whose deadline passes while it
//! shows reads `ui-badge-expired` on the next build (A3.8).

use dpro_core::{
    health::RebootReason,
    plan::{ChangeKind, PlanSummary, Reversibility},
    units::UnixMillis,
};

/// A badge's colors: the toolkit's `BadgeTone`, one to one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    Undoable,
    Permanent,
    Neutral,
    Info,
    Caution,
}

/// A badge's word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Text {
    /// `badge-undoable`.
    Undoable,
    /// `ui-badge-undoable-days`, with `$days`: whole days left, rounded up.
    UndoableDays { days: u64 },
    /// `ui-badge-expired`: the undo's deadline passed.
    Expired,
    /// `badge-permanent`.
    Permanent,
    /// `ui-badge-restart`.
    Restart,
    /// `ui-badge-no-restart`.
    NoRestart,
    /// `ui-badge-running`.
    Running,
    /// `ui-badge-partial`.
    Partial,
}

impl Text {
    /// Its Fluent ID.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Undoable => "badge-undoable",
            Self::UndoableDays { .. } => "ui-badge-undoable-days",
            Self::Expired => "ui-badge-expired",
            Self::Permanent => "badge-permanent",
            Self::Restart => "ui-badge-restart",
            Self::NoRestart => "ui-badge-no-restart",
            Self::Running => "ui-badge-running",
            Self::Partial => "ui-badge-partial",
        }
    }
}

/// What a badge's word means, for assistive technology.
#[derive(Debug, PartialEq, Eq)]
pub enum Description<'a, S> {
    /// `ui-badge-undoable-until-desc`, with `$until`.
    UndoableUntil { until: UnixMillis },
    /// `ui-badge-undoable-via-desc`, with the inverse change's name.
    UndoableVia { inverse: &'a ChangeKind<S> },
    /// `ui-badge-permanent-desc`, with `reason` (a Fluent ID) resolved.
    Permanent { reason: &'a S },
    /// The reason's `reboot-reason-*` message.
    Reboot(RebootReason),
}

impl<S> Description<'_, S> {
    /// Its Fluent ID.
    #[must_use]
    pub fn id(&self) -> &'static str {
        match self {
            Self::UndoableUntil { .. } => "ui-badge-undoable-until-desc",
            Self::UndoableVia { .. } => "ui-badge-undoable-via-desc",
            Self::Permanent { .. } => "ui-badge-permanent-desc",
            Self::Reboot(reason) => reason.message_id(),
        }
    }
}

impl<S> Clone for Description<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for Description<'_, S> {}

/// A badge, before the view resolves its words.
#[derive(Debug, PartialEq, Eq)]
pub struct Badge<'a, S> {
    pub tone: Tone,
    pub text: Text,
    pub description: Option<Description<'a, S>>,
}

impl<S> Clone for Badge<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for Badge<'_, S> {}

/// A running job's badge.
#[must_use]
pub const fn running<'a, S>() -> Badge<'a, S> {
    Badge {
        tone: Tone::Info,
        text: Text::Running,
        description: None,
    }
}

/// A partially applied job's badge.
#[must_use]
pub const fn partial<'a, S>() -> Badge<'a, S> {
    Badge {
        tone: Tone::Caution,
        text: Text::Partial,
        description: None,
    }
}

/// A day, in milliseconds.
const DAY_MS: u64 = 86_400_000;

/// A change's undo badge at `now`. A deadline counts the whole days left, rounded up, so the
/// last millisecond still reads "1 day"; at the deadline or past it, the undo has expired.
#[must_use]
pub fn reversibility<S>(r: &Reversibility<S>, now: UnixMillis) -> Badge<'_, S> {
    match r {
        Reversibility::Reversible => Badge {
            tone: Tone::Undoable,
            text: Text::Undoable,
            description: None,
        },
        Reversibility::ReversibleUntil { until } if until.0 > now.0 => Badge {
            tone: Tone::Undoable,
            text: Text::UndoableDays {
                days: (until.0 - now.0).div_ceil(DAY_MS),
            },
            description: Some(Description::UndoableUntil { until: *until }),
        },
        Reversibility::ReversibleUntil { .. } => Badge {
            tone: Tone::Permanent,
            text: Text::Expired,
            description: None,
        },
        Reversibility::ReversibleVia { inverse } => Badge {
            tone: Tone::Undoable,
            text: Text::Undoable,
            description: Some(Description::UndoableVia { inverse }),
        },
        Reversibility::Irreversible { reason } => Badge {
            tone: Tone::Permanent,
            text: Text::Permanent,
            description: Some(Description::Permanent { reason }),
        },
    }
}

/// A change's or a plan's restart badge, described by `reason` when there is one. Nothing to
/// restart for gives no reason, whatever `reason` says.
#[must_use]
pub fn restart<'a, S>(required: bool, reason: Option<RebootReason>) -> Badge<'a, S> {
    if required {
        Badge {
            tone: Tone::Caution,
            text: Text::Restart,
            description: reason.map(Description::Reboot),
        }
    } else {
        Badge {
            tone: Tone::Neutral,
            text: Text::NoRestart,
            description: None,
        }
    }
}

/// A plan's restart badge, from its summary.
#[must_use]
pub fn plan_restart<'a, S>(summary: &PlanSummary) -> Badge<'a, S> {
    restart(summary.reboot_required, None)
}
