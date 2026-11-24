//! A job's toast (TD-0009 A3.5 (7)): the job's title, its status line (a polite live region), its
//! progress and up to two actions, in the toast window at the owner's bottom end, built in
//! [`Model::toast_view`](crate::platform::Model::toast_view).
//!
//! A client sees a group named `region` in the toast window's own tree. The `Ui` showing it keeps
//! its Dismiss: Esc that nothing else takes sends it, and so does an ended toast's hide once
//! Windows' message duration has passed with neither the pointer nor the keyboard focus in the
//! window. That wait costs the platform one wake ([`Ui::expiry_timer`]), then nothing (U6).

use crate::{
    Align, Axis, Cx, Dim, Element, Icon, Insets, Justify, Key, Live, Look, NodeId, Radius, Size,
    Style, TextStyle, Tone, Ui,
    tokens::{SPACE, font, progress, toast},
    tree::NIL,
    ui::MESSAGE_MS,
};

use super::{CARD, STRONG};

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

const HEAD: Key = Key::name("dpro-ui.toast.head");
const GLYPH: Key = Key::name("dpro-ui.toast.glyph");
const DISMISS_GLYPH: Key = Key::name("dpro-ui.toast.dismiss.glyph");

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

impl ToastTone {
    /// The glyph and its ink.
    const fn glyph(self) -> (Icon, Tone) {
        match self {
            Self::Running => (Icon::Info, Tone::Accent),
            Self::Done => (Icon::Completed, Tone::Ok),
            Self::Warning => (Icon::Warning, Tone::Warn),
            Self::Failed => (Icon::ErrorBadge, Tone::Danger),
        }
    }
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
    /// A job's toast: a card holding its glyph, title and Dismiss, its line, its bar while it
    /// has progress, and its actions. One per window: the `Ui` keeps its Dismiss and its hide.
    pub fn toast(&mut self, key: Key, t: &Toast<'_, M>) -> NodeId {
        let id = self.add_with(
            Element::stack(key)
                .name(t.region)
                .style(Style {
                    gap: SPACE[2],
                    padding: Insets::all(toast::PADDING),
                    ..Style::default()
                })
                .look(CARD),
            |cx| toast_parts(cx, t),
        );
        let ended = t.tone != ToastTone::Running;
        let e = &mut self.ui.expiry;
        // Another toast, or this one ending: its wait starts afresh.
        if e.ended != ended || e.toast.as_ref().is_none_or(|(n, _)| *n != id) {
            e.since = None;
            e.sent = false;
        }
        e.ended = ended;
        e.toast = Some((id, t.dismiss.1.clone()));
        id
    }
}

/// The toast's head (its glyph, title and Dismiss), line, bar and actions.
fn toast_parts<M: Clone>(cx: &mut Cx<'_, M>, t: &Toast<'_, M>) {
    let (icon, ink) = t.tone.glyph();
    cx.add_with(Element::stack(HEAD).style(ROW), |cx| {
        cx.add(Element::glyph(GLYPH, icon).look(Look { ink, ..Look::PLAIN }));
        cx.add(
            Element::text(TOAST_TITLE, t.title)
                .text_style(STRONG)
                .style(Style {
                    grow: 1.0,
                    ..Style::default()
                }),
        );
        let (name, msg) = &t.dismiss;
        cx.add_with(
            // Named, so its glyph is presentational (A3.5 addition 9). A square.
            Element::button(TOAST_DISMISS, "", msg.clone())
                .name(name)
                .style(Style {
                    align: Align::Center,
                    justify: Justify::Center,
                    min: Size {
                        w: toast::BUTTON_H,
                        h: toast::BUTTON_H,
                    },
                    ..Style::default()
                })
                .look(Look::SUBTLE),
            |cx| {
                cx.add(Element::glyph(DISMISS_GLYPH, Icon::Cancel));
            },
        );
    });
    cx.add(
        Element::text(TOAST_LINE, t.line)
            .text_style(LINE)
            .live(Live::Polite),
    );
    if let Some(p) = t.progress {
        cx.add(
            Element::progress(TOAST_BAR, p)
                .name(t.title)
                .style(Style {
                    height: Dim::Fixed(progress::JOB_H),
                    ..Style::default()
                })
                .look(Look {
                    radius: Radius::Pill,
                    ..Look::PLAIN
                }),
        );
    }
    if t.actions.iter().any(Option::is_some) {
        cx.add_with(
            Element::stack(TOAST_ACTIONS).style(Style {
                justify: Justify::End,
                ..ROW
            }),
            |cx| {
                for (i, (label, msg)) in (0..).zip(t.actions.iter().flatten()) {
                    cx.add(
                        Element::button(Key::index(i), label, msg.clone())
                            .style(Style {
                                min: Size {
                                    w: 0.0,
                                    h: toast::BUTTON_H,
                                },
                                padding: Insets::xy(SPACE[3], 0.0),
                                ..Style::default()
                            })
                            .look(Look::SECONDARY),
                    );
                }
            },
        );
    }
}

