//! Design tokens (doc 05 §4) and the [`Theme`] that gives color tokens their values (TD-0009 §5.1).
//!
//! Views name colors by [`Tone`] and sizes by the constants in [`tokens`], never by value
//! (CLAUDE.md non-negotiable 7). Painting looks tones up in the theme it's given, so switching
//! between light and dark is a repaint ([`Ui::repaint_all`](crate::Ui::repaint_all)), not a
//! rebuild.
//!
//! The colors are doc 05's solid-fallback values, which every contrast check is made against
//! (tested below: WCAG 2.2 AA). [`Theme::glass`] turns light or dark into the glass materials
//! (ADR-0007), for a composed window over Mica.
//!
//! The composites' colors (TD-0009 §12 items 9 and 10, A3.5, decisions-log #45) are here too, and
//! nowhere else: the badges' text and fill pairs, `danger.border`, the error, warning and info
//! tints with their borders, the batch bar's inverse four, and glass's own `text.secondary`. The
//! design docs give the light values; the dark ones are computed so that every text pair is at
//! least 4.5:1, which the tests below check in light, dark, glass and high contrast.
//!
//! **High contrast** ([`Theme::high_contrast`], TD-0009 §4.4) shows the system's colors only: no
//! tints, no state layers. Text takes the color the system pairs with the fill under it
//! ([`Theme::ink`]): `HighlightText` on the highlight fills (accent, danger, selection, the batch
//! bar's primary action), `ButtonText` on `ButtonFace`, else `WindowText`, with `GrayText` for
//! secondary text and `HotLight` for accent text. The focus ring is `Highlight`.

/// An sRGB color with straight (not premultiplied) alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// An opaque color from `0xRRGGBB`.
    #[must_use]
    pub const fn hex(rgb: u32) -> Self {
        let [_, r, g, b] = rgb.to_be_bytes();
        Self { r, g, b, a: 255 }
    }

    /// The same color at `a` (0–255).
    #[must_use]
    pub const fn alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// `self` composited over the opaque `under`, in sRGB like the compositor: a translucent
    /// token flattened into its solid fallback.
    #[must_use]
    pub const fn over(self, under: Self) -> Self {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "div255 of at most 255 × 255 is at most 255"
        )]
        const fn mix(top: u8, under: u8, alpha: u8) -> u8 {
            let (t, u, a) = (top as u32, under as u32, alpha as u32);
            div255(t * a + u * (255 - a)) as u8
        }
        Self {
            r: mix(self.r, under.r, self.a),
            g: mix(self.g, under.g, self.a),
            b: mix(self.b, under.b, self.a),
            a: 255,
        }
    }
}

/// `x / 255`, rounded to nearest, for `x ≤ 255 × 255`: exact, without a division.
pub(crate) const fn div255(x: u32) -> u32 {
    let x = x + 128;
    (x + (x >> 8)) >> 8
}

/// A color token: what a color is for, not what it is (doc 05 §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    /// `bg.app`: the window.
    App,
    /// `bg.surface`: cards and tables.
    Surface,
    /// `bg.subtle`: the rail, quiet fills.
    Subtle,
    /// `border`: hairlines.
    Border,
    /// `text.primary`, and the focus ring.
    Text,
    /// `text.secondary`, and disabled text. Glass has its own, darker in light and lighter in
    /// dark, for the white layers it stacks (§12 item 9).
    TextSecondary,
    /// `accent`: primary actions, the online target.
    Accent,
    /// Text and icons on an accent or danger fill.
    OnAccent,
    /// `target.offline`: the offline-install pill.
    Offline,
    /// `target.image`: the mounted-image pill.
    Image,
    Ok,
    Warn,
    Danger,
    /// The Permanent badge's text.
    PermanentText,
    /// The Permanent badge's fill.
    PermanentFill,
    /// `fill.selected`: accent at 13–14 %, flattened over the surface.
    Selected,
    /// The state layer over a hovered control: ink at 6 %, over any fill.
    Hover,
    /// The state layer over a pressed control: ink at 4 %, lighter than hover, as on Windows 11's
    /// subtle buttons.
    Pressed,
    // The badges (A3.5 (1), decision #45): one text and fill pair per tone. Permanent's is the
    // pair above. Badges stay opaque under glass.
    /// The Undoable badge's text.
    UndoableText,
    /// The Undoable badge's fill.
    UndoableFill,
    /// The Neutral badge's text: No restart needed.
    NeutralText,
    /// The Neutral badge's fill: ink at 6 %, flattened over the surface.
    NeutralFill,
    /// The Running badge's text (`BadgeTone::Info`: a running job, something new).
    RunningText,
    /// The Running badge's fill: accent at 13–14 %, flattened over the surface.
    RunningFill,
    /// The Caution badge's text: Restart required, Partially applied.
    CautionText,
    /// The Caution badge's fill.
    CautionFill,
    /// `danger.border`: danger at 30 %. The outlined danger button's border
    /// ([`Look::DANGER_OUTLINE`](crate::Look::DANGER_OUTLINE)), and the error panel's and an error
    /// info bar's (A3.5 (9), (14)).
    DangerBorder,
    /// The error panel's tint, and an error info bar's: danger at 8 %, flattened over the
    /// surface. Opaque under glass too, so the message reads the same over any wallpaper.
    ErrorFill,
    /// A partial failure's panel tint, and a warning info bar's: warn at 8 %, flattened.
    WarnFill,
    /// Their border: warn at 30 %.
    WarnBorder,
    /// An info bar's tint (`InfoTone::Info`): accent at 8 %, flattened.
    InfoFill,
    /// Its border: accent at 30 %.
    InfoBorder,
    /// The batch bar's fill (A3.5 (11)): primary text's color, so the bar stands out.
    InverseFill,
    /// Text and glyphs on [`InverseFill`](Self::InverseFill).
    InverseText,
    /// The batch bar's primary action, on [`InverseFill`](Self::InverseFill).
    InverseAccent,
    /// Text on [`InverseAccent`](Self::InverseAccent).
    OnInverseAccent,
}

