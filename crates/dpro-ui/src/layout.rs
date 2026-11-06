//! Layout: the flexbox subset of [`Style`](crate::Style) (TD-0009 §5.1).
//!
//! - Children that don't grow take their natural size along the axis: fixed, or measured from
//!   their content (height for width, for wrapping text).
//! - Growing children share what the others leave, by `grow` and within their min and max,
//!   whatever their content (CSS's `flex: 1`, a basis of 0). Table cells get equal widths and
//!   their text trims; a page's body fills the window under its header.
//! - With nothing growing, `justify` places the free space.
//! - Across the axis, `align` places children or stretches them.
//!
//! `measure` caches a node's natural size for the width it was offered, until the node or a
//! descendant changes. `arrange` skips a subtree that is clean and keeps its rectangle, so a frame
//! lays out only what changed.
//!
//! Damage: a node whose rectangle changed damages where it was and where it is. That covers its
//! descendants, which paint inside it, so they add nothing more.
//!
//! Children that don't fit overflow their parent. Painting clips them; scrolling containers come
//! with the data table.

use crate::{
    geom::{Rect, Size},
    style::{Align, Axis, Dim, Justify, clamp},
    text::{TextSystem, count},
    tree::{NIL, Role},
    ui::{ROOT, Ui},
};

impl<M> Ui<M> {
    /// Lays out what changed since the last layout, damaging what moved.
    pub fn layout(&mut self, text: &mut dyn TextSystem) {
        let window = Rect::new(0.0, 0.0, self.viewport.w, self.viewport.h);
        self.arrange(ROOT, window, text, false);
        self.layout_fields(text);
        // Nodes may have moved, appeared or been enabled under a pointer that stayed still.
        if let Some(p) = self.pointer {
            let hit = self.hit(p);
            self.pointer_over(hit);
        }
    }

    /// The natural outer size of `i` when `avail` logical pixels of width are available.
    pub(crate) fn measure(&mut self, i: u32, avail: f32, text: &mut dyn TextSystem) -> Size {
        {
            let n = &self.nodes[i as usize];
            if n.cache_valid && n.measured_for.to_bits() == avail.to_bits() {
                return n.measured;
            }
        }
        let n = &self.nodes[i as usize];
        let s = n.style;
        let outer_w = clamp(fixed(s.width).unwrap_or(avail), s.min.w, s.max.w);
        let inner_w = (outer_w - s.padding.horizontal()).max(0.0);
        let content = if fixed(s.width).is_some() && fixed(s.height).is_some() {
            // The content can't change the size; `arrange` measures it when placing it.
            Size::ZERO
        } else if n.first == NIL {
            match n.role {
                // A progress bar and a separator are as big as their styles make them.
                Role::Container | Role::Progress | Role::Separator => Size::ZERO,
                Role::Label | Role::Button | Role::Tab | Role::MenuItem | Role::Tooltip => {
                    let m = text.measure(&n.text, &n.text_style, inner_w);
                    Size {
                        w: m.width,
                        h: m.height,
                    }
                }
                // Its value, then a chevron a line square.
                Role::ComboBox => {
                    let line = text.measure("", &n.text_style, f32::INFINITY).height;
                    let m = text.measure(&n.text, &n.text_style, (inner_w - line).max(0.0));
                    Size {
                        w: m.width + line,
                        h: m.height.max(line),
                    }
                }
                // As wide as the view makes it, a line high: its text never changes its size.
                Role::Field => Size {
                    w: 0.0,
                    h: text.measure("", &n.text_style, f32::INFINITY).height,
                },
                Role::CheckBox | Role::Radio => {
                    // The box (or circle) is a line high, with half a line before the label.
                    let line = text.measure("", &n.text_style, f32::INFINITY).height;
                    let label_w = (inner_w - line * 1.5).max(0.0);
                    let m = text.measure(&n.text, &n.text_style, label_w);
                    Size {
                        w: line * 1.5 + m.width,
                        h: m.height.max(line),
                    }
                }
            }
        } else {
            self.measure_children(i, inner_w, text)
        };
        let size = Size {
            w: clamp(
                fixed(s.width).unwrap_or(content.w + s.padding.horizontal()),
                s.min.w,
                s.max.w,
            ),
            h: clamp(
                fixed(s.height).unwrap_or(content.h + s.padding.vertical()),
                s.min.h,
                s.max.h,
            ),
        };
        let n = &mut self.nodes[i as usize];
        n.cache_valid = true;
        n.measured_for = avail;
        n.measured = size;
        size
    }

