//! Per-node layout parameters: size constraints and container styling.

use super::geometry::{Alignment, EdgeInsets, Overflow, SizeMode};

/// Min/max size bounds for a node's layout.
///
/// Fields are private and every constructor/setter enforces
/// `0 <= min <= max` (when a max is set), so an invalid `Constraints` (for
/// example `max_width < min_width`) can never be constructed at all, rather
/// than being silently re-clamped at every place a value is measured against
/// it. This is the model this crate follows wherever a type's fields carry a
/// real invariant — contrast with the plain data carriers in
/// `crate::layout::geometry`, which have none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Constraints {
    min_width: i32,
    max_width: Option<i32>,
    min_height: i32,
    max_height: Option<i32>,
}

impl Constraints {
    #[must_use]
    pub const fn new() -> Self {
        Self { min_width: 0, max_width: None, min_height: 0, max_height: None }
    }

    #[must_use]
    pub const fn with_min_width(mut self, value: i32) -> Self {
        self.min_width = non_negative(value);
        self.max_width = raise_to(self.max_width, self.min_width);
        self
    }

    #[must_use]
    pub const fn with_max_width(mut self, value: i32) -> Self {
        let value = non_negative(value);
        self.max_width = Some(if value > self.min_width { value } else { self.min_width });
        self
    }

    #[must_use]
    pub const fn with_min_height(mut self, value: i32) -> Self {
        self.min_height = non_negative(value);
        self.max_height = raise_to(self.max_height, self.min_height);
        self
    }

    #[must_use]
    pub const fn with_max_height(mut self, value: i32) -> Self {
        let value = non_negative(value);
        self.max_height = Some(if value > self.min_height { value } else { self.min_height });
        self
    }

    #[must_use]
    pub const fn min_width(&self) -> i32 {
        self.min_width
    }
    #[must_use]
    pub const fn max_width(&self) -> Option<i32> {
        self.max_width
    }
    #[must_use]
    pub const fn min_height(&self) -> i32 {
        self.min_height
    }
    #[must_use]
    pub const fn max_height(&self) -> Option<i32> {
        self.max_height
    }

    #[must_use]
    pub const fn clamp_width(self, value: i32) -> i32 {
        let value = if value < self.min_width { self.min_width } else { value };
        match self.max_width {
            // Invariant established by the constructors above:
            // `max_width >= min_width` always holds here, so no defensive
            // re-clamp of `max` against `min` is needed at this use site.
            Some(max) if value > max => max,
            _ => value,
        }
    }

    #[must_use]
    pub const fn clamp_height(self, value: i32) -> i32 {
        let value = if value < self.min_height { self.min_height } else { value };
        match self.max_height {
            Some(max) if value > max => max,
            _ => value,
        }
    }
}

const fn non_negative(value: i32) -> i32 {
    if value > 0 { value } else { 0 }
}

const fn raise_to(max: Option<i32>, min: i32) -> Option<i32> {
    match max {
        Some(max) if max < min => Some(min),
        other => other,
    }
}

/// A node's own sizing, margin, alignment, and constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutStyle {
    pub width: SizeMode,
    pub height: SizeMode,
    pub margin: EdgeInsets,
    pub align_self: Option<Alignment>,
    pub constraints: Constraints,
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            width: SizeMode::Fill,
            height: SizeMode::Auto,
            margin: EdgeInsets::default(),
            align_self: None,
            constraints: Constraints::default(),
        }
    }
}

impl LayoutStyle {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            width: SizeMode::Fill,
            height: SizeMode::Auto,
            margin: EdgeInsets { top: 0, right: 0, bottom: 0, left: 0 },
            align_self: None,
            constraints: Constraints::new(),
        }
    }

    #[must_use]
    pub const fn width(mut self, width: SizeMode) -> Self {
        self.width = width;
        self
    }

    #[must_use]
    pub const fn height(mut self, height: SizeMode) -> Self {
        self.height = height;
        self
    }

    #[must_use]
    pub const fn margin(mut self, margin: EdgeInsets) -> Self {
        self.margin = margin;
        self
    }

    #[must_use]
    pub const fn align_self(mut self, alignment: Alignment) -> Self {
        self.align_self = Some(alignment);
        self
    }

    #[must_use]
    pub const fn constraints(mut self, constraints: Constraints) -> Self {
        self.constraints = constraints;
        self
    }
}

/// A macro-free way to keep [`ColumnStyle`] and [`RowStyle`] — the two
/// axis-specific container styles — from drifting apart, without going as
/// far as merging them into one type and losing the "this is a column vs.
/// this is a row" distinction the rest of the layout/reconciliation code
/// relies on ([`crate::node::Node::column`] vs.
/// [`crate::node::Node::row`]).
macro_rules! container_style {
    ($name:ident) => {
        /// Padding, gap, and cross-axis alignment for a container node along
        /// one axis (see [`ColumnStyle`]/[`RowStyle`]).
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name {
            pub padding: EdgeInsets,
            pub gap: i32,
            pub align_items: Alignment,
            pub overflow: Overflow,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    padding: EdgeInsets::all(24),
                    gap: 12,
                    align_items: Alignment::Stretch,
                    overflow: Overflow::Clip,
                }
            }
        }

        impl $name {
            pub const fn new() -> Self {
                Self {
                    padding: EdgeInsets { top: 24, right: 24, bottom: 24, left: 24 },
                    gap: 12,
                    align_items: Alignment::Stretch,
                    overflow: Overflow::Clip,
                }
            }

            #[must_use]
            pub const fn padding(mut self, padding: EdgeInsets) -> Self {
                self.padding = padding;
                self
            }

            #[must_use]
            pub const fn gap(mut self, gap: i32) -> Self {
                self.gap = gap;
                self
            }

            #[must_use]
            pub const fn align_items(mut self, alignment: Alignment) -> Self {
                self.align_items = alignment;
                self
            }

            #[must_use]
            pub const fn overflow(mut self, overflow: Overflow) -> Self {
                self.overflow = overflow;
                self
            }
        }
    };
}

container_style!(ColumnStyle);
container_style!(RowStyle);

/// The resolved container layout parameters the layout engine's
/// axis-generic helpers need. Grouping `padding`/`gap`/`align_items` here
/// (rather than passing each positionally) removes an argument-order
/// footgun between two same-typed `i32`/`Alignment` parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedContainerStyle {
    pub(crate) padding: EdgeInsets,
    pub(crate) gap: i32,
    pub(crate) align_items: Alignment,
}

impl From<ColumnStyle> for ResolvedContainerStyle {
    fn from(style: ColumnStyle) -> Self {
        Self { padding: style.padding, gap: style.gap, align_items: style.align_items }
    }
}

impl From<RowStyle> for ResolvedContainerStyle {
    fn from(style: RowStyle) -> Self {
        Self { padding: style.padding, gap: style.gap, align_items: style.align_items }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraints_cannot_represent_max_below_min() {
        let constraints = Constraints::new().with_min_width(100).with_max_width(10);
        assert_eq!(constraints.min_width(), 100);
        assert_eq!(constraints.max_width(), Some(100));

        let constraints = Constraints::new().with_max_width(10).with_min_width(100);
        assert_eq!(constraints.max_width(), Some(100));
    }

    #[test]
    fn constraints_reject_negative_bounds() {
        let constraints = Constraints::new().with_min_width(-5).with_max_height(-1);
        assert_eq!(constraints.min_width(), 0);
        assert_eq!(constraints.max_height(), Some(0));
    }
}
