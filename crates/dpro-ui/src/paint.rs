//! Painting (TD-0009 §5.2): the damaged regions are repainted from the retained tree into the
//! [`Canvas`], and the presenter shows exactly those regions.
//!
//! A region is repainted whole: the window's background, then every node that intersects it, in
//! tree order (parents under their children), clipped to the region and to every ancestor. So a
//! region gets exactly the pixels a full repaint would give it, and incremental painting is right
//! whenever the damage is (`tests/paint_damage.rs` checks the scripted session pixel by pixel).
//!
//! What each node shows:
//! - its [`Look`](crate::Look)'s fill, then its border, both with the look's radius;
//! - buttons and tabs: a state layer while hovered (following the hover fade) or pressed. A hold
//!   button held shows the part still to go in the permanent fill, its own fill growing from its
//!   start as the hold runs, and its text in the ink that pairs with each part;
//! - text in the look's ink (secondary when disabled), centered vertically in the content box,
//!   and horizontally too in a button. A node with children (a table row) shows no text of its
//!   own: its text is its accessible name;
//! - check boxes: a box a line high, with the state layer, then the label half a line after it;
//!   radio buttons the same with a circle, the accent with a dot in it when picked;
//! - progress bars: a pill-shaped track, the accent along it as far as its value;
//! - while focus is visible, Windows 11's focus visual over everything else: a ring in text color
//!   with a one-pixel surface-colored ring inside it, so it shows on any fill, accent included.
//!
//! Ink is looked up with the fill under it (the node's own, else its nearest ancestor's), which
//! is how high contrast gives text the system color paired with its background
//! ([`Theme::ink`]). High contrast also draws borders in `WindowText` (`GrayText` when disabled),
//! keeps `Highlight` fills for what can be used, and shows hover and press as an outline instead
//! of a state layer.

use dpro_app::table::Tri;

use crate::{
    canvas::Canvas,
    geom::{Rect, RectPx},
    text::{GlyphSink, Icon, TextSystem},
    theme::{
        Color, Scheme, Theme, Tone,
        tokens::{radius, stroke},
    },
    tree::{NIL, Node, Role},
    ui::{ROOT, Ui},
};

impl<M> Ui<M> {
    /// The canvas this window needs: its viewport in physical pixels.
    #[must_use]
    pub fn pixel_size(&self) -> (u32, u32) {
        let r = Rect::new(0.0, 0.0, self.viewport.w, self.viewport.h).to_px(self.scale);
        (
            u32::try_from(r.x1).unwrap_or(0),
            u32::try_from(r.y1).unwrap_or(0),
        )
    }

    /// Damages the whole window: after a theme switch, or for a canvas whose pixels were lost.
    pub fn repaint_all(&mut self) {
        self.damage_all();
    }

    /// Repaints the damaged regions in `theme`'s colors. The damage stays for the presenter, which
    /// calls [`clear_damage`](Self::clear_damage) once it has shown it.
    pub fn paint(&self, canvas: &mut Canvas<'_>, text: &mut dyn TextSystem, theme: &Theme) {
        for &r in self.damage() {
            self.paint_region(canvas, text, theme, r);
        }
    }

    /// Repaints `region`, in physical pixels, whatever the damage.
    pub fn paint_region(
        &self,
        canvas: &mut Canvas<'_>,
        text: &mut dyn TextSystem,
        theme: &Theme,
        region: RectPx,
    ) {
        let region = region.intersect(canvas.bounds());
        if region.is_empty() {
            return;
        }
        canvas.set_clip(region);
        // Set, not blended: under glass the background is clear, and the old pixels must go.
        canvas.clear(region, theme.color(Tone::App));
        let mut p = Painter {
            canvas,
            text,
            theme,
            scale: self.scale,
        };
        let mut c = self.nodes[ROOT as usize].first;
        while c != NIL {
            self.paint_node(c, region, Tone::App, &mut p);
            c = self.nodes[c as usize].next;
        }
        let all = p.canvas.bounds();
        p.canvas.set_clip(all);
    }

