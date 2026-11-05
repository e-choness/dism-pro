//! Text through DirectWrite (TD-0009 §5.2). `dwrite.dll` is in base `WinPE` (S-01, run
//! 41598796130).
//!
//! `IDWriteTextLayout` shapes, wraps, falls back to other fonts and trims with an ellipsis. Our
//! `IDWriteTextRenderer` turns each glyph run into 8-bit coverage with `IDWriteGlyphRunAnalysis`,
//! which a [`Sink`] blends into the canvas. There's no Direct2D, so the same path works in `WinPE`,
//! under GDI and under `DirectComposition` (S-01's approach).
//!
//! - **Units.** Layouts are made in logical pixels (DIPs) and drawn at the window's scale, so
//!   measuring and drawing share one layout, and a layout doesn't change with the DPI.
//! - **Caches.** A layout per text, style and width, reused while they're unchanged (at most
//!   [`LAYOUT_CACHE_CAP`]); glyph coverage per face, em size, glyph and quarter-pixel phase, at
//!   most [`GLYPH_CACHE_CAP`] bytes (U13). Both evict the least recently used.
//! - **Families** by optical size: Segoe UI Variable Small at 12 px, Text at 13–15, Display at 20
//!   and up; then Segoe UI, then Tahoma. Mono: Cascadia Mono, Consolas, Courier New. `DejaVu`
//!   comes last in both, for Wine (T2). Where Segoe UI has no regular weight (`WinPE` ships
//!   Semilight, Semibold and Bold), regular text is set in Semilight (350), so the metrics stay in
//!   one family rather than DirectWrite picking the nearest weight.
//! - **Icons** from Segoe Fluent Icons, else Segoe MDL2 Assets (in base `WinPE`), by code point.

use std::{
    cell::OnceCell,
    collections::HashMap,
    ffi::c_void,
    hash::{DefaultHasher, Hash, Hasher},
    mem::ManuallyDrop,
};

use windows::{
    Win32::Graphics::DirectWrite::{
        DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_WEIGHT, DWRITE_GLYPH_RUN, DWRITE_GLYPH_RUN_DESCRIPTION,
        DWRITE_GRID_FIT_MODE_DEFAULT, DWRITE_MATRIX, DWRITE_MEASURING_MODE,
        DWRITE_MEASURING_MODE_NATURAL, DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
        DWRITE_STRIKETHROUGH, DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE, DWRITE_TEXT_METRICS,
        DWRITE_TEXTURE_ALIASED_1x1, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
        DWRITE_UNDERLINE, DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
        DWriteCreateFactory, IDWriteFactory, IDWriteFactory2, IDWriteFontCollection,
        IDWriteFontFace, IDWriteInlineObject, IDWritePixelSnapping_Impl, IDWriteTextFormat,
        IDWriteTextLayout, IDWriteTextRenderer, IDWriteTextRenderer_Impl,
    },
    core::{BOOL, HSTRING, IUnknown, Interface, Ref, w},
};

use super::lru::Lru;
use crate::error::Result;

/// U13: the glyph cache's bytes, coverage plus bookkeeping.
pub const GLYPH_CACHE_CAP: usize = 8 << 20;
/// Layouts kept: a page has about 600 text nodes, each measured at one or two widths.
pub const LAYOUT_CACHE_CAP: usize = 4_096;
/// The layout width that stands for "unbounded".
const UNBOUNDED: f32 = 1.0e6;
/// Slack for widths that went through layout arithmetic.
const EPSILON: f32 = 1.0e-3;

/// How a run of text is set, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub size: f32,
    pub weight: u16,
    pub mono: bool,
    /// Wrap at word boundaries to the width; otherwise one line, trimmed with an ellipsis.
    pub wrap: bool,
}

/// The measured box of a run of text, in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    pub width: f32,
    pub height: f32,
    pub lines: u32,
    /// It didn't fit: one-line text is drawn with an ellipsis, and a wrapped word overflows.
    pub trimmed: bool,
}

/// A rectangle in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Where drawn text goes: 8-bit coverage masks in physical pixels, rows of `width` values with
/// their top-left at `(x, y)`. The receiver tints and clips them.
pub trait Sink {
    fn mask(&mut self, x: i32, y: i32, width: u32, coverage: &[u8]);
}

