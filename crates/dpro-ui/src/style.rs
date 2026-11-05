//! Layout style: a flexbox subset (TD-0009 §5.1). Rows and columns with gap, padding, cross-axis
//! alignment, main-axis justification and grow; fixed sizes; min/max constraints. Grid tracks and
//! wrapping come with the data table and card grids.

use crate::geom::{Insets, Size};

/// The direction children are laid out in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Axis {
    Row,
    #[default]
    Column,
}

/// Where children sit across the axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    /// Children without a fixed cross size fill the cross axis.
    #[default]
    Stretch,
}

/// How free space along the axis is shared when no child grows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
}

/// A box dimension.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Dim {
    /// From the content.
    #[default]
    Auto,
    /// Logical pixels.
    Fixed(f32),
}

/// How a node and its children are laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub axis: Axis,
    /// Space between children along the axis.
    pub gap: f32,
    pub padding: Insets,
    pub align: Align,
    pub justify: Justify,
    /// This node's share of its parent's free space along the parent's axis.
    pub grow: f32,
    pub width: Dim,
    pub height: Dim,
    pub min: Size,
    pub max: Size,
}

impl Default for Style {
    /// A column that stretches its children, with no gap, padding or constraints.
    fn default() -> Self {
        Self {
            axis: Axis::Column,
            gap: 0.0,
            padding: Insets::default(),
            align: Align::Stretch,
            justify: Justify::Start,
            grow: 0.0,
            width: Dim::Auto,
            height: Dim::Auto,
            min: Size::ZERO,
            max: Size::INFINITE,
        }
    }
}

/// `v` within `[min, max]`; `min` wins when they cross, as in CSS.
pub(crate) fn clamp(v: f32, min: f32, max: f32) -> f32 {
    v.min(max).max(min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamping_lets_min_win() {
        assert_eq!(clamp(5.0, 0.0, 3.0), 3.0);
        assert_eq!(clamp(-1.0, 0.0, 3.0), 0.0);
        assert_eq!(clamp(5.0, 4.0, 3.0), 4.0);
        assert_eq!(clamp(f32::INFINITY, 0.0, f32::INFINITY), f32::INFINITY);
    }
}