impl Tone {
    /// Every tone, in declaration order (the theme's table order).
    pub const ALL: [Self; 36] = [
        Self::App,
        Self::Surface,
        Self::Subtle,
        Self::Border,
        Self::Text,
        Self::TextSecondary,
        Self::Accent,
        Self::OnAccent,
        Self::Offline,
        Self::Image,
        Self::Ok,
        Self::Warn,
        Self::Danger,
        Self::PermanentText,
        Self::PermanentFill,
        Self::Selected,
        Self::Hover,
        Self::Pressed,
        Self::UndoableText,
        Self::UndoableFill,
        Self::NeutralText,
        Self::NeutralFill,
        Self::RunningText,
        Self::RunningFill,
        Self::CautionText,
        Self::CautionFill,
        Self::DangerBorder,
        Self::ErrorFill,
        Self::WarnFill,
        Self::WarnBorder,
        Self::InfoFill,
        Self::InfoBorder,
        Self::InverseFill,
        Self::InverseText,
        Self::InverseAccent,
        Self::OnInverseAccent,
    ];

    /// A fill high contrast shows as `Highlight`: accent, danger, the selection and the batch
    /// bar's primary action. Text on it is `HighlightText`.
    const fn is_highlight(self) -> bool {
        matches!(
            self,
            Self::Accent | Self::Danger | Self::Selected | Self::InverseAccent
        )
    }
}

/// Light, dark, or the system's high-contrast colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scheme {
    Light,
    Dark,
    HighContrast,
}

/// The system colors high contrast shows (`GetSysColor`), the only colors in that mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SysColors {
    pub window: Color,
    pub window_text: Color,
    pub highlight: Color,
    pub highlight_text: Color,
    pub hot_light: Color,
    pub gray_text: Color,
    pub btn_face: Color,
    pub btn_text: Color,
}

/// The colors of every [`Tone`] in one scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    scheme: Scheme,
    colors: [Color; Tone::ALL.len()],
    /// High contrast's system colors, which pick the ink by the fill under it.
    sys: Option<SysColors>,
}

/// The state layers' ink alphas: 6 % and 4 % (15 and 10 of 255).
const HOVER_ALPHA: u8 = 15;
const PRESSED_ALPHA: u8 = 10;
/// `fill.selected`'s accent alpha: 13 % light, 14 % dark (34 and 36 of 255).
const SELECTED_LIGHT: u8 = 34;
const SELECTED_DARK: u8 = 36;
/// The error, warning and info tints: their color at 8 % (20 of 255) over the surface (A3.5 (9)).
const TINT_ALPHA: u8 = 20;
/// `danger.border` and the tints' borders: their color at 30 % (77 of 255; decision #45).
const OUTLINE_ALPHA: u8 = 77;
/// Dark's Undoable and Caution badge fills: their status color at 16 % (41 of 255) over the
/// surface, as dark as the Permanent badge's tint (decision #45 left the dark values to compute).
const BADGE_TINT_DARK: u8 = 41;

impl Theme {
    pub const LIGHT: Self = {
        let surface = Color::hex(0xFF_FFFF);
        let accent = Color::hex(0x25_63EB);
        let ink = Color::hex(0x14_171C);
        let warn = Color::hex(0xB4_5309);
        let danger = Color::hex(0xB9_1C1C);
        Self {
            scheme: Scheme::Light,
            colors: [
                Color::hex(0xF6_F7F9),
                surface,
                Color::hex(0xEE_F0F4),
                Color::hex(0xDD_E1E8),
                ink,
                Color::hex(0x5A_6270),
                accent,
                Color::hex(0xFF_FFFF),
                Color::hex(0xB4_5309),
                Color::hex(0x7C_3AED),
                Color::hex(0x15_803D),
                warn,
                danger,
                Color::hex(0x9A_3412),
                Color::hex(0xFD_EDE4),
                accent.alpha(SELECTED_LIGHT).over(surface),
                ink.alpha(HOVER_ALPHA),
                ink.alpha(PRESSED_ALPHA),
                // The badges' pairs (§12 item 10, decision #45): Undoable, Neutral on ink at 6 %,
                // Running on accent at 13 %, Caution.
                Color::hex(0x16_6534),
                Color::hex(0xE7_F6EC),
                Color::hex(0x37_4151),
                ink.alpha(HOVER_ALPHA).over(surface),
                Color::hex(0x1E_40AF),
                accent.alpha(SELECTED_LIGHT).over(surface),
                Color::hex(0x7C_3F06),
                Color::hex(0xFE_F6E7),
                // `danger.border`; the error, warning and info tints and borders.
                danger.alpha(OUTLINE_ALPHA),
                danger.alpha(TINT_ALPHA).over(surface),
                warn.alpha(TINT_ALPHA).over(surface),
                warn.alpha(OUTLINE_ALPHA),
                accent.alpha(TINT_ALPHA).over(surface),
                accent.alpha(OUTLINE_ALPHA),
                // The inverse four (A3.5 (11), from artboard 08).
                ink,
                Color::hex(0xE8_EAEE),
                Color::hex(0x6E_A0FF),
                Color::hex(0x0B_1630),
            ],
            sys: None,
        }
    };

    pub const DARK: Self = {
        let surface = Color::hex(0x17_1A21);
        let accent = Color::hex(0x6E_A0FF);
        let ink = Color::hex(0xFF_FFFF);
        let ok = Color::hex(0x4A_DE80);
        let warn = Color::hex(0xF0_B254);
        let danger = Color::hex(0xF8_7171);
        Self {
            scheme: Scheme::Dark,
            colors: [
                Color::hex(0x0F_1115),
                surface,
                Color::hex(0x1E_222B),
                Color::hex(0x2A_2F3A),
                Color::hex(0xE8_EAEE),
                Color::hex(0x9A_A3B2),
                accent,
                Color::hex(0x0F_1115),
                Color::hex(0xF0_B254),
                Color::hex(0xB7_9CFF),
                ok,
                warn,
                danger,
                // Doc 05 gives the Permanent badge for light only: danger on a dark red tint.
                Color::hex(0xF8_7171),
                Color::hex(0x3A_1D1F),
                accent.alpha(SELECTED_DARK).over(surface),
                ink.alpha(HOVER_ALPHA),
                ink.alpha(PRESSED_ALPHA),
                // The badges' pairs, computed (decision #45): light text on its tone's tint, as
                // the Permanent badge is. Neutral's fill is white at 6 %, Running's the selection.
                Color::hex(0x86_EFAC),
                ok.alpha(BADGE_TINT_DARK).over(surface),
                Color::hex(0xD1_D5DB),
                ink.alpha(HOVER_ALPHA).over(surface),
                Color::hex(0x93_C5FD),
                accent.alpha(SELECTED_DARK).over(surface),
                Color::hex(0xFC_D34D),
                warn.alpha(BADGE_TINT_DARK).over(surface),
                // `danger.border`; the error, warning and info tints and borders.
                danger.alpha(OUTLINE_ALPHA),
                danger.alpha(TINT_ALPHA).over(surface),
                warn.alpha(TINT_ALPHA).over(surface),
                warn.alpha(OUTLINE_ALPHA),
                accent.alpha(TINT_ALPHA).over(surface),
                accent.alpha(OUTLINE_ALPHA),
                // The inverse four, light's mirrored (A3.5 (11)).
                Color::hex(0xE8_EAEE),
                Color::hex(0x14_171C),
                Color::hex(0x25_63EB),
                Color::hex(0xFF_FFFF),
            ],
            sys: None,
        }
    };