/// The DirectWrite text system of one UI thread.
pub struct Text {
    factory: IDWriteFactory2,
    renderer: IDWriteTextRenderer,
    formats: Formats,
    layouts: Lru<u64, CachedLayout>,
    glyphs: Lru<GlyphKey, Glyph>,
    /// Every face a glyph is keyed on, kept alive so no other face can reuse its address.
    faces: HashMap<usize, IDWriteFontFace>,
    /// The icon font's face, looked up on first use; `None` if neither font is installed.
    icons: OnceCell<Option<IDWriteFontFace>>,
}

impl std::fmt::Debug for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Text")
            .field("families", &self.formats.families)
            .field("regular", &self.formats.regular)
            .field("layouts", &self.layouts.len())
            .field("glyph_bytes", &self.glyphs.weight())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FormatKey {
    size: u32,
    weight: u16,
    mono: bool,
    wrap: bool,
}

/// A cached layout and what it was made for, to tell a hash collision from a hit.
struct CachedLayout {
    text: Box<str>,
    style: Style,
    width: f32,
    layout: IDWriteTextLayout,
    metrics: Metrics,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    face: usize,
    index: u16,
    em: u32,
    phase: u8,
}

/// A glyph's coverage, its top-left relative to the pen on the baseline.
struct Glyph {
    left: i32,
    top: i32,
    width: u32,
    coverage: Vec<u8>,
}

#[derive(Debug)]
struct Families {
    small: HSTRING,
    text: HSTRING,
    display: HSTRING,
    mono: HSTRING,
}

impl Families {
    fn for_style(&self, style: Style) -> &HSTRING {
        if style.mono {
            &self.mono
        } else if style.size <= 12.0 {
            &self.small
        } else if style.size < 20.0 {
            &self.text
        } else {
            &self.display
        }
    }
}

const SMALL: [&str; 4] = [
    "Segoe UI Variable Small",
    "Segoe UI",
    "Tahoma",
    "DejaVu Sans",
];
const TEXT: [&str; 4] = [
    "Segoe UI Variable Text",
    "Segoe UI",
    "Tahoma",
    "DejaVu Sans",
];
const DISPLAY: [&str; 4] = [
    "Segoe UI Variable Display",
    "Segoe UI",
    "Tahoma",
    "DejaVu Sans",
];
const MONO: [&str; 4] = [
    "Cascadia Mono",
    "Consolas",
    "Courier New",
    "DejaVu Sans Mono",
];
const ICONS: [&str; 2] = ["Segoe Fluent Icons", "Segoe MDL2 Assets"];

/// Text formats by size, weight, family and wrapping, made on first use.
struct Formats {
    factory: IDWriteFactory2,
    families: Families,
    /// The weight regular sans text is set in: 400, or 350 where Segoe UI has no regular.
    regular: u16,
    map: HashMap<FormatKey, IDWriteTextFormat>,
}

impl Formats {
    fn get(&mut self, style: Style) -> Option<IDWriteTextFormat> {
        let key = FormatKey {
            size: style.size.to_bits(),
            weight: style.weight,
            mono: style.mono,
            wrap: style.wrap,
        };
        if let Some(f) = self.map.get(&key) {
            return Some(f.clone());
        }
        let weight = if style.weight == 400 && !style.mono {
            self.regular
        } else {
            style.weight
        };
        let family = self.families.for_style(style);
        // SAFETY: valid strings and enums; the format is configured before any layout uses it.
        let format = unsafe {
            let f = self
                .factory
                .CreateTextFormat(
                    family,
                    None,
                    DWRITE_FONT_WEIGHT(i32::from(weight)),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    style.size,
                    w!("en-us"),
                )
                .ok()?;
            if style.wrap {
                f.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP).ok()?;
            } else {
                f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP).ok()?;
                let sign = self.factory.CreateEllipsisTrimmingSign(&f).ok()?;
                let trimming = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                };
                f.SetTrimming(&raw const trimming, &sign).ok()?;
            }
            f
        };
        self.map.insert(key, format.clone());
        Some(format)
    }
}

fn os_error(e: &windows::core::Error, api: &'static str) -> crate::Error {
    dpro_core::error::OsError {
        code: e.code().0,
        api,
    }
    .into()
}