    /// The children's natural extent: their sizes along the axis plus gaps, by the largest across.
    ///
    /// In a row of a known width, growing children count as the widths `arrange` will give them,
    /// and are measured at those widths, so wrapping text is as tall as it will be.
    fn measure_children(&mut self, i: u32, inner_w: f32, text: &mut dyn TextSystem) -> Size {
        let (axis, gap, first) = {
            let n = &self.nodes[i as usize];
            (n.style.axis, n.style.gap, n.first)
        };
        let (mut main, mut cross, mut len, mut grow) = (0.0f32, 0.0f32, 0usize, 0.0f32);
        let flex = axis == Axis::Row && inner_w.is_finite();
        let mut c = first;
        while c != NIL {
            let child_grow = self.nodes[c as usize].style.grow.max(0.0);
            if flex && child_grow > 0.0 {
                grow += child_grow;
            } else {
                let (child_main, child_cross) = along(axis, self.measure(c, inner_w, text));
                main += child_main;
                cross = cross.max(child_cross);
            }
            len += 1;
            c = self.nodes[c as usize].next;
        }
        main += gap * count(len.saturating_sub(1));
        if grow > 0.0 {
            let space = (inner_w - main).max(0.0);
            let mut c = first;
            while c != NIL {
                let s = self.nodes[c as usize].style;
                if s.grow > 0.0 {
                    let w = clamp(space * s.grow / grow, s.min.w, s.max.w);
                    main += w;
                    cross = cross.max(self.measure(c, w, text).h);
                }
                c = self.nodes[c as usize].next;
            }
        }
        match axis {
            Axis::Row => Size { w: main, h: cross },
            Axis::Column => Size { w: cross, h: main },
        }
    }

    /// Puts `i` at `rect` and lays out its children. `covered`: an ancestor already damaged both
    /// its old and new rectangles, which hold this node's.
    fn arrange(&mut self, i: u32, rect: Rect, text: &mut dyn TextSystem, covered: bool) {
        let (moved, dirty, old) = {
            let n = &self.nodes[i as usize];
            (n.rect != rect, n.dirty, n.rect)
        };
        if !moved && !dirty {
            return;
        }
        let mut covered = covered;
        if moved {
            self.a11y_touch(i);
            if !covered {
                self.damage_rect(old);
                self.damage_rect(rect);
            }
            covered = true;
        } else if !covered && self.nodes[i as usize].first == NIL {
            // A leaf whose content changed in place (text, style) repaints where it is.
            self.damage_rect(rect);
        }
        let n = &mut self.nodes[i as usize];
        n.rect = rect;
        n.dirty = false;
        if n.first != NIL {
            self.arrange_children(i, text, covered);
        }
    }

