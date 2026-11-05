//! Text measurement. The platform implements [`TextMeasure`] with DirectWrite (TD-0009 §5.2);
//! T1 tests and benches use [`FakeText`], whose metrics are fixed and deterministic.

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
    /// Doc 05's body text: 14 px, regular, one line.
    fn default() -> Self {
        Self {
            size: 14.0,
            weight: 400,
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

/// Measures text for layout.
pub trait TextMeasure {
    /// The box of `text` in `style` within `max_width` (possibly infinite): wrapped if the style
    /// wraps, else one line, trimmed when it doesn't fit.
    fn measure(&mut self, text: &str, style: &TextStyle, max_width: f32) -> TextMetrics;
}

/// Deterministic metrics for T1: every character advances half the font size (six tenths in
/// mono), and a line is 1.25 × the size. Wrapping breaks greedily at spaces; a word longer than
/// the line overflows it and counts as trimmed.
#[derive(Clone, Copy, Debug, Default)]
pub struct FakeText;

impl FakeText {
    fn advance(style: TextStyle) -> f32 {
        style.size * if style.mono { 0.6 } else { 0.5 }
    }

    fn line_height(style: TextStyle) -> f32 {
        style.size * 1.25
    }
}

impl TextMeasure for FakeText {
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
        let (mut lines, mut widest, mut current) = (1u32, 0.0f32, 0.0f32);
        let mut trimmed = false;
        for word in text.split(' ') {
            let w = count(word.chars().count()) * advance;
            if current > 0.0 && current + advance + w > max_width {
                lines += 1;
                widest = widest.max(current);
                current = w;
            } else if current > 0.0 {
                current += advance + w;
            } else {
                current = w;
            }
            trimmed |= w > max_width;
        }
        widest = widest.max(current);
        TextMetrics {
            width: widest.min(max_width),
            height: line * count(usize::try_from(lines).unwrap_or(usize::MAX)),
            lines,
            trimmed,
        }
    }
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
    fn counts_saturate() {
        assert_eq!(count(3), 3.0);
        assert_eq!(count(usize::MAX), 65_535.0);
    }
}
