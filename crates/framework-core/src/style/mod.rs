//! Theme and style resolution.
//!
//! - `theme`: the style model itself — colors, typography, per-kind and
//!   per-state defaults, and the merge that resolves them.
//! - `phase`: the two *phases* a node's styling passes through
//!   ([`StyleOverride`] before resolution, [`ResolvedStyle`] after), kept as
//!   distinct types so a backend cannot silently paint from the wrong one.

mod phase;
mod theme;

pub use phase::{ResolvedStyle, StyleOverride};
pub use theme::{Color, ComponentStyle, ControlState, Theme, Typography, VisualStyle};