    /// Paints node `i` and its subtree within `clip`, over the fill `under` (its nearest
    /// ancestor's, which picks high contrast's ink).
    fn paint_node(&self, i: u32, clip: RectPx, under: Tone, p: &mut Painter<'_, '_>) {
        let node = &self.nodes[i as usize];
        let rect = node.rect.snap(p.scale);
        let visible = rect.intersect(clip);
        if visible.is_empty() {
            // Children are clipped to their parent: nothing below shows either.
            return;
        }
        p.canvas.set_clip(visible);
        let corner = node.look.radius.logical() * p.scale;
        let fill = node.look.fill.map(|f| p.theme.fill(f, node.enabled));
        if let Some(fill) = fill {
            p.canvas.rrect(rect, corner, p.theme.color(fill));
        }
        let under = fill.unwrap_or(under);
        let interactive = node.enabled && node.role.interactive();
        let button = matches!(node.role, Role::Button | Role::Tab);
        // A hold pressed: the part still to go turns permanent, and its fill grows back from the
        // start as it's held (doc 05 §4). Where they meet, in physical pixels.
        let split = (node.hold && self.hold.node == i && (node.pressed || self.hold.done))
            .then(|| p.hold(rect, corner, self.hold.t, visible));
        if interactive && button && split.is_none() {
            p.state_layer(node, rect, corner);
        }
        if let Some(border) = node.look.border {
            p.canvas
                .ring(rect, corner, 1, p.theme.stroke(border, node.enabled));
        }
        if interactive && button {
            p.outline(node, rect, corner, under);
        }
        let ink = p.theme.ink(
            if node.enabled {
                node.look.ink
            } else {
                Tone::TextSecondary
            },
            under,
        );
        match node.role {
            Role::CheckBox => p.check_box(node, interactive, ink),
            Role::Radio => p.radio(node, interactive, ink),
            Role::Field => self.paint_field(i, visible, under, ink, p),
            Role::Progress => p.progress(node, rect),
            Role::Label | Role::Button | Role::Tab
                if node.first == NIL && !node.text.is_empty() =>
            {
                let content = node.rect.inset(node.style.padding);
                if let Some(x) = split {
                    // Each part's text in the ink that pairs with its fill.
                    let to_go = p.theme.ink(Tone::PermanentText, Tone::PermanentFill);
                    for (part, ink) in [
                        (RectPx { x1: x, ..rect }, ink),
                        (RectPx { x0: x, ..rect }, to_go),
                    ] {
                        let clip = part.intersect(visible);
                        if !clip.is_empty() {
                            p.canvas.set_clip(clip);
                            p.text(node, content, button, ink);
                        }
                    }
                    p.canvas.set_clip(visible);
                } else {
                    p.text(node, content, button, ink);
                }
            }
            Role::Container | Role::Label | Role::Button | Role::Tab => {}
        }
        let mut child = node.first;
        while child != NIL {
            self.paint_node(child, visible, under, p);
            child = self.nodes[child as usize].next;
        }
        if self.focus == i && self.focus_visible {
            p.canvas.set_clip(visible);
            p.focus_ring(rect, corner);
        }
    }

    /// A field's content, clipped to its content box: the selection, the visible part of its
    /// text (or the placeholder while it's empty), then the caret. Selected text keeps its ink;
    /// high contrast redraws it in `HighlightText` over `Highlight`.
    fn paint_field(
        &self,
        i: u32,
        visible: RectPx,
        under: Tone,
        ink: Color,
        p: &mut Painter<'_, '_>,
    ) {
        let node = &self.nodes[i as usize];
        let field = &self.fields[node.field as usize];
        let content = node.rect.inset(node.style.padding);
        let clip = content.snap(p.scale).intersect(visible);
        if clip.is_empty() {
            return;
        }
        p.canvas.set_clip(clip);
        let line = field.line_box(content);
        // Where the text starts: scrolled left to keep the caret in view.
        let origin = line.x - field.scroll;
        let shows = self.active && self.focused_field() == Some(i);
        let (start, end) = field.selection();
        let selection = (shows && start != end).then(|| {
            let x0 = origin + field.x_of(start);
            let x1 = origin + field.x_of(end);
            Rect::new(x0, line.y, x1 - x0, line.h).snap(p.scale)
        });
        if let Some(sel) = selection {
            let fill = p.theme.fill(Tone::Selected, true);
            p.canvas.fill(sel, p.theme.color(fill));
        }
        if node.text.is_empty() {
            if !field.placeholder.is_empty() {
                let secondary = p.theme.ink(Tone::TextSecondary, under);
                p.line(&field.placeholder, node, line, secondary);
            }
        } else {
            let (slice, offset) = field.visible(&node.text);
            let run = Rect::new(origin + offset, line.y, f32::INFINITY, line.h);
            p.line(slice, node, run, ink);
            let selected_ink = p.theme.ink(Tone::Text, Tone::Selected);
            if let Some(sel) = selection
                && selected_ink != ink
            {
                p.canvas.set_clip(sel.intersect(clip));
                p.line(slice, node, run, selected_ink);
                p.canvas.set_clip(clip);
            }
        }
        if shows && self.caret_on {
            let caret = Rect::new(
                origin + field.x_of(field.caret as usize),
                line.y,
                crate::field::caret_logical(self.caret),
                line.h,
            );
            p.canvas
                .fill(caret.snap(p.scale), p.theme.ink(Tone::Text, under));
        }
        p.canvas.set_clip(visible);
    }
}

