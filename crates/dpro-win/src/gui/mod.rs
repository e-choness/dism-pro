//! The GUI's Win32 platform (TD-0009 §5.1; feature `gui`): a top-level window and its message loop,
//! the GDI presenter, the animation timer, and the probes G1 measures with.
//!
//! The boundary has no `dpro-ui` types, because layers point down; `dpro-ui`'s `platform` module
//! adapts it. A [`Handler`] gets [`Event`]s and paints [`Frame`]s, and the window presents them.
//!
//! - **Presenting (GDI).** The canvas is a top-down 32-bit DIB section of premultiplied BGRA, so the
//!   toolkit paints straight into the memory GDI copies from: no conversion and no staging copy.
//!   The handler names what it damaged with [`WindowCx::invalidate`]. On `WM_PAINT` it paints that
//!   into the canvas ([`Handler::paint`]), and the window copies each rectangle of the update region
//!   to the screen. A region the system invalidated (an uncovered window) is copied from the
//!   canvas, which still holds it, without asking the handler.
//! - **Frames.** While the handler wants frames ([`WindowCx::want_frames`], animations running), a
//!   16 ms timer sends [`Event::Frame`]. The timer is killed when they stop, so a settled window
//!   has no timer and its thread sleeps in `GetMessageW` (U6).
//! - **DPI.** The process is per-monitor DPI aware (v2) through its manifest. Positions and sizes
//!   are physical pixels, and [`Event::Dpi`] says when the scale changes.

pub mod probe;
mod window;

pub use window::{Window, WindowCx};

/// A size in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SizePx {
    pub w: u32,
    pub h: u32,
}

/// A point in physical pixels, relative to the client area's top left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointPx {
    pub x: i32,
    pub y: i32,
}

/// A rectangle in physical pixels, half-open: `[x0, x1) × [y0, y1)`, like `RECT`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RectPx {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

/// Modifier keys held during a key event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per modifier key, as Windows reports them"
)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub win: bool,
}

/// What the window tells its [`Handler`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    PointerMove(PointPx),
    /// The primary button went down; the window captures the pointer until it goes up.
    PointerDown(PointPx),
    PointerUp(PointPx),
    /// The pointer left the client area.
    PointerLeave,
    /// A key went down (auto-repeat included) or up, by virtual-key code.
    Key {
        vk: u16,
        down: bool,
        mods: Mods,
    },
    /// The client area's size. Sent before the first paint, and the canvas is new and blank each
    /// time, so the handler damages all of it.
    Resize(SizePx),
    /// The window's DPI (96 is 100 %). Sent before the first [`Event::Resize`] and whenever the
    /// window moves to a monitor with another scale; the new size follows.
    Dpi(u32),
    /// Keyboard focus came to the window, or left it.
    Focus(bool),
    /// The frame timer fired: advance the animations.
    Frame,
    /// The user asked to close the window. It closes when the handler calls [`WindowCx::close`].
    Close,
}

/// The canvas for one paint: the window's pixels, premultiplied BGRA (`0xAARRGGBB`), row after row
/// with no padding. It keeps its pixels between paints, so only damage needs painting.
pub struct Frame<'a> {
    pub pixels: &'a mut [u32],
    pub size: SizePx,
}

impl std::fmt::Debug for Frame<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

/// The application side of a [`Window`]. It must not pump messages (no modal loop) while it
/// handles an event: an event that arrives meanwhile is dropped.
pub trait Handler {
    /// Handles one event, and says whether it used it: a key it didn't use goes on to the system
    /// (Alt+F4, Alt+Space, F10). Damage goes to [`WindowCx::invalidate`] and is painted in
    /// [`Handler::paint`].
    fn event(&mut self, event: Event, cx: &mut WindowCx<'_>) -> bool;

    /// Paints what the handler damaged into the canvas; the window presents it after.
    fn paint(&mut self, frame: &mut Frame<'_>);
}

/// How a [`Window`] starts.
#[derive(Clone, Copy, Debug)]
pub struct WindowOptions<'a> {
    pub title: &'a str,
    /// The client area, in pixels at 100 % scale; the window scales it to its monitor's DPI.
    pub size: (u32, u32),
    /// The smallest client area the user can size it to, at 100 % scale.
    pub min_size: (u32, u32),
}