    fn arrange_children(&mut self, i: u32, text: &mut dyn TextSystem, covered: bool) {
        let (s, inner, first) = {
            let n = &self.nodes[i as usize];
            (n.style, n.rect.inset(n.style.padding), n.first)
        };
        let (inner_main, inner_cross) = along(
            s.axis,
            Size {
                w: inner.w,
                h: inner.h,
            },
        );

        // Pass 1: the natural sizes of children that don't grow, and the totals.
        let (mut used, mut grow, mut len) = (0.0f32, 0.0f32, 0usize);
        let mut c = first;
        while c != NIL {
            let g = self.nodes[c as usize].style.grow.max(0.0);
            if g > 0.0 {
                grow += g;
                self.nodes[c as usize].main = 0.0;
            } else {
                used += self.child_main(c, s.axis, s.align, inner, text);
            }
            len += 1;
            c = self.nodes[c as usize].next;
        }
        used += s.gap * count(len.saturating_sub(1));
        let mut free = (inner_main - used).max(0.0);

        // Growing children share the free space; what their max sizes leave is placed by `justify`.
        if grow > 0.0 {
            let space = free;
            let mut c = first;
            while c != NIL {
                let node = &mut self.nodes[c as usize];
                let g = node.style.grow.max(0.0);
                if g > 0.0 {
                    node.main = clamp(
                        space * g / grow,
                        node.style.min_main(s.axis),
                        node.style.max_main(s.axis),
                    );
                    free -= node.main;
                }
                c = node.next;
            }
            free = free.max(0.0);
        }
        let (mut at, mut between) = (0.0f32, s.gap);
        match s.justify {
            Justify::Center => at = free / 2.0,
            Justify::End => at = free,
            Justify::SpaceBetween if len > 1 => between += free / count(len - 1),
            Justify::Start | Justify::SpaceBetween => {}
        }

        // Pass 2: cross sizes (height for the final width in a row), then place.
        let mut c = first;
        while c != NIL {
            let main = self.nodes[c as usize].main;
            let cross = self.child_cross(c, s.axis, s.align, main, inner_cross, text);
            let offset = match s.align {
                Align::Start | Align::Stretch => 0.0,
                Align::Center => (inner_cross - cross) / 2.0,
                Align::End => inner_cross - cross,
            };
            let rect = match s.axis {
                Axis::Row => Rect::new(inner.x + at, inner.y + offset, main, cross),
                Axis::Column => Rect::new(inner.x + offset, inner.y + at, cross, main),
            };
            at += main + between;
            let next = self.nodes[c as usize].next;
            self.arrange(c, rect, text, covered);
            c = next;
        }
    }

    /// The natural size along the parent's axis of a child that doesn't grow, kept in `main`
    /// for pass 2.
    fn child_main(
        &mut self,
        c: u32,
        axis: Axis,
        align: Align,
        inner: Rect,
        text: &mut dyn TextSystem,
    ) -> f32 {
        let main = match axis {
            Axis::Row => self.measure(c, inner.w, text).w,
            Axis::Column => {
                // Height for the width the child will get across the column.
                let w = self.cross_width(c, align, inner.w, text);
                self.measure(c, w, text).h
            }
        };
        self.nodes[c as usize].main = main;
        main
    }

    /// A child's size across the parent's axis, given its final `main` size.
    fn child_cross(
        &mut self,
        c: u32,
        axis: Axis,
        align: Align,
        main: f32,
        inner_cross: f32,
        text: &mut dyn TextSystem,
    ) -> f32 {
        match axis {
            Axis::Column => self.cross_width(c, align, inner_cross, text),
            Axis::Row => {
                let s = self.nodes[c as usize].style;
                if align == Align::Stretch && s.height == Dim::Auto {
                    clamp(inner_cross, s.min.h, s.max.h)
                } else {
                    self.measure(c, main, text).h.min(inner_cross.max(s.min.h))
                }
            }
        }
    }

    /// The width a child gets in a column `inner_w` wide.
    fn cross_width(
        &mut self,
        c: u32,
        align: Align,
        inner_w: f32,
        text: &mut dyn TextSystem,
    ) -> f32 {
        let s = self.nodes[c as usize].style;
        if align == Align::Stretch && s.width == Dim::Auto {
            clamp(inner_w, s.min.w, s.max.w)
        } else {
            self.measure(c, inner_w, text).w.min(inner_w.max(s.min.w))
        }
    }
}

fn fixed(d: Dim) -> Option<f32> {
    match d {
        Dim::Auto => None,
        Dim::Fixed(v) => Some(v),
    }
}

/// `(main, cross)` of `size` for `axis`.
fn along(axis: Axis, size: Size) -> (f32, f32) {
    match axis {
        Axis::Row => (size.w, size.h),
        Axis::Column => (size.h, size.w),
    }
}