/// What a paint pass draws with.
struct Painter<'p, 'c> {
    canvas: &'p mut Canvas<'c>,
    text: &'p mut dyn TextSystem,
    theme: &'p Theme,
    scale: f32,
}

impl Painter<'_, '_> {
    /// Windows 11's focus visual inside `rect`: a ring in text color, [`stroke::FOCUS`] wide, then
    /// one pixel of surface color inside it.
    fn focus_ring(&mut self, rect: RectPx, corner: f32) {
        // Whole pixels, at least one.
        let wide = (stroke::FOCUS * self.scale).round().max(1.0);
        let width = px_width(wide);
        self.canvas.ring(rect, corner, width, self.theme.focus());
        let inner = RectPx {
            x0: rect.x0 + width,
            y0: rect.y0 + width,
            x1: rect.x1 - width,
            y1: rect.y1 - width,
        };
        self.canvas.ring(
            inner,
            (corner - wide).max(0.0),
            1,
            self.theme.color(Tone::Surface),
        );
    }

    /// The hover or pressed layer over a control's shape (none in high contrast: see
    /// [`outline`](Self::outline)).
    fn state_layer<M>(&mut self, n: &Node<M>, r: RectPx, corner: f32) {
        if self.theme.scheme() == Scheme::HighContrast {
            return;
        }
        let layer = if n.pressed {
            self.theme.color(Tone::Pressed)
        } else if n.hover_t > 0.0 {
            let c = self.theme.color(Tone::Hover);
            c.alpha(fraction(c.a, n.hover_t))
        } else {
            return;
        };
        self.canvas.rrect(r, corner, layer);
    }

    /// High contrast's hover and press: a whole-pixel outline over the border, in the color that
    /// shows on the fill `under` ([`Theme::hover_outline`]).
    fn outline<M>(&mut self, n: &Node<M>, r: RectPx, corner: f32, under: Tone) {
        let Some(color) = self.theme.hover_outline(under) else {
            return;
        };
        if n.pressed || n.hover_t > 0.0 {
            let width = px_width(self.scale.round().max(1.0));
            self.canvas.ring(r, corner, width, color);
        }
    }

    /// `n`'s text in `ink`, centered vertically in `content` (and horizontally if `center`).
    fn text<M>(&mut self, n: &Node<M>, content: Rect, center: bool, ink: Color) {
        let m = self.text.measure(&n.text, &n.text_style, content.w);
        let x = if center {
            content.x + ((content.w - m.width) / 2.0).max(0.0)
        } else {
            content.x
        };
        let y = content.y + ((content.h - m.height) / 2.0).max(0.0);
        let rect = Rect::new(x, y, content.right() - x, m.height);
        let mut sink = Ink {
            canvas: &mut *self.canvas,
            color: ink,
        };
        self.text
            .draw(&n.text, &n.text_style, rect, self.scale, &mut sink);
    }