    /// High contrast: the system's colors only. Fills map to `Window`, `ButtonFace` (quiet
    /// fills) and `Highlight` (accent, danger, selection, the batch bar's primary action); borders
    /// to `WindowText`; the state layers vanish. Badges, tints and the batch bar have no fill:
    /// they're `Window`, with `WindowText` text and borders (A3.5 (1), (9), (11), (14)). Text goes
    /// through [`ink`](Self::ink).
    #[must_use]
    pub const fn high_contrast(sys: SysColors) -> Self {
        let none = Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        };
        Self {
            scheme: Scheme::HighContrast,
            colors: [
                sys.window,
                sys.window,
                sys.btn_face,
                sys.window_text,
                sys.window_text,
                sys.gray_text,
                sys.highlight,
                sys.highlight_text,
                sys.window_text,
                sys.window_text,
                sys.window_text,
                sys.window_text,
                sys.highlight,
                sys.window_text,
                sys.window,
                sys.highlight,
                none,
                none,
                // Badges: Undoable, Neutral, Running, Caution.
                sys.window_text,
                sys.window,
                sys.window_text,
                sys.window,
                sys.window_text,
                sys.window,
                sys.window_text,
                sys.window,
                // `danger.border`; the tints and their borders.
                sys.window_text,
                sys.window,
                sys.window,
                sys.window_text,
                sys.window,
                sys.window_text,
                // The inverse four.
                sys.window,
                sys.window_text,
                sys.highlight,
                sys.highlight_text,
            ],
            sys: Some(sys),
        }
    }

    /// The glass materials (ADR-0007, doc 05 §4) of light or dark: the window's background is
    /// clear, so Mica shows; cards (`layer.card`), quiet fills (`layer.control`), their hairlines
    /// and the selection are translucent layers over it. Secondary text takes glass's own
    /// `text.secondary` (§12 item 9, decision #45), which keeps 4.5:1 where the white layers
    /// stack. Text, accent, status colors, badges, tints and buttons stay opaque, as do the state
    /// layers. High contrast is never glass, and comes back as it is.
    #[must_use]
    pub const fn glass(self) -> Self {
        // Doc 05's percentages of 255, rounded: 62, 70, 78 % light; 5, 6, 8 % dark.
        let (card, control, border, selected, secondary) = match self.scheme {
            Scheme::Light => (158, 179, 199, SELECTED_LIGHT, Color::hex(0x58_606E)),
            Scheme::Dark => (13, 15, 20, SELECTED_DARK, Color::hex(0xAC_B5C4)),
            Scheme::HighContrast => return self,
        };
        let white = Color::hex(0xFF_FFFF);
        let mut colors = self.colors;
        colors[Tone::App as usize] = white.alpha(0);
        colors[Tone::Surface as usize] = white.alpha(card);
        colors[Tone::Subtle as usize] = white.alpha(control);
        colors[Tone::Border as usize] = white.alpha(border);
        // `fill.selected` unflattened: accent at 13–14 % over whatever is under it.
        colors[Tone::Selected as usize] = self.color(Tone::Accent).alpha(selected);
        colors[Tone::TextSecondary as usize] = secondary;
        Self { colors, ..self }
    }

    /// Whether this is [`glass`](Self::glass): its background is clear.
    #[must_use]
    pub const fn is_glass(&self) -> bool {
        self.color(Tone::App).a == 0
    }

    #[must_use]
    pub const fn scheme(&self) -> Scheme {
        self.scheme
    }

    /// `tone` as a fill.
    #[must_use]
    pub const fn color(&self, tone: Tone) -> Color {
        self.colors[tone as usize]
    }

    /// The tone a fill of `tone` shows as. High contrast keeps `Highlight` for what can be used:
    /// a disabled highlight fill (accent, danger, selected, the batch bar's primary action) shows
    /// as the window, so its `GrayText` reads as disabled. Light and dark: `tone`.
    #[must_use]
    pub const fn fill(&self, tone: Tone, enabled: bool) -> Tone {
        if self.sys.is_some() && !enabled && tone.is_highlight() {
            Tone::Surface
        } else {
            tone
        }
    }

    /// `tone` as a border or outline. High contrast: `WindowText`, or `GrayText` when disabled.
    #[must_use]
    pub const fn stroke(&self, tone: Tone, enabled: bool) -> Color {
        match self.sys {
            Some(sys) if enabled => sys.window_text,
            Some(sys) => sys.gray_text,
            None => self.color(tone),
        }
    }

    /// High contrast has no tints, so a hovered or pressed control on a fill of `under` shows an
    /// outline instead of a state layer: `Highlight`, or `HighlightText` on a highlight fill.
    /// `None` in light and dark.
    #[must_use]
    pub const fn hover_outline(&self, under: Tone) -> Option<Color> {
        match self.sys {
            Some(sys) if under.is_highlight() => Some(sys.highlight_text),
            Some(sys) => Some(sys.highlight),
            None => None,
        }
    }

    /// A state layer, [`Hover`](Tone::Hover) or [`Pressed`](Tone::Pressed) (ink at 6 or 4 %),
    /// over a fill of `under`: in the ink that pairs with that fill. Primary text's everywhere but
    /// on the batch bar's [`InverseFill`](Tone::InverseFill), whose ink is the other way round,
    /// where primary text's color at 6 % wouldn't show (A3.5 (11)). Clear in high contrast,
    /// which outlines instead ([`hover_outline`](Self::hover_outline)).
    #[must_use]
    pub const fn layer(&self, layer: Tone, under: Tone) -> Color {
        let c = self.color(layer);
        match under {
            Tone::InverseFill => self.color(Tone::InverseText).alpha(c.a),
            _ => c,
        }
    }

    /// `tone` as text or a glyph over a fill of `under`. Light and dark: the tone's own color,
    /// whose contrast on every fill it's used on is tested. High contrast: the color the system
    /// pairs with that fill.
    #[must_use]
    pub const fn ink(&self, tone: Tone, under: Tone) -> Color {
        let Some(sys) = self.sys else {
            return self.color(tone);
        };
        match under {
            under if under.is_highlight() => sys.highlight_text,
            Tone::Subtle => sys.btn_text,
            // A disabled, checked box: GrayText's partner.
            Tone::TextSecondary => sys.window,
            _ => match tone {
                Tone::TextSecondary => sys.gray_text,
                Tone::Accent => sys.hot_light,
                _ => sys.window_text,
            },
        }
    }

    /// The keyboard focus ring: text color, or `Highlight` in high contrast (TD-0009 §4.4).
    #[must_use]
    pub const fn focus(&self) -> Color {
        match self.sys {
            Some(sys) => sys.highlight,
            None => self.color(Tone::Text),
        }
    }
}

