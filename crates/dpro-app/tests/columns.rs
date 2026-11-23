//! TD-0009 A2.4 and A2.5, slice 5b2: the columns as the user arranges them, written before the
//! code.
//!
//! - Each of the page's columns has a width: its own track ([`Width::Auto`]) until a divider's drag
//!   or a fit gives it pixels, or [`Width::Fit`] until the table has measured a fit.
//! - A column the page marks hidden at first starts hidden. The column menu shows and hides them,
//!   but never hides the last one shown.
//! - Size all columns to fit sizes the shown columns only. Reset columns brings back every width
//!   and the columns the page shows at first.
//! - Widths and hidden columns are in the page's column order, as the table takes them, and stay
//!   in memory until settings persist (roadmap 3.2).

#![allow(missing_docs)]

use dpro_app::table::{ColumnId, ColumnLayout, Width};

const NAME: ColumnId = ColumnId(0);
const VERSION: ColumnId = ColumnId(1);
const SIZE: ColumnId = ColumnId(2);
const ID: ColumnId = ColumnId(7);

/// Name, Version and Size shown; ID hidden until the user shows it.
fn layout() -> ColumnLayout {
    ColumnLayout::new(&[(NAME, false), (VERSION, false), (SIZE, false), (ID, true)])
}

#[test]
fn every_column_starts_at_its_own_width_and_the_page_s_hidden_ones_hidden() {
    let l = layout();
    assert_eq!(l.widths(), &[Width::Auto; 4]);
    assert_eq!(l.hidden(), &[false, false, false, true]);
    assert_eq!(l.width(SIZE), Width::Auto);
    assert!(l.is_hidden(ID));
    assert!(!l.is_hidden(NAME));
}

#[test]
fn a_resize_gives_a_column_pixels_and_says_whether_it_changed() {
    let mut l = layout();
    assert!(l.resize(VERSION, 112.5));
    assert_eq!(l.width(VERSION), Width::Px(112.5));
    assert_eq!(
        l.widths(),
        &[Width::Auto, Width::Px(112.5), Width::Auto, Width::Auto]
    );
    assert!(!l.resize(VERSION, 112.5), "the same width again");
    assert!(l.resize(VERSION, 90.0));
    assert_eq!(l.width(VERSION), Width::Px(90.0));
}

#[test]
fn a_width_that_isn_t_one_and_a_column_that_isn_t_there_change_nothing() {
    let mut l = layout();
    for px in [f32::NAN, f32::INFINITY, -1.0] {
        assert!(!l.resize(SIZE, px), "{px}");
    }
    assert_eq!(l.width(SIZE), Width::Auto);
    let other = ColumnId(42);
    assert!(!l.resize(other, 100.0));
    assert!(!l.set_hidden(other, true));
    assert!(!l.fit(Some(other)));
    assert_eq!(l.width(other), Width::Auto);
    assert!(!l.is_hidden(other));
    assert_eq!(l, layout());
}

#[test]
fn fit_asks_the_table_to_measure_one_column_or_every_shown_one() {
    let mut l = layout();
    assert!(l.fit(Some(SIZE)));
    assert_eq!(l.width(SIZE), Width::Fit);
    assert!(!l.fit(Some(SIZE)), "already asked");
    // The table's answer.
    assert!(l.resize(SIZE, 64.0));
    assert_eq!(l.width(SIZE), Width::Px(64.0));

    assert!(l.fit(None));
    assert_eq!(
        l.widths(),
        &[Width::Fit, Width::Fit, Width::Fit, Width::Auto],
        "the hidden ID isn't measured"
    );
}

#[test]
fn the_menu_shows_and_hides_columns_but_never_the_last_one_shown() {
    let mut l = layout();
    assert!(l.set_hidden(ID, false));
    assert_eq!(l.hidden(), &[false; 4]);
    assert!(!l.set_hidden(ID, false), "shown already");
    for id in [NAME, VERSION, SIZE] {
        assert!(l.set_hidden(id, true));
    }
    assert!(!l.set_hidden(ID, true), "the last one shown stays");
    assert_eq!(l.hidden(), &[true, true, true, false]);
    assert!(l.set_hidden(SIZE, false));
    assert!(l.set_hidden(ID, true), "now another one shows");
    assert_eq!(l.hidden(), &[true, true, false, true]);
}

#[test]
fn a_hidden_column_keeps_its_width() {
    let mut l = layout();
    assert!(l.resize(VERSION, 150.0));
    assert!(l.set_hidden(VERSION, true));
    assert!(l.set_hidden(VERSION, false));
    assert_eq!(l.width(VERSION), Width::Px(150.0));
}

#[test]
fn reset_brings_back_every_width_and_the_page_s_columns() {
    let mut l = layout();
    assert!(!l.reset(), "nothing to reset");
    assert!(l.resize(NAME, 300.0));
    assert!(l.fit(Some(SIZE)));
    assert!(l.set_hidden(ID, false));
    assert!(l.set_hidden(VERSION, true));
    assert!(l.reset());
    assert_eq!(l, layout());
}

#[test]
fn a_page_that_hides_every_column_at_first_still_shows_its_first() {
    // Found in review: the table needs a column, and reset brought back none.
    let mut l = ColumnLayout::new(&[(NAME, true), (VERSION, true)]);
    assert_eq!(l.hidden(), &[false, true]);
    assert!(l.set_hidden(VERSION, false));
    assert!(l.set_hidden(NAME, true));
    assert!(l.reset());
    assert_eq!(l.hidden(), &[false, true], "reset shows one too");
}