    /// `text` on one line in `n`'s text style, from `rect`'s top-left, untrimmed if `rect` is
    /// unbounded; clipped by the canvas.
    fn line<M>(&mut self, text: &str, n: &Node<M>, rect: Rect, ink: Color) {
        let mut sink = Ink {
            canvas: &mut *self.canvas,
            color: ink,
        };
        self.text
            .draw(text, &n.text_style, rect, self.scale, &mut sink);
    }

    /// A hold's two parts over the fill already painted: from `t` of the way along `rect` to its
    /// end, the permanent fill. Returns where the parts meet. The clip is `visible` again after.
    fn hold(&mut self, rect: RectPx, corner: f32, t: f32, visible: RectPx) -> i32 {
        let x = rect.x0 + px_width((t.clamp(0.0, 1.0) * f(rect.x1 - rect.x0)).round());
        let to_go = RectPx { x0: x, ..rect }.intersect(visible);
        if !to_go.is_empty() {
            self.canvas.set_clip(to_go);
            self.canvas
                .rrect(rect, corner, self.theme.color(Tone::PermanentFill));
            self.canvas.set_clip(visible);
        }
        x
    }

    /// A progress bar: a pill-shaped track (a ring in high contrast, which has no tints), and the
    /// accent from its start, as far as its value.
    fn progress<M>(&mut self, n: &Node<M>, track: RectPx) {
        let round = half(track);
        if self.theme.scheme() == Scheme::HighContrast {
            self.canvas
                .ring(track, round, 1, self.theme.stroke(Tone::Border, true));
        } else {
            self.canvas
                .rrect(track, round, self.theme.color(Tone::Hover));
        }
        let w = px_width((n.value.clamp(0.0, 1.0) * f(track.x1 - track.x0)).round());
        if w > 0 {
            let bar = RectPx {
                x1: track.x0 + w,
                ..track
            };
            let accent = self.theme.fill(Tone::Accent, true);
            self.canvas.rrect(bar, round, self.theme.color(accent));
        }
    }

    /// Where a check box's box (or a radio button's circle) goes: a line high, centered
    /// vertically in the content box. Returns the content box, the line and the box.
    fn mark_box<M>(&mut self, n: &Node<M>) -> (Rect, f32, Rect) {
        let content = n.rect.inset(n.style.padding);
        let line = self.text.measure("", &n.text_style, f32::INFINITY).height;
        let b = Rect::new(
            content.x,
            content.y + ((content.h - line) / 2.0).max(0.0),
            line,
            line,
        );
        (content, line, b)
    }

    /// A check box's or radio button's label, half a line after its box.
    fn mark_label<M>(&mut self, n: &Node<M>, content: Rect, line: f32, ink: Color) {
        if !n.text.is_empty() {
            let label = Rect::new(
                content.x + line * 1.5,
                content.y,
                (content.w - line * 1.5).max(0.0),
                content.h,
            );
            self.text(n, label, false, ink);
        }
    }

    /// The circle, a line high, then the label half a line after it, as for a check box: picked,
    /// the accent with a dot in it; else a ring.
    fn radio<M>(&mut self, n: &Node<M>, interactive: bool, ink: Color) {
        let (content, line, b) = self.mark_box(n);
        let r = b.snap(self.scale);
        let round = half(r);
        let on = n.selected == Some(true);
        let fill = match (on, n.enabled) {
            (true, true) => Tone::Accent,
            (true, false) => Tone::TextSecondary,
            (false, _) => Tone::Surface,
        };
        self.canvas.rrect(r, round, self.theme.color(fill));
        if !on {
            let edge = if n.enabled {
                Tone::TextSecondary
            } else {
                Tone::Border
            };
            self.canvas
                .ring(r, round, 1, self.theme.stroke(edge, n.enabled));
        }
        if interactive {
            self.state_layer(n, r, round);
            self.outline(n, r, round, fill);
        }
        if on {
            let dot = Rect::new(b.x + line * 0.3, b.y + line * 0.3, line * 0.4, line * 0.4)
                .snap(self.scale);
            self.canvas
                .rrect(dot, half(dot), self.theme.ink(Tone::OnAccent, fill));
        }
        self.mark_label(n, content, line, ink);
    }

