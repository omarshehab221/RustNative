//! Semantic input events delivered from a platform backend to a component.

use crate::identity::{NodeId, WindowId};
use crate::layout::{Point, Size};
use crate::window::WindowPresentation;

/// Input produced by a platform backend and delivered to the active
/// component tree.
///
/// `#[non_exhaustive]`: a future platform capability (drag-and-drop, IME
/// composition, pointer capture, ...) can add a variant without breaking
/// every downstream `match` — see `PLAN.md`'s advanced-input milestone.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// A pointer press-and-release (or equivalent activation, e.g. Space/
    /// Enter on a focused control) completed on `target`.
    Click {
        /// The node that was activated.
        target: NodeId,
    },
    /// `target` became the focused node.
    FocusGained {
        /// The node that gained focus.
        target: NodeId,
    },
    /// `target` was the focused node and is no longer.
    FocusLost {
        /// The node that lost focus.
        target: NodeId,
    },
    /// A key was pressed while `target` (or, if `None`, no specific node)
    /// had focus.
    KeyDown {
        /// The focused node the key was delivered to, or `None` if no node
        /// currently has focus.
        target: Option<NodeId>,
        /// The key that was pressed.
        key: KeyCode,
        /// Which modifier keys were held down at the same time.
        modifiers: KeyModifiers,
    },
    /// Committed text input (e.g. from an IME or a printable keystroke),
    /// delivered independently of [`Event::KeyDown`] since one physical key
    /// press can produce zero, one, or many characters.
    TextInput {
        /// The focused node the text was delivered to, or `None` if no node
        /// currently has focus.
        target: Option<NodeId>,
        /// The committed text.
        text: String,
    },
    /// A text-input control's content changed to `value`.
    TextChanged {
        /// The text-input node whose content changed.
        target: NodeId,
        /// The control's new, complete text content.
        value: String,
    },
    /// A window was resized to `size`.
    WindowResized {
        /// The window that was resized.
        window: WindowId,
        /// The window's new client-area size.
        size: Size,
    },
    /// A window was moved to `position`.
    WindowMoved {
        /// The window that was moved.
        window: WindowId,
        /// The window's new top-left position, in screen coordinates.
        position: Point,
    },
    /// The person asked to close a window (e.g. clicked its close button);
    /// the window remains open until the application actually removes it.
    WindowCloseRequested {
        /// The window the close request applies to.
        window: WindowId,
    },
    /// A window's presentation (minimized/maximized/restored) changed.
    WindowStateChanged {
        /// The window whose presentation changed.
        window: WindowId,
        /// The window's new presentation.
        state: WindowPresentation,
    },
    /// A native menu item was selected. Menus are window chrome rather than
    /// part of the declarative node tree, so — like window-lifecycle events
    /// — this always routes to the window's root component rather than to a
    /// specific node owner.
    MenuAction {
        /// The window whose menu bar the selected item belongs to.
        window: WindowId,
        /// The identity of the menu item that was selected.
        item: NodeId,
    },
}

impl Event {
    /// Returns the node this event targets, if any. Window-lifecycle and
    /// menu events have no node target: see `crate::window::event_window_id`
    /// for how those are routed instead.
    #[must_use]
    pub const fn target(&self) -> Option<NodeId> {
        match self {
            Self::Click { target } | Self::FocusGained { target } | Self::FocusLost { target } => {
                Some(*target)
            }
            Self::KeyDown { target, .. } | Self::TextInput { target, .. } => *target,
            Self::TextChanged { target, .. } => Some(*target),
            Self::WindowResized { .. }
            | Self::WindowMoved { .. }
            | Self::WindowCloseRequested { .. }
            | Self::WindowStateChanged { .. }
            | Self::MenuAction { .. } => None,
        }
    }

