//! A running job's card (TD-0009 A3.5 (8)), in the Activity page's Running section: its title and
//! elapsed time, Cancel, a progress bar and its status line.
//!
//! A client sees a group named by the title, holding the bar (named by the title too), Cancel (a
//! Tab stop) and the line, which is a polite live region only when the card is the job's live
//! view: the toast's line is while the toast shows (A3.5 (7)), so two never coexist.

use crate::{
    Align, Axis, Cx, Dim, Element, Insets, Key, Live, Look, NodeId, Radius, Size, Style, TextStyle,
    Tone,
    tokens::{SPACE, font, job, progress},
};

use super::{SECONDARY, SMALL};

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

const HEAD: Key = Key::name("dpro-ui.job.head");

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
    /// A running job's card: its title, elapsed time and Cancel in a row, then its bar and its
    /// line, in a card with the running border.
    pub fn job_card(&mut self, key: Key, j: &JobCard<'_, M>) -> NodeId {
        self.add_with(
            Element::stack(key)
                .name(j.title)
                .style(Style {
                    gap: SPACE[2],
                    padding: Insets::all(job::PADDING),
                    ..Style::default()
                })
                .look(Look {
                    fill: Some(Tone::Surface),
                    border: Some(Tone::RunningBorder),
                    radius: Radius::Card,
                    ink: Tone::Text,
                }),
            |cx| {
                cx.add_with(
                    Element::stack(HEAD).style(Style {
                        axis: Axis::Row,
                        gap: SPACE[3],
                        align: Align::Center,
                        ..Style::default()
                    }),
                    |cx| {
                        cx.add(
                            Element::text(JOB_TITLE, j.title)
                                .text_style(TITLE)
                                .style(Style {
                                    grow: 1.0,
                                    ..Style::default()
                                }),
                        );
                        cx.add(
                            Element::text(JOB_ELAPSED, j.elapsed)
                                .text_style(SMALL)
                                .look(SECONDARY),
                        );
                        if let Some((label, msg)) = &j.cancel {
                            cx.add(
                                Element::button(JOB_CANCEL, label, msg.clone())
                                    .enabled(!j.cancelling)
                                    .style(Style {
                                        min: Size {
                                            w: 0.0,
                                            h: job::BUTTON_H,
                                        },
                                        padding: Insets::xy(SPACE[3], 0.0),
                                        ..Style::default()
                                    })
                                    .look(Look::SECONDARY),
                            );
                        }
                    },
                );
                cx.add(
                    Element::progress(JOB_BAR, j.progress)
                        .name(j.title)
                        .style(Style {
                            height: Dim::Fixed(progress::JOB_H),
                            ..Style::default()
                        })
                        .look(Look {
                            radius: Radius::Pill,
                            ..Look::PLAIN
                        }),
                );
                cx.add(
                    Element::text(JOB_LINE, j.line)
                        .text_style(LINE)
                        .live(if j.live { Live::Polite } else { Live::Off }),
                );
            },
        )
    }
}

/// The title: subtitle text, semibold.
const TITLE: TextStyle = TextStyle {
    size: font::SUBTITLE,
    weight: font::SEMIBOLD,
    mono: false,
    wrap: false,
};

/// The line: caption text that wraps.
const LINE: TextStyle = TextStyle {
    size: font::CAPTION,
    weight: font::REGULAR,
    mono: false,
    wrap: true,
};
