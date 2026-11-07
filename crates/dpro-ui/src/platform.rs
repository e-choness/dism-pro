//! The Win32 platform (TD-0009 §5.1): a [`Ui`] hosted in a `dpro_win::gui::Window`.
//!
//! `dpro-win` owns the window, its loop and its presenters, and knows nothing of the toolkit;
//! [`Host`] adapts one to the other. Each event goes through the frame pipeline (§5.2): input is
//! dispatched and its messages update the [`Model`], the view is rebuilt and laid out, and the
//! damage is handed to the window. On `WM_PAINT` the damage is painted into the window's canvas,
//! which the window then presents. While an animation runs the window ticks the clock every frame;
//! once they all end it stops, so a settled window wakes for nothing (U6).
//!
//! [`DirectWrite`] is the [`TextSystem`] over `dpro_win`'s DirectWrite text.
//!
//! **Presenter** (TD-0009 §5.2). [`run`] decides what the window may compose with:
//! nothing in `WinPE` or before Windows 11 22H2 (no system backdrop), any device for
//! `--materials glass`, else a hardware one. The window reports which it got, and again if a lost
//! device changes it.
//!
//! **Environment.** The window reports Windows' settings at creation and on every change. The
//! host turns them, and the presenter, into the theme (high contrast's system colors, else dark or
//! light, as glass or solid), the motion (*Animation effects* off is reduced motion), the
//! [`Materials`] and the window's backdrop (Mica under glass). A new theme repaints the whole
//! window once; any other change repaints nothing.
//!
//! **Text input.** Typed characters go to the focused field. A copy, cut or paste a field asks
//! for is done with the clipboard's text right after the input. The caret blinks on the window's
//! one-shot wake, set again after every event for when it's next due, and cancelled once it rests
//! (U6). It follows Windows' blink rate and width, and shows only while the window has keyboard
//! focus. Over a field the pointer is an I-beam.
//!
//! **Popups** (TD-0009 A1, `popups.rs`). The model's menu or dropdown list, and the toolkit's
//! tooltip, each show in a popup window of the main one, with a [`Ui`] of its own. Every
//! surface's events reach this one handler, marked with the surface they're for.

mod popups;

use std::time::Instant;

use dpro_win::gui::{
    self, Backdrop, Compose, Event, Frame, Handler, PointPx, Presenter, SizePx, Surface, Window,
    WindowCx, WindowOptions, text,
};

