//! Text (TD-0009 §5.2): measured for layout, drawn for painting. The platform implements
//! [`TextSystem`] with DirectWrite; T1 tests and benches use [`FakeText`], whose metrics and ink
//! are fixed and deterministic.

use crate::{
    geom::{Rect, RectPx},
    theme::tokens::font,
};

/// How a run of text is set. Views pass the theme's type tokens (doc 05 §4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Font size in logical pixels.
    pub size: f32,
    pub weight: u16,
    pub mono: bool,
    /// Wrap at word boundaries to the available width; otherwise one line, trimmed.
    pub wrap: bool,
}

impl Default for TextStyle {
    /// Doc 05's body text: 13 px, regular, one line.
    fn default() -> Self {
        Self {
            size: font::BODY,
            weight: font::REGULAR,
            mono: false,
            wrap: false,
        }
    }
}

/// The measured box of a run of text, in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    pub lines: u32,
    /// The text didn't fit and will be drawn with an ellipsis.
    pub trimmed: bool,
}

/// A glyph of the icon font: Segoe Fluent Icons, else Segoe MDL2 Assets, at the same code point in
/// both (TD-0009 §5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    /// A check box's check.
    CheckMark,
    /// A dropdown's chevron, and a column sorted largest first.
    ChevronDown,
    /// A column sorted smallest first.
    ChevronUp,
}

impl Icon {
    /// Its code point in both icon fonts.
    #[must_use]
    pub const fn codepoint(self) -> u32 {
        match self {
            Self::CheckMark => 0xE73E,
            Self::ChevronDown => 0xE70D,
            Self::ChevronUp => 0xE70E,
        }
    }
}

/// Where drawn text and icons go. The painter tints whatever it receives with the current ink and
/// clips it.
pub trait GlyphSink {
    /// Full coverage over `r`, in physical pixels.
    fn fill(&mut self, r: RectPx);
    /// An 8-bit coverage mask, rows of `width` values, with its top-left at physical `(x, y)`.
    fn mask(&mut self, x: i32, y: i32, width: u32, coverage: &[u8]);
}

/// Measures and draws text.
pub trait TextSystem {
    /// The box of `text` in `style` within `max_width` (possibly infinite): wrapped if the style
    /// wraps, else one line, trimmed when it doesn't fit.
    fn measure(&mut self, text: &str, style: &TextStyle, max_width: f32) -> TextMetrics;

    /// Draws `text` as [`measure`](Self::measure) lays it out at `rect.w`, its top-left at
    /// `rect`'s, at `scale` physical pixels per logical pixel.
    fn draw(
        &mut self,
        text: &str,
        style: &TextStyle,
        rect: Rect,
        scale: f32,
        sink: &mut dyn GlyphSink,
    );

    /// Draws `icon` centered in `rect`, as large as fits.
    fn icon(&mut self, icon: Icon, rect: Rect, scale: f32, sink: &mut dyn GlyphSink);

    /// Replaces `out` with the caret stops of `text` set on one line in `style`, untrimmed: every
    /// place a caret can sit, in logical order, each a cluster boundary (a caret never splits a
    /// base character from its marks, or a surrogate pair), from index 0 at x 0 to `text.len()`.
    /// Reuses `out`'s buffer.
    fn caret_stops(&mut self, text: &str, style: &TextStyle, out: &mut Vec<CaretStop>);
}

/// A place in a line of text where a caret can sit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaretStop {
    /// Byte offset in the text.
    pub index: u32,
    /// Logical pixels from the line's start.
    pub x: f32,
}

/// Deterministic metrics for T1: every character advances half the font size (six tenths in
/// mono), and a line is 1.25 × the size. Wrapping breaks greedily at spaces; a word longer than
/// the line overflows it and counts as trimmed.
///
/// Its ink is a box per visible character: the advance's middle three quarters, from a quarter of
/// the size below the line's top down to its baseline. An icon is a box over its middle half.
/// One-line text that doesn't fit draws the characters that fit whole.
#[derive(Clone, Copy, Debug, Default)]
pub struct FakeText;