impl Text {
    /// Creates the shared DirectWrite factory and picks the font families.
    ///
    /// # Errors
    /// DirectWrite or its system font collection is unavailable.
    pub fn new() -> Result<Self> {
        // SAFETY: creating the shared factory has no preconditions.
        let factory: IDWriteFactory2 = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }
            .map_err(|e| os_error(&e, "DWriteCreateFactory"))?;
        let fonts = system_fonts(&factory).map_err(|e| os_error(&e, "GetSystemFontCollection"))?;
        let text = pick(&fonts, &TEXT);
        let regular = if text == "Segoe UI" && !has_regular(&fonts, &HSTRING::from(text)) {
            350
        } else {
            400
        };
        let families = Families {
            small: HSTRING::from(pick(&fonts, &SMALL)),
            text: HSTRING::from(text),
            display: HSTRING::from(pick(&fonts, &DISPLAY)),
            mono: HSTRING::from(pick(&fonts, &MONO)),
        };
        Ok(Self {
            renderer: com::Renderer.into(),
            formats: Formats {
                factory: factory.clone(),
                families,
                regular,
                map: HashMap::new(),
            },
            factory,
            layouts: Lru::new(LAYOUT_CACHE_CAP),
            glyphs: Lru::new(GLYPH_CACHE_CAP),
            faces: HashMap::new(),
            icons: OnceCell::new(),
        })
    }

    /// The box of `text` in `style` within `max_width` (possibly infinite): wrapped if the style
    /// wraps, else one line, trimmed when it doesn't fit.
    pub fn measure(&mut self, text: &str, style: &Style, max_width: f32) -> Metrics {
        if style.wrap {
            let Some((_, m)) = self.layout(text, *style, max_width) else {
                return Metrics::default();
            };
            return Metrics {
                trimmed: m.width > max_width + EPSILON,
                ..m
            };
        }
        let Some((_, natural)) = self.layout(text, *style, f32::INFINITY) else {
            return Metrics::default();
        };
        Metrics {
            width: natural.width.min(max_width),
            lines: 1,
            trimmed: natural.width > max_width + EPSILON,
            ..natural
        }
    }

    /// Draws `text` as [`measure`](Self::measure) lays it out at `rect.w`, its top-left at
    /// `rect`'s, at `scale` physical pixels per logical pixel.
    pub fn draw(
        &mut self,
        text: &str,
        style: &Style,
        rect: RectF,
        scale: f32,
        sink: &mut dyn Sink,
    ) {
        if text.is_empty() || rect.w <= 0.0 {
            return;
        }
        let width = if style.wrap {
            rect.w
        } else {
            // One line that fits is the unbounded layout; only one that doesn't needs trimming.
            match self.layout(text, *style, f32::INFINITY) {
                Some((_, natural)) if natural.width <= rect.w + EPSILON => f32::INFINITY,
                Some(_) => rect.w,
                None => return,
            }
        };
        let Some((layout, _)) = self.layout(text, *style, width) else {
            return;
        };
        let mut cx = DrawCx {
            sink,
            glyphs: &mut self.glyphs,
            faces: &mut self.faces,
            factory: &self.factory,
            renderer: &self.renderer,
            scale,
        };
        // SAFETY: `cx` outlives this synchronous call, and the renderer reads it only as a DrawCx.
        let _ = unsafe {
            layout.Draw(
                Some((&raw mut cx).cast_const().cast()),
                &self.renderer,
                rect.x,
                rect.y,
            )
        };
    }

    /// Draws the icon font's glyph for `codepoint` centered in `rect`, its em as large as fits.
    /// Nothing is drawn without an icon font, or for a code point it lacks.
    pub fn icon(&mut self, codepoint: u32, rect: RectF, scale: f32, sink: &mut dyn Sink) {
        let Some(face) = self.icon_face() else {
            return;
        };
        let mut index = 0_u16;
        // SAFETY: one code point in, one glyph index out, both live locals.
        if unsafe { face.GetGlyphIndices(&raw const codepoint, 1, &raw mut index) }.is_err()
            || index == 0
        {
            return;
        }
        let em = (rect.w.min(rect.h) * scale).round().max(1.0);
        let key = GlyphKey {
            face: face.as_raw().addr(),
            index,
            em: em.to_bits(),
            phase: 0,
        };
        let faces = &mut self.faces;
        let factory = &self.factory;
        let Some(g) = self.glyphs.get_or_insert_with(key, || {
            faces.entry(key.face).or_insert_with(|| face.clone());
            rasterize(factory, &face, em, index, 0.0)
        }) else {
            return;
        };
        if g.width == 0 {
            return;
        }
        let rows = g.coverage.len() / g.width as usize;
        let cx = (rect.x + rect.w / 2.0) * scale;
        let cy = (rect.y + rect.h / 2.0) * scale;
        sink.mask(
            px(cx - count(g.width as usize) / 2.0),
            px(cy - count(rows) / 2.0),
            g.width,
            &g.coverage,
        );
    }

    /// The glyph cache's bytes (U13).
    #[must_use]
    pub fn glyph_bytes(&self) -> usize {
        self.glyphs.weight()
    }

    fn icon_face(&self) -> Option<IDWriteFontFace> {
        self.icons
            .get_or_init(|| {
                let fonts = system_fonts(&self.factory).ok()?;
                ICONS
                    .iter()
                    .find_map(|name| face_of(&fonts, &HSTRING::from(*name)))
            })
            .clone()
    }

    /// The layout of `text` in `style` at `width` (infinite: unbounded), and its metrics.
    fn layout(
        &mut self,
        text: &str,
        style: Style,
        width: f32,
    ) -> Option<(IDWriteTextLayout, Metrics)> {
        let width = if width.is_finite() {
            width.clamp(0.0, UNBOUNDED)
        } else {
            UNBOUNDED
        };
        let mut hasher = DefaultHasher::new();
        (
            text,
            style.size.to_bits(),
            style.weight,
            style.mono,
            style.wrap,
            width.to_bits(),
        )
            .hash(&mut hasher);
        let key = hasher.finish();
        let formats = &mut self.formats;
        let found = self.layouts.get_or_insert_with(key, || {
            create_layout(formats, text, style, width).map(|l| (l, 1))
        })?;
        if &*found.text == text && found.style == style && found.width.to_bits() == width.to_bits()
        {
            return Some((found.layout.clone(), found.metrics));
        }
        // A hash collision: this one goes uncached.
        create_layout(&mut self.formats, text, style, width).map(|l| (l.layout, l.metrics))
    }
}

