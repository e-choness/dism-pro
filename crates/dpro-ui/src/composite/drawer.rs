//! The Preview drawer's frame (TD-0009 A3.5 (6)): every change before it applies, beside the page
//! or over it, with Cancel and the primary action in a footer that stays put.
//!
//! The drawer is a [scope](crate::Element::scope) named by its title, a region landmark: Esc
//! closes it, and focus goes back to what opened it when it goes. It
//! [slides in](crate::Cx::slide_in) from the window's end over `motion.drawer`, laid out once at
//! its place. What goes in its body is the page's (A6): [sections](crate::Cx::drawer_section),
//! [callouts](crate::Cx::callout) and [code blocks](crate::Cx::code_block).
//!
//! A client sees:
//! - the drawer, a region landmark named by its title, which is a level-2 heading;
//! - Close, a button named `ui-drawer-close`, its glyph presentational;
//! - each section's title, a level-3 heading under the drawer's;
//! - a callout, a group named by its title; a code block's text, and its Copy button;
//! - the footer's Cancel and primary action. On a hold button, UI Automation's Invoke sends
//!   `invoke` (the typed confirmation), never the apply (§8).

use crate::{
    Align, Axes, Axis, Cx, Dim, Edge, Element, Icon, Insets, Justify, Key, Landmark, Look, NodeId,
    Radius, Size, Style, TextStyle, Tone,
    composite::BadgeSpec,
    tokens::{SPACE, drawer, font},
    tree::NIL,
};

use super::{SECONDARY, STRONG};

/// The header: the title and Close, the summary, the badges.
pub const DRAWER_HEADER: Key = Key::name("dpro-ui.drawer.header");
/// The title, a level-2 heading.
pub const DRAWER_TITLE: Key = Key::name("dpro-ui.drawer.title");
/// Close, a glyph button in the header's top corner.
pub const DRAWER_CLOSE: Key = Key::name("dpro-ui.drawer.close");
/// The summary: the large figure and its caption.
pub const DRAWER_SUMMARY: Key = Key::name("dpro-ui.drawer.summary");
/// The row of badges.
pub const DRAWER_BADGES: Key = Key::name("dpro-ui.drawer.badges");
/// The body, a vertical scroll viewport holding what the page puts in it.
pub const DRAWER_BODY: Key = Key::name("dpro-ui.drawer.body");
/// The footer, outside the body's viewport.
pub const DRAWER_FOOTER: Key = Key::name("dpro-ui.drawer.footer");
/// Cancel, in the footer.
pub const DRAWER_CANCEL: Key = Key::name("dpro-ui.drawer.cancel");
/// The primary action, at the footer's end: Apply, or a hold button.
pub const DRAWER_PRIMARY: Key = Key::name("dpro-ui.drawer.primary");
/// A section's title, a level-3 heading.
pub const SECTION_TITLE: Key = Key::name("dpro-ui.drawer.section.title");
/// A callout's title.
pub const CALLOUT_TITLE: Key = Key::name("dpro-ui.callout.title");
/// A callout's text.
pub const CALLOUT_TEXT: Key = Key::name("dpro-ui.callout.text");
/// A code block's text.
pub const CODE_TEXT: Key = Key::name("dpro-ui.code.text");
/// A code block's Copy button.
pub const CODE_COPY: Key = Key::name("dpro-ui.code.copy");

const TITLE_ROW: Key = Key::name("dpro-ui.drawer.title.row");
const CLOSE_GLYPH: Key = Key::name("dpro-ui.drawer.close.glyph");
const FIGURE: Key = Key::name("dpro-ui.drawer.figure");
const CAPTION: Key = Key::name("dpro-ui.drawer.caption");

/// A drawer, as a view describes it for [`Cx::drawer`].
#[derive(Clone, Debug)]
pub struct DrawerFrame<'s, M> {
    /// It names the drawer, and heads it.
    pub title: &'s str,
    /// The large figure and its caption ("2.3 GB", "to free · 41 items").
    pub summary: Option<(&'s str, &'s str)>,
    pub badges: &'s [BadgeSpec<'s>],
    /// What Close and Esc send.
    pub close: fn() -> M,
    /// Close's name: `ui-drawer-close`.
    pub close_label: &'s str,
    pub footer: DrawerFooter<'s, M>,
    /// Over the page rather than beside it ([`drawer_is_sheet`]): the drawer takes its parent's
    /// width, and the page leaves the layout and the accessibility tree (the view leaves it out).
    pub sheet: bool,
    /// The landmark's kind, for a client: `a11y-landmark-region`, resolved.
    pub region: &'s str,
}

