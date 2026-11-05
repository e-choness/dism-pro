//! The Win32 platform (TD-0009 §5.1): a [`Ui`] hosted in a `dpro_win::gui::Window`.
//!
//! `dpro-win` owns the window, its loop and the GDI presenter, and knows nothing of the toolkit;
//! [`Host`] adapts one to the other. Each event goes through the frame pipeline (§5.2): input is
//! dispatched and its messages update the [`Model`], the view is rebuilt and laid out, and the
//! damage is handed to the window. On `WM_PAINT` the damage is painted into the window's canvas,
//! which the window then presents. While an animation runs the window ticks the clock every frame;
//! once they all end it stops, so a settled window wakes for nothing (U6).
//!
//! [`DirectWrite`] is the [`TextSystem`] over `dpro_win`'s DirectWrite text.
//!
//! **Environment** (TD-0009 §5.2). The window reports Windows' settings at creation and on every
//! change. The host turns them into the theme (high contrast's system colors, else dark or light),
//! the motion (*Animation effects* off is reduced motion) and the [`Materials`]. A new theme
//! repaints the whole window once; any other change repaints nothing. The GDI presenter is the
//! only one so far, so the materials are always solid.

use std::time::Instant;

use dpro_win::gui::{
    self, Event, Frame, Handler, PointPx, SizePx, Window, WindowCx, WindowOptions, text,
};

use crate::{
    Canvas, Color, Cx, GlyphSink, Icon, Input, KeyCode, KeyEvent, Mods, Motion, Point, Rect,
    Scheme, Size, SysColors, TextMetrics, TextStyle, TextSystem, Theme, Ui,
    materials::{Environment, GlassPref, Materials, PresenterKind, SolidReason, materials},
};

/// Text measured and drawn with DirectWrite.
#[derive(Debug)]
pub struct DirectWrite(text::Text);

impl DirectWrite {
    /// # Errors
    /// DirectWrite or its system font collection is unavailable.
    pub fn new() -> Result<Self, dpro_win::Error> {
        text::Text::new().map(Self)
    }
}

impl TextSystem for DirectWrite {
    fn measure(&mut self, text: &str, style: &TextStyle, max_width: f32) -> TextMetrics {
        let m = self.0.measure(text, &text_style(*style), max_width);
        TextMetrics {
            width: m.width,
            height: m.height,
            lines: m.lines,
            trimmed: m.trimmed,
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
        self.0.draw(
            text,
            &text_style(*style),
            rect_f(rect),
            scale,
            &mut Masks(sink),
        );
    }

    fn icon(&mut self, icon: Icon, rect: Rect, scale: f32, sink: &mut dyn GlyphSink) {
        self.0
            .icon(icon.codepoint(), rect_f(rect), scale, &mut Masks(sink));
    }
}

/// The painter's sink, as the one `dpro_win`'s text draws into.
struct Masks<'a>(&'a mut dyn GlyphSink);

impl text::Sink for Masks<'_> {
    fn mask(&mut self, x: i32, y: i32, width: u32, coverage: &[u8]) {
        self.0.mask(x, y, width, coverage);
    }
}

fn text_style(s: TextStyle) -> text::Style {
    text::Style {
        size: s.size,
        weight: s.weight,
        mono: s.mono,
        wrap: s.wrap,
    }
}

fn rect_f(r: Rect) -> text::RectF {
    text::RectF {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
    }
}

/// A view-model and its view: what a [`Host`] shows.
pub trait Model {
    type Msg: Clone;

    /// Describes the page; called after every change.
    fn view(&self, cx: &mut Cx<'_, Self::Msg>);

    /// Handles a message from the view.
    fn update(&mut self, msg: Self::Msg);
}

/// A window's [`Handler`]: the [`Ui`] of a [`Model`], with its text system, theme and materials.
pub struct Host<P: Model> {
    ui: Ui<P::Msg>,
    model: P,
    text: Box<dyn TextSystem>,
    theme: Theme,
    glass: GlassPref,
    materials: Materials,
    msgs: Vec<P::Msg>,
    /// The animation clock's zero.
    start: Instant,
}

impl<P: Model> std::fmt::Debug for Host<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host")
            .field("viewport", &self.ui.viewport())
            .field("scale", &self.ui.scale())
            .field("scheme", &self.theme.scheme())
            .field("materials", &self.materials)
            .finish_non_exhaustive()
    }
}

