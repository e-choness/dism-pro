//! A wrapping row (TD-0009 A3.5 addition 3, A3.9): children go on lines, each child's `min.w`
//! its basis; on each line the growing children split what's left; a child wider than the row
//! gets a line of its own, clamped to the row; lines start at the logical start, and a line's
//! children follow `justify`. A row that doesn't wrap lays out as before. (No right-to-left case:
//! A7.2.)

#![allow(missing_docs)]

use dpro_ui::{
    Align, Axis, Element, FakeText, Insets, Justify, Key, Motion, Rect, Size, Style, Ui, tokens,
};

const ROW: Key = Key::name("row");

fn ui(w: f32) -> Ui<()> {
    Ui::new(
        Size { w, h: 600.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    )
}

/// A wrapping row with `gap` 10 and padding 5, at most `w` wide, of children `basis` wide (their
/// `min.w`) and 20 high, growing by `grow`; its children's boxes.
fn lay_out(w: f32, children: &[(f32, f32)], justify: Justify, wrap: bool) -> (Rect, Vec<Rect>) {
    let mut ui = ui(w);
    ui.build(|cx| {
        cx.add_with(
            Element::stack(ROW).style(Style {
                axis: Axis::Row,
                gap: 10.0,
                padding: Insets::all(5.0),
                align: Align::Start,
                justify,
                wrap,
                ..Style::default()
            }),
            |cx| {
                for (i, (basis, grow)) in (0..).zip(children) {
                    cx.add(Element::stack(Key::index(i)).style(Style {
                        min: Size { w: *basis, h: 20.0 },
                        grow: *grow,
                        ..Style::default()
                    }));
                }
            },
        );
    });
    ui.layout(&mut FakeText);
    let row = ui.find(&[ROW]).and_then(|id| ui.info(id)).map(|n| n.rect);
    let kids = (0..)
        .take(children.len())
        .filter_map(|i| ui.find(&[ROW, Key::index(i)]).and_then(|id| ui.info(id)))
        .map(|n| n.rect)
        .collect();
    (row.unwrap_or_default(), kids)
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn a_row_that_doesnt_wrap_lays_out_on_one_line() {
    let (_, kids) = lay_out(300.0, &[(100.0, 0.0); 4], Justify::Start, false);
    assert!(kids.iter().all(|r| near(r.y, kids[0].y)), "{kids:?}");
}

#[test]
fn children_go_on_as_many_lines_as_they_need() {
    // 290 inside the padding: two 100s and a gap fit (210), a third doesn't (320).
    let (row, kids) = lay_out(300.0, &[(100.0, 0.0); 5], Justify::Start, true);
    let lines: Vec<f32> = kids.iter().map(|r| r.y).collect();
    assert!(near(lines[0], 5.0) && near(lines[1], 5.0), "{kids:?}");
    assert!(
        near(lines[2], 5.0 + 20.0 + 10.0) && near(lines[3], lines[2]),
        "{kids:?}"
    );
    assert!(near(lines[4], 5.0 + 2.0 * 30.0), "{kids:?}");
    // Each line starts at the logical start; the gap applies between lines too.
    assert!(near(kids[0].x, 5.0) && near(kids[2].x, 5.0) && near(kids[4].x, 5.0));
    assert!(near(kids[1].x, 5.0 + 100.0 + 10.0));
    assert!(near(row.h, 3.0 * 20.0 + 2.0 * 10.0 + 10.0), "{row:?}");
}

#[test]
fn the_growing_children_split_what_is_left_on_their_line() {
    // Line 1: 100 (grow 1) and 100 (grow 3) in 290: 80 left, 20 and 60.
    // Line 2: 100 (grow 0) alone: stays 100.
    let (_, kids) = lay_out(
        300.0,
        &[(100.0, 1.0), (100.0, 3.0), (100.0, 0.0)],
        Justify::Start,
        true,
    );
    assert!(near(kids[0].w, 120.0) && near(kids[1].w, 160.0), "{kids:?}");
    assert!(near(kids[1].x, 5.0 + 120.0 + 10.0), "{kids:?}");
    assert!(near(kids[2].w, 100.0) && near(kids[2].x, 5.0), "{kids:?}");
}

#[test]
fn a_child_wider_than_the_row_gets_a_line_of_its_own_clamped_to_the_row() {
    let (_, kids) = lay_out(
        300.0,
        &[(50.0, 0.0), (500.0, 0.0), (50.0, 0.0)],
        Justify::Start,
        true,
    );
    assert!(near(kids[0].y, 5.0));
    assert!(kids[1].y > kids[0].y && near(kids[1].x, 5.0), "{kids:?}");
    assert!(kids[1].w <= 290.0 + 0.01, "clamped to the row: {kids:?}");
    assert!(
        kids[2].y > kids[1].y,
        "the next child starts a new line: {kids:?}"
    );
}

#[test]
fn each_lines_children_follow_justify() {
    let (_, kids) = lay_out(300.0, &[(100.0, 0.0); 3], Justify::End, true);
    // Line 1 ends at the row's end (295), line 2 too.
    assert!(near(kids[1].x + kids[1].w, 295.0), "{kids:?}");
    assert!(near(kids[2].x + kids[2].w, 295.0), "{kids:?}");
}

#[test]
fn a_resize_rewraps() {
    let wide = lay_out(1000.0, &[(220.0, 1.0); 4], Justify::Start, true).1;
    assert!(
        wide.iter().all(|r| near(r.y, wide[0].y)),
        "one line: {wide:?}"
    );
    let narrow = lay_out(500.0, &[(220.0, 1.0); 4], Justify::Start, true).1;
    assert!(
        near(narrow[0].y, narrow[1].y) && narrow[2].y > narrow[0].y,
        "{narrow:?}"
    );
}
