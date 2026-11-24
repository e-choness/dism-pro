//! Notices (TD-0009 A4.5, slice 7b): what a screen reader is told without a focus move and with
//! no lasting text on screen, as UI Automation notifications. Job progress steps, "Copied.",
//! result and filter counts, a listing done, a hold released early (A4.4's table). Text the user
//! can see change is a [live region](crate::Element::live) instead.

use dpro_app::announce::Announcement;

use crate::ui::Ui;

/// What happened, as UI Automation's `NotificationKind` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    ItemAdded,
    ItemRemoved,
    ActionCompleted,
    ActionAborted,
    Other,
}

/// Which notices replace each other: a notice with [`Notice::replace`] set drops a queued one of
/// the same activity, and a client speaking the newest drops older ones of it. The platform
/// names it `DismPro.Progress.<n>`, `DismPro.Copy`, `DismPro.Count` or `DismPro.Hold`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// A job's progress, by its announcer slot.
    Progress(u32),
    Copy,
    Count,
    Hold,
}

/// A notice to speak. Its text is the app's, resolved from the catalog; it's borrowed, as every
/// [`Element`](crate::Element) string is, and copied into the queue sanitized and capped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Notice<'s> {
    pub text: &'s str,
    pub kind: NoticeKind,
    /// Spoken even over other speech (UI Automation's `Important…` processing).
    pub important: bool,
    /// Supersedes earlier notices of its activity (`…MostRecent`), rather than queuing behind
    /// them (`…All`).
    pub replace: bool,
    pub activity: Activity,
}

impl<'s> Notice<'s> {
    /// `announcement` as A4.4's table delivers it, saying `text` (its message, resolved).
    #[must_use]
    pub const fn of(announcement: &Announcement, text: &'s str) -> Self {
        let _ = announcement;
        Self {
            text,
            kind: NoticeKind::Other,
            important: false,
            replace: true,
            activity: Activity::Count,
        }
    }
}

/// Notices waiting for the platform.
#[derive(Debug, Default)]
pub(crate) struct Notices;

impl<M> Ui<M> {
    /// Queues `notice` for the platform to raise on the window. Dropped at once unless a client
    /// listens.
    pub fn notify(&mut self, notice: Notice<'_>) {
        let _ = (notice, &mut self.notices);
    }

    /// Hands the queued notices to `f`, oldest first, and empties the queue.
    pub fn take_notices(&mut self, mut f: impl FnMut(Notice<'_>)) {
        let _ = (&mut f, &mut self.notices);
    }
}