/// Sizes and durations (doc 05 §4), in logical pixels and milliseconds.
pub mod tokens {
    /// Spacing on the 4 px grid.
    pub const SPACE: [f32; 6] = [4.0, 8.0, 12.0, 16.0, 24.0, 32.0];

    /// Corner radii.
    pub mod radius {
        /// Badges and check boxes (the canvas's badges).
        pub const SMALL: f32 = 4.0;
        pub const CONTROL: f32 = 6.0;
        pub const CARD: f32 = 10.0;
        /// Drawers and dialogs.
        pub const DRAWER: f32 = 14.0;
    }

    /// The type scale (Segoe UI Variable; Cascadia Mono for paths and ids) and its two weights.
    pub mod font {
        pub const CAPTION: f32 = 12.0;
        pub const BODY: f32 = 13.0;
        pub const SUBTITLE: f32 = 15.0;
        pub const TITLE: f32 = 20.0;
        pub const DISPLAY: f32 = 28.0;
        pub const REGULAR: u16 = 400;
        pub const SEMIBOLD: u16 = 600;
        /// The badge step (§12 item 10, decision #45): the scale's smallest size, at
        /// [`BADGE_WEIGHT`]. Windows' smallest text is Fluent's Caption, 12, so there's no 11.
        pub const BADGE: f32 = CAPTION;
        pub const BADGE_WEIGHT: u16 = SEMIBOLD;
    }

    /// Strokes.
    pub mod stroke {
        /// The keyboard focus ring's width (doc 05 §6).
        pub const FOCUS: f32 = 2.0;
        /// A hairline drawn as a box: the dividers between settings rows (A3.5 (15)).
        pub const HAIRLINE: f32 = 1.0;
        /// `stroke.contrast` (TD-0009 A4.4, A4.10 item 7): a transient surface's border in high
        /// contrast, as Microsoft asks of flyouts and dialogs.
        pub const CONTRAST: f32 = 2.0;
    }

    /// Menus, dropdown lists and tooltips (TD-0009 A1.10 item 1: the metrics of `WinUI`, until the
    /// canvas draws them).
    pub mod popup {
        /// Inside a menu's or a list's edge.
        pub const PADDING: f32 = 4.0;
        /// An item's least height, and a dropdown's.
        pub const ITEM_HEIGHT: f32 = 32.0;
        /// An item's text from its sides.
        pub const ITEM_PAD: f32 = 12.0;
        /// A separator: a hairline with 4 px above and below.
        pub const SEPARATOR: f32 = 9.0;
        /// Between a popup and its anchor.
        pub const GAP: f32 = 4.0;
        /// A tooltip's text from its sides, and from its top and bottom.
        pub const TIP_PAD_X: f32 = 8.0;
        pub const TIP_PAD_Y: f32 = 5.0;
        /// A tooltip's widest; longer text wraps.
        pub const TIP_MAX_W: f32 = 320.0;
        /// A menu's tallest, as a share of the work area's height.
        pub const MAX_HEIGHT: f32 = 0.5;
        /// Autopop: a tooltip hides after this many of its delays (Windows' `TTDT_AUTOPOP`).
        pub const TIP_SHOWS_FOR: u32 = 10;
        /// Reshow: moving to a neighbor within this share of the delay shows its tip at once
        /// (Windows' `TTDT_RESHOW`, a fifth).
        pub const TIP_RESHOW_DIV: u32 = 5;
        /// The delay when Windows hasn't said: its default double-click time.
        pub const TIP_DELAY_MS: u32 = 500;
    }

    /// Durations; "Show animations" off makes them all 0.
    pub mod motion {
        /// Hover and press.
        pub const HOVER_MS: u16 = 120;
        pub const DRAWER_MS: u16 = 180;
        /// Hold to apply.
        pub const HOLD_MS: u16 = 1_500;
    }

    /// Scroll bars (TD-0009 A2.10 item 3, accepted 2026-11-06). No fade, so no timer: the bar is
    /// an indicator or expanded, by where the pointer is.
    pub mod scroll {
        /// The indicator's width at rest, while the content overflows.
        pub const INDICATOR: f32 = 2.0;
        /// The bar's width with its track: the pointer on its strip, or auto-hide off.
        pub const BAR: f32 = 8.0;
        /// The strip along the viewport's edge that's the bar's: the pointer there expands it.
        pub const STRIP: f32 = 12.0;
        /// The thumb's least length, so it stays a 24 px target (§4.4) however long the content
        /// (a proposal for design, TD-0009 §13).
        pub const MIN_THUMB: f32 = 24.0;
        /// The indicator and the bar from the viewport's edge, so the bar is centered in its strip
        /// (a proposal for design, TD-0009 §13).
        pub const INSET: f32 = 2.0;
    }