impl FakeText {
    fn advance(style: TextStyle) -> f32 {
        style.size * if style.mono { 0.6 } else { 0.5 }
    }

    fn line_height(style: TextStyle) -> f32 {
        style.size * 1.25
    }

    /// Greedy wrapping: calls `place(line, x, word)` for every word, `x` from the line's start.
    /// Returns the line count and the widest line.
    fn wrap(
        text: &str,
        advance: f32,
        max_width: f32,
        mut place: impl FnMut(u32, f32, &str),
    ) -> (u32, f32) {
        let (mut lines, mut widest, mut current) = (1u32, 0.0f32, 0.0f32);
        for word in text.split(' ') {
            let w = count(word.chars().count()) * advance;
            let x = if current > 0.0 && current + advance + w > max_width {
                lines += 1;
                widest = widest.max(current);
                current = w;
                0.0
            } else if current > 0.0 {
                let x = current + advance;
                current += advance + w;
                x
            } else {
                current = w;
                0.0
            };
            place(lines - 1, x, word);
        }
        (lines, widest.max(current))
    }

    /// The ink of `run`'s characters from logical `(left, top)`, those that end by `limit`.
    fn ink(
        run: &str,
        left: f32,
        top: f32,
        style: TextStyle,
        scale: f32,
        limit: f32,
        sink: &mut dyn GlyphSink,
    ) {
        let advance = Self::advance(style);
        for (i, ch) in run.chars().enumerate() {
            if left + count(i + 1) * advance > limit {
                break;
            }
            if ch.is_whitespace() {
                continue;
            }
            let glyph = Rect::new(
                left + count(i) * advance + advance / 8.0,
                top + style.size / 4.0,
                advance * 0.75,
                style.size * 0.75,
            );
            sink.fill(glyph.snap(scale));
        }
    }
}

impl TextSystem for FakeText {
    fn measure(&mut self, text: &str, style: &TextStyle, max_width: f32) -> TextMetrics {
        let advance = Self::advance(*style);
        let line = Self::line_height(*style);
        if !style.wrap {
            let width = count(text.chars().count()) * advance;
            return TextMetrics {
                width: width.min(max_width),
                height: line,
                lines: 1,
                trimmed: width > max_width,
            };
        }
        let mut trimmed = false;
        let (lines, widest) = Self::wrap(text, advance, max_width, |_, _, word| {
            trimmed |= count(word.chars().count()) * advance > max_width;
        });
        TextMetrics {
            width: widest.min(max_width),
            height: line * count(usize::try_from(lines).unwrap_or(usize::MAX)),
            lines,
            trimmed,
        }
    }

    fn draw(
        &mut self,
        text: &str,
        style: &TextStyle,
        rect: Rect,
        scale: f32,
        sink: &mut dyn GlyphSink,
    ) {
        let style = *style;
        if style.wrap {
            let line = Self::line_height(style);
            Self::wrap(text, Self::advance(style), rect.w, |n, x, word| {
                let top = rect.y + line * count(usize::try_from(n).unwrap_or(usize::MAX));
                Self::ink(word, rect.x + x, top, style, scale, f32::INFINITY, sink);
            });
        } else {
            // A hair of slack: `rect.w` went through layout's arithmetic (padding off and on).
            let limit = rect.right() + 1e-3;
            Self::ink(text, rect.x, rect.y, style, scale, limit, sink);
        }
    }

    fn icon(&mut self, _icon: Icon, rect: Rect, scale: f32, sink: &mut dyn GlyphSink) {
        let side = rect.w.min(rect.h) / 2.0;
        let c = rect.center();
        sink.fill(Rect::new(c.x - side / 2.0, c.y - side / 2.0, side, side).snap(scale));
    }

    /// A stop at every character boundary, an advance apart.
    fn caret_stops(&mut self, text: &str, style: &TextStyle, out: &mut Vec<CaretStop>) {
        let advance = Self::advance(*style);
        out.clear();
        let mut x = 0.0;
        for (k, _) in text.char_indices() {
            out.push(CaretStop { index: index(k), x });
            x += advance;
        }
        out.push(CaretStop {
            index: index(text.len()),
            x,
        });
    }
}

