//! Plain geometric and layout-direction value types.
//!
//! Defined in the `no_std` crate `framework-types` so targets without an
//! operating system can share them, and re-exported here at their
//! historical paths.

pub use framework_types::geometry::{Alignment, EdgeInsets, Overflow, Point, Rect, Size, SizeMode};