    /// The data table (TD-0009 A2.10 item 2: proposals for design until the canvas's artboard 08
    /// gives them; the canvas wins).
    pub mod table {
        /// A row's least height, and the header's.
        pub const ROW: f32 = 36.0;
        /// Above and below a row's text: a row is its line and these, at least [`ROW`].
        pub const ROW_PAD_Y: f32 = 9.0;
        /// A cell's text from its sides.
        pub const CELL_PAD_X: f32 = 12.0;
        /// The column headers' text size.
        pub const HEADER_TEXT: f32 = 12.0;
        /// The check column's width.
        pub const CHECK_COL: f32 = 28.0;
        /// The empty table's status line from the view's edge, and its action from it.
        pub const STATUS_PAD: f32 = 16.0;
        pub const STATUS_GAP: f32 = 8.0;
        /// A column divider's strip in the header, centred on the column's right edge: where the
        /// pointer shows the left-right arrows and a press drags the column's width.
        pub const DIVIDER: f32 = 8.0;
    }

    // The composites' sizes (TD-0009 A3.10 item 8, decision #45; slice 6a). Component tokens, as
    // `table::STATUS_PAD` is: card padding 20 is off the spacing scale on purpose.

    /// Badges (A3.5 (1)): 12 px semibold text inside these.
    pub mod badge {
        pub const PAD_X: f32 = 8.0;
        pub const PAD_Y: f32 = 3.0;
    }

    /// Summary and scan cards (A3.5 (4), (5)).
    pub mod card {
        /// Inside a summary card's edge.
        pub const PADDING: f32 = 20.0;
        /// A card's narrowest in a card grid: its basis on a wrapping line.
        pub const MIN_W: f32 = 220.0;
        /// A status value's dot, beside its word in title text.
        pub const DOT: f32 = 10.0;
    }

    /// The category scan card (A3.5 (5)). Its check box is the toolkit's, a line high, with the
    /// toolkit's gutter after it, so there's no box size here.
    pub mod scan {
        /// The size's slot, right-aligned and fixed, so a size update damages it alone.
        pub const SIZE_W: f32 = 72.0;
        /// Inside the card's edge: 14 × 16.
        pub const PAD_X: f32 = 16.0;
        pub const PAD_Y: f32 = 14.0;
        /// Between the check box, the badge, the size and Details.
        pub const GAP: f32 = 14.0;
        /// Details' least height.
        pub const DETAILS_H: f32 = 32.0;
        /// The bar along the card's bottom while its category scans.
        pub const BAR: f32 = 2.0;
    }

    /// The inline error panel (A3.5 (9)).
    pub mod error {
        /// Inside the panel's edge.
        pub const PADDING: f32 = 16.0;
        /// The most lines of a log excerpt Details shows.
        pub const LOG_LINES: usize = 8;
        /// Its buttons' least height, as a scan card's Details and a job card's Cancel are;
        /// taller when the text size needs it.
        pub const BUTTON_H: f32 = 32.0;
    }

    /// The batch selection bar (A3.5 (11)).
    pub mod batch {
        /// Inside the bar's edge: 10 × 14, and each button's text from its sides.
        pub const PAD_X: f32 = 14.0;
        pub const PAD_Y: f32 = 10.0;
        /// Its buttons' least height; taller when the text size needs it.
        pub const BUTTON_H: f32 = 34.0;
        /// The count's slot at 100 % text ("99,999 selected" in body semibold fits): a count that
        /// changes inside it damages the count alone. It grows with Windows' text size, and a
        /// longer count still widens it.
        pub const COUNT_W: f32 = 104.0;
    }

    /// The info bar (A3.5 (14)).
    pub mod info {
        /// Inside the bar's edge: 12 × 16.
        pub const PAD_X: f32 = 16.0;
        pub const PAD_Y: f32 = 12.0;
        /// Its action's least height, and Dismiss's square: `WinUI`'s standard button, as a scan
        /// card's Details is. Taller when the text size needs it.
        pub const BUTTON_H: f32 = 32.0;
    }

