//! Semantic input events delivered from a platform backend to a component.

use crate::identity::{NodeId, WindowId};
use crate::input::{
    ClipboardAction, Composition, DragData, GamepadInput, Gesture, PointerEvent, WheelDelta,
};
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
    /// A key was released. Delivered like [`Event::KeyDown`].
    KeyUp {
        /// The focused node, or `None` if no node has focus.
        target: Option<NodeId>,
        /// The key that was released.
        key: KeyCode,
        /// Which modifier keys were held down at the same time.
        modifiers: KeyModifiers,
    },
    /// A pointer contact began (button pressed, finger or pen touched) on
    /// a node with pointer interest (see [`crate::InputInterest`]).
    PointerDown {
        /// The interested node.
        target: NodeId,
        /// The sample, in `target`'s local coordinates.
        pointer: PointerEvent,
    },
    /// A pointer moved over (or, while captured, anywhere on behalf of) a
    /// node with pointer interest.
    PointerMove {
        /// The interested node.
        target: NodeId,
        /// The sample, in `target`'s local coordinates.
        pointer: PointerEvent,
    },
    /// A pointer contact ended.
    PointerUp {
        /// The interested node.
        target: NodeId,
        /// The sample, in `target`'s local coordinates.
        pointer: PointerEvent,
    },
    /// A pointer contact was taken away without ending normally — pointer
    /// capture was lost, or the window was deactivated mid-press. Treat
    /// like an up event that must not commit anything.
    PointerCancel {
        /// The interested node.
        target: NodeId,
        /// The last sample, in `target`'s local coordinates.
        pointer: PointerEvent,
    },
    /// The pointer entered a node with pointer interest (hover began).
    PointerEnter {
        /// The interested node.
        target: NodeId,
    },
    /// The pointer left a node with pointer interest (hover ended).
    PointerLeave {
        /// The interested node.
        target: NodeId,
    },
    /// A scroll wheel or trackpad scrolled over a node with wheel interest.
    Wheel {
        /// The interested node.
        target: NodeId,
        /// How far, and in which unit.
        delta: WheelDelta,
    },
    /// A gesture was recognized on a node with gesture interest.
    Gesture {
        /// The interested node.
        target: NodeId,
        /// The gesture, in `target`'s local coordinates.
        gesture: Gesture,
    },
    /// An input-method composition step for the focused node.
    Composition {
        /// The focused node, or `None` if no node has focus.
        target: Option<NodeId>,
        /// The composition step.
        composition: Composition,
    },
    /// The person performed a clipboard operation on the focused node.
    Clipboard {
        /// The focused node, or `None` if no node has focus.
        target: Option<NodeId>,
        /// The operation.
        action: ClipboardAction,
    },
    /// The system clipboard's content changed (from any application).
    /// Routed like a window-lifecycle event, to the window's root
    /// component.
    ClipboardChanged {
        /// The window being notified.
        window: WindowId,
    },
    /// A drag entered a drop-target node. Answer with
    /// `ComponentContext::input().set_drop_effect(...)` to accept it.
    DragEnter {
        /// The drop-target node.
        target: NodeId,
        /// What is being dragged.
        data: DragData,
        /// Pointer position in `target`'s local coordinates.
        position: Point,
    },
    /// A drag moved over a drop-target node.
    DragOver {
        /// The drop-target node.
        target: NodeId,
        /// What is being dragged.
        data: DragData,
        /// Pointer position in `target`'s local coordinates.
        position: Point,
    },
    /// A drag left a drop-target node without dropping.
    DragLeave {
        /// The drop-target node.
        target: NodeId,
    },
    /// Data was dropped on a drop-target node that accepted it.
    Drop {
        /// The drop-target node.
        target: NodeId,
        /// What was dropped.
        data: DragData,
        /// Drop position in `target`'s local coordinates.
        position: Point,
    },
    /// A game controller changed, delivered to the first node (in
    /// declarative order) of the active window that declared gamepad
    /// interest.
    Gamepad {
        /// The interested node.
        target: NodeId,
        /// Which controller (slot number, stable while it stays connected).
        gamepad: u32,
        /// What changed.
        input: GamepadInput,
    },
}

