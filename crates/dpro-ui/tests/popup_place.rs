//! TD-0009 A1.9: where a popup goes ([`place`]), in screen pixels. It goes on its preferred side
//! of its anchor if it fits there, else on the other side if it fits there, else on the roomier
//! side; then it's moved inside the work area. It starts at the anchor's left edge, or ends at the
//! anchor's right edge if that overflows (so a point's menu opens leftward). A popup larger than
//! the work area starts at its top left. Monitors left of or above the primary one have negative
//! coordinates.

use dpro_ui::{Placement, RectPx, place};

const fn r(x0: i32, y0: i32, x1: i32, y1: i32) -> RectPx {
    RectPx { x0, y0, x1, y1 }
}

/// A 1920 × 1080 monitor less a 40 px taskbar.
const WORK: RectPx = r(0, 0, 1920, 1040);
const GAP: i32 = 4;

#[test]
fn it_goes_on_its_side_when_it_fits() {
    let anchor = r(300, 400, 420, 432);
    assert_eq!(
        place(anchor, 200, 180, WORK, Placement::Below, GAP),
        r(300, 436, 500, 616)
    );
    assert_eq!(
        place(anchor, 200, 180, WORK, Placement::Above, GAP),
        r(300, 216, 500, 396)
    );
}

#[test]
fn it_flips_when_its_side_has_no_room() {
    // Near the taskbar, a menu goes above its anchor.
    let low = r(300, 950, 420, 982);
    assert_eq!(
        place(low, 200, 180, WORK, Placement::Below, GAP),
        r(300, 766, 500, 946)
    );
    // Near the top, a tooltip goes below.
    let high = r(300, 20, 420, 52);
    assert_eq!(
        place(high, 200, 180, WORK, Placement::Above, GAP),
        r(300, 56, 500, 236)
    );
}

#[test]
fn neither_side_fits_so_it_takes_the_roomier_and_stays_inside() {
    let short = r(0, 0, 800, 300);
    // More room below (166 px) than above (96): below, moved up to fit.
    assert_eq!(
        place(
            r(100, 100, 200, 130),
            200,
            250,
            short,
            Placement::Below,
            GAP
        ),
        r(100, 50, 300, 300)
    );
    // More room above (196 px) than below (66): above, moved down to fit.
    assert_eq!(
        place(
            r(100, 200, 200, 230),
            200,
            250,
            short,
            Placement::Below,
            GAP
        ),
        r(100, 0, 300, 250)
    );
}

#[test]
fn overflowing_the_right_it_ends_at_the_anchor_s_right_edge() {
    assert_eq!(
        place(
            r(1800, 400, 1900, 432),
            200,
            180,
            WORK,
            Placement::Below,
            GAP
        ),
        r(1700, 436, 1900, 616)
    );
    // A right-click's point: the menu opens leftward from it.
    assert_eq!(
        place(
            r(1850, 500, 1850, 500),
            200,
            180,
            WORK,
            Placement::Below,
            GAP
        ),
        r(1650, 504, 1850, 684)
    );
    // At the left edge, a point's menu opens rightward.
    assert_eq!(
        place(r(0, 500, 0, 500), 200, 180, WORK, Placement::Below, GAP),
        r(0, 504, 200, 684)
    );
}

#[test]
fn a_popup_larger_than_the_work_area_starts_at_its_top_left() {
    assert_eq!(
        place(
            r(100, 100, 200, 130),
            2000,
            1100,
            WORK,
            Placement::Below,
            GAP
        ),
        r(0, 0, 2000, 1100)
    );
}

#[test]
fn monitors_with_negative_coordinates() {
    // A monitor left of the primary one, and higher.
    let left = r(-1920, -200, 0, 880);
    assert_eq!(
        place(
            r(-500, 300, -380, 332),
            200,
            180,
            left,
            Placement::Below,
            GAP
        ),
        r(-500, 336, -300, 516)
    );
    assert_eq!(
        place(
            r(-100, 300, -20, 332),
            200,
            180,
            left,
            Placement::Below,
            GAP
        ),
        r(-220, 336, -20, 516)
    );
    assert_eq!(
        place(
            r(-500, -190, -380, -160),
            200,
            180,
            left,
            Placement::Above,
            GAP
        ),
        r(-500, -156, -300, 24)
    );
}

#[test]
fn negative_sizes_and_gaps_count_as_zero() {
    assert_eq!(
        place(r(300, 400, 420, 432), -5, -5, WORK, Placement::Below, -3),
        r(300, 432, 300, 432)
    );
}