/// A row of the toast's parts, centered across.
const ROW: Style = Style {
    axis: Axis::Row,
    gap: SPACE[2],
    padding: Insets::all(0.0),
    align: Align::Center,
    justify: Justify::Start,
    grow: 0.0,
    width: Dim::Auto,
    height: Dim::Auto,
    min: Size::ZERO,
    max: Size::INFINITE,
    wrap: false,
};

/// The toast's line: caption text that wraps.
const LINE: TextStyle = TextStyle {
    size: font::CAPTION,
    weight: font::REGULAR,
    mono: false,
    wrap: true,
};

/// The toast a `Ui` shows, its Dismiss and its hide (A3.5 (7)).
#[derive(Debug)]
pub(crate) struct Expiry<M> {
    /// The toast's node and its Dismiss, as its last build left them; the node may be gone.
    toast: Option<(NodeId, M)>,
    /// It ended: it hides after the message duration.
    ended: bool,
    /// When its wait started, on the platform's clock: the first [`Ui::expiry_timer`] after it
    /// ended, or after the pointer and the focus left.
    since: Option<u64>,
    /// Its hide went out.
    sent: bool,
    /// Windows' message duration, in milliseconds.
    message_ms: u32,
}

impl<M> Expiry<M> {
    pub(crate) const fn new() -> Self {
        Self {
            toast: None,
            ended: false,
            since: None,
            sent: false,
            message_ms: MESSAGE_MS,
        }
    }
}

impl<M> Ui<M> {
    /// Windows' message duration changed (`SPI_GETMESSAGEDURATION`), in milliseconds: how long an
    /// ended toast shows. 0 is taken as Windows' default, [`MESSAGE_MS`].
    pub fn set_message_duration(&mut self, ms: u32) {
        self.expiry.message_ms = if ms == 0 { MESSAGE_MS } else { ms };
    }

    /// How long until an ended toast hides, if one shows here: its wait starts at the first call
    /// after it ended, and starts again once the pointer leaves the window or the keyboard focus
    /// leaves it. `None` while the pointer is over it or focus is in it. The platform runs it after
    /// every event, as the caret's timer, and [`tick`](Self::tick) sends the toast's Dismiss once
    /// it's due: one wake, then nothing (U6).
    pub fn expiry_timer(&mut self, now_ms: u64) -> Option<u64> {
        if !self.toast_waits() {
            self.expiry.since = None;
            return None;
        }
        let since = *self.expiry.since.get_or_insert(now_ms);
        let due = since.saturating_add(u64::from(self.expiry.message_ms));
        Some(due.saturating_sub(now_ms))
    }

    /// Sends the ended toast's Dismiss once its wait is over.
    pub(crate) fn tick_expiry(&mut self, now_ms: u64, out: &mut Vec<M>)
    where
        M: Clone,
    {
        if !self.expiry.ended || !self.toast_waits() {
            return;
        }
        let Some(since) = self.expiry.since else {
            return;
        };
        if now_ms >= since.saturating_add(u64::from(self.expiry.message_ms))
            && let Some((_, dismiss)) = &self.expiry.toast
        {
            out.push(dismiss.clone());
            self.expiry.sent = true;
        }
    }

    /// Esc that nothing took in this window: the toast's Dismiss, if a toast shows here.
    pub(crate) fn escape_toast(&self, out: &mut Vec<M>) -> bool
    where
        M: Clone,
    {
        match &self.expiry.toast {
            Some((id, dismiss)) if self.index(*id).is_some() => {
                out.push(dismiss.clone());
                true
            }
            _ => false,
        }
    }

    /// An ended toast shows here, its hide not yet sent, with neither the pointer over the window
    /// nor the keyboard focus in it.
    fn toast_waits(&self) -> bool {
        let e = &self.expiry;
        let shows = e
            .toast
            .as_ref()
            .is_some_and(|(id, _)| self.index(*id).is_some());
        let held = self.pointer.is_some() || (self.active && self.focus != NIL);
        shows && e.ended && !e.sent && !held
    }
}
