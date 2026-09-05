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
    Click {
        target: NodeId,
    },
    FocusGained {
        target: NodeId,
    },
    FocusLost {
        target: NodeId,
    },
    KeyDown {
        target: Option<NodeId>,
        key: KeyCode,
        modifiers: KeyModifiers,
    },
    TextInput {
        target: Option<NodeId>,
        text: String,
    },
    TextChanged {
        target: NodeId,
        value: String,
    },
    WindowResized {
        window: WindowId,
        size: Size,
    },
    WindowMoved {
        window: WindowId,
        position: Point,
    },
    WindowCloseRequested {
        window: WindowId,
    },
    WindowStateChanged {
        window: WindowId,
        state: WindowPresentation,
    },
    /// A native menu item was selected. Menus are window chrome rather than
    /// part of the declarative node tree, so — like window-lifecycle events
    /// — this always routes to the window's root component rather than to a
    /// specific node owner.
    MenuAction {
        window: WindowId,
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
    Enter,
    Space,
    Tab,
    Escape,
    Backspace,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Character(char),
    /// A key this crate does not yet name explicitly, carrying the
    /// backend's native virtual-key code for diagnostics/escape-hatch use.
    Unknown(u32),
}

/// Which modifier keys were held down when a [`KeyCode`] was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// A portable accessibility role. Mirrors the small set of controls this
/// crate's declarative `Node` API currently exposes; see `PLAN.md`'s
/// accessibility-bridge milestone for the custom-semantic-node roadmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AccessibilityRole {
    None,
    Label,
    Button,
    TextInput,
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
    #[must_use]
    pub fn new(role: AccessibilityRole) -> Self {
        Self { role, name: None, description: None, focusable: false }
    }

    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    #[must_use]
    pub const fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    #[must_use]
    pub const fn role(&self) -> AccessibilityRole {
        self.role
    }

    #[must_use]
    pub fn name_hint(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[must_use]
    pub fn description_hint(&self) -> Option<&str> {
        self.description.as_deref()
    }

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