impl Event {
    /// Returns the node this event targets, if any. Window-lifecycle and
    /// menu events have no node target: see `crate::window::event_window_id`
    /// for how those are routed instead.
    #[must_use]
    pub const fn target(&self) -> Option<NodeId> {
        match self {
            Self::Click { target }
            | Self::FocusGained { target }
            | Self::FocusLost { target }
            | Self::TextChanged { target, .. }
            | Self::PointerDown { target, .. }
            | Self::PointerMove { target, .. }
            | Self::PointerUp { target, .. }
            | Self::PointerCancel { target, .. }
            | Self::PointerEnter { target }
            | Self::PointerLeave { target }
            | Self::Wheel { target, .. }
            | Self::Gesture { target, .. }
            | Self::DragEnter { target, .. }
            | Self::DragOver { target, .. }
            | Self::DragLeave { target }
            | Self::Drop { target, .. }
            | Self::Gamepad { target, .. } => Some(*target),
            Self::KeyDown { target, .. }
            | Self::KeyUp { target, .. }
            | Self::TextInput { target, .. }
            | Self::Composition { target, .. }
            | Self::Clipboard { target, .. } => *target,
            Self::WindowResized { .. }
            | Self::WindowMoved { .. }
            | Self::WindowCloseRequested { .. }
            | Self::WindowStateChanged { .. }
            | Self::MenuAction { .. }
            | Self::ClipboardChanged { .. } => None,
        }
    }

    /// Rewrites a platform-facing target to the component-local key exposed
    /// to `Component::update`. Window and menu events have no node target.
    pub(crate) fn with_local_target(mut self, target: Option<NodeId>) -> Self {
        match &mut self {
            Self::Click { target: current }
            | Self::FocusGained { target: current }
            | Self::FocusLost { target: current }
            | Self::TextChanged { target: current, .. }
            | Self::PointerDown { target: current, .. }
            | Self::PointerMove { target: current, .. }
            | Self::PointerUp { target: current, .. }
            | Self::PointerCancel { target: current, .. }
            | Self::PointerEnter { target: current }
            | Self::PointerLeave { target: current }
            | Self::Wheel { target: current, .. }
            | Self::Gesture { target: current, .. }
            | Self::DragEnter { target: current, .. }
            | Self::DragOver { target: current, .. }
            | Self::DragLeave { target: current }
            | Self::Drop { target: current, .. }
            | Self::Gamepad { target: current, .. } => {
                if let Some(target) = target {
                    *current = target;
                }
            }
            Self::KeyDown { target: current, .. }
            | Self::KeyUp { target: current, .. }
            | Self::TextInput { target: current, .. }
            | Self::Composition { target: current, .. }
            | Self::Clipboard { target: current, .. } => {
                *current = target;
            }
            Self::WindowResized { .. }
            | Self::WindowMoved { .. }
            | Self::WindowCloseRequested { .. }
            | Self::WindowStateChanged { .. }
            | Self::MenuAction { .. }
            | Self::ClipboardChanged { .. } => {}
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
    /// The Delete (forward delete) key.
    Delete,
    /// The Insert key.
    Insert,
    /// The Home key.
    Home,
    /// The End key.
    End,
    /// The Page Up key.
    PageUp,
    /// The Page Down key.
    PageDown,
    /// A function key, `F1` through `F24`, carrying its number.
    Function(u8),
    /// A printable character key, carrying the character it produces.
    Character(char),
    /// A key this crate does not yet name explicitly, carrying the
    /// backend's native virtual-key code for diagnostics/escape-hatch use.
    Unknown(u32),
}

/// Which modifier keys were held down when a [`KeyCode`] was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "four independent physical keys, each held or not; not a state machine in disguise"
)]
pub struct KeyModifiers {
    /// Whether either Shift key was held down.
    pub shift: bool,
    /// Whether either Ctrl key was held down.
    pub ctrl: bool,
    /// Whether either Alt key was held down.
    pub alt: bool,
    /// Whether the platform's "meta" key (Windows key, Command, Super) was
    /// held down.
    pub meta: bool,
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