fn create_layout(
    formats: &mut Formats,
    text: &str,
    style: Style,
    width: f32,
) -> Option<CachedLayout> {
    let format = formats.get(style)?;
    let factory = &formats.factory;
    let utf16: Vec<u16> = text.encode_utf16().collect();
    // SAFETY: valid UTF-16 and format; the layout copies the string.
    let layout = unsafe { factory.CreateTextLayout(&utf16, &format, width, UNBOUNDED) }.ok()?;
    let mut m = DWRITE_TEXT_METRICS::default();
    // SAFETY: a live layout and a live out-pointer.
    unsafe { layout.GetMetrics(&raw mut m) }.ok()?;
    Some(CachedLayout {
        text: text.into(),
        style,
        width,
        layout,
        metrics: Metrics {
            width: m.width,
            height: m.height,
            lines: m.lineCount,
            trimmed: false,
        },
    })
}

fn system_fonts(factory: &IDWriteFactory2) -> windows::core::Result<IDWriteFontCollection> {
    let base: &IDWriteFactory = factory;
    let mut fonts = None;
    // SAFETY: a live out-pointer; FALSE: don't check for updates.
    unsafe { base.GetSystemFontCollection(&raw mut fonts, false) }?;
    fonts.ok_or_else(windows::core::Error::empty)
}

