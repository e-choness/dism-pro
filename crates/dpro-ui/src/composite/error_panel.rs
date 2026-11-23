//! The inline error panel (TD-0009 A3.5 (9)): the cause, the next step, and on Details the code, the
//! call and a log excerpt, with up to two actions and Copy details.
//!
//! dpro-app's `errors::ErrorSpec` maps a `Finding` or a diagnosed `OsError` to its words; the view
//! model never reads a log. A client sees a group named by the title. The title is live when the
//! panel appears on its own (assertive, A4.4) and not when the user's action moved focus into it.

use crate::{
    Align, Axis, Cx, Element, Icon, Insets, Key, Live, Look, NodeId, Radius, Style, TextStyle,
    Tone,
    tokens::{SPACE, error, font},
};

use super::{SECONDARY, SMALL, STRONG};

/// The panel's glyph: [`Icon::ErrorBadge`], or [`Icon::Warning`] for a partial failure.
pub const ERROR_GLYPH: Key = Key::name("dpro-ui.error.glyph");
/// The cause.
pub const ERROR_TITLE: Key = Key::name("dpro-ui.error.title");
/// The next step.
pub const ERROR_NEXT: Key = Key::name("dpro-ui.error.next");
/// Details, which expands the panel.
pub const ERROR_DETAILS: Key = Key::name("dpro-ui.error.details");
/// When expanded: the code ("Code 0x80070005").
pub const ERROR_CODE: Key = Key::name("dpro-ui.error.code");
/// The call that failed.
pub const ERROR_API: Key = Key::name("dpro-ui.error.api");
/// The log's source line.
pub const ERROR_LOG_SOURCE: Key = Key::name("dpro-ui.error.log-source");
/// The log excerpt, mono, at most [`error::LOG_LINES`] lines.
pub const ERROR_LOG: Key = Key::name("dpro-ui.error.log");
/// The row of actions, then Copy details.
pub const ERROR_ACTIONS: Key = Key::name("dpro-ui.error.actions");
/// Copy details, last in [`ERROR_ACTIONS`].
pub const ERROR_COPY: Key = Key::name("dpro-ui.error.copy");

const TEXT: Key = Key::name("dpro-ui.error.text");
const BODY: Key = Key::name("dpro-ui.error.body");

/// A full failure, or a partial one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorTone {
    /// Nothing was done: the Danger tint and [`Icon::ErrorBadge`].
    Error,
    /// Some of it was done: the Warn tint and Warning.
    Partial,
}

/// A log excerpt Details shows: where it's from, and its last lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogExcerpt<'s> {
    /// `ui-error-log`: the log's path.
    pub source: &'s str,
    /// The lines; Details shows the last [`error::LOG_LINES`].
    pub text: &'s str,
}

