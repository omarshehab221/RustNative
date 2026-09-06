//! Plain geometric and layout-direction value types.
//!
//! These are intentionally simple public-field data carriers rather than
//! encapsulated types with accessor methods: they have no invariants beyond
//! "these are numbers", every arithmetic consumer (the layout engine, both
//! crates' tests, a platform backend translating a `Rect` into a native
//! placement call) needs unrestricted field access, and getters here would
//! add call-site noise without protecting anything — the same reasoning the
//! Rust ecosystem generally applies to types like `std::ops::Range` or a
//! `Point { x, y }`. Contrast this with [`crate::layout::Constraints`] a few
//! modules over, whose fields *do* carry a real invariant (`min <= max`) and
//! are therefore private.

/// A size in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// The width, in logical pixels.
    pub width: u32,
    /// The height, in logical pixels.
    pub height: u32,
}

impl Size {
    /// Creates a size from its width and height.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// A rectangle in a native parent's local coordinate space (see
/// `crate::layout::LayoutEngine` for the coordinate-space contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// The x-coordinate of the rectangle's top-left corner.
    pub x: i32,
    /// The y-coordinate of the rectangle's top-left corner.
    pub y: i32,
    /// The rectangle's width.
    pub width: i32,
    /// The rectangle's height.
    pub height: i32,
}

impl Rect {
    /// Creates a rectangle from its top-left corner and size.
    #[must_use]
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }
}

/// How a node's width or height along one axis is determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SizeMode {
    /// Sized to fit the node's content.
    #[default]
    Auto,
    /// Sized to an exact value, in logical pixels.
    Fixed(i32),
    /// Sized to fill the remaining space its parent offers.
    Fill,
}

/// Cross-axis (or `align_self`) alignment within the space a node is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Alignment {
    /// Aligned to the start of the cross axis.
    Start,
    /// Centered on the cross axis.
    Center,
    /// Aligned to the end of the cross axis.
    End,
    /// Stretched to fill the cross axis.
    #[default]
    Stretch,
}

/// A position in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {
    /// The x-coordinate.
    pub x: i32,
    /// The y-coordinate.
    pub y: i32,
}

impl Point {
    /// Creates a point from its coordinates.
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// How a container handles content that exceeds its bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overflow {
    /// Content is drawn even where it exceeds the container's bounds.
    Visible,
    /// Content beyond the container's bounds is clipped.
    #[default]
    Clip,
    /// Content beyond the container's bounds is reachable by scrolling.
    Scroll,
}

/// Independent top/right/bottom/left insets (padding or margin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeInsets {
    /// The top inset.
    pub top: i32,
    /// The right inset.
    pub right: i32,
    /// The bottom inset.
    pub bottom: i32,
    /// The left inset.
    pub left: i32,
}

impl EdgeInsets {
    /// Creates equal insets on all four edges.
    #[must_use]
    pub const fn all(value: i32) -> Self {
        Self { top: value, right: value, bottom: value, left: value }
    }

    /// Creates insets that are equal on the top/bottom edges and equal on
    /// the left/right edges.
    #[must_use]
    pub const fn symmetric(vertical: i32, horizontal: i32) -> Self {
        Self { top: vertical, right: horizontal, bottom: vertical, left: horizontal }
    }

    /// The sum of the left and right insets. Saturating: an adversarial or
    /// pathologically large edge-inset pair clamps instead of overflowing
    /// (see `crate::layout::engine` for why every layout computation makes
    /// this same choice).
    #[must_use]
    pub const fn horizontal(self) -> i32 {
        self.left.saturating_add(self.right)
    }

    /// The sum of the top and bottom insets. See [`Self::horizontal`].
    #[must_use]
    pub const fn vertical(self) -> i32 {
        self.top.saturating_add(self.bottom)
    }
}

impl Default for EdgeInsets {
    fn default() -> Self {
        Self::all(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_insets_sums_saturate_instead_of_overflowing() {
        let insets = EdgeInsets::all(i32::MAX);
        assert_eq!(insets.horizontal(), i32::MAX);
        assert_eq!(insets.vertical(), i32::MAX);
    }
}
