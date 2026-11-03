//! Text through DirectWrite (`dwrite.dll` is in base WinPE; S-01 facts, run 41589907500).
//!
//! `IDWriteTextLayout` does shaping, font fallback, alignment and ellipsis trimming; our
//! `IDWriteTextRenderer` turns each glyph run into grayscale coverage with
//! `IDWriteGlyphRunAnalysis` (the approach Skia's DirectWrite backend uses) and blends it into the
//! software canvas. No Direct2D, so the same text path works in WinPE, under GDI and under
//! DirectComposition. Layouts live while they're drawn every frame; glyph masks are cached per
//! face, size and quarter-pixel phase.

use std::{
    collections::HashMap,
    ffi::c_void,
    hash::{DefaultHasher, Hash, Hasher},
    mem::ManuallyDrop,
};

use scene::{layout::Rect, tokens::Rgb};
use windows::{
    Win32::Graphics::DirectWrite::{
        DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_GLYPH_RUN,
        DWRITE_GLYPH_RUN_DESCRIPTION, DWRITE_GRID_FIT_MODE_DEFAULT, DWRITE_MATRIX,
        DWRITE_MEASURING_MODE, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
        DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC, DWRITE_STRIKETHROUGH,
        DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
        DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE,
        DWRITE_TEXTURE_ALIASED_1x1, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
        DWRITE_UNDERLINE, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory,
        IDWriteFactory2, IDWriteFontFace, IDWriteInlineObject, IDWritePixelSnapping_Impl,
        IDWriteTextFormat, IDWriteTextLayout, IDWriteTextRenderer, IDWriteTextRenderer_Impl,
    },
    core::{BOOL, HSTRING, IUnknown, Interface, Ref, Result, w},
};

use crate::canvas::{Canvas, Coverage};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Right,
    Center,
}

#[derive(Clone, Copy, Debug)]
pub struct Style {
    /// Logical px (doc 05 type scale).
    pub size: f32,
    pub bold: bool,
    pub mono: bool,
    pub color: Rgb,
    pub alpha: u8,
    pub align: Align,
}

impl Style {
    pub const fn new(size: f32, color: Rgb) -> Self {
        Self {
            size,
            bold: false,
            mono: false,
            color,
            alpha: 255,
            align: Align::Left,
        }
    }

    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub const fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    pub const fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }
}

struct Glyph {
    left: i32,
    top: i32,
    w: u32,
    h: u32,
    coverage: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    face: usize,
    index: u16,
    em: u32,
    phase: u8,
}

struct Glyphs {
    factory: IDWriteFactory2,
    map: HashMap<GlyphKey, Glyph>,
    /// Keeps every face we've keyed on alive, so its address can't be reused by another face.
    faces: HashMap<usize, IDWriteFontFace>,
}

impl Glyphs {
    fn get(&mut self, face: &IDWriteFontFace, em: f32, index: u16, phase: u8) -> Option<&Glyph> {
        let key = GlyphKey {
            face: face.as_raw() as usize,
            index,
            em: em.to_bits(),
            phase,
        };
        if !self.map.contains_key(&key) {
            self.faces.entry(key.face).or_insert_with(|| face.clone());
            let g = match rasterize(&self.factory, face, em, index, f32::from(phase) / 4.0) {
                Ok(g) => g,
                Err(e) => {
                    debug(&format!("glyph {index} at {em}: {e}"));
                    return None;
                }
            };
            self.map.insert(key, g);
        }
        self.map.get(&key)
    }
}

