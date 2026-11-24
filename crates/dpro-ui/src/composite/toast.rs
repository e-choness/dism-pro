//! A job's toast (TD-0009 A3.5 (7)): the job's title, its status line (a polite live region), its
//! progress and up to two actions, in the toast window at the owner's bottom end, built in
//! [`Model::toast_view`](crate::platform::Model::toast_view).
//!
//! The MEASURE commit's stub: the API, and the region holding its line alone.

use crate::{Cx, Element, Key, NodeId, Ui};

/// The job's title.
pub const TOAST_TITLE: Key = Key::name("dpro-ui.toast.title");
/// The status line, a polite live region.
pub const TOAST_LINE: Key = Key::name("dpro-ui.toast.line");
/// The progress bar, while there's progress to show.
pub const TOAST_BAR: Key = Key::name("dpro-ui.toast.bar");
/// The actions' row (View, Undo…).
pub const TOAST_ACTIONS: Key = Key::name("dpro-ui.toast.actions");
/// Dismiss, a glyph button at the title's end.
pub const TOAST_DISMISS: Key = Key::name("dpro-ui.toast.dismiss");

/// How a job stands: it picks the toast's glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToastTone {
    /// It runs: [`Icon::Info`](crate::Icon::Info). The toast stays until the job ends or it's
    /// dismissed.
    Running,
    /// It finished: [`Icon::Completed`](crate::Icon::Completed).
    Done,
    /// It finished with something to do (a restart) or was cancelled:
    /// [`Icon::Warning`](crate::Icon::Warning).
    Warning,
    /// It failed or stopped: [`Icon::ErrorBadge`](crate::Icon::ErrorBadge).
    Failed,
}

/// A toast, as a view describes it for [`Cx::toast`].
#[derive(Clone, Debug)]
pub struct Toast<'s, M> {
    pub tone: ToastTone,
    /// The region's name (`ui-toast-region`).
    pub region: &'s str,
    /// The job.
    pub title: &'s str,
    /// Its status (`ui-toast-*`): a polite live region, which the caller changes at most once
    /// every 2 s for progress, and at once for a phase or the end (§4.4).
    pub line: &'s str,
    /// How far it has come, 0 to 1; `None` for no bar.
    pub progress: Option<f32>,
    /// View and Undo… (`ui-toast-view`, `ui-toast-undo`), in order.
    pub actions: [Option<(&'s str, M)>; 2],
    /// Dismiss's name (`ui-toast-dismiss`) and message: Dismiss, Esc in the toast, and an ended
    /// toast's hide send it.
    pub dismiss: (&'s str, M),
}

impl<M: Clone> Cx<'_, M> {
    /// A job's toast.
    pub fn toast(&mut self, key: Key, t: &Toast<'_, M>) -> NodeId {
        self.add_with(Element::stack(key).name(t.region), |cx| {
            cx.add(Element::text(TOAST_LINE, t.line));
        })
    }
}

/// The toast's hide wake (A3.5 (7)): the `Ui` keeps it, as it keeps the caret's blink.
impl<M> Ui<M> {
    /// Windows' message duration changed (`SPI_GETMESSAGEDURATION`), in milliseconds: how long an
    /// ended toast shows.
    pub fn set_message_duration(&mut self, _ms: u32) {}

    /// How long until an ended toast hides, if one shows here: its wait starts at the first call
    /// after it ended, and starts again once the pointer leaves the window or the keyboard focus
    /// leaves it. `None` while the pointer is over it or focus is in it. The platform runs it after
    /// every event, as the caret's timer, and [`tick`](Self::tick) sends the toast's Dismiss once
    /// it's due: one wake, then nothing (U6).
    pub fn expiry_timer(&mut self, _now_ms: u64) -> Option<u64> {
        None
    }
}
