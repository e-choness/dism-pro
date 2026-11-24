//! Composites (TD-0009 A3): doc 05 §5's components as [`Cx`] methods, built from the existing
//! roles as [`Cx::table`] is.
//!
//! Each composite is a method `Cx::<name>(key, &<Name>)` that takes a props struct of borrowed
//! text, numbers and messages, builds its nodes and returns its [`NodeId`](crate::NodeId).
//! Composites keep no state: the model owns what's open, checked or expanded. The toolkit adds no
//! strings either: every word, and every separator a description joins, is a prop the app resolves
//! from its `ui-` Fluent IDs (A3.4). Child keys are `Key::name` constants, as
//! [`TABLE_HEADER`](crate::TABLE_HEADER) is, so tests and the platform can find a part.
//!
//! Slice 6a has badges, summary and scan cards, the inline error panel, the batch bar, the info bar
//! and settings rows, one file each, so later slices add theirs beside them: 6b grouped rows, 6c
//! the target pill, palette, toast, job card and dialog frame, 6d the drawer frame, 6e the path
//! picker.
//!
//! They're built from the toolkit's roles and slice 6a's additions (glyphs, pills, wrapping rows,
//! check boxes with label lines, descriptions, live regions, headings and landmarks), measured by
//! the composites page (`benches/fixtures/composites.rs`, A3.7, A3.9).
//!
//! [`Cx`]: crate::Cx
//! [`Cx::table`]: crate::Cx::table

mod badge;
mod batch_bar;
mod card;
mod error_panel;
mod info_bar;
mod settings_row;
// Slice 6c.
mod dialog;
mod job_card;
mod palette;
mod toast;

pub use badge::{BadgeSpec, BadgeTone};
pub use batch_bar::{BATCH_CLEAR, BATCH_COUNT, BATCH_HIDDEN, BATCH_NOTE, BatchBar};
pub use card::{
    CARD_DELTA, CARD_DETAIL, CARD_LABEL, CARD_VALUE, CardValue, SCAN_BADGE, SCAN_CHECK,
    SCAN_DESCRIPTION, SCAN_DETAILS, SCAN_NAME, SCAN_SIZE, ScanCard, ScanLabels, ScanSize,
    StatusTone, SummaryCard,
};
pub use error_panel::{
    ERROR_ACTIONS, ERROR_API, ERROR_CODE, ERROR_COPY, ERROR_DETAILS, ERROR_GLYPH, ERROR_LOG,
    ERROR_LOG_SOURCE, ERROR_NEXT, ERROR_TITLE, ErrorPanel, ErrorTone, LogExcerpt,
};
pub use info_bar::{
    INFO_ACTION, INFO_DISMISS, INFO_GLYPH, INFO_MESSAGE, INFO_TITLE, InfoBar, InfoTone,
};
pub use settings_row::{SETTINGS_DESCRIPTION, SETTINGS_HEADING, SETTINGS_LABEL, SettingsRow};
// Slice 6c.
pub use dialog::{DIALOG_BODY, DIALOG_FOOTER, DIALOG_TITLE, DialogFrame};
pub use job_card::{JOB_BAR, JOB_CANCEL, JOB_ELAPSED, JOB_LINE, JOB_TITLE, JobCard};
pub use palette::{PALETTE_FIELD, PALETTE_LIST, PALETTE_STATUS, PaletteRow, PaletteView};
pub use toast::{
    TOAST_ACTIONS, TOAST_BAR, TOAST_DISMISS, TOAST_LINE, TOAST_TITLE, Toast, ToastTone,
};

use crate::{
    Look, Radius, TextStyle, Tone,
    tokens::font::{BODY, CAPTION, SEMIBOLD},
};

/// How an action button stands out (A3.5 (11), (13)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Emphasis {
    /// The page's main action: the accent.
    Primary,
    /// Any other.
    #[default]
    Normal,
    /// An action that can't be taken back, in a dialog (slice 6c): the danger fill. The batch
    /// bar shows it as [`Normal`](Self::Normal).
    Danger,
}

/// A card's look: the surface in a hairline (`layer.card` under glass), rounded as a card.
const CARD: Look = Look {
    fill: Some(Tone::Surface),
    border: Some(Tone::Border),
    radius: Radius::Card,
    ink: Tone::Text,
};

/// Body text, semibold: a card's name, a panel's title.
const STRONG: TextStyle = TextStyle {
    size: BODY,
    weight: SEMIBOLD,
    mono: false,
    wrap: false,
};

/// Caption text: details, descriptions.
const SMALL: TextStyle = TextStyle {
    size: CAPTION,
    weight: crate::tokens::font::REGULAR,
    mono: false,
    wrap: false,
};

/// Secondary text's look.
const SECONDARY: Look = Look {
    ink: Tone::TextSecondary,
    ..Look::PLAIN
};

/// A button's look for `emphasis`, on the page: the dialog frame's buttons. (The batch bar has
/// its own: it's the inverse of the page.)
#[allow(
    dead_code,
    reason = "slice 6c's dialog frame uses it with the code (the MEASURE commit's stub)"
)]
const fn button_look(emphasis: Emphasis) -> Look {
    match emphasis {
        Emphasis::Primary => Look::PRIMARY,
        Emphasis::Normal => Look::SECONDARY,
        Emphasis::Danger => Look::DANGER,
    }
}

/// `parts` joined by `separator` into `out`, cleared first, skipping empty parts: a description a
/// composite assembles from its props ("{description}. {size}. {badge}").
fn join(out: &mut String, separator: &str, parts: &[&str]) {
    out.clear();
    for part in parts.iter().filter(|p| !p.is_empty()) {
        if !out.is_empty() {
            out.push_str(separator);
        }
        out.push_str(part);
    }
}