impl crate::style::Style {
    fn min_main(&self, axis: Axis) -> f32 {
        along(axis, self.min).0
    }

    fn max_main(&self, axis: Axis) -> f32 {
        along(axis, self.max).0
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Align, Axis, Dim, Element, FakeText, Insets, Justify, Key, Motion, Rect, RectPx, Size,
        Style, TextStyle, Ui,
    };

    /// 10 px text: 5 px per character, 12.5 px lines.
    const SMALL: TextStyle = TextStyle {
        size: 10.0,
        weight: 400,
        mono: false,
        wrap: false,
    };

    fn ui(w: f32, h: f32) -> Ui<()> {
        Ui::new(Size { w, h }, 1.0, Motion::default())
    }

    fn rect(ui: &Ui<()>, path: &[u64]) -> Rect {
        let keys: Vec<Key> = path.iter().map(|&k| Key(k)).collect();
        ui.info(ui.find(&keys).unwrap()).unwrap().rect
    }

    fn fixed(w: f32, h: f32) -> Style {
        Style {
            width: Dim::Fixed(w),
            height: Dim::Fixed(h),
            ..Style::default()
        }
    }

    #[test]
    fn columns_stack_with_gap_and_padding_and_stretch_across() {
        let mut ui = ui(200.0, 100.0);
        ui.build(|cx| {
            let style = Style {
                padding: Insets::all(10.0),
                gap: 5.0,
                ..Style::default()
            };
            cx.add_with(Element::stack(Key(1)).style(style), |cx| {
                cx.add(Element::text(Key(2), "abcd").text_style(SMALL));
                cx.add(Element::stack(Key(3)).style(fixed(50.0, 20.0)));
            });
        });
        ui.layout(&mut FakeText);
        assert_eq!(rect(&ui, &[1]), Rect::new(0.0, 0.0, 200.0, 57.5));
        assert_eq!(rect(&ui, &[1, 2]), Rect::new(10.0, 10.0, 180.0, 12.5));
        // A fixed width isn't stretched.
        assert_eq!(rect(&ui, &[1, 3]), Rect::new(10.0, 27.5, 50.0, 20.0));
    }

    #[test]
    fn rows_share_free_space_by_grow_within_min_and_max() {
        let mut ui = ui(300.0, 50.0);
        let row = Style {
            axis: Axis::Row,
            ..Style::default()
        };
        ui.build(|cx| {
            cx.add_with(Element::stack(Key(1)).style(row), |cx| {
                cx.add(Element::stack(Key(2)).style(fixed(60.0, 10.0)));
                cx.add(Element::stack(Key(3)).style(Style {
                    grow: 1.0,
                    ..Style::default()
                }));
                cx.add(Element::stack(Key(4)).style(Style {
                    grow: 2.0,
                    max: Size {
                        w: 100.0,
                        h: f32::INFINITY,
                    },
                    ..Style::default()
                }));
            });
        });
        ui.layout(&mut FakeText);
        // 240 px free: 80 and 160, the second capped at 100. Stretched to the row's height,
        // which is its tallest child's.
        assert_eq!(rect(&ui, &[1, 2]), Rect::new(0.0, 0.0, 60.0, 10.0));
        assert_eq!(rect(&ui, &[1, 3]), Rect::new(60.0, 0.0, 80.0, 10.0));
        assert_eq!(rect(&ui, &[1, 4]), Rect::new(140.0, 0.0, 100.0, 10.0));
    }

