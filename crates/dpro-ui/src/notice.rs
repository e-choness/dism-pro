//! Notices (TD-0009 A4.5, slice 7b): what a screen reader is told without a focus move and with
//! no lasting text on screen, as UI Automation notifications. Job progress steps, "Copied.",
//! result and filter counts, a listing done, a hold released early (A4.4's table). Text the user
//! can see change is a [live region](crate::Element::live) instead.
//!
//! The queue holds [`QUEUE`] notices, each sanitized as a single-line field takes text
//! (TD-0009 §8: no control characters, line breaks or bidi overrides) and capped at
//! [`MAX_NOTICE_CHARS`]. Each place in the queue owns its buffer, reserved for the longest text
//! the first time it's used, so queuing a notice allocates nothing after that (U22). Nothing is
//! queued unless a client listens, and the queue empties when the last one leaves.

use dpro_app::announce::Announcement;

use crate::{field::single_line_chars, ui::Ui};

/// Notices waiting at most: a ninth drops the oldest.
pub(crate) const QUEUE: usize = 8;
/// A notice's text is cut to this many characters before it becomes a BSTR (A4.8).
pub const MAX_NOTICE_CHARS: usize = 256;

/// What happened, as UI Automation's `NotificationKind` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    ItemAdded,
    ItemRemoved,
    ActionCompleted,
    ActionAborted,
    Other,
}

/// Which notices replace each other: a notice with [`Notice::replace`] set drops the queued ones
/// of the same activity, and a client speaking the newest drops older ones of it. The platform
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
    /// `announcement` as A4.4's table delivers it, saying `text` (its message, resolved). Each
    /// replaces the older notices of its activity; only a hold released early is important.
    #[must_use]
    pub fn of(announcement: &Announcement, text: &'s str) -> Self {
        let (kind, important, activity) = match *announcement {
            Announcement::Progress { slot, .. } => (
                NoticeKind::Other,
                false,
                Activity::Progress(u32::from(slot)),
            ),
            Announcement::Copied => (NoticeKind::ActionCompleted, false, Activity::Copy),
            Announcement::Results(_) | Announcement::FilterCount { .. } => {
                (NoticeKind::Other, false, Activity::Count)
            }
            Announcement::Listed(_) => (NoticeKind::ItemAdded, false, Activity::Count),
            Announcement::HoldReleased => (NoticeKind::ActionAborted, true, Activity::Hold),
        };
        Self {
            text,
            kind,
            important,
            replace: true,
            activity,
        }
    }
}

/// One place in the queue, its buffer kept at its working size.
#[derive(Debug)]
struct Queued {
    text: String,
    kind: NoticeKind,
    important: bool,
    replace: bool,
    activity: Activity,
}

impl Queued {
    const EMPTY: Self = Self {
        text: String::new(),
        kind: NoticeKind::Other,
        important: false,
        replace: false,
        activity: Activity::Count,
    };

    fn notice(&self) -> Notice<'_> {
        Notice {
            text: self.text.as_str(),
            kind: self.kind,
            important: self.important,
            replace: self.replace,
            activity: self.activity,
        }
    }
}

/// Notices waiting for the platform, oldest first: `queue[..len]`.
#[derive(Debug)]
pub(crate) struct Notices {
    queue: [Queued; QUEUE],
    len: usize,
}

impl Default for Notices {
    fn default() -> Self {
        Self {
            queue: [Queued::EMPTY; QUEUE],
            len: 0,
        }
    }
}

impl Notices {
    /// Queues `n`'s text, sanitized and capped, after dropping the queued notices of its activity
    /// if it replaces them, or the oldest if the queue is full. A text with nothing left of it is
    /// dropped, and drops nothing.
    fn push(&mut self, n: Notice<'_>) {
        if single_line_chars(n.text).next().is_none() {
            return;
        }
        if n.replace {
            for j in (0..self.len).rev() {
                if self.queue[j].activity == n.activity {
                    self.remove(j);
                }
            }
        }
        if self.len == QUEUE {
            self.remove(0);
        }
        let q = &mut self.queue[self.len];
        q.text.clear();
        // At most 4 bytes a character: reserved once, so a long text never reallocates.
        q.text.reserve(MAX_NOTICE_CHARS * 4);
        q.text
            .extend(single_line_chars(n.text).take(MAX_NOTICE_CHARS));
        q.kind = n.kind;
        q.important = n.important;
        q.replace = n.replace;
        q.activity = n.activity;
        self.len += 1;
    }

    /// Drops queued notice `j`; its buffer moves to the free end.
    fn remove(&mut self, j: usize) {
        self.queue[j..self.len].rotate_left(1);
        self.len -= 1;
    }

    /// Drops every queued notice, keeping the buffers.
    pub(crate) const fn clear(&mut self) {
        self.len = 0;
    }
}

impl<M> Ui<M> {
    /// Queues `notice` for the platform to raise on the window. Dropped at once unless a client
    /// listens: then its text is copied, sanitized and capped at [`MAX_NOTICE_CHARS`], dropping
    /// the queued notices of its activity if it [replaces](Notice::replace) them, or the oldest
    /// of 8.
    pub fn notify(&mut self, notice: Notice<'_>) {
        if self.a11y.active() {
            self.notices.push(notice);
        }
    }

    /// Hands the queued notices to `f`, oldest first, and empties the queue.
    pub fn take_notices(&mut self, mut f: impl FnMut(Notice<'_>)) {
        let n = &mut self.notices;
        for q in &n.queue[..n.len] {
            f(q.notice());
        }
        n.clear();
    }
}