impl<P: Model> Host<P> {
    /// The window sends Windows' settings, its DPI and its size before the first frame, so the
    /// [`Ui`] starts empty, light and solid. `glass` is the user's preference (or `--materials`).
    #[must_use]
    pub fn new(model: P, text: Box<dyn TextSystem>, glass: GlassPref) -> Self {
        Self {
            ui: Ui::new(Size { w: 0.0, h: 0.0 }, 1.0, Motion::STANDARD),
            model,
            text,
            theme: Theme::LIGHT,
            glass,
            materials: Materials::Solid(SolidReason::NoHardwareGpu),
            msgs: Vec::with_capacity(4),
            start: Instant::now(),
        }
    }

    /// The hosted model.
    #[must_use]
    pub fn model(&self) -> &P {
        &self.model
    }

    /// What the window shows, and why it's solid if it is (Settings › Appearance's line).
    #[must_use]
    pub fn materials(&self) -> Materials {
        self.materials
    }

    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Windows' settings changed (or arrived): the theme, motion and materials follow. Returns
    /// whether the theme changed, which repaints everything.
    fn environment(&mut self, env: &gui::env::Env) -> bool {
        self.materials = materials(&environment(env), self.glass);
        self.ui.set_motion(Motion::system(env.animations));
        let theme = theme_of(env);
        if theme == self.theme {
            return false;
        }
        self.theme = theme;
        self.ui.repaint_all();
        true
    }

    /// Dispatches `input` and updates the model with the messages it produced.
    fn input(&mut self, input: Input) -> bool {
        let used = self.ui.dispatch(input, &mut self.msgs);
        for msg in self.msgs.drain(..) {
            self.model.update(msg);
        }
        used
    }

    /// Rebuilds and lays out the view, and hands the damage to the window.
    fn frame(&mut self, cx: &mut WindowCx<'_>) {
        let model = &self.model;
        self.ui.build(|c| model.view(c));
        self.ui.layout(self.text.as_mut());
        for r in self.ui.damage() {
            cx.invalidate(gui::RectPx {
                x0: r.x0,
                y0: r.y0,
                x1: r.x1,
                y1: r.y1,
            });
        }
        cx.want_frames(self.ui.wants_frame());
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

impl<P: Model> Handler for Host<P> {
    fn event(&mut self, event: Event, cx: &mut WindowCx<'_>) -> bool {
        let scale = self.ui.scale();
        let at = |p: PointPx| Point {
            x: logical(p.x, scale),
            y: logical(p.y, scale),
        };
        let used = match event {
            Event::Dpi(dpi) => {
                self.ui.set_scale(scale_of(dpi));
                true
            }
            Event::Resize(SizePx { w, h }) => {
                self.ui.set_viewport(Size {
                    w: logical_size(w, scale),
                    h: logical_size(h, scale),
                });
                // The window's canvas is new and blank.
                self.ui.repaint_all();
                true
            }
            Event::PointerMove(p) => self.input(Input::PointerMove(at(p))),
            Event::PointerDown(p) => self.input(Input::PointerDown(at(p))),
            Event::PointerUp(p) => self.input(Input::PointerUp(at(p))),
            Event::PointerLeave => self.input(Input::PointerLeave),
            Event::Key { vk, down, mods } => self.input(Input::Key(KeyEvent {
                key: key_code(vk),
                down,
                mods: Mods {
                    shift: mods.shift,
                    ctrl: mods.ctrl,
                    alt: mods.alt,
                    win: mods.win,
                },
            })),
            Event::Env(env) => {
                if self.environment(&env) {
                    cx.set_dark_frame(self.theme.scheme() == Scheme::Dark);
                }
                true
            }
            Event::Focus(_) => false,
            Event::Frame => {
                let now = self.now_ms();
                self.ui.tick(now);
                true
            }
            Event::Close => {
                cx.close();
                return true;
            }
        };
        self.frame(cx);
        used
    }

    fn paint(&mut self, frame: &mut Frame<'_>) {
        if let Some(mut canvas) = Canvas::new(frame.pixels, frame.size.w, frame.size.h) {
            self.ui.paint(&mut canvas, self.text.as_mut(), &self.theme);
        }
        self.ui.clear_damage();
    }

    fn a11y_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        Some(self.ui.a11y_tree())
    }

    fn a11y_update(&mut self) -> Option<accesskit::TreeUpdate> {
        self.ui.a11y_update()
    }

    /// Runs like a click (or a focus move), then the frame shows it and the window serves it.
    fn a11y_action(&mut self, request: &accesskit::ActionRequest, cx: &mut WindowCx<'_>) {
        if self.ui.a11y_action(request, &mut self.msgs) {
            for msg in self.msgs.drain(..) {
                self.model.update(msg);
            }
            self.frame(cx);
        }
    }
}

/// Opens a window on `host` and runs it until it closes.
///
/// # Errors
/// The window couldn't be created, or its message loop failed.
pub fn run<P: Model + 'static>(
    options: &WindowOptions<'_>,
    host: Host<P>,
) -> Result<(), dpro_win::Error> {
    let window = Window::new(options, Box::new(host))?;
    window.show();
    window.run()
}

