//! The inline error panel (TD-0009 A3.5 (9)): the cause, the next step, and on Details the code, the
//! call and a log excerpt, with up to two actions and Copy details.
//!
//! The data starts as a `Finding`, or an `OsError` whose diagnosis the engine attaches with
//! TD-0006's `diagnosis_for` before it reaches dpro-app, which maps it to words; the view model
//! never reads a log. The panel shows them as given, except the excerpt: log lines are anyone's
//! text, so control characters and bidirectional overrides show as U+FFFD (TD-0009 §8, A3.8).
//!
//! A client sees a group named by the title. The title is a level-2 heading (A4.5), live when
//! the panel appears on its own (assertive, A4.4) and not when the user's action moved focus into
//! it. Details is a button that says whether it's open. Expanding reflows the panel's column
//! once; nothing in the panel changes while it's open.

use std::mem;

use crate::{
    Align, Axis, Cx, Dim, Element, Icon, Insets, Justify, Key, Live, Look, MAX_FIELD_LEN, NodeId,
    Radius, Size, Style, TextStyle, Tone,
    tokens::{SPACE, error, font},
};

use super::SECONDARY;

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

/// The title and the next step.
const TEXT: Key = Key::name("dpro-ui.error.text");
/// Everything after the glyph.
const BODY: Key = Key::name("dpro-ui.error.body");
/// Details' row, its word and its chevron.
const DETAILS_ROW: Key = Key::name("dpro-ui.error.details.row");
const DETAILS_LABEL: Key = Key::name("dpro-ui.error.details.label");
const DETAILS_CHEVRON: Key = Key::name("dpro-ui.error.details.chevron");

/// A full failure, or a partial one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorTone {
    /// Nothing was done: the danger tint and [`Icon::ErrorBadge`].
    Error,
    /// Some of it was done: the warning tint and [`Icon::Warning`].
    Partial,
}

/// A log excerpt Details shows: where it's from, and its last lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogExcerpt<'s> {
    /// `ui-error-log`: the log's path.
    pub source: &'s str,
    /// The lines; Details shows the last [`error::LOG_LINES`], each cut at 1,024 bytes, with
    /// control characters and bidirectional overrides as U+FFFD.
    pub text: &'s str,
}

/// An error panel, as a view describes it for [`Cx::error_panel`].
#[derive(Clone, Debug)]
pub struct ErrorPanel<'s, M> {
    pub tone: ErrorTone,
    /// The cause: `ui-error-title`, `ui-error-partial` or the finding's diagnosis. It names the
    /// panel.
    pub title: &'s str,
    /// `ui-error-code`, with `OsError::hresult_bits()`; empty for a finding that isn't an OS
    /// error.
    pub code: &'s str,
    /// `ui-error-api`; empty when there's no call to name.
    pub api: &'s str,
    /// `ui-error-next`, or the diagnosis' next step.
    pub next: &'s str,
    pub log: Option<LogExcerpt<'s>>,
    /// Whether Details is open; the model owns it.
    pub expanded: bool,
    /// Details' word (`ui-error-details`) and the message that opens or closes it.
    pub details: (&'s str, M),
    /// Try again (`ui-error-retry`) and Open log (`ui-error-open-log`), each if the page has it.
    pub actions: [Option<(&'s str, M)>; 2],
    /// Copy details (`ui-error-copy`) and its message.
    pub copy: (&'s str, M),
    /// [`Live::Assertive`] when the panel came unbidden (a failed listing); [`Live::Off`] when
    /// the user's action moved focus into it (a failed plan), so it isn't spoken twice (A4.4).
    pub live: Live,
}

impl<M: Clone> Cx<'_, M> {
    /// An inline error panel. Details, the actions and Copy details are Tab stops in visual
    /// order; Enter or Space on Details opens or closes it, which reflows its column once.
    pub fn error_panel(&mut self, key: Key, p: &ErrorPanel<'_, M>) -> NodeId {
        // The tint, its border, and the status color the glyph is drawn in.
        let (fill, border, status, icon) = match p.tone {
            ErrorTone::Error => (
                Tone::ErrorFill,
                Tone::DangerBorder,
                Tone::Danger,
                Icon::ErrorBadge,
            ),
            ErrorTone::Partial => (Tone::WarnFill, Tone::WarnBorder, Tone::Warn, Icon::Warning),
        };
        self.add_with(
            Element::stack(key)
                .style(Style {
                    axis: Axis::Row,
                    gap: SPACE[2],
                    align: Align::Start,
                    padding: Insets::all(error::PADDING),
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
                    ink: status,
                    ..Look::PLAIN
                }));
                // Stretched, so its wrapping text is measured at the column's width alone.
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
                                    .text_style(TITLE)
                                    .heading(2)
                                    .live(p.live),
                            );
                            cx.add(Element::text(ERROR_NEXT, p.next).text_style(PROSE));
                        });
                        details(cx, p);
                        if p.expanded {
                            expanded(cx, p);
                        }
                        actions(cx, p);
                    },
                );
            },
        )
    }
}