/// A byte offset as a stop's index: a field holds at most 64 KiB, so it always fits.
fn index(k: usize) -> u32 {
    u32::try_from(k).unwrap_or(u32::MAX)
}

/// A count as `f32`, exact up to 65,535 and saturating beyond (more characters than a UI line
/// ever holds).
pub(crate) fn count(n: usize) -> f32 {
    f32::from(u16::try_from(n).unwrap_or(u16::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: TextStyle = TextStyle {
        size: 10.0,
        weight: 400,
        mono: false,
        wrap: false,
    };

    /// Records what's drawn.
    #[derive(Default)]
    struct Boxes(Vec<RectPx>);

    impl GlyphSink for Boxes {
        fn fill(&mut self, r: RectPx) {
            self.0.push(r);
        }
        fn mask(&mut self, _: i32, _: i32, _: u32, _: &[u8]) {
            unreachable!("FakeText draws boxes");
        }
    }

    fn px(x0: i32, y0: i32, x1: i32, y1: i32) -> RectPx {
        RectPx { x0, y0, x1, y1 }
    }

    #[test]
    fn one_line_text_is_trimmed_to_fit() {
        let m = FakeText.measure("abcdef", &BODY, f32::INFINITY);
        assert_eq!(
            m,
            TextMetrics {
                width: 30.0,
                height: 12.5,
                lines: 1,
                trimmed: false
            }
        );
        let m = FakeText.measure("abcdef", &BODY, 20.0);
        assert_eq!((m.width, m.trimmed), (20.0, true));
        let mono = TextStyle { mono: true, ..BODY };
        assert_eq!(FakeText.measure("ab", &mono, 100.0).width, 12.0);
        assert_eq!(FakeText.measure("", &BODY, 100.0).height, 12.5);
    }

    #[test]
    fn wrapping_breaks_greedily_at_spaces() {
        let wrap = TextStyle { wrap: true, ..BODY };
        // Words of 3, 2 and 4 characters: 15, 10 and 20 px; a space is 5 px.
        let m = FakeText.measure("abc de fghi", &wrap, 30.0);
        assert_eq!(
            m,
            TextMetrics {
                width: 30.0,
                height: 25.0,
                lines: 2,
                trimmed: false
            }
        );
        let m = FakeText.measure("abc de fghi", &wrap, f32::INFINITY);
        assert_eq!((m.lines, m.width), (1, 55.0));
        let m = FakeText.measure("abcdefgh x", &wrap, 20.0);
        assert_eq!((m.lines, m.width, m.trimmed), (2, 20.0, true));
    }

    #[test]
    fn ink_follows_the_layout() {
        // One line, trimmed to the characters that fit whole; the space draws nothing. Advance 5:
        // ink from x + 0.625 to x + 4.375 and y 2.5 to 10, rounded.
        let mut b = Boxes::default();
        FakeText.draw("a bc", &BODY, Rect::new(0.0, 0.0, 15.0, 20.0), 1.0, &mut b);
        assert_eq!(b.0, [px(1, 3, 4, 10), px(11, 3, 14, 10)]);
        // Wrapped: "de" starts the second line, 12.5 lower; at scale 2.
        let mut b = Boxes::default();
        let wrap = TextStyle { wrap: true, ..BODY };
        FakeText.draw(
            "abc de",
            &wrap,
            Rect::new(0.0, 0.0, 20.0, 40.0),
            2.0,
            &mut b,
        );
        assert_eq!(b.0.len(), 5);
        assert_eq!(b.0[3], px(1, 30, 9, 45));
        // Icons fill their middle half.
        let mut b = Boxes::default();
        FakeText.icon(
            Icon::CheckMark,
            Rect::new(10.0, 10.0, 8.0, 8.0),
            1.0,
            &mut b,
        );
        assert_eq!(b.0, [px(12, 12, 16, 16)]);
    }

    #[test]
    fn counts_saturate() {
        assert_eq!(count(3), 3.0);
        assert_eq!(count(usize::MAX), 65_535.0);
    }
}