    /// The box, a line high, then the label half a line after it (the size layout measured).
    fn check_box<M>(&mut self, n: &Node<M>, interactive: bool, ink: Color) {
        let (content, line, b) = self.mark_box(n);
        let r = b.snap(self.scale);
        let corner = radius::SMALL * self.scale;
        let on = n.check != Tri::Off;
        let fill = match (on, n.enabled) {
            (true, true) => Tone::Accent,
            (true, false) => Tone::TextSecondary,
            (false, _) => Tone::Surface,
        };
        self.canvas.rrect(r, corner, self.theme.color(fill));
        if !on {
            let edge = if n.enabled {
                Tone::TextSecondary
            } else {
                Tone::Border
            };
            self.canvas
                .ring(r, corner, 1, self.theme.stroke(edge, n.enabled));
        }
        if interactive {
            self.state_layer(n, r, corner);
            self.outline(n, r, corner, fill);
        }
        let mark = self.theme.ink(Tone::OnAccent, fill);
        match n.check {
            Tri::On => {
                let mut sink = Ink {
                    canvas: &mut *self.canvas,
                    color: mark,
                };
                self.text.icon(Icon::CheckMark, b, self.scale, &mut sink);
            }
            Tri::Mixed => {
                // A bar across the middle half.
                let bar = Rect::new(
                    b.x + line / 4.0,
                    b.y + line * 7.0 / 16.0,
                    line / 2.0,
                    line / 8.0,
                );
                self.canvas.fill(bar.snap(self.scale), mark);
            }
            Tri::Off => {}
        }
        self.mark_label(n, content, line, ink);
    }
}

/// Draws glyphs in one color.
struct Ink<'p, 'c> {
    canvas: &'p mut Canvas<'c>,
    color: Color,
}

impl GlyphSink for Ink<'_, '_> {
    fn fill(&mut self, r: RectPx) {
        self.canvas.fill(r, self.color);
    }

    fn mask(&mut self, x: i32, y: i32, width: u32, coverage: &[u8]) {
        self.canvas.mask(x, y, width, coverage, self.color);
    }
}

/// `a × t`, for `t` in [0, 1].
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the product is clamped to [0, 255]"
)]
fn fraction(a: u8, t: f32) -> u8 {
    (f32::from(a) * t.clamp(0.0, 1.0)).round() as u8
}

/// A whole-pixel stroke width as an integer.
#[allow(
    clippy::cast_possible_truncation,
    reason = "stroke widths are a few whole pixels; `as` saturates anyway"
)]
fn px_width(w: f32) -> i32 {
    w as i32
}

/// A pixel count as a float.
#[allow(
    clippy::cast_precision_loss,
    reason = "canvas sizes are far below 2^24"
)]
fn f(v: i32) -> f32 {
    v as f32
}

/// Half of `r`'s shorter side: the radius that rounds it into a circle or a pill.
fn half(r: RectPx) -> f32 {
    f((r.x1 - r.x0).min(r.y1 - r.y0).max(0)) / 2.0
}

#[cfg(test)]
mod tests {
    use crate::{
        Canvas, Element, FakeText, Input, Insets, Key, KeyCode, KeyEvent, Look, Mods, Motion,
        Radius, Rect, RectPx, Size, Style, Theme, Tone, Tri, Ui,
    };

    /// A 100 × 60 window: a card (padding 4) holding an accent button and a check box. `FakeText`
    /// at 13 px: lines are 16.25 high, so the card is 0..100 × 0..40.5 and the button
    /// 4..96 × 4..20.25.
    fn window(scale: f32) -> Ui<u8> {
        let mut ui = Ui::new(Size { w: 100.0, h: 60.0 }, scale, Motion::default());
        ui.build(|cx| {
            cx.add_with(
                Element::stack(Key(1))
                    .style(Style {
                        padding: Insets::all(4.0),
                        ..Style::default()
                    })
                    .look(Look {
                        fill: Some(Tone::Surface),
                        border: Some(Tone::Border),
                        radius: Radius::Card,
                        ..Look::PLAIN
                    }),
                |cx| {
                    cx.add(Element::button(Key(2), "Go", 1).look(Look {
                        fill: Some(Tone::Accent),
                        ink: Tone::OnAccent,
                        radius: Radius::Control,
                        border: None,
                    }));
                    cx.add(Element::toggle(Key(3), "On", Tri::On, 2));
                },
            );
        });
        ui.layout(&mut FakeText);
        ui
    }