fn rasterize(
    factory: &IDWriteFactory2,
    face: &IDWriteFontFace,
    em: f32,
    index: u16,
    phase: f32,
) -> Result<Glyph> {
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
    // SAFETY: `run` and the arrays it points to outlive the call; grayscale antialiasing makes the
    // ALIASED_1x1 texture 8-bit coverage.
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
    let analysis = analysis?;
    // SAFETY: a live analysis object.
    let b = unsafe { analysis.GetAlphaTextureBounds(DWRITE_TEXTURE_ALIASED_1x1)? };
    let (w, h) = (
        (b.right - b.left).max(0) as u32,
        (b.bottom - b.top).max(0) as u32,
    );
    let mut coverage = vec![0_u8; (w * h) as usize];
    if w > 0 && h > 0 {
        // SAFETY: the buffer is exactly w × h bytes for these bounds.
        unsafe {
            analysis.CreateAlphaTexture(DWRITE_TEXTURE_ALIASED_1x1, &raw const b, &mut coverage)?;
        };
    }
    Ok(Glyph {
        left: b.left,
        top: b.top,
        w,
        h,
        coverage,
    })
}

/// What a `Draw` call hands our renderer through DirectWrite's client drawing context.
struct DrawCtx<'a> {
    canvas: &'a mut Canvas,
    glyphs: &'a mut Glyphs,
    renderer: &'a IDWriteTextRenderer,
    color: Rgb,
    alpha: u8,
}

/// The COM object. In its own module so the allow covers only the `#[implement]` expansion, which
/// trips pedantic lints on generated code.
#[allow(clippy::inline_always, clippy::ref_as_ptr)]
mod com {
    use windows::{Win32::Graphics::DirectWrite::IDWriteTextRenderer, core::implement};

    #[implement(IDWriteTextRenderer)]
    pub struct Renderer;
}
use com::{Renderer, Renderer_Impl};

impl IDWritePixelSnapping_Impl for Renderer_Impl {
    fn IsPixelSnappingDisabled(&self, _: *const c_void) -> Result<BOOL> {
        Ok(BOOL(0))
    }