/// Details: its word and a chevron that says which way it goes. Named, so its parts are
/// presentational (A3.5 addition 9). In a row of its own, so it's as wide as its text, not the
/// panel.
fn details<M: Clone>(cx: &mut Cx<'_, M>, p: &ErrorPanel<'_, M>) {
    let (label, toggle) = &p.details;
    let chevron = if p.expanded {
        Icon::ChevronUp
    } else {
        Icon::ChevronDown
    };
    cx.add_with(
        Element::stack(DETAILS_ROW).style(Style {
            axis: Axis::Row,
            ..Style::default()
        }),
        |cx| {
            cx.add_with(
                Element::button(ERROR_DETAILS, "", toggle.clone())
                    .name(label)
                    .style(Style {
                        axis: Axis::Row,
                        gap: SPACE[0],
                        align: Align::Center,
                        padding: Insets::xy(SPACE[1], 0.0),
                        ..BUTTON
                    })
                    .look(Look::SUBTLE)
                    .expanded(p.expanded),
                |cx| {
                    cx.add(Element::text(DETAILS_LABEL, label));
                    cx.add(Element::glyph(DETAILS_CHEVRON, chevron));
                },
            );
        },
    );
}

/// What Details opens: the code, the call, the log's source and its last lines. A finding that
/// isn't an OS error has no code or call: an empty one takes no line.
fn expanded<M>(cx: &mut Cx<'_, M>, p: &ErrorPanel<'_, M>) {
    for (key, text) in [(ERROR_CODE, p.code), (ERROR_API, p.api)] {
        if !text.is_empty() {
            cx.add(Element::text(key, text).text_style(CAPTION));
        }
    }
    let Some(log) = p.log else {
        return;
    };
    cx.add(
        Element::text(ERROR_LOG_SOURCE, log.source)
            .text_style(CAPTION)
            .look(SECONDARY),
    );
    // The window's text buffer, as a table row's cells use it: a warm panel allocates nothing.
    let mut text = mem::take(&mut cx.ui.cell_text);
    excerpt(log.text, &mut text);
    cx.add(
        Element::text(ERROR_LOG, &text)
            .text_style(MONO)
            .style(Style {
                padding: Insets::xy(SPACE[1], SPACE[0]),
                ..Style::default()
            })
            .look(Look {
                fill: Some(Tone::Subtle),
                radius: Radius::Small,
                ..Look::PLAIN
            }),
    );
    cx.ui.cell_text = text;
}

/// The actions in their slots' order, then Copy details, wrapping when they don't fit.
fn actions<M: Clone>(cx: &mut Cx<'_, M>, p: &ErrorPanel<'_, M>) {
    cx.add_with(
        Element::stack(ERROR_ACTIONS).style(Style {
            axis: Axis::Row,
            gap: SPACE[1],
            align: Align::Center,
            wrap: true,
            ..Style::default()
        }),
        |cx| {
            // Keyed by slot, so Open log keeps its node whether Try again shows or not.
            for (i, action) in (0..).zip(&p.actions) {
                if let Some((label, msg)) = action {
                    cx.add(
                        Element::button(Key::index(i), label, msg.clone())
                            .style(BUTTON)
                            .look(Look::SECONDARY),
                    );
                }
            }
            let (label, copy) = &p.copy;
            cx.add(
                Element::button(ERROR_COPY, label, copy.clone())
                    .style(BUTTON)
                    .look(Look::SUBTLE),
            );
        },
    );
}

/// The most bytes of a log line the excerpt keeps, as a table cell lays out (A2.8): a line is
/// laid out whole, and it's anyone's text.
const LINE_BYTES: usize = 1_024;

