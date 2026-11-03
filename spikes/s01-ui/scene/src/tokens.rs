//! Design tokens from doc 05 §4 (the solid-fallback values every contrast is checked against).

/// An opaque sRGB color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    #[must_use]
    pub const fn hex(v: u32) -> Self {
        Self((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    /// `self` at `alpha` (0–255) over `under`, in sRGB like every UI compositor.
    #[must_use]
    pub const fn over(self, alpha: u8, under: Self) -> Self {
        const fn mix(a: u8, b: u8, t: u8) -> u8 {
            ((a as u32 * t as u32 + b as u32 * (255 - t as u32) + 127) / 255) as u8
        }
        Self(
            mix(self.0, under.0, alpha),
            mix(self.1, under.1, alpha),
            mix(self.2, under.2, alpha),
        )
    }

    /// Largest per-channel difference.
    #[must_use]
    pub const fn distance(self, other: Self) -> u8 {
        const fn d(a: u8, b: u8) -> u8 {
            a.abs_diff(b)
        }
        let (r, g, b) = (d(self.0, other.0), d(self.1, other.1), d(self.2, other.2));
        let m = if r > g { r } else { g };
        if m > b { m } else { b }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Self; 2] = [Self::Light, Self::Dark];

    #[must_use]
    pub const fn palette(self) -> &'static Palette {
        match self {
            Self::Light => &LIGHT,
            Self::Dark => &DARK,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.name() == s)
    }
}

/// doc 05 §4 color tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub bg_app: Rgb,
    pub bg_surface: Rgb,
    pub bg_subtle: Rgb,
    pub border: Rgb,
    pub text_primary: Rgb,
    pub text_secondary: Rgb,
    pub accent: Rgb,
    /// Text on an accent fill.
    pub on_accent: Rgb,
    pub target_offline: Rgb,
    pub target_image: Rgb,
    pub ok: Rgb,
    pub warn: Rgb,
    pub danger: Rgb,
    pub permanent_fg: Rgb,
    pub permanent_bg: Rgb,
    /// `fill.selected`: accent at ~13 % over the surface, flattened.
    pub selected: Rgb,
}

pub const LIGHT: Palette = Palette {
    bg_app: Rgb::hex(0xF6F7F9),
    bg_surface: Rgb::hex(0xFFFFFF),
    bg_subtle: Rgb::hex(0xEEF0F4),
    border: Rgb::hex(0xDDE1E8),
    text_primary: Rgb::hex(0x14171C),
    text_secondary: Rgb::hex(0x5A6270),
    accent: Rgb::hex(0x2563EB),
    on_accent: Rgb::hex(0xFFFFFF),
    target_offline: Rgb::hex(0xB45309),
    target_image: Rgb::hex(0x7C3AED),
    ok: Rgb::hex(0x15803D),
    warn: Rgb::hex(0xB45309),
    danger: Rgb::hex(0xB91C1C),
    permanent_fg: Rgb::hex(0x9A3412),
    permanent_bg: Rgb::hex(0xFDEDE4),
    selected: Rgb::hex(0x2563EB).over(34, Rgb::hex(0xFFFFFF)),
};

pub const DARK: Palette = Palette {
    bg_app: Rgb::hex(0x0F1115),
    bg_surface: Rgb::hex(0x171A21),
    bg_subtle: Rgb::hex(0x1E222B),
    border: Rgb::hex(0x2A2F3A),
    text_primary: Rgb::hex(0xE8EAEE),
    text_secondary: Rgb::hex(0x9AA3B2),
    accent: Rgb::hex(0x6EA0FF),
    on_accent: Rgb::hex(0x0F1115),
    target_offline: Rgb::hex(0xF0B254),
    target_image: Rgb::hex(0xB79CFF),
    ok: Rgb::hex(0x4ADE80),
    warn: Rgb::hex(0xF0B254),
    danger: Rgb::hex(0xF87171),
    // doc 05 gives the Permanent badge for light only; the spike uses danger on a dark red tint.
    permanent_fg: Rgb::hex(0xF87171),
    permanent_bg: Rgb::hex(0x3A1D1F),
    selected: Rgb::hex(0x6EA0FF).over(36, Rgb::hex(0x171A21)),
};

impl Palette {
    /// Fill and text of a TD-0015 row badge; `None` for rows without one.
    #[must_use]
    pub const fn badge(&self, b: crate::data::Badge) -> Option<(Rgb, Rgb)> {
        use crate::data::Badge;
        match b {
            Badge::None => None,
            Badge::New => Some((self.accent.over(40, self.bg_surface), self.accent)),
            Badge::Changed => Some((self.warn.over(40, self.bg_surface), self.warn)),
            Badge::Orphan => Some((self.bg_subtle, self.text_secondary)),
        }
    }

    /// Fill and text of a reversibility badge (doc 05 §5).
    #[must_use]
    pub const fn reversibility(&self, r: crate::data::Reversibility) -> (Rgb, Rgb) {
        use crate::data::Reversibility;
        match r {
            Reversibility::Undoable | Reversibility::UndoableDays => {
                (self.ok.over(36, self.bg_surface), self.ok)
            }
            Reversibility::Permanent => (self.permanent_bg, self.permanent_fg),
        }
    }
}

/// Type scale (doc 05 §4), logical px.
pub mod font {
    pub const CAPTION: f32 = 12.0;
    pub const BODY: f32 = 13.0;
    pub const SUBTITLE: f32 = 15.0;
    pub const TITLE: f32 = 20.0;
    pub const DISPLAY: f32 = 28.0;
}

/// Corner radii (doc 05 §4), logical px.
pub mod radius {
    pub const CONTROL: f32 = 6.0;
    pub const CARD: f32 = 10.0;
    pub const DRAWER: f32 = 14.0;
}