    /// Settings rows (A3.5 (15)).
    pub mod settings {
        /// Inside a row's edge: 14 × 16, as a scan card's.
        pub const PAD_X: f32 = 16.0;
        pub const PAD_Y: f32 = 14.0;
        /// The label and description's narrowest: where they'd get narrower, the row wraps its
        /// control under them.
        pub const TEXT_MIN_W: f32 = 240.0;
        /// A window narrower than this puts every row's control under its text.
        pub const NARROW: f32 = 600.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.2 relative luminance.
    fn luminance(c: Color) -> f64 {
        let lin = |v: u8| {
            let s = f64::from(v) / 255.0;
            if s <= 0.040_45 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
    }

    fn contrast(a: Color, b: Color) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn hex_alpha_and_compositing() {
        assert_eq!(
            Color::hex(0x12_3456),
            Color {
                r: 0x12,
                g: 0x34,
                b: 0x56,
                a: 255
            }
        );
        let white = Color::hex(0xFF_FFFF);
        assert_eq!(Color::hex(0).alpha(0).over(white), white);
        assert_eq!(Color::hex(0).alpha(255).over(white), Color::hex(0));
        // Half of black over white rounds to 128.
        assert_eq!(Color::hex(0).alpha(127).over(white), Color::hex(0x80_8080));
        for x in 0..=255 * 255 {
            assert_eq!(div255(x), (x + 127) / 255, "{x}");
        }
    }

    #[test]
    fn the_table_follows_the_tones() {
        assert_eq!(Theme::LIGHT.color(Tone::Accent), Color::hex(0x25_63EB));
        assert_eq!(Theme::DARK.color(Tone::Text), Color::hex(0xE8_EAEE));
        assert_eq!(Theme::LIGHT.color(Tone::Pressed).a, PRESSED_ALPHA);
        assert_eq!(Theme::DARK.scheme(), Scheme::Dark);
        for (i, t) in Tone::ALL.into_iter().enumerate() {
            assert_eq!(t as usize, i);
        }
    }

    /// TD-0009 §4.4: high contrast shows the system's colors only, and each text in the color
    /// the system pairs with what's under it.
    #[test]
    fn high_contrast_pairs_every_ink_with_its_fill() {
        use Tone::{Accent, App, OnAccent, Selected, Subtle, Surface, Text, TextSecondary};
        let sys = SysColors {
            window: Color::hex(0x00_0001),
            window_text: Color::hex(0x00_0002),
            highlight: Color::hex(0x00_0003),
            highlight_text: Color::hex(0x00_0004),
            hot_light: Color::hex(0x00_0005),
            gray_text: Color::hex(0x00_0006),
            btn_face: Color::hex(0x00_0007),
            btn_text: Color::hex(0x00_0008),
        };
        let t = Theme::high_contrast(sys);
        assert_eq!(t.scheme(), Scheme::HighContrast);
        let system = [
            sys.window,
            sys.window_text,
            sys.highlight,
            sys.highlight_text,
            sys.hot_light,
            sys.gray_text,
            sys.btn_face,
            sys.btn_text,
        ];
        for tone in Tone::ALL {
            let c = t.color(tone);
            assert!(c.a == 0 || system.contains(&c), "{tone:?} is {c:?}");
        }
        // No tints: the state layers are clear.
        assert_eq!((t.color(Tone::Hover).a, t.color(Tone::Pressed).a), (0, 0));
        // Fills and their text.
        assert_eq!(t.color(Accent), sys.highlight);
        assert_eq!(t.ink(OnAccent, Accent), sys.highlight_text);
        assert_eq!(t.ink(Text, Selected), sys.highlight_text);
        assert_eq!(t.color(Subtle), sys.btn_face);
        assert_eq!(t.ink(Text, Subtle), sys.btn_text);
        assert_eq!(t.ink(Text, Surface), sys.window_text);
        assert_eq!(t.ink(TextSecondary, App), sys.gray_text);
        assert_eq!(t.ink(Accent, Surface), sys.hot_light);
        // A disabled highlight fill shows as the window, so its gray text reads as disabled.
        assert_eq!(t.fill(Accent, true), Accent);
        assert_eq!(t.fill(Accent, false), Surface);
        assert_eq!(t.ink(TextSecondary, t.fill(Accent, false)), sys.gray_text);
        // Borders, focus and hover.
        assert_eq!(t.stroke(TextSecondary, true), sys.window_text);
        assert_eq!(t.stroke(Tone::Border, false), sys.gray_text);
        assert_eq!(t.focus(), sys.highlight);
        assert_eq!(t.hover_outline(Surface), Some(sys.highlight));
        assert_eq!(t.hover_outline(Accent), Some(sys.highlight_text));
        // The composites (A3.5 (1), (9), (11), (14)): badges, tints and the batch bar have no
        // fill, with window text and borders; the batch bar's primary action is a highlight.
        for (text, fill) in [
            (Tone::PermanentText, Tone::PermanentFill),
            (Tone::UndoableText, Tone::UndoableFill),
            (Tone::NeutralText, Tone::NeutralFill),
            (Tone::RunningText, Tone::RunningFill),
            (Tone::CautionText, Tone::CautionFill),
            (Text, Tone::ErrorFill),
            (Text, Tone::WarnFill),
            (Text, Tone::InfoFill),
            (Tone::Danger, Tone::ErrorFill),
            (Tone::InverseText, Tone::InverseFill),
        ] {
            assert_eq!(t.color(fill), sys.window, "{fill:?}");
            assert_eq!(t.ink(text, fill), sys.window_text, "{text:?} on {fill:?}");
            assert_eq!(t.stroke(fill, true), sys.window_text, "{fill:?}'s border");
        }
        for border in [Tone::DangerBorder, Tone::WarnBorder, Tone::InfoBorder] {
            assert_eq!(t.color(border), sys.window_text, "{border:?}");
        }
        assert_eq!(t.color(Tone::InverseAccent), sys.highlight);
        assert_eq!(
            t.ink(Tone::OnInverseAccent, Tone::InverseAccent),
            sys.highlight_text
        );
        assert_eq!(t.fill(Tone::InverseAccent, false), Surface);
        assert_eq!(
            t.hover_outline(Tone::InverseAccent),
            Some(sys.highlight_text)
        );
        // The outlined danger button: window text in a window-text border.
        assert_eq!(t.ink(Tone::Danger, Surface), sys.window_text);
        assert_eq!(t.stroke(Tone::DangerBorder, true), sys.window_text);
        // Light and dark are unchanged by all of it.
        for theme in [Theme::LIGHT, Theme::DARK] {
            assert_eq!(theme.ink(Text, Accent), theme.color(Text));
            assert_eq!(theme.fill(Accent, false), Accent);
            assert_eq!(theme.stroke(Tone::Border, false), theme.color(Tone::Border));
            assert_eq!(theme.focus(), theme.color(Text));
            assert_eq!(theme.hover_outline(Surface), None);
        }
    }

    /// Every text pair the views paint (doc 05 §6, A3.5): a text or glyph tone over a stack of
    /// fills, the first on the window (Mica, under glass), each over the one before. A state layer
    /// last is that control hovered or pressed. The contrast tests below check each pair in light,
    /// dark, glass and high contrast.
    fn text_pairs() -> Vec<(Tone, Vec<Tone>)> {
        use Tone::{
            Accent, App, CautionFill, CautionText, Danger, ErrorFill, Hover, Image, InfoFill,
            InverseAccent, InverseFill, InverseText, NeutralFill, NeutralText, Offline, Ok,
            OnAccent, OnInverseAccent, PermanentFill, PermanentText, Pressed, RunningFill,
            RunningText, Selected, Subtle, Surface, Text, TextSecondary, UndoableFill,
            UndoableText, Warn, WarnFill,
        };
        // Solid fills are opaque, so `[Surface, Selected]` is the selection alone there; under
        // glass the selection is translucent and sits on a card, as a table's current row does.
        let mut pairs = Vec::new();
        // At rest, hovered and pressed.
        let mut layered = |fg: Tone, stack: &[Tone]| {
            pairs.push((fg, stack.to_vec()));
            for layer in [Hover, Pressed] {
                let mut lit = stack.to_vec();
                lit.push(layer);
                pairs.push((fg, lit));
            }
        };
        // Primary and secondary text: on the window, a card, a quiet control, and a control or
        // the selection (a table's current row) on a card; on the error panel's and the info
        // bar's tints, and a log excerpt's quiet fill on them.
        for stack in [
            &[App][..],
            &[Surface],
            &[Subtle],
            &[Surface, Subtle],
            &[Surface, Selected],
            &[ErrorFill],
            &[WarnFill],
            &[InfoFill],
            &[ErrorFill, Subtle],
            &[WarnFill, Subtle],
            // An info bar's action button, a quiet control on its tint.
            &[InfoFill, Subtle],
        ] {
            for fg in [Text, TextSecondary] {
                layered(fg, stack);
            }
        }
        // Danger text on the window and on a card is also the outlined danger button's
        // (`Look::DANGER_OUTLINE`), so hovered and pressed too.
        for stack in [&[App][..], &[Surface]] {
            layered(Danger, stack);
        }
        // Text on the accent and danger fills, on the batch bar and on its primary action.
        for (fg, bg) in [
            (OnAccent, Accent),
            (OnAccent, Danger),
            (InverseText, InverseFill),
            (OnInverseAccent, InverseAccent),
        ] {
            layered(fg, &[bg]);
        }
        // Status colors on the window and on a card; the panels' glyphs on their tints.
        for stack in [&[App][..], &[Surface]] {
            for fg in [Accent, Offline, Image, Ok, Warn] {
                pairs.push((fg, stack.to_vec()));
            }
        }
        for (fg, bg) in [(Danger, ErrorFill), (Warn, WarnFill), (Accent, InfoFill)] {
            pairs.push((fg, vec![bg]));
        }
        // Each badge's pair (decision #45). Badges are static: no state layer.
        for (fg, bg) in [
            (PermanentText, PermanentFill),
            (UndoableText, UndoableFill),
            (NeutralText, NeutralFill),
            (RunningText, RunningFill),
            (CautionText, CautionFill),
        ] {
            pairs.push((fg, vec![bg]));
        }
        pairs
    }

    /// `stack`'s fills composited over `under`, in `theme`. A state layer is painted in the ink
    /// of the fill under it ([`Theme::layer`]).
    fn flat(theme: &Theme, under: Color, stack: &[Tone]) -> Color {
        let mut fill = Tone::App;
        stack.iter().fold(under, |c, &t| {
            let color = if matches!(t, Tone::Hover | Tone::Pressed) {
                theme.layer(t, fill)
            } else {
                fill = t;
                theme.color(t)
            };
            color.over(c)
        })
    }

    /// A quiet button on the batch bar shows its hover and press (A3.5 (11)): the state layer is
    /// the bar's own ink, so it moves the fill as far as it does on a card. Elsewhere it's
    /// primary text's, and high contrast paints none.
    #[test]
    fn a_state_layer_shows_on_the_inverse_fill() {
        for theme in [Theme::LIGHT, Theme::DARK, Theme::LIGHT.glass()] {
            for layer in [Tone::Hover, Tone::Pressed] {
                let a = theme.color(layer).a;
                assert_eq!(
                    theme.layer(layer, Tone::InverseFill),
                    theme.color(Tone::InverseText).alpha(a)
                );
                assert_eq!(theme.layer(layer, Tone::Surface), theme.color(layer));
                let bar = theme.color(Tone::InverseFill);
                let lit = theme.layer(layer, Tone::InverseFill).over(bar);
                let card = theme.color(Tone::Surface).over(Color::hex(0xFF_FFFF));
                let lit_card = theme.color(layer).over(card);
                // How far the layer moves its fill, as a contrast ratio less one.
                let moved = |a: Color, b: Color| contrast(a, b) - 1.0;
                assert!(
                    moved(lit, bar) >= moved(lit_card, card) / 2.0,
                    "{:?} {layer:?}: {lit:?} on {bar:?}",
                    theme.scheme()
                );
            }
        }
        let sys = SysColors {
            window: Color::hex(0),
            window_text: Color::hex(0xFF_FFFF),
            highlight: Color::hex(0x1A_EBFF),
            highlight_text: Color::hex(0),
            hot_light: Color::hex(0xFF_FF00),
            gray_text: Color::hex(0x3F_F23F),
            btn_face: Color::hex(0),
            btn_text: Color::hex(0xFF_FFFF),
        };
        let hc = Theme::high_contrast(sys);
        assert_eq!(hc.layer(Tone::Hover, Tone::InverseFill).a, 0);
    }

    /// Doc 05 §6: WCAG 2.2 AA (4.5:1) for every text color on every background it's used on, in
    /// both schemes, including under the hover and pressed state layers: the solid fallback,
    /// which is the gate (ADR-0007).
    #[test]
    fn text_contrast_is_aa_in_both_schemes() {
        for theme in [Theme::LIGHT, Theme::DARK] {
            for (fg, stack) in text_pairs() {
                // Every solid fill a text sits on is opaque, so what's under it doesn't show.
                assert_eq!(theme.color(stack[0]).a, 255, "{fg:?} on {stack:?}");
                let ratio = contrast(theme.color(fg), flat(&theme, Color::hex(0), &stack));
                assert!(
                    ratio >= 4.5,
                    "{:?}: {fg:?} on {stack:?} is {ratio:.2}:1",
                    theme.scheme()
                );
            }
        }
    }

    /// High contrast: every text pair is the pair the system gives for its fill (`ink`), so it's
    /// as legible as the system's palette makes it. Checked over a black and a white palette.
    #[test]
    fn high_contrast_text_pairs_are_the_systems() {
        let black = SysColors {
            window: Color::hex(0),
            window_text: Color::hex(0xFF_FFFF),
            highlight: Color::hex(0x1A_EBFF),
            highlight_text: Color::hex(0),
            hot_light: Color::hex(0xFF_FF00),
            gray_text: Color::hex(0x3F_F23F),
            btn_face: Color::hex(0),
            btn_text: Color::hex(0xFF_FFFF),
        };
        let white = SysColors {
            window: Color::hex(0xFF_FFFF),
            window_text: Color::hex(0),
            highlight: Color::hex(0x37_006E),
            highlight_text: Color::hex(0xFF_FFFF),
            hot_light: Color::hex(0x00_009F),
            gray_text: Color::hex(0x60_0000),
            btn_face: Color::hex(0xFF_FFFF),
            btn_text: Color::hex(0),
        };
        for sys in [black, white] {
            let theme = Theme::high_contrast(sys);
            for (fg, stack) in text_pairs() {
                // The fill the text is inked for: the last one under it that isn't a state layer.
                let under = stack
                    .iter()
                    .rev()
                    .copied()
                    .find(|t| !matches!(t, Tone::Hover | Tone::Pressed))
                    .unwrap_or(Tone::App);
                let ratio = contrast(theme.ink(fg, under), flat(&theme, sys.window, &stack));
                assert!(ratio >= 4.5, "{fg:?} on {stack:?} is {ratio:.2}:1");
            }
        }
    }

    /// Decision #45's values: the light badge pairs and the inverse four as the design gives them,
    /// dark's inverse four mirrored, and `danger.border` and the tints as alphas of their colors
    /// in both schemes.
    #[test]
    fn the_composites_tokens_are_decision_45s() {
        use Tone::{
            Accent, CautionFill, CautionText, Danger, DangerBorder, ErrorFill, Hover, InfoBorder,
            InfoFill, InverseAccent, InverseFill, InverseText, NeutralFill, NeutralText,
            OnInverseAccent, RunningFill, RunningText, Selected, Surface, UndoableFill,
            UndoableText, Warn, WarnBorder, WarnFill,
        };
        let light = |t| Theme::LIGHT.color(t);
        let dark = |t| Theme::DARK.color(t);
        let hex = Color::hex;
        let badges = [UndoableText, UndoableFill, NeutralText, RunningText];
        assert_eq!(
            badges.map(light),
            [0x16_6534, 0xE7_F6EC, 0x37_4151, 0x1E_40AF].map(hex)
        );
        assert_eq!(
            [CautionText, CautionFill].map(light),
            [0x7C_3F06, 0xFE_F6E7].map(hex)
        );
        assert_eq!(
            [InverseFill, InverseText, InverseAccent, OnInverseAccent].map(light),
            [0x14_171C, 0xE8_EAEE, 0x6E_A0FF, 0x0B_1630].map(hex)
        );
        assert_eq!(
            [InverseFill, InverseText, InverseAccent, OnInverseAccent].map(dark),
            [0xE8_EAEE, 0x14_171C, 0x25_63EB, 0xFF_FFFF].map(hex)
        );
        for theme in [Theme::LIGHT, Theme::DARK] {
            let c = |t| theme.color(t);
            // Neutral on ink at 6 %, Running on the selection's accent, flattened.
            assert_eq!(c(NeutralFill), c(Hover).over(c(Surface)));
            assert_eq!(c(RunningFill), c(Selected));
            // `danger.border` and the tints' borders at 30 %; the tints at 8 %.
            for (border, fill, color) in [
                (DangerBorder, ErrorFill, Danger),
                (WarnBorder, WarnFill, Warn),
                (InfoBorder, InfoFill, Accent),
            ] {
                assert_eq!(c(border), c(color).alpha(OUTLINE_ALPHA));
                assert_eq!(c(fill), c(color).alpha(TINT_ALPHA).over(c(Surface)));
            }
        }
        assert_eq!((OUTLINE_ALPHA, TINT_ALPHA), (77, 20), "30 % and 8 % of 255");
        let outline = crate::Look::DANGER_OUTLINE;
        assert_eq!(
            (outline.fill, outline.border, outline.ink),
            (None, Some(DangerBorder), Danger)
        );
        // The badge step: 12 px semibold.
        assert_eq!(tokens::font::BADGE.to_bits(), 12.0_f32.to_bits());
        assert_eq!(tokens::font::BADGE_WEIGHT, 600);
    }

    /// ADR-0007: glass changes only the layers; everything opaque, and high contrast, stays.
    #[test]
    fn glass_layers_are_translucent_and_the_rest_unchanged() {
        use Tone::{App, Border, Selected, Subtle, Surface};
        for theme in [Theme::LIGHT, Theme::DARK] {
            let glass = theme.glass();
            assert!(glass.is_glass() && !theme.is_glass());
            assert_eq!(glass.scheme(), theme.scheme());
            assert_eq!(glass.color(App).a, 0);
            for tone in Tone::ALL {
                // The layers, and glass's own secondary text (§12 item 9).
                let glassy = matches!(
                    tone,
                    App | Surface | Subtle | Border | Selected | Tone::TextSecondary
                );
                assert_eq!(glass.color(tone) == theme.color(tone), !glassy, "{tone:?}");
            }
            // `fill.selected`: 13 % light, 14 % dark, of the accent itself. `text.secondary`:
            // decision #45's.
            let (selected, secondary) = if theme.scheme() == Scheme::Light {
                (34, 0x58_606E)
            } else {
                (36, 0xAC_B5C4)
            };
            assert_eq!(
                glass.color(Selected),
                theme.color(Tone::Accent).alpha(selected)
            );
            assert_eq!(glass.color(Tone::TextSecondary), Color::hex(secondary));
        }
        assert_eq!(
            (
                Theme::LIGHT.glass().color(Surface).a,
                Theme::DARK.glass().color(Surface).a
            ),
            (158, 13),
            "layer.card: 62 % and 5 % white"
        );
        let hc = Theme::high_contrast(SysColors {
            window: Color::hex(0),
            window_text: Color::hex(0xFF_FFFF),
            highlight: Color::hex(0x1A_EBFF),
            highlight_text: Color::hex(0),
            hot_light: Color::hex(0xFF_FF00),
            gray_text: Color::hex(0x3F_F23F),
            btn_face: Color::hex(0),
            btn_text: Color::hex(0xFF_FFFF),
        });
        assert_eq!(hc.glass(), hc);
        assert!(!hc.is_glass());
    }

    /// Over untinted Mica (#F3F3F3 light, #202020 dark), glass text keeps WCAG 2.2 AA wherever
    /// the solid fallback does, except the pair listed. Real Mica is tinted by the wallpaper, so
    /// the gate is the solid fallback (ADR-0007, tested above); this records how far glass strays
    /// from it, so a token change that moves any pair shows here.
    ///
    /// Doc 05's `text.secondary` left six pairs short where white layers stack, 3.67:1 at worst
    /// (§12 item 9); glass's own (decision #45) brings them over. The one left is dark's danger
    /// text on a hovered card: the outlined danger button (`Look::DANGER_OUTLINE`) under its
    /// hover layer, where two white layers lift Mica toward the danger color. It's 5.39:1 solid.
    #[test]
    fn glass_text_contrast_over_untinted_mica() {
        let mut short = Vec::new();
        for (theme, mica) in [
            (Theme::LIGHT.glass(), Color::hex(0xF3_F3F3)),
            (Theme::DARK.glass(), Color::hex(0x20_2020)),
        ] {
            for (fg, stack) in text_pairs() {
                let ratio = contrast(theme.color(fg), flat(&theme, mica, &stack));
                if ratio < 4.5 {
                    short.push(format!(
                        "{:?} {fg:?} on {stack:?}: {ratio:.2}",
                        theme.scheme()
                    ));
                }
            }
        }
        let known = ["Dark Danger on [Surface, Hover]: 4.30"];
        assert_eq!(short, known, "glass pairs below 4.5:1");
    }
}
