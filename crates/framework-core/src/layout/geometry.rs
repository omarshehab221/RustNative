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
    pub width: u32,
    pub height: u32,
}

impl Size {
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// A rectangle in a native parent's local coordinate space (see
/// `crate::layout::LayoutEngine` for the coordinate-space contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }
}

/// How a node's width or height along one axis is determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SizeMode {
    #[default]
    Auto,
    Fixed(i32),
    Fill,
}

/// Cross-axis (or `align_self`) alignment within the space a node is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Alignment {
    Start,
    Center,
    End,
    #[default]
    Stretch,
}

/// A position in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// How a container handles content that exceeds its bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overflow {
    Visible,
    #[default]
    Clip,
    Scroll,
}

/// Independent top/right/bottom/left insets (padding or margin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeInsets {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

impl EdgeInsets {
    #[must_use]
    pub const fn all(value: i32) -> Self {
        Self { top: value, right: value, bottom: value, left: value }
    }

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