/// The materials' view of Windows' settings. The GDI presenter is the only one yet: it never
/// composes, whatever the GPU, so `hardware_gpu` stays false until the composition presenter
/// probes the adapter.
const fn environment(env: &gui::env::Env) -> Environment {
    Environment {
        winpe: env.winpe,
        build: env.build,
        presenter: PresenterKind::Gdi,
        hardware_gpu: false,
        device_lost: false,
        transparency: env.transparency,
        high_contrast: env.high_contrast,
        remote: env.remote,
        battery_saver: env.battery_saver,
    }
}

/// High contrast's system colors, else the app mode's scheme (TD-0009 §4.4).
const fn theme_of(env: &gui::env::Env) -> Theme {
    if env.high_contrast {
        let c = env.colors;
        Theme::high_contrast(SysColors {
            window: Color::hex(c.window),
            window_text: Color::hex(c.window_text),
            highlight: Color::hex(c.highlight),
            highlight_text: Color::hex(c.highlight_text),
            hot_light: Color::hex(c.hot_light),
            gray_text: Color::hex(c.gray_text),
            btn_face: Color::hex(c.btn_face),
            btn_text: Color::hex(c.btn_text),
        })
    } else if env.dark {
        Theme::DARK
    } else {
        Theme::LIGHT
    }
}

/// Physical pixels per logical pixel at `dpi`.
fn scale_of(dpi: u32) -> f32 {
    f32::from(u16::try_from(dpi).unwrap_or(u16::MAX)) / 96.0
}

/// A physical coordinate in logical pixels.
#[allow(
    clippy::cast_precision_loss,
    reason = "window coordinates are far below f32's 2^24 exact integers"
)]
fn logical(v: i32, scale: f32) -> f32 {
    v as f32 / scale
}

/// A physical size in logical pixels.
#[allow(
    clippy::cast_precision_loss,
    reason = "window sizes are far below f32's 2^24 exact integers"
)]
fn logical_size(v: u32, scale: f32) -> f32 {
    v as f32 / scale
}

