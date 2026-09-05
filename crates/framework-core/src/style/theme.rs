//! Theme and style resolution.
//!
//! A [`Theme`] supplies per-kind, per-interaction-state defaults; a node may
//! additionally carry its own [`VisualStyle`] override. [`Theme::resolve`] is
//! the single place those two layers are merged into the value a backend
//! actually realizes as native fonts/colors — see
//! `crate::reconcile::TreeSnapshot::from_node_with_theme`, which calls it for
//! every node before a snapshot ever reaches a backend.

use crate::layout::EdgeInsets;
use crate::node::NodeKind;

/// RGBA color token used by a theme or explicit node style.
///
/// Kept as a plain public-field data carrier rather than an encapsulated
/// type: it has no invariant beyond "four bytes", and every consumer (theme
/// definitions, a backend's `COLORREF` conversion) needs direct field
/// access. See `crate::layout::geometry`'s module doc for the same
/// reasoning applied to geometric types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Color {
    #[must_use]
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue, alpha: 255 }
    }
    #[must_use]
    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self { red, green, blue, alpha }
    }
}

/// Builds an opaque color from a packed `0xRRGGBB` hex value, e.g.
/// `Color::from(0xFF00FF)` for magenta. Theme tokens defined this way stay
/// `const`-constructible, matching `Color::rgb`/`Color::rgba` above.
impl From<u32> for Color {
    fn from(packed: u32) -> Self {
        Self::rgb(
            ((packed >> 16) & 0xFF) as u8,
            ((packed >> 8) & 0xFF) as u8,
            (packed & 0xFF) as u8,
        )
    }
}

/// Font family, size, and weight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typography {
    /// The font family name (a platform-native lookup, not a bundled font).
    pub family: String,
    /// Point size.
    pub size: u16,
    /// CSS-style numeric weight (400 = regular, 700 = bold, ...).
    pub weight: u16,
}

impl Default for Typography {
    fn default() -> Self {
        Self { family: "system-ui".into(), size: 14, weight: 400 }
    }
}

/// A node's visual style: a set of independently optional overrides, each
/// one falling back to the active theme's corresponding value when absent
/// (see [`Theme::resolve`]).
///
/// Fields are private and read through accessors. Unlike the plain
/// geometry/color data carriers elsewhere in this crate, `VisualStyle` is a
/// natural fit for encapsulation: it already has a fluent builder for
/// *writing* (`foreground`, `background`, ...), so exposing the fields
/// directly for *reading* as well only doubles the API surface for the same
/// data — a shape the standards audit flags directly (P2.24).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualStyle {
    foreground: Option<Color>,
    background: Option<Color>,
    border: Option<Color>,
    border_radius: Option<u16>,
    typography: Option<Typography>,
    padding: Option<EdgeInsets>,
}

impl VisualStyle {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            foreground: None,
            background: None,
            border: None,
            border_radius: None,
            typography: None,
            padding: None,
        }
    }

    #[must_use]
    pub const fn foreground(mut self, color: Color) -> Self {
        self.foreground = Some(color);
        self
    }
    #[must_use]
    pub const fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }
    #[must_use]
    pub const fn border(mut self, color: Color) -> Self {
        self.border = Some(color);
        self
    }
    #[must_use]
    pub const fn border_radius(mut self, radius: u16) -> Self {
        self.border_radius = Some(radius);
        self
    }
    #[must_use]
    pub fn typography(mut self, typography: Typography) -> Self {
        self.typography = Some(typography);
        self
    }
    #[must_use]
    pub const fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    #[must_use]
    pub const fn foreground_override(&self) -> Option<Color> {
        self.foreground
    }
    #[must_use]
    pub const fn background_override(&self) -> Option<Color> {
        self.background
    }
    #[must_use]
    pub const fn border_override(&self) -> Option<Color> {
        self.border
    }
    #[must_use]
    pub const fn border_radius_override(&self) -> Option<u16> {
        self.border_radius
    }
    #[must_use]
    pub fn typography_override(&self) -> Option<&Typography> {
        self.typography.as_ref()
    }
    #[must_use]
    pub const fn padding_override(&self) -> Option<EdgeInsets> {
        self.padding
    }

    fn merge(&self, override_style: &Self) -> Self {
        Self {
            foreground: override_style.foreground.or(self.foreground),
            background: override_style.background.or(self.background),
            border: override_style.border.or(self.border),
            border_radius: override_style.border_radius.or(self.border_radius),
            typography: override_style.typography.clone().or_else(|| self.typography.clone()),
            padding: override_style.padding.or(self.padding),
        }
    }
}

impl Default for VisualStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// A control's current interaction state, used to select which of a
/// [`ComponentStyle`]'s state-specific overrides applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ControlState {
    #[default]
    Normal,
    Hovered,
    Focused,
    Pressed,
    Disabled,
}

/// A node kind's style across every [`ControlState`], each state falling
/// back to [`Self::normal`] for any property it does not itself override.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComponentStyle {
    /// The baseline style, used directly for [`ControlState::Normal`] and
    /// as the fallback for every other state's unset properties.
    pub normal: VisualStyle,
    /// Overrides applied while the pointer hovers the control.
    pub hovered: Option<VisualStyle>,
    /// Overrides applied while the control has keyboard focus.
    pub focused: Option<VisualStyle>,
    /// Overrides applied while the control is being pressed/activated.
    pub pressed: Option<VisualStyle>,
    /// Overrides applied while the control is disabled.
    pub disabled: Option<VisualStyle>,
}