/// A drawer's footer: Cancel, then the primary action.
#[derive(Clone, Debug)]
pub struct DrawerFooter<'s, M> {
    /// Cancel's label (`ui-drawer-cancel`) and message.
    pub cancel: (&'s str, M),
    pub primary: DrawerPrimary<'s, M>,
    /// The job is applying: the primary action is disabled (A6 shows the progress).
    pub applying: bool,
}

/// The footer's primary action.
#[derive(Clone, Debug)]
pub enum DrawerPrimary<'s, M> {
    /// A plain action, in the accent: its label and message.
    Apply(&'s str, M),
    /// A danger action held to apply ([`Element::hold`]): its label, what it sends once held for
    /// `motion.hold`, and what UI Automation's Invoke sends instead (the typed confirmation).
    Hold(&'s str, fn() -> M, fn() -> M),
}

/// A callout in a drawer's body ([`Cx::callout`]): the permanent note, in its own colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Callout<'s> {
    /// It names the callout, in body semibold.
    pub title: &'s str,
    /// The note, wrapping; empty for none.
    pub text: &'s str,
}

/// Whether a drawer beside a page in `width` logical pixels (the window less the rail) would leave
/// the page narrower than [`drawer::PAGE_MIN`]: then it's a sheet over the page instead.
#[must_use]
pub fn drawer_is_sheet(width: f32) -> bool {
    width - drawer::WIDTH < drawer::PAGE_MIN
}

