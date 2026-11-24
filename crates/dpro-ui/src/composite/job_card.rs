//! A running job's card (TD-0009 A3.5 (8)), in the Activity page's Running section: its title and
//! elapsed time, Cancel, a progress bar and its status line.
//!
//! The MEASURE commit's stub: the API, and the group holding its line alone.

use crate::{Cx, Element, Key, NodeId};

/// The job's title.
pub const JOB_TITLE: Key = Key::name("dpro-ui.job.title");
/// How long it has run.
pub const JOB_ELAPSED: Key = Key::name("dpro-ui.job.elapsed");
/// Cancel.
pub const JOB_CANCEL: Key = Key::name("dpro-ui.job.cancel");
/// The progress bar, named by the title.
pub const JOB_BAR: Key = Key::name("dpro-ui.job.bar");
/// The status line, live when the card's view of the job is the live one.
pub const JOB_LINE: Key = Key::name("dpro-ui.job.line");

/// A job card, as a view describes it for [`Cx::job_card`].
#[derive(Clone, Debug)]
pub struct JobCard<'s, M> {
    /// The job: it names the card and its bar.
    pub title: &'s str,
    /// How long it has run, as the caller formats it once a second (`shell-activity-*`).
    pub elapsed: &'s str,
    /// How far it has come, 0 to 1.
    pub progress: f32,
    pub line: &'s str,
    /// Whether the line is a live region: only one view of a job is, the toast's while it shows
    /// (A3.5 (7)).
    pub live: bool,
    /// Cancel's text (`shell-activity-cancel`, then `ui-cancelling`) and message; `None` for a job
    /// that can't be cancelled.
    pub cancel: Option<(&'s str, M)>,
    /// A cancel was asked for: Cancel is disabled until the job ends.
    pub cancelling: bool,
}

impl<M: Clone> Cx<'_, M> {
    /// A running job's card.
    pub fn job_card(&mut self, key: Key, j: &JobCard<'_, M>) -> NodeId {
        self.add_with(Element::stack(key).name(j.title), |cx| {
            cx.add(Element::text(JOB_LINE, j.line));
        })
    }
}