    #[test]
    fn justify_and_align_place_children() {
        let mut ui = ui(100.0, 40.0);
        let style = |justify| Style {
            axis: Axis::Row,
            justify,
            align: Align::Center,
            height: Dim::Fixed(40.0),
            ..Style::default()
        };
        for (justify, xs) in [
            (Justify::Start, [0.0, 10.0, 20.0]),
            (Justify::Center, [35.0, 45.0, 55.0]),
            (Justify::End, [70.0, 80.0, 90.0]),
            (Justify::SpaceBetween, [0.0, 45.0, 90.0]),
        ] {
            ui.build(|cx| {
                cx.add_with(Element::stack(Key(1)).style(style(justify)), |cx| {
                    for k in 2..5 {
                        cx.add(Element::stack(Key(k)).style(fixed(10.0, 10.0)));
                    }
                });
            });
            ui.layout(&mut FakeText);
            for (k, x) in (2..5).zip(xs) {
                assert_eq!(
                    rect(&ui, &[1, k]),
                    Rect::new(x, 15.0, 10.0, 10.0),
                    "{justify:?}"
                );
            }
        }
    }

    #[test]
    fn wrapping_text_is_as_tall_as_the_width_it_gets() {
        let mut ui = ui(60.0, 100.0);
        let wrap = TextStyle {
            wrap: true,
            ..SMALL
        };
        ui.build(|cx| {
            // 3 words of 5 characters: 25 px each, 5 px spaces; two fit in 60 px.
            cx.add(Element::text(Key(1), "aaaaa bbbbb ccccc").text_style(wrap));
        });
        ui.layout(&mut FakeText);
        assert_eq!(rect(&ui, &[1]), Rect::new(0.0, 0.0, 60.0, 25.0));
        ui.set_viewport(Size { w: 100.0, h: 100.0 });
        ui.layout(&mut FakeText);
        assert_eq!(rect(&ui, &[1]), Rect::new(0.0, 0.0, 100.0, 12.5));
    }

    #[test]
    fn an_unchanged_frame_lays_out_and_damages_nothing() {
        let mut ui = ui(100.0, 100.0);
        let page = |ui: &mut Ui<()>, second: &str| {
            ui.build(|cx| {
                cx.add_with(Element::stack(Key(1)), |cx| {
                    cx.add(Element::text(Key(2), "first").text_style(SMALL));
                    cx.add(Element::text(Key(3), second).text_style(SMALL));
                    cx.add(Element::text(Key(4), "third").text_style(SMALL));
                });
            });
            ui.layout(&mut FakeText);
        };
        page(&mut ui, "second");
        ui.clear_damage();
        page(&mut ui, "second");
        assert_eq!(ui.damage(), [] as [RectPx; 0]);
        assert!(ui.nodes.iter().all(|n| !n.dirty));

        // New text, same size: only that line repaints.
        page(&mut ui, "SECOND");
        assert_eq!(
            ui.damage(),
            [RectPx {
                x0: 0,
                y0: 12,
                x1: 100,
                y1: 25
            }]
        );
    }

    #[test]
    fn moved_and_removed_nodes_damage_old_and_new_places() {
        let mut ui = ui(100.0, 100.0);
        let page = |ui: &mut Ui<()>, first: bool| {
            ui.build(|cx| {
                if first {
                    cx.add(Element::stack(Key(1)).style(fixed(10.0, 10.0)));
                }
                cx.add(Element::stack(Key(2)).style(fixed(10.0, 10.0)));
            });
            ui.layout(&mut FakeText);
        };
        page(&mut ui, true);
        ui.clear_damage();
        page(&mut ui, false);
        let px = |y0, y1| RectPx {
            x0: 0,
            y0,
            x1: 10,
            y1,
        };
        // Key 1's old place (also Key 2's new one), and Key 2's old place.
        let mut damage = ui.damage().to_vec();
        damage.sort_by_key(|r| r.y0);
        assert_eq!(damage, [px(0, 10), px(10, 20)]);
    }

