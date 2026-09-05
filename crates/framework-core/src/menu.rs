//! Native menu bars.
//!
//! A [`MenuBar`] is window chrome, not part of the declarative node tree
//! (`crate::node`): it is attached to a [`crate::window::Window`] once, at
//! window-creation time, and its selection events route through
//! `crate::event::Event::MenuAction` to the window's root component rather
//! than to a specific node owner — the same routing model window-lifecycle
//! events use (see `crate::window::event_window_id`).

use crate::identity::NodeId;

/// A single entry in a native [`MenuBar`]. Leaf items (those with no
/// children) dispatch `Event::MenuAction { item, .. }` when selected, using
/// the same stable [`NodeId::from_key`] identity already used to target UI
/// nodes. Items with children render as a native submenu instead of being
/// individually actionable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    id: NodeId,
    label: String,
    enabled: bool,
    checked: Option<bool>,
    separator: bool,
    children: Vec<MenuItem>,
}

impl MenuItem {
    /// A leaf menu item that dispatches `Event::MenuAction` with the id
    /// derived from `key` when selected.
    pub fn action(key: impl AsRef<str>, label: impl Into<String>) -> Self {
        Self {
            id: NodeId::from_key(key.as_ref()),
            label: label.into(),
            enabled: true,
            checked: None,
            separator: false,
            children: Vec::new(),
        }
    }

    /// A submenu item that groups other items instead of dispatching an
    /// action itself.
    pub fn submenu(
        key: impl AsRef<str>,
        label: impl Into<String>,
        children: impl IntoIterator<Item = MenuItem>,
    ) -> Self {
        Self {
            id: NodeId::from_key(key.as_ref()),
            label: label.into(),
            enabled: true,
            checked: None,
            separator: false,
            children: children.into_iter().collect(),
        }
    }

    /// A non-actionable visual divider between items in the same menu.
    #[must_use]
    pub fn separator() -> Self {
        Self {
            id: NodeId::from_key(""),
            label: String::new(),
            enabled: true,
            checked: None,
            separator: true,
            children: Vec::new(),
        }
    }

    #[must_use]
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Renders the item with a checkmark. Passing `false` renders an
    /// explicit (unchecked) checkable item rather than an ordinary one; use
    /// `action`/`submenu` alone to opt out of the checkable presentation.
    #[must_use]
    pub const fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    #[must_use]
    pub fn id(&self) -> NodeId {
        self.id
    }
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    #[must_use]
    pub fn is_checked(&self) -> Option<bool> {
        self.checked
    }
    #[must_use]
    pub fn is_separator(&self) -> bool {
        self.separator
    }
    #[must_use]
    pub fn is_submenu(&self) -> bool {
        !self.children.is_empty()
    }
    #[must_use]
    pub fn children(&self) -> &[MenuItem] {
        &self.children
    }
}

/// A portable, declarative native menu bar attached to a `Window`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuBar {
    items: Vec<MenuItem>,
}

impl MenuBar {
    pub fn new(items: impl IntoIterator<Item = MenuItem>) -> Self {
        Self { items: items.into_iter().collect() }
    }

    #[must_use]
    pub fn items(&self) -> &[MenuItem] {
        &self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_bar_exposes_actions_submenus_and_separators() {
        let menu = MenuBar::new([
            MenuItem::submenu(
                "file",
                "File",
                [MenuItem::action("file.new", "New"), MenuItem::separator()],
            ),
            MenuItem::action("help", "Help").enabled(false),
        ]);
        assert_eq!(menu.items().len(), 2);
        assert!(menu.items()[0].is_submenu());
        assert_eq!(menu.items()[0].children().len(), 2);
        assert!(menu.items()[0].children()[1].is_separator());
        assert!(!menu.items()[1].is_enabled());
    }
}