/// An error panel, as a view describes it for [`Cx::error_panel`].
#[derive(Clone, Debug)]
pub struct ErrorPanel<'s, M> {
    pub tone: ErrorTone,
    /// The cause: `ui-error-title` or the finding's diagnosis. It names the panel.
    pub title: &'s str,
    /// `ui-error-code`.
    pub code: &'s str,
    /// `ui-error-api`.
    pub api: &'s str,
    /// `ui-error-next`.
    pub next: &'s str,
    pub log: Option<LogExcerpt<'s>>,
    /// Whether Details is open; the model owns it.
    pub expanded: bool,
    /// Details' label (`ui-error-details`) and its message.
    pub details: (&'s str, M),
    /// Try again, Open log.
    pub actions: [Option<(&'s str, M)>; 2],
    /// Copy details (`ui-error-copy`) and its message.
    pub copy: (&'s str, M),
    /// [`Live::Assertive`] when the panel came unbidden; [`Live::Off`] when the user's action
    /// moved focus into it.
    pub live: Live,
}

impl<M: Clone> Cx<'_, M> {
    /// An inline error panel. Details, the actions and Copy details are Tab stops in visual order;
    /// expanding reflows its column once.
    pub fn error_panel(&mut self, key: Key, p: &ErrorPanel<'_, M>) -> NodeId {
        let (fill, border, icon) = match p.tone {
            ErrorTone::Error => (Tone::PermanentFill, Tone::Danger, Icon::ErrorBadge),
            ErrorTone::Partial => (Tone::Subtle, Tone::Warn, Icon::Warning),
        };
        self.add_with(
            Element::stack(key)
                .style(Style {
                    axis: Axis::Row,
                    gap: SPACE[2],
                    padding: Insets::all(PADDING),
                    ..Style::default()
                })
                .look(Look {
                    fill: Some(fill),
                    border: Some(border),
                    radius: Radius::Card,
                    ink: Tone::Text,
                })
                .name(p.title),
            |cx| {
                cx.add(Element::glyph(ERROR_GLYPH, icon).look(Look {
                    ink: border,
                    ..Look::PLAIN
                }));
                cx.add_with(
                    Element::stack(BODY).style(Style {
                        gap: SPACE[1],
                        grow: 1.0,
                        ..Style::default()
                    }),
                    |cx| {
                        cx.add_with(Element::stack(TEXT), |cx| {
                            cx.add(
                                Element::text(ERROR_TITLE, p.title)
                                    .text_style(STRONG)
                                    .live(p.live),
                            );
                            cx.add(
                                Element::text(ERROR_NEXT, p.next)
                                    .text_style(WRAP)
                                    .look(SECONDARY),
                            );
                        });
                        let (label, toggle) = &p.details;
                        cx.add(
                            Element::button(ERROR_DETAILS, label, toggle.clone())
                                .look(Look::SUBTLE)
                                .expanded(p.expanded),
                        );
                        if p.expanded {
                            cx.add(Element::text(ERROR_CODE, p.code).text_style(SMALL));
                            cx.add(Element::text(ERROR_API, p.api).text_style(SMALL));
                            if let Some(log) = p.log {
                                cx.add(
                                    Element::text(ERROR_LOG_SOURCE, log.source)
                                        .text_style(SMALL)
                                        .look(SECONDARY),
                                );
                                cx.add(
                                    Element::text(ERROR_LOG, last_lines(log.text))
                                        .text_style(MONO)
                                        .style(Style {
                                            padding: Insets::all(SPACE[1]),
                                            ..Style::default()
                                        })
                                        .look(Look {
                                            fill: Some(Tone::Subtle),
                                            radius: Radius::Small,
                                            ..Look::PLAIN
                                        }),
                                );
                            }
                        }
                        cx.add_with(
                            Element::stack(ERROR_ACTIONS).style(Style {
                                axis: Axis::Row,
                                gap: SPACE[1],
                                align: Align::Center,
                                ..Style::default()
                            }),
                            |cx| {
                                for (i, (label, msg)) in (0..).zip(p.actions.iter().flatten()) {
                                    cx.add(
                                        Element::button(Key::index(i), label, msg.clone())
                                            .look(Look::SECONDARY),
                                    );
                                }
                                let (label, copy) = &p.copy;
                                cx.add(
                                    Element::button(ERROR_COPY, label, copy.clone())
                                        .look(Look::SUBTLE),
                                );
                            },
                        );
                    },
                );
            },
        )
    }
}

/// Padding inside the panel's edge (A3.5 (9)).
const PADDING: f32 = 16.0;

/// The next step wraps.
const WRAP: TextStyle = TextStyle {
    size: font::BODY,
    weight: font::REGULAR,
    mono: false,
    wrap: true,
};

/// The excerpt: mono 12, wrapping.
const MONO: TextStyle = TextStyle {
    size: font::CAPTION,
    weight: font::REGULAR,
    mono: true,
    wrap: true,
};

/// The last [`error::LOG_LINES`] lines of `text`.
fn last_lines(text: &str) -> &str {
    let mut start = text.len();
    let mut lines = 0;
    for (i, b) in text.trim_end_matches('\n').bytes().enumerate().rev() {
        if b == b'\n' {
            lines += 1;
            if lines == error::LOG_LINES {
                break;
            }
        }
        start = i;
    }
    &text[start..]
}
