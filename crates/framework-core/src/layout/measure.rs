//! Platform-supplied intrinsic (content-driven) measurement.
//!
//! See `crate::layout::engine`'s module doc for why this file's `i32`/
//! `usize`/`u32` casts are allowed at the module level: every cast here is
//! likewise preceded by an explicit `.min`/`.max` establishing the value is
//! in range, which this module's own
//! `measurement_never_panics_on_pathologically_large_input` test exercises
//! directly.
#![allow(clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::cast_possible_truncation)]

use super::geometry::Size;
use crate::node::NodeKind;

/// Supplies platform-specific intrinsic measurements for leaf nodes (how
/// large a label/button/text-input's own content wants to be, independent of
/// the space a parent container happens to offer it).
pub trait IntrinsicMeasurer {
    fn measure(&self, kind: NodeKind, text: Option<&str>, max_width: Option<i32>) -> Size;
}

/// A platform-independent measurement heuristic used by tests and any
/// backend that has not yet supplied its own native text metrics.
#[derive(Debug, Default)]
pub struct DefaultIntrinsicMeasurer;

impl IntrinsicMeasurer for DefaultIntrinsicMeasurer {
    fn measure(&self, kind: NodeKind, text: Option<&str>, max_width: Option<i32>) -> Size {
        // Every arithmetic step here is saturating: this measurer sits
        // directly on data an application can make arbitrarily large (an
        // enormous string, `LayoutStyle::constraints` set to `i32::MAX`),
        // and layout must degrade to a clamped value rather than panic
        // (debug builds) or silently wrap (release builds) on such input —
        // see the standards audit's P1.19 finding.
        let characters =
            text.map_or(0, |value| value.chars().count()).min(i32::MAX as usize) as i32;
        let intrinsic_width = characters
            .saturating_mul(8)
            .saturating_add(match kind {
                NodeKind::Button | NodeKind::TextInput => 24,
                NodeKind::Label | NodeKind::Column | NodeKind::Row => 0,
            })
            .max(1);
        let available_width = max_width.unwrap_or(intrinsic_width).max(1);
        let lines = intrinsic_width
            .saturating_add(available_width.saturating_sub(1))
            .saturating_div(available_width)
            .max(1);
        let width = intrinsic_width.min(available_width);

        Size::new(width as u32, lines.saturating_mul(32) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurement_never_panics_on_pathologically_large_input() {
        let measurer = DefaultIntrinsicMeasurer;
        let huge_text = "x".repeat(1_000_000);
        let size = measurer.measure(NodeKind::Label, Some(&huge_text), Some(i32::MAX));
        assert!(size.width > 0);
        assert!(size.height > 0);

        let size = measurer.measure(NodeKind::Label, Some(&huge_text), Some(1));
        assert!(size.width > 0);
        assert!(size.height > 0);
    }
}