use crate::{
    Bytes, Canvas, Caret, CaretStop, ClipboardRequest, Color, Cursor, Cx, Formats, GlyphSink, Icon,
    Input, Key, KeyCode, KeyEvent, MAX_FIELD_LEN, Mods, Motion, NodeId, Point, Popup, Rect, Scheme,
    Size, SizeUnits, SysColors, TextMetrics, TextStyle, TextSystem, Theme, Ui, UnixMillis,
    format_size,
    materials::{
        Environment, GLASS_BUILD, GlassPref, Materials, PresenterKind, SolidReason, materials,
    },
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

    fn caret_stops(&mut self, text: &str, style: &TextStyle, out: &mut Vec<CaretStop>) {
        out.clear();
        self.0.caret_stops(text, &text_style(*style), |index, x| {
            out.push(CaretStop { index, x });
        });
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

    /// The menu or dropdown list the model has open (TD-0009 A1.5), if any. After every event
    /// the host opens, moves or hides its window to match.
    fn popup(&self) -> Option<Popup<Self::Msg>> {
        None
    }

    /// What the open [`popup`](Self::popup) shows: one [`Element::menu`](crate::Element::menu)
    /// or [`Element::options`](crate::Element::options), with its items.
    fn popup_view(&self, _cx: &mut Cx<'_, Self::Msg>) {}

    /// The message that opens the context menu of the node keyed `key`, which a context request
    /// (Shift+F10, the Menu key, a right-click) found: it's marked
    /// [`context`](crate::Element::context), focused, and picked if it's selectable. `None` if
    /// it has no menu.
    fn context_menu(&self, _key: Key) -> Option<Self::Msg> {
        None
    }

    /// The size units' labels, from the app's catalog (TD-0009 A2.5): a table's sizes read
    /// with them. By default, the number alone.
    fn size_units(&self) -> SizeUnits {
        SizeUnits::default()
    }
}

/// The user's formats (TD-0009 A2.5): Windows' days, moments and decimal separator, with the
/// app's unit labels.
struct UserFormats {
    locale: gui::locale::Locale,
    units: SizeUnits,
}

impl Formats for UserFormats {
    fn size(&self, bytes: Bytes, out: &mut String) {
        format_size(bytes, self.locale.decimal(), &self.units, out);
    }

    fn date(&self, day: UnixMillis, out: &mut String) {
        self.locale.date(day, out);
    }

    fn time(&self, t: UnixMillis, out: &mut String) {
        self.locale.time(t, out);
    }
}

/// A window's [`Handler`]: the [`Ui`] of a [`Model`], with its text system, theme and materials.
pub struct Host<P: Model> {
    ui: Ui<P::Msg>,
    model: P,
    text: Box<dyn TextSystem>,
    theme: Theme,
    glass: GlassPref,
    materials: Materials,
    /// Windows' settings, once the window has sent them.
    env: Option<gui::env::Env>,
    presenter: Presenter,
    msgs: Vec<P::Msg>,
    /// The animation clock's zero.
    start: Instant,
    /// When the window's wake is set for, on that clock: the caret's next blink, or a tooltip's
    /// next change.
    wake_due: Option<u64>,
    /// The popup windows' UIs, in [`popups::KINDS`]' order: a menu, a dropdown's list and the
    /// tooltip.
    layers: [popups::Layer<P::Msg>; 3],
    /// Which of the menu and the list shows the model's popup.
    open: Option<gui::PopupKind>,
    /// Whose tip the tooltip window shows, and its text.
    tip_for: Option<(Surface, NodeId)>,
    tip_text: String,
}

/// What new settings or a new presenter changed in how the window looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Restyle {
    /// A new theme: everything repaints, and the frame follows dark or light.
    theme: bool,
    /// Glass came (`true`) or went: the backdrop follows.
    glass: Option<bool>,
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
    /// The window sends its presenter, Windows' settings, its DPI and its size before the first
    /// frame, so the [`Ui`] starts empty, light and solid. `glass` is the user's preference (or
    /// `--materials`).
    #[must_use]
    pub fn new(model: P, text: Box<dyn TextSystem>, glass: GlassPref) -> Self {
        Self {
            ui: Ui::new(Size { w: 0.0, h: 0.0 }, 1.0, Motion::STANDARD),
            model,
            text,
            theme: Theme::LIGHT,
            glass,
            materials: Materials::Solid(SolidReason::NoHardwareGpu),
            env: None,
            presenter: Presenter::default(),
            msgs: Vec::with_capacity(4),
            start: Instant::now(),
            wake_due: None,
            layers: std::array::from_fn(|_| popups::Layer::new()),
            open: None,
            tip_for: None,
            tip_text: String::new(),
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

    /// Windows' settings changed (or arrived): the motion, the caret and the text size follow,
    /// and the look.
    fn set_env(&mut self, env: gui::env::Env) -> Restyle {
        if self.env.map(|e| e.locale) != Some(env.locale) {
            // The user's formats, read now; every value cell is formatted again.
            self.ui.set_formats(UserFormats {
                locale: gui::locale::Locale::user(),
                units: self.model.size_units(),
            });
        }
        self.env = Some(env);
        self.ui.set_motion(Motion::system(env.animations));
        self.ui.set_caret(Caret {
            width: env.caret_width.max(1),
            blink_ms: env.caret_blink_ms,
        });
        // Percent to a factor; `Env`'s default, 0, is clamped to 1.
        let text_scale = f32::from(env.text_scale) / 100.0;
        self.ui.set_text_scale(text_scale);
        self.ui.set_tooltip_delay(env.tooltip_ms);
        // High contrast shows the full scroll bar always (TD-0009 A2.10 item 3).
        let auto_hide = env.auto_hide_scroll_bars && !env.high_contrast;
        self.ui.set_wheel(env.wheel_lines, env.wheel_chars);
        self.ui.set_drag_distance(env.drag_px);
        self.ui.set_auto_hide_scroll_bars(auto_hide);
        for layer in &mut self.layers {
            layer.ui.set_motion(Motion::system(env.animations));
            layer.ui.set_text_scale(text_scale);
            layer.ui.set_tooltip_delay(env.tooltip_ms);
            layer.ui.set_wheel(env.wheel_lines, env.wheel_chars);
            layer.ui.set_auto_hide_scroll_bars(auto_hide);
        }
        self.restyle()
    }

    /// The window's presenter changed (or was chosen): the look follows.
    fn set_presenter(&mut self, presenter: Presenter) -> Restyle {
        self.presenter = presenter;
        self.restyle()
    }

    /// The materials and the theme for the settings and presenter as they are. A new theme
    /// repaints everything.
    fn restyle(&mut self) -> Restyle {
        let Some(env) = self.env else {
            return Restyle::default();
        };
        let was_glass = matches!(self.materials, Materials::Glass { .. });
        self.materials = materials(&environment(&env, self.presenter), self.glass);
        let glass = matches!(self.materials, Materials::Glass { .. });
        let theme = if glass {
            theme_of(&env).glass()
        } else {
            theme_of(&env)
        };
        let new_theme = theme != self.theme;
        if new_theme {
            self.theme = theme;
            self.ui.repaint_all();
            for layer in &mut self.layers {
                layer.ui.repaint_all();
            }
        }
        Restyle {
            theme: new_theme,
            glass: (glass != was_glass).then_some(glass),
        }
    }

    /// Tells the window what changed: the frame's dark or light, and the backdrop.
    fn apply(&self, restyle: Restyle, cx: &mut WindowCx<'_>) {
        if restyle.theme {
            cx.set_dark_frame(self.theme.scheme() == Scheme::Dark);
        }
        if let Some(glass) = restyle.glass {
            cx.set_backdrop(if glass {
                Backdrop::Mica
            } else {
                Backdrop::None
            });
        }
    }

    /// Dispatches `input` and updates the model with the messages it produced.
    fn input(&mut self, input: Input) -> bool {
        let used = self.ui.dispatch(input, &mut self.msgs);
        for msg in self.msgs.drain(..) {
            self.model.update(msg);
        }
        used
    }

    /// Carries out the copy, cut or paste the last input asked for, with the clipboard's text, and
    /// updates the model with the edit.
    fn clipboard(&mut self, cx: &mut WindowCx<'_>) {
        let Some(request) = self.ui.take_clipboard() else {
            return;
        };
        match request {
            ClipboardRequest::Copy => {
                cx.set_clipboard_text(self.ui.selected_text());
            }
            ClipboardRequest::Cut => {
                if cx.set_clipboard_text(self.ui.selected_text()) {
                    self.ui.cut_selection(&mut self.msgs);
                }
            }
            ClipboardRequest::Paste => {
                // A field holds at most MAX_FIELD_LEN bytes, and every code unit is at least one.
                if let Some(text) = cx.clipboard_text(MAX_FIELD_LEN) {
                    self.ui.paste(&text, &mut self.msgs);
                }
            }
        }
        for msg in self.msgs.drain(..) {
            self.model.update(msg);
        }
    }

    /// Rebuilds and lays out the view and the popups, and hands each surface's damage to its
    /// window.
    fn frame(&mut self, cx: &mut WindowCx<'_>) {
        // A context request asks the model for that node's menu.
        if let Some(key) = self
            .ui
            .take_context()
            .and_then(|id| self.ui.info(id))
            .map(|n| n.key)
            && let Some(msg) = self.model.context_menu(key)
        {
            self.model.update(msg);
        }
        self.build();
        // A popup whose anchor went away closes (A1.8).
        if self.ui.popup_anchor().is_none()
            && let Some(popup) = self.model.popup()
        {
            self.model.update(popup.on_close);
            self.build();
        }
        self.sync_popup(cx);
        // Before the damage goes out: a blink damages the caret.
        let now = self.now_ms();
        let next = [
            self.ui.caret_timer(now),
            self.ui.tooltip_timer(now),
            self.layers[0].ui.tooltip_timer(now),
            self.layers[1].ui.tooltip_timer(now),
        ]
        .into_iter()
        .flatten()
        .min();
        self.sync_tip(cx);
        for r in self.ui.damage() {
            cx.invalidate(popups::gui_px(*r));
        }
        self.popup_damage(cx);
        let frames =
            self.ui.wants_frame() || self.layers.iter().any(|l| l.shown && l.ui.wants_frame());
        cx.want_frames(frames);
        let due = next.map(|ms| now.saturating_add(ms));
        if due != self.wake_due {
            self.wake_due = due;
            cx.wake_after(next.map(|ms| u32::try_from(ms).unwrap_or(u32::MAX)));
        }
    }

    /// Rebuilds and lays out the main view, and once more if a table asks: a new table guesses
    /// its rows' height and view until a layout has measured them, and a scroll can bring other
    /// rows into view than it built (TD-0009 A2.5).
    fn build(&mut self) {
        // Again if a table asks (its first build guesses its rows), and once more after a
        // column's fit, which layout measures and the model takes (TD-0009 A2.4).
        for _ in 0..3 {
            let model = &self.model;
            self.ui.build(|c| model.view(c));
            self.ui.layout(self.text.as_mut());
            self.ui.take_messages(&mut self.msgs);
            let fitted = !self.msgs.is_empty();
            for msg in self.msgs.drain(..) {
                self.model.update(msg);
            }
            if !fitted && !self.ui.wants_build() {
                break;
            }
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

impl<P: Model> Host<P> {
    /// An event for the main window. Returns whether it was used, or `None` once the window is
    /// closing (no frame follows).
    fn main_event(&mut self, event: Event, cx: &mut WindowCx<'_>) -> Option<bool> {
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
            Event::PointerDown {
                at: p,
                mods,
                double,
            } => self.input(Input::Press {
                at: at(p),
                mods: mods_of(mods),
                double,
            }),
            Event::Char(c) => self.input(Input::Char(c)),
            Event::PointerUp(p) => self.input(Input::PointerUp(at(p))),
            Event::PointerLeave => self.input(Input::PointerLeave),
            Event::Key { vk, down, mods } => self.input(Input::Key(KeyEvent {
                key: key_code(vk),
                down,
                mods: mods_of(mods),
            })),
            Event::ContextMenu(p) => self.input(Input::ContextMenu(p.map(at))),
            // A wheel over the window closes its menu or list, whose anchor it may scroll away
            // (TD-0009 A2.4), and scrolls what's under the pointer.
            Event::Wheel {
                delta,
                horizontal,
                at: p,
            } => {
                let open = self.model.popup().is_some();
                self.close_popup();
                let used = self.input(Input::Wheel {
                    at: at(p),
                    delta,
                    horizontal,
                });
                used || open
            }
            // Only a popup is dismissed.
            Event::Dismiss => false,
            Event::Presenter(presenter) => {
                let restyle = self.set_presenter(presenter);
                self.apply(restyle, cx);
                true
            }
            Event::Env(env) => {
                let restyle = self.set_env(env);
                self.apply(restyle, cx);
                true
            }
            Event::Focus(active) => {
                self.ui.set_active(active);
                false
            }
            Event::Wake => {
                // One shot: it's gone, and the frame sets the next.
                self.wake_due = None;
                true
            }
            Event::Frame => {
                let now = self.now_ms();
                // A hold that comes to its end sends its message here; a popup's hover eases.
                self.ui.tick(now, &mut self.msgs);
                for layer in self.layers.iter_mut().filter(|l| l.shown) {
                    layer.ui.tick(now, &mut self.msgs);
                }
                for msg in self.msgs.drain(..) {
                    self.model.update(msg);
                }
                true
            }
            Event::Close => {
                cx.close();
                return None;
            }
        };
        Some(used)
    }
}

impl<P: Model> Handler for Host<P> {
    fn event(&mut self, event: Event, cx: &mut WindowCx<'_>) -> bool {
        let used = match cx.surface() {
            Surface::Main => match self.main_event(event, cx) {
                Some(used) => used,
                None => return true,
            },
            Surface::Popup(kind) => self.popup_event(kind, event),
        };
        self.clipboard(cx);
        self.frame(cx);
        used
    }

    fn paint(&mut self, frame: &mut Frame<'_>) {
        let (ui, theme) = match frame.surface {
            Surface::Main => (&mut self.ui, self.theme),
            Surface::Popup(kind) => {
                let theme = self.popup_theme(kind);
                (&mut self.layers[popups::slot(kind)].ui, theme)
            }
        };
        if let Some(mut canvas) = Canvas::new(frame.pixels, frame.size.w, frame.size.h) {
            ui.paint(&mut canvas, self.text.as_mut(), &theme);
        }
        ui.clear_damage();
    }

    fn cursor(&self) -> gui::Cursor {
        match self.ui.cursor() {
            Cursor::Text => gui::Cursor::IBeam,
            Cursor::Arrow => gui::Cursor::Arrow,
            Cursor::SizeWE => gui::Cursor::SizeWE,
        }
    }

    fn a11y_tree(&mut self, surface: Surface) -> Option<accesskit::TreeUpdate> {
        Some(self.ui_of(surface).a11y_tree())
    }

    fn a11y_update(&mut self, surface: Surface) -> Option<accesskit::TreeUpdate> {
        self.ui_of(surface).a11y_update()
    }

    /// Runs like a click (or a focus move), then the frame shows it and the window serves it.
    /// Collapse closes the open popup (the dropdown's); an item's message in a popup closes it
    /// first, as a click does.
    fn a11y_action(&mut self, request: &accesskit::ActionRequest, cx: &mut WindowCx<'_>) {
        let surface = cx.surface();
        if surface == Surface::Main && request.action == accesskit::Action::Collapse {
            self.close_popup();
            self.frame(cx);
            return;
        }
        let ui = match surface {
            Surface::Main => &mut self.ui,
            Surface::Popup(kind) => &mut self.layers[popups::slot(kind)].ui,
        };
        if ui.a11y_action(request, &mut self.msgs) {
            if surface == Surface::Main {
                for msg in self.msgs.drain(..) {
                    self.model.update(msg);
                }
            } else {
                self.item_messages();
            }
            self.frame(cx);
        }
    }
}

impl<P: Model> Host<P> {
    /// The UI that `surface` shows.
    fn ui_of(&mut self, surface: Surface) -> &mut Ui<P::Msg> {
        match surface {
            Surface::Main => &mut self.ui,
            Surface::Popup(kind) => &mut self.layers[popups::slot(kind)].ui,
        }
    }
}

/// Opens a window on `host` and runs it until it closes. The window composes as Windows and the
/// host's glass preference allow ([`compose_for`]), whatever `options.compose` says.
///
/// # Errors
/// The window couldn't be created, or its message loop failed.
pub fn run<P: Model + 'static>(
    options: &WindowOptions<'_>,
    host: Host<P>,
) -> Result<(), dpro_win::Error> {
    let options = WindowOptions {
        compose: compose_for(&gui::env::Env::read(), host.glass),
        ..*options
    };
    let window = Window::new(&options, Box::new(host))?;
    window.show();
    window.run()
}

/// What a window may compose with (TD-0009 §5.2): nothing where there's no system backdrop
/// (`WinPE`, Windows before 11 22H2), any device when glass is forced (`--materials glass`, which
/// G1 runs on WARP), else a hardware one. Every other reason for solid materials can change while
/// the window is open, so the window composes and the materials decide.
#[must_use]
pub const fn compose_for(env: &gui::env::Env, pref: GlassPref) -> Compose {
    if env.winpe || env.build < GLASS_BUILD {
        Compose::Never
    } else if matches!(pref, GlassPref::Force) {
        Compose::Any
    } else {
        Compose::Hardware
    }
}

/// The materials' view of Windows' settings and the window's presenter.
const fn environment(env: &gui::env::Env, presenter: Presenter) -> Environment {
    Environment {
        winpe: env.winpe,
        build: env.build,
        presenter: if presenter.composed {
            PresenterKind::Composition
        } else {
            PresenterKind::Gdi
        },
        // A device lost for WARP still means the machine has a GPU: the reason is the loss.
        hardware_gpu: presenter.hardware || presenter.device_lost,
        device_lost: presenter.device_lost,
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
        0x08 => KeyCode::Backspace,
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
        0x2D => KeyCode::Insert,
        0x2E => KeyCode::Delete,
        0x73 => KeyCode::F4,
        other => KeyCode::Other(other),
    }
}

/// The toolkit's modifiers for the window's.
const fn mods_of(m: gui::Mods) -> Mods {
    Mods {
        shift: m.shift,
        ctrl: m.ctrl,
        alt: m.alt,
        win: m.win,
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

    #[derive(Clone, Debug, PartialEq)]
    enum Pick {
        OpenMenu,
        OpenList,
        Close,
    }

    /// A button with a tip that opens a menu, whose one item opens a list.
    #[derive(Default)]
    struct Picker {
        open: Option<crate::PopupKind>,
        closes: u32,
    }

    impl Model for Picker {
        type Msg = Pick;

        fn view(&self, cx: &mut Cx<'_, Pick>) {
            cx.add(Element::button(Key::name("b"), "b", Pick::OpenMenu).tooltip("tip"));
        }

        fn update(&mut self, msg: Pick) {
            match msg {
                Pick::OpenMenu => self.open = Some(crate::PopupKind::Menu),
                Pick::OpenList => self.open = Some(crate::PopupKind::List),
                Pick::Close => {
                    self.open = None;
                    self.closes += 1;
                }
            }
        }

        fn popup(&self) -> Option<Popup<Pick>> {
            self.open.map(|kind| Popup {
                kind,
                on_close: Pick::Close,
            })
        }

        fn popup_view(&self, cx: &mut Cx<'_, Pick>) {
            cx.add_with(Element::menu(Key::name("m")), |cx| {
                cx.add(Element::menu_item(
                    Key::name("list"),
                    "list",
                    Pick::OpenList,
                ));
            });
        }
    }

    /// An item's message runs after its menu closes, as a Windows menu's command does: one that
    /// opens another popup keeps it open (A1.4).
    #[test]
    fn an_item_s_message_comes_after_its_menu_closes() {
        let mut host = Host::new(Picker::default(), Box::new(FakeText), GlassPref::Auto);
        host.model.update(Pick::OpenMenu);
        let layer = &mut host.layers[popups::slot(gui::PopupKind::Menu)];
        layer.ui.set_viewport(Size { w: 200.0, h: 100.0 });
        let model = &host.model;
        layer.ui.build(|c| model.popup_view(c));
        layer.ui.layout(host.text.as_mut());
        layer.ui.focus_first(true);
        let enter = Event::Key {
            vk: 0x0D,
            down: true,
            mods: gui::Mods::default(),
        };
        assert!(host.popup_event(gui::PopupKind::Menu, enter));
        assert_eq!(host.model.closes, 1, "the menu closed");
        assert_eq!(
            host.model.popup().map(|p| p.kind),
            Some(crate::PopupKind::List),
            "and the list its item opened stays open"
        );
    }

    /// A dismissed tooltip window (the main window moved under it) takes the tip with it.
    #[test]
    fn a_dismissed_tooltip_hides_its_tip() {
        let mut host = Host::new(Picker::default(), Box::new(FakeText), GlassPref::Auto);
        host.ui.set_viewport(Size { w: 200.0, h: 100.0 });
        host.ui.set_active(true);
        let model = &host.model;
        host.ui.build(|c| model.view(c));
        host.ui.layout(host.text.as_mut());
        let at = host
            .ui
            .find(&[Key::name("b")])
            .and_then(|id| host.ui.info(id))
            .map(|n| n.rect.center())
            .expect("the button");
        let mut out = Vec::new();
        host.ui.dispatch(Input::PointerMove(at), &mut out);
        host.ui.tooltip_timer(0);
        host.ui
            .tooltip_timer(u64::from(crate::tokens::popup::TIP_DELAY_MS));
        assert_eq!(host.ui.tooltip().map(|(_, t)| t), Some("tip"));
        assert!(host.popup_event(gui::PopupKind::Tooltip, Event::Dismiss));
        assert_eq!(host.ui.tooltip(), None);
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
        assert_eq!(key_code(0x08), KeyCode::Backspace);
        assert_eq!(key_code(0x2D), KeyCode::Insert);
        assert_eq!(key_code(0x2E), KeyCode::Delete);
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
        // The presenter comes first, before any settings: nothing to decide yet.
        assert_eq!(host.set_presenter(Presenter::default()), Restyle::default());
        // Light is where the host starts: nothing to repaint.
        assert_eq!(host.set_env(light), Restyle::default());
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
        assert!(host.set_env(dark).theme);
        assert_eq!(*host.theme(), Theme::DARK);
        assert_ne!(host.ui.damage().len(), 0);
        host.ui.clear_damage();
        // Reduced motion and transparency off change no pixel.
        let calm = gui::env::Env {
            animations: false,
            transparency: false,
            ..dark
        };
        assert_eq!(host.set_env(calm), Restyle::default());
        assert_eq!(host.ui.motion(), Motion::REDUCED);
        assert_eq!(host.ui.damage().len(), 0);
        // The text size (§12 item 7), in percent: a new size repaints, the same one doesn't, and
        // an unread one (0) is 100 %.
        let large = gui::env::Env {
            text_scale: 175,
            ..calm
        };
        assert_eq!(host.set_env(large), Restyle::default());
        assert!((host.ui.text_scale() - 1.75).abs() < 1e-6);
        assert_ne!(host.ui.damage().len(), 0);
        host.ui.clear_damage();
        host.set_env(large);
        assert_eq!(host.ui.damage().len(), 0);
        host.set_env(calm);
        assert!((host.ui.text_scale() - 1.0).abs() < 1e-6);
        host.ui.clear_damage();
        // High contrast: the system's colors, whatever the app mode.
        let hc = gui::env::Env {
            high_contrast: true,
            ..calm
        };
        assert!(host.set_env(hc).theme);
        assert_eq!(host.theme().scheme(), Scheme::HighContrast);
        assert_eq!(
            host.theme().color(crate::Tone::Accent),
            Color::hex(0x00_78D7)
        );
        // WinPE says so, whatever else holds.
        let pe = gui::env::Env { winpe: true, ..hc };
        host.set_env(pe);
        assert_eq!(host.materials(), Materials::Solid(SolidReason::WinPe));
    }

    /// TD-0009 §5.2: a composed window on hardware shows glass and asks for Mica; switching
    /// *Transparency effects* off goes solid live, and a lost device for WARP stays solid.
    #[test]
    fn a_composed_window_goes_glass_and_back_live() {
        let mut host = Host::new(Counter::default(), Box::new(FakeText), GlassPref::Auto);
        host.ui.set_viewport(Size { w: 200.0, h: 100.0 });
        let hardware = Presenter {
            composed: true,
            hardware: true,
            device_lost: false,
        };
        assert_eq!(host.set_presenter(hardware), Restyle::default());
        let env = gui::env::Env {
            build: 26_100,
            transparency: true,
            animations: true,
            ..gui::env::Env::default()
        };
        host.ui.clear_damage();
        let glass = Restyle {
            theme: true,
            glass: Some(true),
        };
        assert_eq!(host.set_env(env), glass);
        assert_eq!(
            host.materials(),
            Materials::Glass {
                in_app_acrylic: true
            }
        );
        assert_eq!(*host.theme(), Theme::LIGHT.glass());
        assert_ne!(host.ui.damage().len(), 0, "the new theme repaints");
        let solid = Restyle {
            theme: true,
            glass: Some(false),
        };
        let opaque = gui::env::Env {
            transparency: false,
            ..env
        };
        assert_eq!(host.set_env(opaque), solid);
        assert_eq!(*host.theme(), Theme::LIGHT);
        assert_eq!(host.set_env(env), glass);
        // Hardware lost for WARP: solid until the window is opened again.
        let warp = Presenter {
            hardware: false,
            device_lost: true,
            ..hardware
        };
        assert_eq!(host.set_presenter(warp), solid);
        assert_eq!(host.materials(), Materials::Solid(SolidReason::DeviceLost));
        // Forced glass (G1) composes on WARP.
        let mut forced = Host::new(Counter::default(), Box::new(FakeText), GlassPref::Force);
        forced.set_presenter(Presenter {
            hardware: false,
            ..hardware
        });
        assert_eq!(forced.set_env(env), glass);
        assert_eq!(
            forced.materials(),
            Materials::Glass {
                in_app_acrylic: false
            }
        );
    }

    #[test]
    fn a_window_composes_only_where_glass_can_show() {
        let env = gui::env::Env {
            build: GLASS_BUILD,
            ..gui::env::Env::default()
        };
        assert_eq!(compose_for(&env, GlassPref::Auto), Compose::Hardware);
        // Off may become Auto in Settings while the window is open.
        assert_eq!(compose_for(&env, GlassPref::Off), Compose::Hardware);
        assert_eq!(compose_for(&env, GlassPref::Force), Compose::Any);
        let old = gui::env::Env {
            build: GLASS_BUILD - 1,
            ..env
        };
        let pe = gui::env::Env { winpe: true, ..env };
        for env in [old, pe] {
            for pref in [GlassPref::Auto, GlassPref::Force] {
                assert_eq!(compose_for(&env, pref), Compose::Never);
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct SizesMsg(crate::TableEvent);

    /// A table of one program: its size and the day it was installed (2026-11-06).
    struct Sizes;

    const DAY: UnixMillis = UnixMillis(1_793_923_200_000);

    const SIZE_COLUMNS: [crate::Column<'static>; 2] =
        [sizes_column(0, "Size"), sizes_column(1, "Installed")];

    const fn sizes_column(id: u16, header: &'static str) -> crate::Column<'static> {
        crate::Column {
            id: crate::ColumnId(id),
            header,
            width: crate::Track::Fixed(120.0),
            min: 64.0,
            align: crate::Align::Start,
            mono: false,
            sort: None,
            hideable: true,
        }
    }

    impl Model for Sizes {
        type Msg = SizesMsg;

        fn view(&self, cx: &mut Cx<'_, SizesMsg>) {
            let table = crate::Table {
                rows: 1,
                columns: &SIZE_COLUMNS,
                widths: &[],
                hidden: &[],
                sorted: None,
                checks: None,
                current: None,
                status: None,
                context: false,
                on: SizesMsg,
            };
            cx.table(
                Element::table(Key::name("t")).style(crate::Style {
                    height: crate::Dim::Fixed(150.0),
                    ..crate::Style::default()
                }),
                &table,
                |_, cells| {
                    cells.size(Bytes(1_536));
                    cells.date(DAY);
                    crate::Row {
                        key: crate::RowKey(0),
                        check: None,
                        locked: "",
                    }
                },
            );
        }

        fn update(&mut self, _: SizesMsg) {}

        fn size_units(&self) -> SizeUnits {
            SizeUnits::new(["{} B", "{} KB", "{} MB", "{} GB", "{} TB", "{} PB", "{} EB"])
        }
    }

    /// T2/T3 (TD-0009 A2.5, slice 5b3): a table's values read in the user's formats from the
    /// first settings, with the app's unit labels; the formats are read again only when their
    /// stamp changes.
    #[test]
    fn a_table_s_values_read_in_the_user_s_formats() {
        let mut host = Host::new(Sizes, Box::new(FakeText), GlassPref::Auto);
        host.ui.set_viewport(Size { w: 400.0, h: 200.0 });
        let env = gui::env::Env {
            locale: 1,
            ..gui::env::Env::default()
        };
        host.set_env(env);
        host.build();
        let cell = |host: &Host<Sizes>, col: u64| {
            let path = [
                Key::name("t"),
                crate::TABLE_BODY,
                Key::index(0),
                Key::index(col),
            ];
            let id = host.ui.find(&path).expect("a cell");
            host.ui.info(id).expect("live").text.to_owned()
        };
        let user = gui::locale::Locale::user();
        let mut day = String::new();
        user.date(DAY, &mut day);
        assert_eq!(cell(&host, 0), format!("1{}50 KB", user.decimal()));
        assert_eq!(cell(&host, 1), day);
        let generation = host.ui.formats_generation;
        host.set_env(env);
        assert_eq!(host.ui.formats_generation, generation, "the same formats");
        host.set_env(gui::env::Env { locale: 2, ..env });
        assert_ne!(host.ui.formats_generation, generation, "new formats");
    }
}