impl<M: Clone> Cx<'_, M> {
    /// A drawer, its body built by `body`, at the end of a row beside the page: a pane
    /// [`drawer::WIDTH`] wide, or a [sheet](DrawerFrame::sheet) that fills the row (no narrower
    /// than [`drawer::MIN_W`]).
    ///
    /// Opening it slides it in from the window's end over `motion.drawer` (at once with
    /// animations off), and the page takes its new width at once: one layout. It moves focus to
    /// the body's first control, or to Close if the body has none. Esc closes it ([`close`]),
    /// and focus goes back to what opened it ([`Element::scope`]); a sheet's page is built again
    /// when it closes, so there focus goes to the row's first control. Tab goes through the
    /// header, the body and the footer, in that order.
    ///
    /// [`close`]: DrawerFrame::close
    pub fn drawer(
        &mut self,
        key: Key,
        d: &DrawerFrame<'_, M>,
        body: impl FnOnce(&mut Cx<'_, M>),
    ) -> NodeId {
        let size = if d.sheet {
            Style {
                grow: 1.0,
                min: Size {
                    w: drawer::MIN_W,
                    h: 0.0,
                },
                ..Style::default()
            }
        } else {
            Style {
                width: Dim::Fixed(drawer::WIDTH),
                ..Style::default()
            }
        };
        let (mut close, mut viewport) = (NIL, NIL);
        let id = self.add_with(
            Element::scope(key)
                .on_escape(d.close)
                .style(size)
                .look(PANE)
                .name(d.title)
                .landmark(Landmark::Region { kind: d.region }),
            |cx| {
                close = header(cx, d);
                viewport = cx
                    .add_with(
                        Element::scroll(DRAWER_BODY, Axes::Y).style(Style {
                            grow: 1.0,
                            gap: drawer::GAP,
                            padding: Insets {
                                left: drawer::PAD_X,
                                top: 0.0,
                                right: drawer::PAD_X,
                                bottom: drawer::GAP,
                            },
                            ..Style::default()
                        }),
                        body,
                    )
                    .index;
                footer(cx, &d.footer);
            },
        );
        // Opening moves focus to the body's first control, or to Close if it has none.
        if self.ui.scope_opened(id.index) {
            let first = self.ui.first_stop_below(viewport);
            self.ui.move_focus(if first == NIL { close } else { first });
        }
        self.slide_in(id, Edge::End);
        id
    }

    /// A section of a drawer's body: its title, a level-3 heading under the drawer's, then what
    /// `body` builds.
    pub fn drawer_section(
        &mut self,
        key: Key,
        title: &str,
        body: impl FnOnce(&mut Cx<'_, M>),
    ) -> NodeId {
        self.add_with(
            Element::stack(key).style(Style {
                gap: SPACE[2],
                ..Style::default()
            }),
            |cx| {
                cx.add(
                    Element::text(SECTION_TITLE, title)
                        .text_style(SECTION)
                        .heading(3),
                );
                body(cx);
            },
        )
    }

    /// A callout: the permanent note, a group named by its title, in the callout's colors.
    pub fn callout(&mut self, key: Key, c: &Callout<'_>) -> NodeId {
        self.add_with(
            Element::stack(key)
                .style(Style {
                    gap: SPACE[0],
                    padding: Insets::all(drawer::CALLOUT_PAD),
                    ..Style::default()
                })
                .look(CALLOUT)
                .name(c.title),
            |cx| {
                cx.add(
                    Element::text(CALLOUT_TITLE, c.title)
                        .text_style(STRONG)
                        .look(CALLOUT_INK),
                );
                if !c.text.is_empty() {
                    cx.add(
                        Element::text(CALLOUT_TEXT, c.text)
                            .text_style(PROSE)
                            .look(CALLOUT_INK),
                    );
                }
            },
        )
    }

    /// A code block: `text` (a command line) in mono, wrapping, on the inverse fill, and Copy
    /// with its label (`ui-code-copy`) and message.
    pub fn code_block(&mut self, key: Key, text: &str, copy: (&str, M)) -> NodeId {
        let (label, msg) = copy;
        self.add_with(
            Element::stack(key)
                .style(Style {
                    axis: Axis::Row,
                    gap: SPACE[1],
                    align: Align::Center,
                    padding: Insets {
                        left: drawer::CODE_PAD,
                        top: SPACE[1],
                        right: SPACE[1],
                        bottom: SPACE[1],
                    },
                    ..Style::default()
                })
                .look(CODE),
            |cx| {
                cx.add(
                    Element::text(CODE_TEXT, text)
                        .text_style(MONO)
                        .style(Style {
                            grow: 1.0,
                            ..Style::default()
                        })
                        .look(INVERSE),
                );
                cx.add(
                    Element::button(CODE_COPY, label, msg)
                        .style(Style {
                            min: Size {
                                w: 0.0,
                                h: drawer::COPY_H,
                            },
                            padding: Insets::xy(SPACE[2], 0.0),
                            ..Style::default()
                        })
                        .look(COPY),
                );
            },
        )
    }
}

/// The header; returns Close's node.
fn header<M: Clone>(cx: &mut Cx<'_, M>, d: &DrawerFrame<'_, M>) -> u32 {
    let mut close = NIL;
    cx.add_with(
        Element::stack(DRAWER_HEADER).style(Style {
            gap: SPACE[2],
            padding: Insets {
                left: drawer::PAD_X,
                top: drawer::HEADER_TOP,
                right: drawer::PAD_X,
                bottom: drawer::HEADER_BOTTOM,
            },
            ..Style::default()
        }),
        |cx| {
            cx.add_with(
                Element::stack(TITLE_ROW).style(Style {
                    axis: Axis::Row,
                    gap: SPACE[2],
                    align: Align::Start,
                    ..Style::default()
                }),
                |cx| {
                    cx.add(
                        Element::text(DRAWER_TITLE, d.title)
                            .text_style(TITLE)
                            .style(Style {
                                grow: 1.0,
                                ..Style::default()
                            })
                            .heading(2),
                    );
                    close = cx
                        .add_with(
                            // Named explicitly, so its glyph is presentational (A3.5 addition 9).
                            Element::button(DRAWER_CLOSE, "", (d.close)())
                                .name(d.close_label)
                                .style(Style {
                                    align: Align::Center,
                                    justify: Justify::Center,
                                    min: Size {
                                        w: drawer::CLOSE,
                                        h: drawer::CLOSE,
                                    },
                                    ..Style::default()
                                })
                                .look(Look::SUBTLE),
                            |cx| {
                                cx.add(Element::glyph(CLOSE_GLYPH, Icon::Cancel));
                            },
                        )
                        .index;
                },
            );
            if let Some((figure, caption)) = d.summary {
                cx.add_with(
                    Element::stack(DRAWER_SUMMARY).style(Style {
                        axis: Axis::Row,
                        gap: SPACE[1],
                        align: Align::End,
                        wrap: true,
                        ..Style::default()
                    }),
                    |cx| {
                        cx.add(Element::text(FIGURE, figure).text_style(DISPLAY));
                        cx.add(
                            Element::text(CAPTION, caption)
                                .text_style(super::SMALL)
                                .look(SECONDARY),
                        );
                    },
                );
            }
            if !d.badges.is_empty() {
                cx.badges(DRAWER_BADGES, d.badges);
            }
        },
    );
    close
}

/// The footer: Cancel, then the primary action, at its end.
fn footer<M: Clone>(cx: &mut Cx<'_, M>, f: &DrawerFooter<'_, M>) {
    let button = Style {
        min: Size {
            w: 0.0,
            h: drawer::BUTTON_H,
        },
        padding: Insets::xy(SPACE[3], 0.0),
        ..Style::default()
    };
    cx.add_with(
        Element::stack(DRAWER_FOOTER).style(Style {
            axis: Axis::Row,
            gap: SPACE[1],
            align: Align::Center,
            justify: Justify::End,
            padding: Insets::xy(drawer::PAD_X, drawer::FOOTER_PAD_Y),
            wrap: true,
            ..Style::default()
        }),
        |cx| {
            let (label, msg) = &f.cancel;
            cx.add(
                Element::button(DRAWER_CANCEL, label, msg.clone())
                    .style(button)
                    .look(Look::SECONDARY),
            );
            let primary = match &f.primary {
                DrawerPrimary::Apply(label, msg) => {
                    Element::button(DRAWER_PRIMARY, label, msg.clone()).look(Look::PRIMARY)
                }
                DrawerPrimary::Hold(label, apply, invoke) => {
                    Element::button(DRAWER_PRIMARY, label, apply())
                        .hold(*invoke)
                        .look(Look::DANGER)
                }
            };
            cx.add(
                primary
                    .style(button)
                    .text_style(STRONG)
                    .enabled(!f.applying),
            );
        },
    );
}

/// The drawer: the surface in a hairline (`layer.card` under glass), square, as a pane at the
/// window's edge is. The hairline is a transient surface's: 2 px of `WindowText` in high
/// contrast (A4.4).
const PANE: Look = Look {
    fill: Some(Tone::Surface),
    border: Some(Tone::TransientBorder),
    radius: Radius::None,
    ink: Tone::Text,
};

/// The callout: its tint in its border, rounded as a card (artboard 02).
const CALLOUT: Look = Look {
    fill: Some(Tone::CalloutFill),
    border: Some(Tone::CalloutBorder),
    radius: Radius::Card,
    ink: Tone::CalloutText,
};

const CALLOUT_INK: Look = Look {
    ink: Tone::CalloutText,
    ..Look::PLAIN
};

/// The code block: the inverse fill, rounded as a control.
const CODE: Look = Look {
    fill: Some(Tone::InverseFill),
    border: None,
    radius: Radius::Control,
    ink: Tone::InverseText,
};

const INVERSE: Look = Look {
    ink: Tone::InverseText,
    ..Look::PLAIN
};

/// Copy: a quiet button on the inverse fill, whose state layer is the fill's own ink.
const COPY: Look = Look {
    fill: None,
    border: None,
    radius: Radius::Control,
    ink: Tone::InverseText,
};

/// The title: title text, semibold, wrapping.
const TITLE: TextStyle = TextStyle {
    size: font::TITLE,
    weight: font::SEMIBOLD,
    mono: false,
    wrap: true,
};

/// The summary's figure: display text, semibold.
const DISPLAY: TextStyle = TextStyle {
    size: font::DISPLAY,
    weight: font::SEMIBOLD,
    mono: false,
    wrap: false,
};

/// A section's title: subtitle text, semibold.
const SECTION: TextStyle = TextStyle {
    size: font::SUBTITLE,
    weight: font::SEMIBOLD,
    mono: false,
    wrap: true,
};

/// A callout's note, wrapping.
const PROSE: TextStyle = TextStyle {
    size: font::BODY,
    weight: font::REGULAR,
    mono: false,
    wrap: true,
};

/// A code block's text: caption-sized mono, wrapping.
const MONO: TextStyle = TextStyle {
    size: font::CAPTION,
    weight: font::REGULAR,
    mono: true,
    wrap: true,
};