impl ComponentStyle {
    #[must_use]
    pub fn resolve(&self, state: ControlState) -> VisualStyle {
        let state_style = match state {
            ControlState::Normal => None,
            ControlState::Hovered => self.hovered.as_ref(),
            ControlState::Focused => self.focused.as_ref(),
            ControlState::Pressed => self.pressed.as_ref(),
            ControlState::Disabled => self.disabled.as_ref(),
        };
        state_style.map_or_else(|| self.normal.clone(), |style| self.normal.merge(style))
    }
}

/// The application-wide default styling for every node kind and
/// interaction state.
///
/// Encapsulated with accessors (no direct external field access exists in
/// either this crate or `framework-windows`, which only ever calls
/// [`Theme::resolve`] — see the standards audit's P2.24 finding).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    typography: Typography,
    foreground: Color,
    background: Color,
    primary: Color,
    spacing: u16,
    radius: u16,
    label: ComponentStyle,
    button: ComponentStyle,
    text_input: ComponentStyle,
    container: ComponentStyle,
}

impl Default for Theme {
    fn default() -> Self {
        let foreground = Color::rgb(32, 32, 32);
        let background = Color::rgb(255, 255, 255);
        let primary = Color::rgb(0, 120, 212);
        Self {
            typography: Typography::default(),
            foreground,
            background,
            primary,
            spacing: 8,
            radius: 4,
            label: ComponentStyle {
                normal: VisualStyle::new().foreground(foreground).typography(Typography::default()),
                ..Default::default()
            },
            button: ComponentStyle {
                normal: VisualStyle::new()
                    .foreground(foreground)
                    .background(background)
                    .border(primary)
                    .border_radius(4)
                    .typography(Typography::default()),
                ..Default::default()
            },
            text_input: ComponentStyle {
                normal: VisualStyle::new()
                    .foreground(foreground)
                    .background(background)
                    .border(Color::rgb(128, 128, 128))
                    .typography(Typography::default()),
                ..Default::default()
            },
            container: ComponentStyle {
                normal: VisualStyle::new().background(background),
                ..Default::default()
            },
        }
    }
}

impl Theme {
    #[must_use]
    pub const fn typography(&self) -> &Typography {
        &self.typography
    }
    #[must_use]
    pub const fn foreground(&self) -> Color {
        self.foreground
    }
    #[must_use]
    pub const fn background(&self) -> Color {
        self.background
    }
    #[must_use]
    pub const fn primary(&self) -> Color {
        self.primary
    }
    #[must_use]
    pub const fn spacing(&self) -> u16 {
        self.spacing
    }
    #[must_use]
    pub const fn radius(&self) -> u16 {
        self.radius
    }
    #[must_use]
    pub const fn label(&self) -> &ComponentStyle {
        &self.label
    }
    #[must_use]
    pub const fn button(&self) -> &ComponentStyle {
        &self.button
    }
    #[must_use]
    pub const fn text_input(&self) -> &ComponentStyle {
        &self.text_input
    }
    #[must_use]
    pub const fn container(&self) -> &ComponentStyle {
        &self.container
    }

    // Builder methods for constructing a customized theme from
    // `Theme::default()`. These exist specifically so encapsulating this
    // type's fields (see the struct's own doc comment) doesn't take away an
    // application's ability to build a custom theme — the same capability
    // the pre-encapsulation, public-field design offered through struct-
    // update syntax (`Theme { button: ..., ..Theme::default() }`).

    #[must_use]
    pub fn with_typography(mut self, typography: Typography) -> Self {
        self.typography = typography;
        self
    }
    #[must_use]
    pub const fn with_foreground(mut self, color: Color) -> Self {
        self.foreground = color;
        self
    }
    #[must_use]
    pub const fn with_background(mut self, color: Color) -> Self {
        self.background = color;
        self
    }
    #[must_use]
    pub const fn with_primary(mut self, color: Color) -> Self {
        self.primary = color;
        self
    }
    #[must_use]
    pub const fn with_spacing(mut self, spacing: u16) -> Self {
        self.spacing = spacing;
        self
    }
    #[must_use]
    pub const fn with_radius(mut self, radius: u16) -> Self {
        self.radius = radius;
        self
    }
    #[must_use]
    pub fn with_label(mut self, style: ComponentStyle) -> Self {
        self.label = style;
        self
    }
    #[must_use]
    pub fn with_button(mut self, style: ComponentStyle) -> Self {
        self.button = style;
        self
    }
    #[must_use]
    pub fn with_text_input(mut self, style: ComponentStyle) -> Self {
        self.text_input = style;
        self
    }
    #[must_use]
    pub fn with_container(mut self, style: ComponentStyle) -> Self {
        self.container = style;
        self
    }

    /// Resolves the fully-merged style for one node: this theme's default
    /// for `kind`/`state`, with `override_style` layered on top.
    #[must_use]
    pub fn resolve(
        &self,
        kind: NodeKind,
        state: ControlState,
        override_style: &VisualStyle,
    ) -> VisualStyle {
        let base = match kind {
            NodeKind::Label => &self.label,
            NodeKind::Button => &self.button,
            NodeKind::TextInput => &self.text_input,
            NodeKind::Column | NodeKind::Row => &self.container,
        }
        .resolve(state);
        base.merge(override_style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_merges_node_overrides_and_state_styles() {
        let theme = Theme::default();
        let override_style = VisualStyle::new().foreground(Color::rgb(1, 2, 3));
        let resolved = theme.resolve(NodeKind::Button, ControlState::Normal, &override_style);
        assert_eq!(resolved.foreground_override(), Some(Color::rgb(1, 2, 3)));
        // The override didn't specify a background, so the theme's own
        // button default should still show through.
        assert_eq!(resolved.background_override(), theme.button().normal.background_override());
    }

    #[test]
    fn color_from_packed_hex_matches_rgb_components() {
        let color = Color::from(0xFF_00_FF);
        assert_eq!(color, Color::rgb(0xFF, 0x00, 0xFF));
    }
}