/// The toolkit's key for a virtual-key code (`WinUser.h`'s `VK_*` values).
fn key_code(vk: u16) -> KeyCode {
    match vk {
        0x09 => KeyCode::Tab,
        0x0D => KeyCode::Enter,
        0x1B => KeyCode::Escape,
        0x20 => KeyCode::Space,
        0x21 => KeyCode::PageUp,
        0x22 => KeyCode::PageDown,
        0x23 => KeyCode::End,
        0x24 => KeyCode::Home,
        0x25 => KeyCode::Left,
        0x26 => KeyCode::Up,
        0x27 => KeyCode::Right,
        0x28 => KeyCode::Down,
        other => KeyCode::Other(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Element, FakeText, Key};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Pressed,
    }

    /// One button that counts its presses.
    #[derive(Default)]
    struct Counter {
        presses: u32,
    }

    impl Model for Counter {
        type Msg = Msg;

        fn view(&self, cx: &mut Cx<'_, Msg>) {
            cx.add(Element::button(Key::name("b"), "b", Msg::Pressed));
        }

        fn update(&mut self, msg: Msg) {
            match msg {
                Msg::Pressed => self.presses += 1,
            }
        }
    }

    /// T2: a button's label set and inked by DirectWrite (Wine's fonts), inside the button.
    #[test]
    fn directwrite_inks_a_label_inside_its_button() {
        let text = DirectWrite::new().expect("DirectWrite");
        let mut host = Host::new(Counter::default(), Box::new(text), GlassPref::Auto);
        host.ui.set_scale(1.5);
        host.ui.set_viewport(Size { w: 200.0, h: 100.0 });
        let model = &host.model;
        host.ui.build(|c| model.view(c));
        host.ui.layout(host.text.as_mut());
        host.ui.repaint_all();
        let button = host
            .ui
            .find(&[Key::name("b")])
            .and_then(|id| host.ui.info(id))
            .map(|n| n.rect)
            .expect("the button");
        let (w, h) = (300_u32, 150_u32);
        let mut pixels = vec![0_u32; (w * h) as usize];
        let mut canvas = Canvas::new(&mut pixels, w, h).expect("a canvas");
        host.ui.paint(&mut canvas, host.text.as_mut(), &host.theme);
        // Dark pixels are the label's ink: the light theme's button and its border are light.
        let dark: Vec<(u32, u32)> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let p = pixels[(y * w + x) as usize];
                [p >> 16, p >> 8, p].iter().all(|c| c & 0xFF < 0x80)
            })
            .collect();
        assert!(dark.len() >= 4, "no ink: {} dark pixels", dark.len());
        for &(x, y) in &dark {
            let p = Point {
                x: logical(i32::try_from(x).unwrap_or(0), 1.5),
                y: logical(i32::try_from(y).unwrap_or(0), 1.5),
            };
            assert!(button.contains(p), "ink at {p:?} outside {button:?}");
        }
    }

    #[test]
    fn keys_map_to_the_toolkit_s() {
        assert_eq!(key_code(0x09), KeyCode::Tab);
        assert_eq!(key_code(0x22), KeyCode::PageDown);
        assert_eq!(key_code(0x41), KeyCode::Other(0x41));
    }

    #[test]
    fn coordinates_scale_to_logical_pixels() {
        assert!((scale_of(144) - 1.5).abs() < f32::EPSILON);
        assert!((logical(150, 1.5) - 100.0).abs() < 1e-4);
        assert!((logical_size(1500, 1.5) - 1000.0).abs() < 1e-3);
    }

    #[test]
    fn a_press_at_a_physical_point_reaches_the_model() {
        let mut host = Host::new(Counter::default(), Box::new(FakeText), GlassPref::Auto);
        host.ui.set_scale(scale_of(144));
        host.ui.set_viewport(Size { w: 200.0, h: 100.0 });
        let model = &host.model;
        host.ui.build(|c| model.view(c));
        host.ui.layout(host.text.as_mut());
        let button = host
            .ui
            .find(&[Key::name("b")])
            .and_then(|id| host.ui.info(id))
            .map(|n| n.rect)
            .expect("the button");
        // The window reports physical pixels: (6, 6) at 150 % is (4, 4), inside the button at the
        // top left.
        let p = PointPx { x: 6, y: 6 };
        let point = Point {
            x: logical(p.x, host.ui.scale()),
            y: logical(p.y, host.ui.scale()),
        };
        assert!(button.contains(point), "{button:?} doesn't hold {point:?}");
        assert!(host.input(Input::PointerDown(point)));
        host.input(Input::PointerUp(point));
        assert_eq!(host.model().presses, 1);
    }

    /// TD-0009 §5.2: the theme, motion and materials follow Windows' settings, and only a new
    /// theme repaints.
    #[test]
    fn windows_settings_set_the_theme_motion_and_materials() {
        let mut host = Host::new(Counter::default(), Box::new(FakeText), GlassPref::Auto);
        host.ui.set_viewport(Size { w: 200.0, h: 100.0 });
        host.ui.clear_damage();
        let light = gui::env::Env {
            build: 26_100,
            transparency: true,
            animations: true,
            colors: gui::env::SysColors {
                window: 0xFF_FFFF,
                window_text: 0x00_0000,
                highlight: 0x00_78D7,
                highlight_text: 0xFF_FFFF,
                hot_light: 0x00_66CC,
                gray_text: 0x6D_6D6D,
                btn_face: 0xF0_F0F0,
                btn_text: 0x00_0000,
            },
            ..gui::env::Env::default()
        };
        // Light is where the host starts: nothing to repaint.
        assert!(!host.environment(&light));
        assert_eq!(host.ui.damage().len(), 0);
        assert_eq!(host.ui.motion(), Motion::STANDARD);
        // GDI never composes.
        assert_eq!(
            host.materials(),
            Materials::Solid(SolidReason::NoHardwareGpu)
        );
        // Dark: a new theme, all of it damaged.
        let dark = gui::env::Env {
            dark: true,
            ..light
        };
        assert!(host.environment(&dark));
        assert_eq!(*host.theme(), Theme::DARK);
        assert_ne!(host.ui.damage().len(), 0);
        host.ui.clear_damage();
        // Reduced motion and transparency off change no pixel.
        let calm = gui::env::Env {
            animations: false,
            transparency: false,
            ..dark
        };
        assert!(!host.environment(&calm));
        assert_eq!(host.ui.motion(), Motion::REDUCED);
        assert_eq!(host.ui.damage().len(), 0);
        // High contrast: the system's colors, whatever the app mode.
        let hc = gui::env::Env {
            high_contrast: true,
            ..calm
        };
        assert!(host.environment(&hc));
        assert_eq!(host.theme().scheme(), Scheme::HighContrast);
        assert_eq!(
            host.theme().color(crate::Tone::Accent),
            Color::hex(0x00_78D7)
        );
        // WinPE says so, whatever else holds.
        let pe = gui::env::Env { winpe: true, ..hc };
        host.environment(&pe);
        assert_eq!(host.materials(), Materials::Solid(SolidReason::WinPe));
    }
}