/// The first of `candidates` installed, else the last (DirectWrite then falls back on its own).
fn pick(fonts: &IDWriteFontCollection, candidates: &[&'static str]) -> &'static str {
    candidates
        .iter()
        .copied()
        .find(|name| family_index(fonts, &HSTRING::from(*name)).is_some())
        .unwrap_or(candidates[candidates.len() - 1])
}

fn family_index(fonts: &IDWriteFontCollection, name: &HSTRING) -> Option<u32> {
    let (mut index, mut exists) = (0_u32, BOOL(0));
    // SAFETY: live out-pointers.
    unsafe { fonts.FindFamilyName(name, &raw mut index, &raw mut exists) }.ok()?;
    exists.as_bool().then_some(index)
}

/// Whether `family`'s best match for weight 400 is 400 (`WinPE`'s Segoe UI has no regular).
fn has_regular(fonts: &IDWriteFontCollection, family: &HSTRING) -> bool {
    let Some(index) = family_index(fonts, family) else {
        return true;
    };
    // SAFETY: an index FindFamilyName returned, and plain enum arguments.
    unsafe {
        fonts
            .GetFontFamily(index)
            .and_then(|f| {
                f.GetFirstMatchingFont(
                    DWRITE_FONT_WEIGHT(400),
                    DWRITE_FONT_STRETCH_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                )
            })
            .is_ok_and(|font| font.GetWeight().0 == 400)
    }
}

/// The regular face of `family`, if installed.
fn face_of(fonts: &IDWriteFontCollection, family: &HSTRING) -> Option<IDWriteFontFace> {
    let index = family_index(fonts, family)?;
    // SAFETY: an index FindFamilyName returned, and plain enum arguments.
    unsafe {
        fonts
            .GetFontFamily(index)
            .and_then(|f| {
                f.GetFirstMatchingFont(
                    DWRITE_FONT_WEIGHT(400),
                    DWRITE_FONT_STRETCH_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                )
            })
            .and_then(|font| font.CreateFontFace())
            .ok()
    }
}

/// One glyph's coverage at `em` pixels, its pen `phase` pixels right of a pixel boundary, and its
/// weight in the cache.
fn rasterize(
    factory: &IDWriteFactory2,
    face: &IDWriteFontFace,
    em: f32,
    index: u16,
    phase: f32,
) -> Option<(Glyph, usize)> {
    let advance = 0.0_f32;
    let run = DWRITE_GLYPH_RUN {
        fontFace: ManuallyDrop::new(Some(face.clone())),
        fontEmSize: em,
        glyphCount: 1,
        glyphIndices: &raw const index,
        glyphAdvances: &raw const advance,
        glyphOffsets: std::ptr::null(),
        isSideways: BOOL(0),
        bidiLevel: 0,
    };
    // SAFETY: `run` and the values it points to outlive the call. Grayscale antialiasing makes
    // the ALIASED_1x1 texture one byte of coverage per pixel.
    let analysis = unsafe {
        factory.CreateGlyphRunAnalysis(
            &raw const run,
            None,
            DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
            DWRITE_MEASURING_MODE_NATURAL,
            DWRITE_GRID_FIT_MODE_DEFAULT,
            DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE,
            phase,
            0.0,
        )
    };
    drop(ManuallyDrop::into_inner(run.fontFace));
    let analysis = analysis.ok()?;
    // SAFETY: a live analysis object.
    let b = unsafe { analysis.GetAlphaTextureBounds(DWRITE_TEXTURE_ALIASED_1x1) }.ok()?;
    let width = u32::try_from(b.right.saturating_sub(b.left)).unwrap_or(0);
    let height = u32::try_from(b.bottom.saturating_sub(b.top)).unwrap_or(0);
    let len = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    let mut coverage = vec![0_u8; len];
    if len > 0 {
        // SAFETY: the buffer is exactly width × height bytes for these bounds.
        unsafe {
            analysis.CreateAlphaTexture(DWRITE_TEXTURE_ALIASED_1x1, &raw const b, &mut coverage)
        }
        .ok()?;
    }
    let weight = len + size_of::<Glyph>() + size_of::<GlyphKey>();
    Some((
        Glyph {
            left: b.left,
            top: b.top,
            width: if len > 0 { width } else { 0 },
            coverage,
        },
        weight,
    ))
}

/// What `IDWriteTextLayout::Draw` hands our renderer as its client drawing context.
struct DrawCx<'a> {
    sink: &'a mut dyn Sink,
    glyphs: &'a mut Lru<GlyphKey, Glyph>,
    faces: &'a mut HashMap<usize, IDWriteFontFace>,
    factory: &'a IDWriteFactory2,
    renderer: &'a IDWriteTextRenderer,
    scale: f32,
}

impl DrawCx<'_> {
    /// Blits one glyph with its pen at `pen_x` and its baseline at `baseline`, physical pixels.
    fn glyph(&mut self, face: &IDWriteFontFace, em: f32, index: u16, pen_x: f32, baseline: i32) {
        let mut left = pen_x.floor();
        let mut phase = px((pen_x - left) * 4.0);
        if phase == 4 {
            left += 1.0;
            phase = 0;
        }
        let key = GlyphKey {
            face: face.as_raw().addr(),
            index,
            em: em.to_bits(),
            phase: u8::try_from(phase).unwrap_or(0),
        };
        let (faces, factory) = (&mut *self.faces, self.factory);
        let Some(g) = self.glyphs.get_or_insert_with(key, || {
            faces.entry(key.face).or_insert_with(|| face.clone());
            rasterize(factory, face, em, index, f32::from(key.phase) / 4.0)
        }) else {
            return;
        };
        if g.width > 0 {
            self.sink
                .mask(px(left) + g.left, baseline + g.top, g.width, &g.coverage);
        }
    }
}

