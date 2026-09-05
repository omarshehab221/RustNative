//! Window-domain state and definitions: what a window *is* (title, initial
//! size, menu) and what it currently *looks like* (size, position,
//! presentation) as observed through platform events. See
//! `crate::application` for window lifecycle (open/close/multi-window
//! orchestration), which is a distinct, coordinator-level responsibility
//! this module deliberately does not own.

use crate::event::Event;
use crate::identity::WindowId;
use crate::layout::{Point, Size};
use crate::menu::MenuBar;

/// How a window is currently presented on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowPresentation {
    #[default]
    Normal,
    Minimized,
    Maximized,
    Fullscreen,
}

/// A window's live state, kept authoritative by
/// [`crate::application::Application`] as platform events arrive — see
/// `apply_window_event`.
///
/// Encapsulated (private fields + accessors/setters) because, unlike a
/// plain geometry value, this type's fields are meant to always reflect
/// reality as last reported by the platform; a setter API keeps that
/// "who is allowed to claim this changed" question explicit rather than
/// letting arbitrary code silently overwrite one field of a live struct
/// (see the standards audit's P2.24 finding).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowState {
    size: Size,
    position: Point,
    presentation: WindowPresentation,
    visible: bool,
    modal_parent: Option<WindowId>,
}

impl WindowState {
    pub(crate) const fn new(size: Size) -> Self {
        Self {
            size,
            position: Point::new(0, 0),
            presentation: WindowPresentation::Normal,
            visible: true,
            modal_parent: None,
        }
    }

    pub(crate) const fn with_modal_parent(mut self, modal_parent: Option<WindowId>) -> Self {
        self.modal_parent = modal_parent;
        self
    }

    #[must_use]
    pub const fn size(&self) -> Size {
        self.size
    }
    #[must_use]
    pub const fn position(&self) -> Point {
        self.position
    }
    #[must_use]
    pub const fn presentation(&self) -> WindowPresentation {
        self.presentation
    }
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.visible
    }
    #[must_use]
    pub const fn modal_parent(&self) -> Option<WindowId> {
        self.modal_parent
    }

    pub(crate) fn set_size(&mut self, size: Size) {
        self.size = size;
    }
    pub(crate) fn set_position(&mut self, position: Point) {
        self.position = position;
    }
    pub(crate) fn set_presentation(&mut self, presentation: WindowPresentation) {
        self.presentation = presentation;
        self.visible = presentation != WindowPresentation::Minimized;
    }
}

/// Returns the window named by a window-lifecycle event. UI input events are
/// routed by their node ownership and therefore have no window ID here.
pub(crate) fn event_window_id(event: &Event) -> Option<WindowId> {
    match event {
        Event::WindowResized { window, .. }
        | Event::WindowMoved { window, .. }
        | Event::WindowCloseRequested { window }
        | Event::WindowStateChanged { window, .. }
        | Event::MenuAction { window, .. } => Some(*window),
        _ => None,
    }
}

/// Keeps the application-owned lifecycle snapshot authoritative before the
/// component observes the corresponding framework event.
pub(crate) fn apply_window_event(state: &mut WindowState, event: &Event) {
    match event {
        Event::WindowResized { size, .. } => state.set_size(*size),
        Event::WindowMoved { position, .. } => state.set_position(*position),
        Event::WindowStateChanged { state: presentation, .. } => {
            state.set_presentation(*presentation);
        }
        // `WindowCloseRequested` carries no state to record here (closing
        // is a decision `Application`/a component makes, not a fact about
        // current size/position/presentation), so it — like every other
        // event kind — falls through to the no-op wildcard below.
        _ => {}
    }
}

/// A window's static definition: title, initial size, and (optionally) a
/// native menu bar. Like a window's title/size, its menu is fixed at
/// creation time — see [`Self::with_menu`].
#[derive(Debug, Clone)]
pub struct Window {
    title: String,
    size: Size,
    menu: Option<MenuBar>,
}

impl Window {
    pub fn new(title: impl Into<String>, size: Size) -> Self {
        Self { title: title.into(), size, menu: None }
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub const fn size(&self) -> Size {
        self.size
    }

    /// Attaches a native menu bar realized by the platform backend when this
    /// window is created. The menu is part of the window's static
    /// definition, the same maturity level as its title and initial size:
    /// changing it after the window opens is not yet supported.
    #[must_use]
    pub fn with_menu(mut self, menu: MenuBar) -> Self {
        self.menu = Some(menu);
        self
    }

    #[must_use]
    pub fn menu(&self) -> Option<&MenuBar> {
        self.menu.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_window_event_updates_size_position_and_presentation() {
        let mut state = WindowState::new(Size::new(100, 100));
        apply_window_event(
            &mut state,
            &Event::WindowResized { window: WindowId::PRIMARY, size: Size::new(200, 300) },
        );
        assert_eq!(state.size(), Size::new(200, 300));

        apply_window_event(
            &mut state,
            &Event::WindowMoved { window: WindowId::PRIMARY, position: Point::new(5, 6) },
        );
        assert_eq!(state.position(), Point::new(5, 6));

        apply_window_event(
            &mut state,
            &Event::WindowStateChanged {
                window: WindowId::PRIMARY,
                state: WindowPresentation::Minimized,
            },
        );
        assert_eq!(state.presentation(), WindowPresentation::Minimized);
        assert!(!state.is_visible());
    }
}