    #[test]
    fn damage_is_in_physical_pixels_and_clipped_to_the_window() {
        let mut ui = Ui::<()>::new(Size { w: 100.0, h: 100.0 }, 1.5, Motion::default());
        assert_eq!(
            ui.damage(),
            [RectPx {
                x0: 0,
                y0: 0,
                x1: 150,
                y1: 150
            }]
        );
        ui.build(|cx| {
            cx.add(Element::stack(Key(1)).style(fixed(10.0, 200.0)));
        });
        ui.layout(&mut FakeText);
        ui.clear_damage();
        ui.build(|cx| {
            cx.add(Element::stack(Key(1)).style(fixed(20.0, 200.0)));
        });
        ui.layout(&mut FakeText);
        assert_eq!(
            ui.damage(),
            [RectPx {
                x0: 0,
                y0: 0,
                x1: 30,
                y1: 150
            }]
        );
    }

    #[test]
    fn growing_children_share_the_space_whatever_their_content() {
        let mut ui = ui(100.0, 100.0);
        let wrap = TextStyle {
            wrap: true,
            ..SMALL
        };
        let grow = Style {
            grow: 1.0,
            ..Style::default()
        };
        let row = Style {
            axis: Axis::Row,
            align: Align::Start,
            ..Style::default()
        };
        ui.build(|cx| {
            cx.add_with(Element::stack(Key(1)).style(row), |cx| {
                cx.add(Element::stack(Key(2)).style(fixed(40.0, 10.0)));
                // Words of 25 px and 5 px spaces: two words fit in the 60 px left.
                cx.add(
                    Element::text(Key(3), "aaaaa bbbbb ccccc")
                        .text_style(wrap)
                        .style(grow),
                );
            });
            cx.add_with(Element::stack(Key(4)).style(row), |cx| {
                cx.add(
                    Element::text(Key(5), "aaaaaaaaaaaaaaaaaaaaaaaa")
                        .text_style(SMALL)
                        .style(grow),
                );
                cx.add(Element::text(Key(6), "b").text_style(SMALL).style(grow));
            });
        });
        ui.layout(&mut FakeText);
        // The row is as tall as the text wrapped at the width it gets.
        assert_eq!(rect(&ui, &[1]), Rect::new(0.0, 0.0, 100.0, 25.0));
        assert_eq!(rect(&ui, &[1, 3]), Rect::new(40.0, 0.0, 60.0, 25.0));
        // Equal shares: the long text is trimmed, the short one gets room it doesn't need.
        assert_eq!(rect(&ui, &[4, 5]), Rect::new(0.0, 25.0, 50.0, 12.5));
        assert_eq!(rect(&ui, &[4, 6]), Rect::new(50.0, 25.0, 50.0, 12.5));
    }

    /// Counts measurements.
    struct Counting(u32);

    impl crate::TextSystem for Counting {
        fn measure(&mut self, text: &str, style: &TextStyle, max_width: f32) -> crate::TextMetrics {
            self.0 += 1;
            FakeText.measure(text, style, max_width)
        }

        fn draw(&mut self, _: &str, _: &TextStyle, _: Rect, _: f32, _: &mut dyn crate::GlyphSink) {}

        fn icon(&mut self, _: crate::Icon, _: Rect, _: f32, _: &mut dyn crate::GlyphSink) {}

        fn caret_stops(&mut self, text: &str, style: &TextStyle, out: &mut Vec<crate::CaretStop>) {
            FakeText.caret_stops(text, style, out);
        }
    }

    #[test]
    fn only_what_changed_is_measured_again() {
        let mut ui = ui(100.0, 100.0);
        let page = |ui: &mut Ui<()>, second: &str, text: &mut Counting| {
            ui.build(|cx| {
                cx.add(Element::text(Key(1), "first"));
                cx.add(Element::text(Key(2), second));
                cx.add(Element::text(Key(3), "third"));
                // Fixed in both directions: its text can't change its size.
                cx.add(Element::button(Key(4), "fixed", ()).style(fixed(50.0, 20.0)));
            });
            ui.layout(text);
        };
        let mut text = Counting(0);
        page(&mut ui, "second", &mut text);
        assert_eq!(text.0, 3);
        page(&mut ui, "second", &mut text);
        assert_eq!(text.0, 3);
        page(&mut ui, "changed", &mut text);
        assert_eq!(text.0, 4);
    }
}