/// The renderer's COM object. In its own module so the allow covers only `#[implement]`'s
/// expansion, which trips pedantic lints in generated code.
#[allow(
    clippy::inline_always,
    clippy::ref_as_ptr,
    missing_debug_implementations
)]
mod com {
    use windows::{Win32::Graphics::DirectWrite::IDWriteTextRenderer, core::implement};

    #[implement(IDWriteTextRenderer)]
    pub(super) struct Renderer;
}

impl IDWritePixelSnapping_Impl for com::Renderer_Impl {
    fn IsPixelSnappingDisabled(&self, _: *const c_void) -> windows::core::Result<BOOL> {
        Ok(BOOL(0))
    }

    fn GetCurrentTransform(
        &self,
        _: *const c_void,
        transform: *mut DWRITE_MATRIX,
    ) -> windows::core::Result<()> {
        // SAFETY: DirectWrite passes a valid out-pointer.
        unsafe {
            transform.write(DWRITE_MATRIX {
                m11: 1.0,
                m12: 0.0,
                m21: 0.0,
                m22: 1.0,
                dx: 0.0,
                dy: 0.0,
            });
        };
        Ok(())
    }

    fn GetPixelsPerDip(&self, cx: *const c_void) -> windows::core::Result<f32> {
        // SAFETY: the context is the DrawCx that Text::draw passed to this synchronous Draw.
        Ok(unsafe { (*cx.cast::<DrawCx<'_>>()).scale })
    }
}

impl IDWriteTextRenderer_Impl for com::Renderer_Impl {
    fn DrawGlyphRun(
        &self,
        cx: *const c_void,
        x: f32,
        y: f32,
        _: DWRITE_MEASURING_MODE,
        run: *const DWRITE_GLYPH_RUN,
        _: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _: Ref<IUnknown>,
    ) -> windows::core::Result<()> {
        // SAFETY: `cx` is the DrawCx that Text::draw passed to this synchronous Draw, used by
        // nothing else meanwhile; `run` is valid for the callback.
        let (cx, run) = unsafe { (&mut *cx.cast_mut().cast::<DrawCx<'_>>(), &*run) };
        let Some(face) = run.fontFace.as_ref() else {
            return Ok(());
        };
        let n = usize::try_from(run.glyphCount).unwrap_or(0);
        if n == 0 || run.glyphIndices.is_null() || run.glyphAdvances.is_null() {
            return Ok(());
        }
        // SAFETY: DirectWrite gives glyphCount entries in each array; offsets may be null.
        let (indices, advances, offsets) = unsafe {
            (
                std::slice::from_raw_parts(run.glyphIndices, n),
                std::slice::from_raw_parts(run.glyphAdvances, n),
                (!run.glyphOffsets.is_null())
                    .then(|| std::slice::from_raw_parts(run.glyphOffsets, n)),
            )
        };
        let scale = cx.scale;
        let em = run.fontEmSize * scale;
        let rtl = run.bidiLevel & 1 == 1;
        let baseline = px(y * scale);
        let mut pen = x * scale;
        for (i, (&index, &advance)) in indices.iter().zip(advances).enumerate() {
            let advance = advance * scale;
            if rtl {
                pen -= advance;
            }
            let (ox, oy) = offsets.map_or((0.0, 0.0), |o| {
                (o[i].advanceOffset * scale, o[i].ascenderOffset * scale)
            });
            let gx = pen + if rtl { -ox } else { ox };
            cx.glyph(face, em, index, gx, baseline - px(oy));
            if !rtl {
                pen += advance;
            }
        }
        Ok(())
    }

    fn DrawUnderline(
        &self,
        _: *const c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_UNDERLINE,
        _: Ref<IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn DrawStrikethrough(
        &self,
        _: *const c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_STRIKETHROUGH,
        _: Ref<IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    /// A trimmed line's ellipsis is an inline object, which draws itself through us.
    fn DrawInlineObject(
        &self,
        cx: *const c_void,
        x: f32,
        y: f32,
        object: Ref<IDWriteInlineObject>,
        sideways: BOOL,
        rtl: BOOL,
        _: Ref<IUnknown>,
    ) -> windows::core::Result<()> {
        let object = object.ok()?;
        // SAFETY: as in DrawGlyphRun; the renderer outlives the outer Draw call.
        let renderer = unsafe { (*cx.cast::<DrawCx<'_>>()).renderer.clone() };
        // SAFETY: forwards our own context to a nested, synchronous Draw.
        unsafe {
            object.Draw(
                Some(cx),
                &renderer,
                x,
                y,
                sideways.as_bool(),
                rtl.as_bool(),
                None::<&IUnknown>,
            )
        }
    }
}

/// A float pixel coordinate as an integer: `as` saturates, and NaN becomes 0.
#[allow(
    clippy::cast_possible_truncation,
    reason = "float-to-int `as` saturates, and NaN becomes 0"
)]
fn px(v: f32) -> i32 {
    v.round() as i32
}

/// A small count as `f32`, exact up to 65,535 and saturating beyond.
fn count(n: usize) -> f32 {
    f32::from(u16::try_from(n).unwrap_or(u16::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collects what's drawn: the bounding box of the ink, and the total coverage.
    #[derive(Default)]
    struct Ink {
        masks: usize,
        sum: u64,
        bounds: Option<(i32, i32, i32, i32)>,
    }

    impl Sink for Ink {
        fn mask(&mut self, x: i32, y: i32, width: u32, coverage: &[u8]) {
            self.masks += 1;
            self.sum += coverage.iter().map(|&c| u64::from(c)).sum::<u64>();
            let w = i32::try_from(width).unwrap_or(0);
            let h = i32::try_from(coverage.len() / width as usize).unwrap_or(0);
            let (x0, y0, x1, y1) = self.bounds.unwrap_or((x, y, x + w, y + h));
            self.bounds = Some((x0.min(x), y0.min(y), x1.max(x + w), y1.max(y + h)));
        }
    }

    const BODY: Style = Style {
        size: 13.0,
        weight: 400,
        mono: false,
        wrap: false,
    };

    /// Every family and weight the type ramp uses sets text. The output names the families picked,
    /// which T3 reports from the runner (TD-0009 §5.2's verification).
    #[test]
    fn every_family_and_weight_sets_text() {
        let mut text = Text::new().expect("DirectWrite");
        println!("{text:?}");
        for (size, mono) in [
            (12.0, false),
            (13.0, false),
            (20.0, false),
            (28.0, false),
            (13.0, true),
        ] {
            for weight in [400, 600] {
                let style = Style {
                    size,
                    weight,
                    mono,
                    ..BODY
                };
                let m = text.measure("Review changes", &style, f32::INFINITY);
                assert!(m.width > size && m.height >= size, "{style:?}: {m:?}");
            }
        }
    }

    /// U13: drawing far more glyph coverage than the cap holds keeps the cache under it.
    #[test]
    fn the_glyph_cache_stays_under_8_mib() {
        assert_eq!(GLYPH_CACHE_CAP, 8 << 20);
        let mut text = Text::new().expect("DirectWrite");
        let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut peak = 0;
        for size in (16_u16..=160).step_by(4) {
            let style = Style {
                size: f32::from(size),
                ..BODY
            };
            let rect = RectF {
                w: 1.0e5,
                h: f32::from(size) * 2.0,
                ..RectF::default()
            };
            text.draw(alphabet, &style, rect, 2.0, &mut Ink::default());
            peak = peak.max(text.glyph_bytes());
            assert!(
                text.glyph_bytes() <= GLYPH_CACHE_CAP,
                "{size}: {}",
                text.glyph_bytes()
            );
        }
        assert!(
            peak > GLYPH_CACHE_CAP / 2,
            "the test never filled the cache: {peak}"
        );
    }

    #[test]
    fn an_icon_is_centered_in_its_box_when_the_icon_font_exists() {
        let mut text = Text::new().expect("DirectWrite");
        let mut ink = Ink::default();
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            w: 16.0,
            h: 16.0,
        };
        // U+E73E CheckMark. Wine has neither icon font; then nothing is drawn. Windows has one,
        // and it must have the check (T3).
        let font = text.icon_face().is_some();
        text.icon(0xE73E, rect, 1.5, &mut ink);
        assert_eq!(ink.bounds.is_some(), font, "icon font: {font}");
        if let Some((x0, y0, x1, y1)) = ink.bounds {
            assert!(
                x0 >= 0 && y0 >= 0 && x1 <= 24 && y1 <= 24,
                "{:?}",
                ink.bounds
            );
            assert!(
                (x0 + x1 - 24).abs() <= 1 && (y0 + y1 - 24).abs() <= 1,
                "{:?}",
                ink.bounds
            );
        }
        // A code point no font has (plane 16's private use area) draws nothing.
        let mut none = Ink::default();
        text.icon(0x10_FFFD, rect, 1.0, &mut none);
        assert_eq!(none.masks, 0);
    }

    #[test]
    fn measuring_grows_with_the_text_and_trims_what_does_not_fit() {
        let mut text = Text::new().expect("DirectWrite");
        let one = text.measure("W", &BODY, f32::INFINITY);
        let many = text.measure("WWWWWWWWWW", &BODY, f32::INFINITY);
        assert!(one.width > 0.0 && one.height > 0.0, "{one:?}");
        assert!(many.width > one.width * 5.0, "{one:?} {many:?}");
        assert_eq!((many.lines, many.trimmed), (1, false));
        let cut = text.measure("WWWWWWWWWW", &BODY, one.width * 3.0);
        assert!(
            cut.trimmed && cut.width <= one.width * 3.0 + EPSILON,
            "{cut:?}"
        );
        let wrapped = text.measure(
            "the quick brown fox jumps over the lazy dog",
            &Style { wrap: true, ..BODY },
            many.width,
        );
        assert!(
            wrapped.lines > 1 && wrapped.height > one.height,
            "{wrapped:?}"
        );
    }

    #[test]
    fn drawing_inks_inside_the_box_at_any_scale_and_reuses_glyphs() {
        let mut text = Text::new().expect("DirectWrite");
        let m = text.measure("Hello", &BODY, f32::INFINITY);
        let rect = RectF {
            x: 10.0,
            y: 20.0,
            w: m.width,
            h: m.height,
        };
        let mut ink = Ink::default();
        text.draw("Hello", &BODY, rect, 1.0, &mut ink);
        let (x0, y0, x1, y1) = ink.bounds.expect("ink");
        assert!(ink.sum > 0);
        assert!(x0 >= 9 && y0 >= 19, "{:?}", ink.bounds);
        assert!(
            x1 <= 12 + px(m.width) && y1 <= 21 + px(m.height),
            "{:?}",
            ink.bounds
        );
        let bytes = text.glyph_bytes();
        assert!(bytes > 0);
        let mut again = Ink::default();
        text.draw("Hello", &BODY, rect, 1.0, &mut again);
        assert_eq!(text.glyph_bytes(), bytes, "the glyphs came from the cache");
        assert_eq!(again.sum, ink.sum);
        // At 200 % the same text covers about four times the pixels.
        let mut double = Ink::default();
        text.draw("Hello", &BODY, rect, 2.0, &mut double);
        let (dx0, _, dx1, _) = double.bounds.expect("ink");
        assert!(dx0 >= 19 && dx1 > x1, "{:?}", double.bounds);
        assert!(double.sum > ink.sum * 3, "{} vs {}", double.sum, ink.sum);
    }
}