    fn GetCurrentTransform(&self, _: *const c_void, transform: *mut DWRITE_MATRIX) -> Result<()> {
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

    fn GetPixelsPerDip(&self, _: *const c_void) -> Result<f32> {
        Ok(1.0) // layouts are made in physical pixels
    }
}

impl IDWriteTextRenderer_Impl for Renderer_Impl {
    fn DrawGlyphRun(
        &self,
        ctx: *const c_void,
        x: f32,
        y: f32,
        _: DWRITE_MEASURING_MODE,
        run: *const DWRITE_GLYPH_RUN,
        _: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _: Ref<IUnknown>,
    ) -> Result<()> {
        // SAFETY: `ctx` is the DrawCtx that `Text::draw` passed to this synchronous Draw call,
        // and `run` is valid for the duration of the callback.
        let (ctx, run) = unsafe { (&mut *ctx.cast_mut().cast::<DrawCtx<'_>>(), &*run) };
        let Some(face) = run.fontFace.as_ref() else {
            return Ok(());
        };
        let n = run.glyphCount as usize;
        debug(&format!(
            "run of {n} glyphs at ({x}, {y}), em {}",
            run.fontEmSize
        ));
        if n == 0 {
            return Ok(());
        }
        // SAFETY: DirectWrite guarantees glyphCount entries in each array; offsets may be null.
        let (indices, advances, offsets) = unsafe {
            (
                std::slice::from_raw_parts(run.glyphIndices, n),
                std::slice::from_raw_parts(run.glyphAdvances, n),
                (!run.glyphOffsets.is_null())
                    .then(|| std::slice::from_raw_parts(run.glyphOffsets, n)),
            )
        };
        let rtl = run.bidiLevel & 1 == 1;
        let baseline = y.round() as i32;
        let mut pen = x;
        for i in 0..n {
            if rtl {
                pen -= advances[i];
            }
            let (ox, oy) =
                offsets.map_or((0.0, 0.0), |o| (o[i].advanceOffset, o[i].ascenderOffset));
            let gx = pen + if rtl { -ox } else { ox };
            let mut fx = gx.floor();
            let mut phase = ((gx - fx) * 4.0).round() as u8;
            if phase == 4 {
                fx += 1.0;
                phase = 0;
            }
            if let Some(g) = ctx.glyphs.get(face, run.fontEmSize, indices[i], phase) {
                let gy = baseline - oy.round() as i32;
                ctx.canvas.blit(
                    &Coverage {
                        x: fx as i32 + g.left,
                        y: gy + g.top,
                        w: g.w,
                        h: g.h,
                        data: &g.coverage,
                    },
                    ctx.color,
                    ctx.alpha,
                );
            }
            if !rtl {
                pen += advances[i];
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
    ) -> Result<()> {
        Ok(())
    }

    fn DrawStrikethrough(
        &self,
        _: *const c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_STRIKETHROUGH,
        _: Ref<IUnknown>,
    ) -> Result<()> {
        Ok(())
    }

    /// The ellipsis of a trimmed line is an inline object; it draws itself through us.
    fn DrawInlineObject(
        &self,
        ctx: *const c_void,
        x: f32,
        y: f32,
        object: Ref<IDWriteInlineObject>,
        sideways: BOOL,
        rtl: BOOL,
        _: Ref<IUnknown>,
    ) -> Result<()> {
        let object = object.ok()?;
        // SAFETY: as in DrawGlyphRun; the renderer reference outlives the outer Draw call.
        let renderer = unsafe { (*ctx.cast::<DrawCtx<'_>>()).renderer.clone() };
        // SAFETY: forwards our own context to a nested, synchronous Draw.
        unsafe {
            object.Draw(
                Some(ctx),
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

struct Entry {
    text: Box<str>,
    layout: IDWriteTextLayout,
    used: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FormatKey {
    size: u32,
    bold: bool,
    mono: bool,
}

pub struct Text {
    factory: IDWriteFactory2,
    renderer: IDWriteTextRenderer,
    formats: HashMap<FormatKey, IDWriteTextFormat>,
    layouts: HashMap<u64, Entry>,
    glyphs: Glyphs,
    sans: HSTRING,
    mono: HSTRING,
    frame: u64,
}

impl Text {
    pub fn new() -> Result<Self> {
        // SAFETY: creating the shared DirectWrite factory has no preconditions.
        let factory: IDWriteFactory2 = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let (sans, mono) = (pick(&factory, &SANS), pick(&factory, &MONO));
        debug(&format!("families: {sans}, {mono}"));
        let (sans, mono) = (HSTRING::from(sans), HSTRING::from(mono));
        Ok(Self {
            sans,
            glyphs: Glyphs {
                factory: factory.clone(),
                map: HashMap::new(),
                faces: HashMap::new(),
            },
            factory,
            renderer: Renderer.into(),
            formats: HashMap::new(),
            layouts: HashMap::new(),
            mono,
            frame: 0,
        })
    }

    fn format(&mut self, key: FormatKey) -> Result<IDWriteTextFormat> {
        if let Some(f) = self.formats.get(&key) {
            return Ok(f.clone());
        }
        let family = if key.mono {
            self.mono.clone()
        } else {
            self.sans.clone()
        };
        let weight = if key.bold {
            DWRITE_FONT_WEIGHT_SEMI_BOLD
        } else {
            DWRITE_FONT_WEIGHT_NORMAL
        };
        // SAFETY: valid strings and enums; the format is configured before any layout uses it.
        let f = unsafe {
            let f = self.factory.CreateTextFormat(
                &family,
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                f32::from_bits(key.size),
                w!("en-us"),
            )?;
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            let sign = self.factory.CreateEllipsisTrimmingSign(&f)?;
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            f.SetTrimming(&raw const trimming, &sign)?;
            f
        };
        self.formats.insert(key, f.clone());
        Ok(f)
    }

    /// Draws `s` in `r` (logical): vertically centered, aligned, cut with an ellipsis to `r.w`.
    pub fn draw(&mut self, canvas: &mut Canvas, s: &str, r: Rect, st: Style) {
        if s.is_empty() || r.w <= 0.0 {
            return;
        }
        let scale = canvas.scale;
        let key = FormatKey {
            size: (st.size * scale).to_bits(),
            bold: st.bold,
            mono: st.mono,
        };
        let (w, h) = ((r.w * scale).round(), (r.h * scale).round());
        let mut hasher = DefaultHasher::new();
        (s, key, w.to_bits(), h.to_bits(), st.align).hash(&mut hasher);
        let id = hasher.finish();
        let frame = self.frame;
        let layout = match self.layouts.get_mut(&id) {
            Some(e) if *e.text == *s => {
                e.used = frame;
                e.layout.clone()
            }
            _ => {
                let Ok(layout) = self.layout(s, key, w, h, st.align) else {
                    return;
                };
                self.layouts.insert(
                    id,
                    Entry {
                        text: s.into(),
                        layout: layout.clone(),
                        used: frame,
                    },
                );
                layout
            }
        };
        let renderer = self.renderer.clone();
        let mut ctx = DrawCtx {
            canvas,
            glyphs: &mut self.glyphs,
            renderer: &renderer,
            color: st.color,
            alpha: st.alpha,
        };
        let origin = ((r.x * scale).round(), (r.y * scale).round());
        // SAFETY: `ctx` outlives this synchronous call; the renderer only reads it as a DrawCtx.
        let r = unsafe {
            layout.Draw(
                Some((&raw mut ctx).cast_const().cast()),
                &renderer,
                origin.0,
                origin.1,
            )
        };
        if let Err(e) = r {
            debug(&format!("Draw failed: {e}"));
        }
    }

    fn layout(
        &mut self,
        s: &str,
        key: FormatKey,
        w: f32,
        h: f32,
        align: Align,
    ) -> Result<IDWriteTextLayout> {
        let format = self.format(key)?;
        let utf16: Vec<u16> = s.encode_utf16().collect();
        // SAFETY: valid UTF-16 and format; the layout copies the string.
        unsafe {
            let layout = self.factory.CreateTextLayout(&utf16, &format, w, h)?;
            layout.SetTextAlignment(match align {
                Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
            })?;
            Ok(layout)
        }
    }

    /// Drops layouts not drawn in the last two frames (rows that scrolled away). Call per frame.
    pub fn end_frame(&mut self) {
        let frame = self.frame;
        self.layouts.retain(|_, e| frame - e.used <= 1);
        self.frame += 1;
    }

    pub fn cached(&self) -> (usize, usize) {
        (self.layouts.len(), self.glyphs.map.len())
    }
}

/// UI text: Segoe UI (doc 05), then fallbacks where it's absent (C1's Wine; WinPE: ci/winpe.ps1).
const SANS: [&str; 4] = ["Segoe UI", "Tahoma", "DejaVu Sans", "Arial"];
/// Paths, IDs, commands: Cascadia Mono (doc 05), then what older systems and C1 have.
const MONO: [&str; 4] = [
    "Cascadia Mono",
    "Consolas",
    "DejaVu Sans Mono",
    "Courier New",
];

/// The first of `candidates` installed on this system (the last one if none is, or on error).
fn pick(factory: &IDWriteFactory2, candidates: &[&'static str]) -> &'static str {
    let last = candidates[candidates.len() - 1];
    let base: &IDWriteFactory = factory;
    let mut fonts = None;
    // SAFETY: out-pointer to a local; FALSE = don't check for updates.
    if unsafe { base.GetSystemFontCollection(&raw mut fonts, false) }.is_err() {
        return last;
    }
    let Some(fonts) = fonts else { return last };
    for &name in candidates {
        let (mut index, mut exists) = (0_u32, BOOL(0));
        // SAFETY: out-pointers to locals.
        if unsafe { fonts.FindFamilyName(&HSTRING::from(name), &raw mut index, &raw mut exists) }
            .is_ok()
            && exists.as_bool()
        {
            return name;
        }
    }
    debug(&format!("none of {candidates:?} is installed"));
    last
}

/// Diagnostics on stderr when `S01_DEBUG` is set (a spike aid; the first 20 messages only).
fn debug(msg: &str) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    if std::env::var_os("S01_DEBUG").is_some() && N.fetch_add(1, Ordering::Relaxed) < 20 {
        eprintln!("s01-a text: {msg}");
    }
}