    struct Frame {
        px: Vec<u32>,
        width: u32,
    }

    impl Frame {
        fn at(&self, x: i32, y: i32) -> u32 {
            let (x, y) = (u32::try_from(x).unwrap(), u32::try_from(y).unwrap());
            self.px[usize::try_from(y * self.width + x).unwrap()]
        }
    }

    fn paint(ui: &Ui<u8>, theme: &Theme) -> Frame {
        let (width, h) = ui.pixel_size();
        let mut px = vec![0; usize::try_from(width * h).unwrap()];
        let mut canvas = Canvas::new(&mut px, width, h).unwrap();
        ui.paint(&mut canvas, &mut FakeText, theme);
        Frame { px, width }
    }

    fn argb(theme: &Theme, tone: Tone) -> u32 {
        let c = theme.color(tone);
        0xFF00_0000 | u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
    }

    fn button(ui: &Ui<u8>) -> Rect {
        ui.info(ui.find(&[Key(1), Key(2)]).unwrap()).unwrap().rect
    }

    #[test]
    fn a_frame_shows_fills_borders_and_ink() {
        let ui = window(1.0);
        let t = Theme::LIGHT;
        let f = paint(&ui, &t);
        assert_eq!((ui.pixel_size(), f.px.len()), ((100, 60), 6000));
        assert_eq!(button(&ui), Rect::new(4.0, 4.0, 92.0, 16.25));
        // The card's border at its top edge, its surface inside, the window's background below.
        assert_eq!(f.at(50, 0), argb(&t, Tone::Border));
        assert_eq!(f.at(50, 2), argb(&t, Tone::Surface));
        assert_eq!(f.at(50, 50), argb(&t, Tone::App));
        // The button (4..96 × 4..20 in pixels): accent, with "Go" centered in on-accent ink. Its
        // glyph boxes run from y 7 to 17, the first from x 44 to 49.
        let go = button(&ui).snap(1.0);
        assert_eq!(
            go,
            RectPx {
                x0: 4,
                y0: 4,
                x1: 96,
                y1: 20
            }
        );
        assert_eq!(f.at(5, 12), argb(&t, Tone::Accent));
        assert_eq!(f.at(46, 12), argb(&t, Tone::OnAccent));
        assert!((20..80).all(|x| f.at(x, 5) == argb(&t, Tone::Accent)));
    }