/// The last [`error::LOG_LINES`] lines of `log` into `out`, cleared first, as the screen may show
/// them (TD-0009 §8, A3.8). Only the log's last 64 KiB are read. Each line is cut at
/// [`LINE_BYTES`]; a carriage return is dropped and a tab is a space; any other character that
/// isn't [shown](shown) is U+FFFD, so it's visible and can't reorder or hide the text around it.
fn excerpt(log: &str, out: &mut String) {
    out.clear();
    let mut from = log.len().saturating_sub(MAX_FIELD_LEN);
    while !log.is_char_boundary(from) {
        from += 1;
    }
    let log = log[from..].trim_end_matches(['\n', '\r']);
    let mut start = 0;
    let mut lines = 0;
    for (i, b) in log.bytes().enumerate().rev() {
        if b == b'\n' {
            lines += 1;
            if lines == error::LOG_LINES {
                start = i + 1;
                break;
            }
        }
    }
    for (n, line) in log[start..].split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let mut room = LINE_BYTES;
        for c in line.chars() {
            let c = match c {
                '\r' => continue,
                '\t' => ' ',
                c if shown(c) => c,
                _ => char::REPLACEMENT_CHARACTER,
            };
            let Some(left) = room.checked_sub(c.len_utf8()) else {
                break;
            };
            room = left;
            out.push(c);
        }
    }
}

/// Whether a log's character shows as itself: not a control character, a bidirectional override
/// or isolate (U+202A–U+202E, U+2066–U+2069, which would reorder the text around it), or a line
/// or paragraph separator (which would add a line past the cap).
fn shown(c: char) -> bool {
    !c.is_control()
        && !matches!(
            c,
            '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}'
        )
}

/// The title: body semibold, wrapping.
const TITLE: TextStyle = TextStyle {
    size: font::BODY,
    weight: font::SEMIBOLD,
    mono: false,
    wrap: true,
};

/// The next step: body, wrapping.
const PROSE: TextStyle = TextStyle {
    size: font::BODY,
    weight: font::REGULAR,
    mono: false,
    wrap: true,
};

/// The code, the call and the log's source: caption, wrapping (a path can be long).
const CAPTION: TextStyle = TextStyle {
    size: font::CAPTION,
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

/// A button: at least [`error::BUTTON_H`] high, its text 12 px from its sides.
const BUTTON: Style = Style {
    axis: Axis::Column,
    gap: 0.0,
    padding: Insets::xy(SPACE[2], 0.0),
    align: Align::Stretch,
    justify: Justify::Start,
    grow: 0.0,
    width: Dim::Auto,
    height: Dim::Auto,
    min: Size {
        w: 0.0,
        h: error::BUTTON_H,
    },
    max: Size::INFINITE,
    wrap: false,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn cut(log: &str) -> String {
        let mut out = String::new();
        excerpt(log, &mut out);
        out
    }

    #[test]
    fn the_excerpt_keeps_the_last_lines_whole() {
        let lines: Vec<String> = (1..=12).map(|n| format!("line {n}")).collect();
        let log = lines.join("\n") + "\n";
        assert_eq!(cut(&log), lines[4..].join("\n"));
        assert_eq!(cut("one\r\ntwo\r\n"), "one\ntwo");
        assert_eq!(cut(""), "");
        assert_eq!(cut("a\tb"), "a b");
    }

    #[test]
    fn the_excerpt_shows_what_would_hide_or_reorder_text() {
        assert_eq!(
            cut("ok\u{1b}[31m\u{7}\n\u{202e}evil\u{2069}\u{2028}x\u{85}"),
            "ok\u{fffd}[31m\u{fffd}\n\u{fffd}evil\u{fffd}\u{fffd}x\u{fffd}"
        );
    }

    #[test]
    fn a_long_line_is_cut_at_a_character_boundary_and_only_the_end_is_read() {
        let long = "é".repeat(LINE_BYTES);
        let out = cut(&long);
        assert_eq!(out.len(), LINE_BYTES);
        assert!(out.chars().all(|c| c == 'é'));
        // A log longer than a text's cap: its first line is never read.
        let big = format!("first\n{}\nlast", "x".repeat(MAX_FIELD_LEN));
        let out = cut(&big);
        assert!(
            !out.contains("first") && out.ends_with("\nlast"),
            "{}",
            out.len()
        );
    }
}