    /// Rewrites a platform-facing target to the component-local key exposed
    /// to `Component::update`. Window and menu events have no node target.
    pub(crate) fn with_local_target(mut self, target: Option<NodeId>) -> Self {
        match &mut self {
            Self::Click { target: current }
            | Self::FocusGained { target: current }
            | Self::FocusLost { target: current }
            | Self::TextChanged { target: current, .. } => {
                if let Some(target) = target {
                    *current = target;
                }
            }
            Self::KeyDown { target: current, .. } | Self::TextInput { target: current, .. } => {
                *current = target;
            }
            Self::WindowResized { .. }
            | Self::WindowMoved { .. }
            | Self::WindowCloseRequested { .. }
            | Self::WindowStateChanged { .. }
            | Self::MenuAction { .. } => {}
        }
        self
    }
}

/// A platform-independent keyboard key. Backends translate their native
/// virtual-key constants into this set rather than exposing them directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum KeyCode {
    /// The Enter/Return key.
    Enter,
    /// The Space bar.
    Space,
    /// The Tab key.
    Tab,
    /// The Escape key.
    Escape,
    /// The Backspace key.
    Backspace,
    /// The left arrow key.
    ArrowLeft,
    /// The right arrow key.
    ArrowRight,
    /// The up arrow key.
    ArrowUp,
    /// The down arrow key.
    ArrowDown,
    /// A printable character key, carrying the character it produces.
    Character(char),
    /// A key this crate does not yet name explicitly, carrying the
    /// backend's native virtual-key code for diagnostics/escape-hatch use.
    Unknown(u32),
}

/// Which modifier keys were held down when a [`KeyCode`] was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyModifiers {
    /// Whether either Shift key was held down.
    pub shift: bool,
    /// Whether either Ctrl key was held down.
    pub ctrl: bool,
    /// Whether either Alt key was held down.
    pub alt: bool,
}

/// A portable accessibility role. Mirrors the small set of controls this
/// crate's declarative `Node` API currently exposes; see `PLAN.md`'s
/// accessibility-bridge milestone for the custom-semantic-node roadmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AccessibilityRole {
    /// No specific role; the node is not exposed as a distinct
    /// accessibility element.
    None,
    /// A static, non-interactive text label.
    Label,
    /// An activatable control (e.g. a push button).
    Button,
    /// An editable text field.
    TextInput,
    /// A container grouping other accessible elements.
    Group,
}

/// Portable accessibility metadata attached to a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibilityInfo {
    role: AccessibilityRole,
    name: Option<String>,
    description: Option<String>,
    focusable: bool,
}

impl AccessibilityInfo {
    /// Creates accessibility metadata for `role`, with no name or
    /// description yet and not focusable.
    #[must_use]
    pub fn new(role: AccessibilityRole) -> Self {
        Self { role, name: None, description: None, focusable: false }
    }

    /// Sets the accessible name (the primary label assistive technology
    /// announces for this node).
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the accessible description (supplementary detail announced
    /// after the name).
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets whether this node can receive keyboard focus.
    #[must_use]
    pub const fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Returns the accessible role.
    #[must_use]
    pub const fn role(&self) -> AccessibilityRole {
        self.role
    }

    /// Returns the accessible name, if one was set.
    #[must_use]
    pub fn name_hint(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns the accessible description, if one was set.
    #[must_use]
    pub fn description_hint(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns whether this node can receive keyboard focus.
    #[must_use]
    pub const fn is_focusable(&self) -> bool {
        self.focusable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessibility_info_builder_round_trips() {
        let info = AccessibilityInfo::new(AccessibilityRole::Button)
            .name("Submit")
            .description("Submits the form")
            .focusable(true);
        assert_eq!(info.role(), AccessibilityRole::Button);
        assert_eq!(info.name_hint(), Some("Submit"));
        assert_eq!(info.description_hint(), Some("Submits the form"));
        assert!(info.is_focusable());
    }

    #[test]
    fn event_target_is_none_for_window_and_menu_events() {
        assert_eq!(Event::WindowCloseRequested { window: WindowId::PRIMARY }.target(), None);
        assert_eq!(
            Event::MenuAction { window: WindowId::PRIMARY, item: NodeId::from_key("x") }.target(),
            None
        );
    }
}
