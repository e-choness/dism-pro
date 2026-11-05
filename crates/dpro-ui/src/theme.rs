//! Design tokens (doc 05 §4) and the [`Theme`] that gives color tokens their values (TD-0009 §5.1).
//!
//! Views name colors by [`Tone`] and sizes by the constants in [`tokens`], never by value
//! (CLAUDE.md non-negotiable 7). Painting looks tones up in the theme it's given, so switching
//! between light and dark is a repaint ([`Ui::repaint_all`](crate::Ui::repaint_all)), not a
//! rebuild.
//!
//! The colors are doc 05's solid-fallback values, which every contrast check is made against
//! (tested below: WCAG 2.2 AA). The glass materials and high contrast's system colors come with
//! the Win32 platform.

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

/// Light or dark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scheme {
    Light,
    Dark,
}

/// The colors of every [`Tone`] in one scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    scheme: Scheme,
    colors: [Color; Tone::ALL.len()],
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
        }
    };

    #[must_use]
    pub const fn scheme(&self) -> Scheme {
        self.scheme
    }

    #[must_use]
    pub const fn color(&self, tone: Tone) -> Color {
        self.colors[tone as usize]
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
