//! Design tokens (doc 05 §4) and the [`Theme`] that gives color tokens their values (TD-0009 §5.1).
//!
//! Views name colors by [`Tone`] and sizes by the constants in [`tokens`], never by value
//! (CLAUDE.md non-negotiable 7). Painting looks tones up in the theme it's given, so switching
//! between light and dark is a repaint ([`Ui::repaint_all`](crate::Ui::repaint_all)), not a
//! rebuild.
//!
//! The colors are doc 05's solid-fallback values, which every contrast check is made against
//! (tested below: WCAG 2.2 AA). The glass materials come with the composition presenter.
//!
//! **High contrast** ([`Theme::high_contrast`], TD-0009 §4.4) shows the system's colors only: no
//! tints, no state layers. Text takes the color the system pairs with the fill under it
//! ([`Theme::ink`]): `HighlightText` on the highlight fills (accent, danger, selection),
//! `ButtonText` on `ButtonFace`, else `WindowText`, with `GrayText` for secondary text and
//! `HotLight` for accent text. The focus ring is `Highlight`.

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
    /// `text.secondary`, and disabled text.
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
}

impl Tone {
    /// Every tone, in declaration order (the theme's table order).
    pub const ALL: [Self; 18] = [
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
    ];
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

impl Theme {
    pub const LIGHT: Self = {
        let surface = Color::hex(0xFF_FFFF);
        let accent = Color::hex(0x25_63EB);
        let ink = Color::hex(0x14_171C);
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
                Color::hex(0xB4_5309),
                Color::hex(0xB9_1C1C),
                Color::hex(0x9A_3412),
                Color::hex(0xFD_EDE4),
                // 13 %.
                accent.alpha(34).over(surface),
                ink.alpha(HOVER_ALPHA),
                ink.alpha(PRESSED_ALPHA),
            ],
            sys: None,
        }
    };

    pub const DARK: Self = {
        let surface = Color::hex(0x17_1A21);
        let accent = Color::hex(0x6E_A0FF);
        let ink = Color::hex(0xFF_FFFF);
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
                Color::hex(0x4A_DE80),
                Color::hex(0xF0_B254),
                Color::hex(0xF8_7171),
                // Doc 05 gives the Permanent badge for light only: danger on a dark red tint.
                Color::hex(0xF8_7171),
                Color::hex(0x3A_1D1F),
                // 14 %.
                accent.alpha(36).over(surface),
                ink.alpha(HOVER_ALPHA),
                ink.alpha(PRESSED_ALPHA),
            ],
            sys: None,
        }
    };

    /// High contrast: the system's colors only. Fills map to `Window`, `ButtonFace` (quiet
    /// fills) and `Highlight` (accent, danger, selection); borders to `WindowText`; the state
    /// layers vanish. Text goes through [`ink`](Self::ink).
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
            ],
            sys: Some(sys),
        }
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
    /// a disabled accent, danger or selected fill shows as the window, so its `GrayText` reads
    /// as disabled. Light and dark: `tone`.
    #[must_use]
    pub const fn fill(&self, tone: Tone, enabled: bool) -> Tone {
        if self.sys.is_some()
            && !enabled
            && matches!(tone, Tone::Accent | Tone::Danger | Tone::Selected)
        {
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
            Some(sys) if matches!(under, Tone::Accent | Tone::Danger | Tone::Selected) => {
                Some(sys.highlight_text)
            }
            Some(sys) => Some(sys.highlight),
            None => None,
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
            Tone::Accent | Tone::Danger | Tone::Selected => sys.highlight_text,
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
    }

    /// Strokes.
    pub mod stroke {
        /// The keyboard focus ring's width (doc 05 §6).
        pub const FOCUS: f32 = 2.0;
    }

    /// Durations; "Show animations" off makes them all 0.
    pub mod motion {
        /// Hover and press.
        pub const HOVER_MS: u16 = 120;
        pub const DRAWER_MS: u16 = 180;
        /// Hold to apply.
        pub const HOLD_MS: u16 = 1_500;
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
        // Light and dark are unchanged by all of it.
        for theme in [Theme::LIGHT, Theme::DARK] {
            assert_eq!(theme.ink(Text, Accent), theme.color(Text));
            assert_eq!(theme.fill(Accent, false), Accent);
            assert_eq!(theme.stroke(Tone::Border, false), theme.color(Tone::Border));
            assert_eq!(theme.focus(), theme.color(Text));
            assert_eq!(theme.hover_outline(Surface), None);
        }
    }

    /// Doc 05 §6: WCAG 2.2 AA (4.5:1) for every text color on every background it's used on, in
    /// both schemes, including under the hover and pressed state layers.
    #[test]
    fn text_contrast_is_aa_in_both_schemes() {
        use Tone::{
            Accent, App, Danger, Hover, Image, Offline, Ok, OnAccent, PermanentFill, PermanentText,
            Pressed, Selected, Subtle, Surface, Text, TextSecondary, Warn,
        };
        for theme in [Theme::LIGHT, Theme::DARK] {
            let c = |t| theme.color(t);
            let mut pairs = Vec::new();
            for bg in [App, Surface, Subtle, Selected] {
                for fg in [Text, TextSecondary] {
                    pairs.push((fg, c(fg), bg, c(bg)));
                    for layer in [Hover, Pressed] {
                        pairs.push((fg, c(fg), layer, c(layer).over(c(bg))));
                    }
                }
            }
            for bg in [App, Surface] {
                for fg in [Accent, Offline, Image, Ok, Warn, Danger] {
                    pairs.push((fg, c(fg), bg, c(bg)));
                }
            }
            for bg in [Accent, Danger] {
                pairs.push((OnAccent, c(OnAccent), bg, c(bg)));
                for layer in [Hover, Pressed] {
                    pairs.push((OnAccent, c(OnAccent), layer, c(layer).over(c(bg))));
                }
            }
            pairs.push((
                PermanentText,
                c(PermanentText),
                PermanentFill,
                c(PermanentFill),
            ));
            for (fg, f, bg, b) in pairs {
                let ratio = contrast(f, b);
                assert!(
                    ratio >= 4.5,
                    "{:?}: {fg:?} on {bg:?} is {ratio:.2}:1",
                    theme.scheme()
                );
            }
        }
    }
}