    /// TD-0009 §4.4: only the system's colors, and each text in the color paired with its fill.
    #[test]
    fn high_contrast_paints_system_colors_paired_with_their_fills() {
        use crate::{Color, SysColors};
        // Distinct values, so every pixel says which system color it is.
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
        let opaque =
            |c: Color| 0xFF00_0000 | u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b);
        let mut ui = window(1.0);
        let f = paint(&ui, &t);
        // The card: a WindowText border around Window.
        assert_eq!(f.at(50, 0), opaque(sys.window_text));
        assert_eq!(f.at(50, 2), opaque(sys.window));
        assert_eq!(f.at(50, 50), opaque(sys.window));
        // The accent button is Highlight, and its label HighlightText (the "Go" glyph box at 46).
        assert_eq!(f.at(5, 12), opaque(sys.highlight));
        assert_eq!(f.at(46, 12), opaque(sys.highlight_text));
        // No color outside the system's, anywhere.
        let allowed: Vec<u32> = [
            sys.window,
            sys.window_text,
            sys.highlight,
            sys.highlight_text,
            sys.hot_light,
            sys.gray_text,
            sys.btn_face,
            sys.btn_text,
        ]
        .into_iter()
        .map(opaque)
        .collect();
        assert!(
            f.px.iter().all(|p| allowed.contains(p)),
            "a color that isn't a system color"
        );
        // Hovering tints nothing: the accent button gets a HighlightText outline, and the rest
        // of it stays as it was.
        let center = button(&ui).center();
        let go = button(&ui).snap(1.0);
        let mut msgs = Vec::new();
        ui.dispatch(Input::PointerMove(center), &mut msgs);
        ui.repaint_all();
        let hovered = paint(&ui, &t);
        assert_eq!(hovered.at(go.x0, 12), opaque(sys.highlight_text));
        assert_eq!(hovered.at(go.x0 + 1, 12), opaque(sys.highlight));
        assert!(hovered.px.iter().all(|p| allowed.contains(p)));
        let outside = |x: i32, y: i32| x < go.x0 || x >= go.x1 || y < go.y0 || y >= go.y1;
        for (k, (now, was)) in hovered.px.iter().zip(&f.px).enumerate() {
            let (x, y) = (
                i32::try_from(k % 100).unwrap(),
                i32::try_from(k / 100).unwrap(),
            );
            if outside(x, y) {
                assert_eq!(now, was, "({x}, {y}) changed");
            }
        }
        ui.dispatch(Input::PointerLeave, &mut msgs);
        let tab = Input::Key(KeyEvent {
            key: KeyCode::Tab,
            down: true,
            mods: Mods::default(),
        });
        ui.dispatch(tab, &mut msgs);
        let focused = paint(&ui, &t);
        let r = button(&ui).snap(1.0);
        assert_eq!(
            focused.at(r.x0, i32::midpoint(r.y0, r.y1)),
            opaque(sys.highlight)
        );
    }

    /// A danger hold button 0..200 × 0..20, a progress bar 0..200 × 20..26 at 41 %, and a picked
    /// radio button from y 26, its circle 16 px across.
    fn controls() -> Ui<u8> {
        let mut ui = Ui::new(Size { w: 200.0, h: 60.0 }, 1.0, Motion::default());
        let high = |h| Style {
            height: crate::Dim::Fixed(h),
            ..Style::default()
        };
        ui.build(|cx| {
            cx.add(
                Element::button(Key(1), "Hold", 1)
                    .hold(|| 2)
                    .look(Look::DANGER)
                    .style(high(20.0)),
            );
            cx.add(Element::progress(Key(2), 0.41).style(high(6.0)));
            cx.add(Element::radio(Key(3), "On", true, 3));
        });
        ui.layout(&mut FakeText);
        ui
    }

    fn opaque(c: crate::Color) -> u32 {
        0xFF00_0000 | u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
    }

    #[test]
    fn a_held_button_fills_from_its_start_and_a_bar_to_its_value() {
        let t = Theme::LIGHT;
        let mut ui = controls();
        let f = paint(&ui, &t);
        // At rest, the danger fill (above the text, clear of the corners).
        assert_eq!(
            (f.at(50, 2), f.at(150, 2)),
            (argb(&t, Tone::Danger), argb(&t, Tone::Danger))
        );
        // The bar: the accent to 41 % of 200 px, then the track.
        assert_eq!(f.at(40, 23), argb(&t, Tone::Accent));
        // Clear of its rounded end, which ends at 82.
        assert_eq!(f.at(78, 23), argb(&t, Tone::Accent));
        // The track: the hover tint over the window, within the canvas's rounding of it.
        let track = opaque(t.color(Tone::Hover).over(t.color(Tone::App)));
        let near = |p: u32| {
            (0..3).all(|k| ((p >> (8 * k)) & 0xFF).abs_diff((track >> (8 * k)) & 0xFF) <= 1)
        };
        assert!(
            near(f.at(83, 23)) && f.at(150, 23) == f.at(83, 23),
            "{:x}",
            f.at(83, 23)
        );
        // The radio button: an accent circle with an on-accent dot.
        assert_eq!(
            (f.at(3, 34), f.at(8, 34)),
            (argb(&t, Tone::Accent), argb(&t, Tone::OnAccent))
        );

        // Pressed: all of it still to go, in the permanent fill; then the fill grows back.
        let mut out = Vec::new();
        ui.dispatch(
            Input::PointerDown(crate::Point { x: 100.0, y: 10.0 }),
            &mut out,
        );
        let f = paint(&ui, &t);
        assert_eq!(
            (f.at(50, 2), f.at(150, 2)),
            (argb(&t, Tone::PermanentFill), argb(&t, Tone::PermanentFill))
        );
        ui.tick(0, &mut out);
        ui.tick(750, &mut out);
        let f = paint(&ui, &t);
        assert_eq!(
            (f.at(99, 2), f.at(100, 2)),
            (argb(&t, Tone::Danger), argb(&t, Tone::PermanentFill))
        );
        ui.tick(1_500, &mut out);
        let f = paint(&ui, &t);
        assert_eq!(
            (out.as_slice(), f.at(150, 2)),
            (&[1_u8] as &[u8], argb(&t, Tone::Danger))
        );
        // Let go, and the pointer gone (so no hover layer): as it was.
        ui.dispatch(
            Input::PointerUp(crate::Point { x: 100.0, y: 10.0 }),
            &mut out,
        );
        ui.dispatch(Input::PointerLeave, &mut out);
        assert_eq!(paint(&ui, &t).at(150, 2), argb(&t, Tone::Danger));
    }

    /// High contrast mid-hold: still only the system's colors.
    #[test]
    fn controls_in_high_contrast_paint_only_system_colors() {
        use crate::{Color, SysColors};
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
        let allowed = [
            sys.window,
            sys.window_text,
            sys.highlight,
            sys.highlight_text,
            sys.hot_light,
            sys.gray_text,
            sys.btn_face,
            sys.btn_text,
        ]
        .map(opaque);
        let mut ui = controls();
        let mut out = Vec::new();
        ui.dispatch(
            Input::PointerDown(crate::Point { x: 100.0, y: 10.0 }),
            &mut out,
        );
        ui.tick(0, &mut out);
        ui.tick(750, &mut out);
        let f = paint(&ui, &t);
        assert!(
            f.px.iter().all(|p| allowed.contains(p)),
            "a color that isn't a system color"
        );
        // The elapsed part is Highlight, the rest Window; the bar's track is a WindowText ring.
        assert_eq!(
            (f.at(50, 2), f.at(150, 2)),
            (opaque(sys.highlight), opaque(sys.window))
        );
        assert_eq!(
            (f.at(150, 20), f.at(150, 23)),
            (opaque(sys.window_text), opaque(sys.window))
        );
    }

    #[test]
    fn dark_paints_in_dark_colors_and_the_damage_stays() {
        let ui = window(1.0);
        let f = paint(&ui, &Theme::DARK);
        assert_eq!(f.at(50, 59), argb(&Theme::DARK, Tone::App));
        assert_ne!(ui.damage(), [] as [RectPx; 0], "the presenter clears it");
    }

    #[test]
    fn hover_press_and_focus_change_the_pixels() {
        let mut ui = window(1.5);
        let t = Theme::LIGHT;
        let center = button(&ui).center();
        let before = paint(&ui, &t);
        ui.clear_damage();
        let mut msgs = Vec::new();
        // Motion::default() has no fade: the highlight is there at once.
        ui.dispatch(Input::PointerMove(center), &mut msgs);
        let hovered = paint(&ui, &t);
        assert_ne!(hovered.px, before.px);
        ui.dispatch(Input::PointerDown(center), &mut msgs);
        let pressed = paint(&ui, &t);
        assert_ne!(pressed.px, hovered.px);
        ui.dispatch(Input::PointerUp(center), &mut msgs);
        ui.dispatch(Input::PointerLeave, &mut msgs);
        assert_eq!(msgs, [1]);
        // The press focused the button; two Tabs come back to it (via the check box) with the
        // ring showing: 3 px of text color at its edge, then 1 px of surface.
        let tab = Input::Key(KeyEvent {
            key: KeyCode::Tab,
            down: true,
            mods: Mods::default(),
        });
        ui.dispatch(tab, &mut msgs);
        ui.dispatch(tab, &mut msgs);
        assert_eq!(ui.focus(), ui.find(&[Key(1), Key(2)]));
        let focused = paint(&ui, &t);
        let r = button(&ui).snap(1.5);
        let mid = i32::midpoint(r.y0, r.y1);
        assert_eq!(focused.at(r.x0, mid), argb(&t, Tone::Text));
        assert_eq!(focused.at(r.x0 + 2, mid), argb(&t, Tone::Text));
        assert_eq!(focused.at(r.x0 + 3, mid), argb(&t, Tone::Surface));
        assert_eq!(focused.at(r.x0 + 4, mid), argb(&t, Tone::Accent));
    }
}
