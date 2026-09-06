//! The framework's declarative UI tree: [`Node`] and its per-kind payloads.
//!
//! A [`Node`] tree is what a [`crate::Component`] returns from `view`/
//! `render`. It is pure data — no native object, no reconciliation state —
//! which is what lets [`crate::reconcile::TreeSnapshot`] treat it as the
//! single source of truth to diff against on every render (see
//! `crate::reconcile` for the next stage of the pipeline).

use std::collections::HashMap;

use crate::event::AccessibilityInfo;
use crate::identity::NodeId;
use crate::layout::{ColumnStyle, LayoutStyle, RowStyle};
use crate::style::VisualStyle;

/// The framework's declarative UI tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A static, non-interactive text label.
    Label(Label),
    /// An activatable push button.
    Button(Button),
    /// An editable single-line text field.
    TextInput(TextInput),
    /// A container that lays its children out vertically.
    Column(Column),
    /// A container that lays its children out horizontally.
    Row(Row),
}

impl Node {
    /// Creates a [`Label`] node with the default layout.
    pub fn label(key: impl AsRef<str>, text: impl Into<String>) -> Self {
        Self::Label(Label::new(NodeId::from_key(key.as_ref()), text, LayoutStyle::default()))
    }

    /// Creates a [`Label`] node with an explicit layout.
    pub fn label_with_layout(
        key: impl AsRef<str>,
        text: impl Into<String>,
        layout: LayoutStyle,
    ) -> Self {
        Self::Label(Label::new(NodeId::from_key(key.as_ref()), text, layout))
    }

    /// Creates a [`Button`] node with the default layout.
    pub fn button(key: impl AsRef<str>, text: impl Into<String>) -> Self {
        Self::Button(Button::new(NodeId::from_key(key.as_ref()), text, LayoutStyle::default()))
    }

    /// Creates a [`Button`] node with an explicit layout.
    pub fn button_with_layout(
        key: impl AsRef<str>,
        text: impl Into<String>,
        layout: LayoutStyle,
    ) -> Self {
        Self::Button(Button::new(NodeId::from_key(key.as_ref()), text, layout))
    }

    /// Creates a [`TextInput`] node with the default layout.
    pub fn text_input(key: impl AsRef<str>, value: impl Into<String>) -> Self {
        Self::TextInput(TextInput::new(
            NodeId::from_key(key.as_ref()),
            value,
            LayoutStyle::default(),
        ))
    }

    /// Creates a [`TextInput`] node with an explicit layout.
    pub fn text_input_with_layout(
        key: impl AsRef<str>,
        value: impl Into<String>,
        layout: LayoutStyle,
    ) -> Self {
        Self::TextInput(TextInput::new(NodeId::from_key(key.as_ref()), value, layout))
    }

    /// Creates a [`Column`] node with the default layout and column style.
    pub fn column(key: impl AsRef<str>, children: impl IntoIterator<Item = Node>) -> Self {
        Self::Column(Column::new(
            NodeId::from_key(key.as_ref()),
            children.into_iter().collect(),
            ColumnStyle::default(),
            LayoutStyle::default(),
        ))
    }

    /// Creates a [`Column`] node with an explicit layout and column style.
    pub fn column_with_layout(
        key: impl AsRef<str>,
        children: impl IntoIterator<Item = Node>,
        layout: LayoutStyle,
        style: ColumnStyle,
    ) -> Self {
        Self::Column(Column::new(
            NodeId::from_key(key.as_ref()),
            children.into_iter().collect(),
            style,
            layout,
        ))
    }

    /// Creates a [`Row`] node with the default layout and row style.
    pub fn row(key: impl AsRef<str>, children: impl IntoIterator<Item = Node>) -> Self {
        Self::Row(Row::new(
            NodeId::from_key(key.as_ref()),
            children.into_iter().collect(),
            RowStyle::default(),
            LayoutStyle::default(),
        ))
    }

    /// Creates a [`Row`] node with an explicit layout and row style.
    pub fn row_with_layout(
        key: impl AsRef<str>,
        children: impl IntoIterator<Item = Node>,
        layout: LayoutStyle,
        style: RowStyle,
    ) -> Self {
        Self::Row(Row::new(
            NodeId::from_key(key.as_ref()),
            children.into_iter().collect(),
            style,
            layout,
        ))
    }

    /// Returns `self` with its accessibility metadata replaced.
    #[must_use]
    pub fn with_accessibility(self, accessibility: AccessibilityInfo) -> Self {
        match self {
            Self::Label(mut node) => {
                node.accessibility = accessibility;
                Self::Label(node)
            }
            Self::Button(mut node) => {
                node.accessibility = accessibility;
                Self::Button(node)
            }
            Self::TextInput(mut node) => {
                node.accessibility = accessibility;
                Self::TextInput(node)
            }
            Self::Column(mut node) => {
                node.accessibility = accessibility;
                Self::Column(node)
            }
            Self::Row(mut node) => {
                node.accessibility = accessibility;
                Self::Row(node)
            }
        }
    }

    /// Applies a node-level visual override. The active theme supplies any
    /// unspecified values when the backend resolves the style.
    #[must_use]
    pub fn with_style(self, style: VisualStyle) -> Self {
        match self {
            Self::Label(mut node) => {
                node.visual_style = style;
                Self::Label(node)
            }
            Self::Button(mut node) => {
                node.visual_style = style;
                Self::Button(node)
            }
            Self::TextInput(mut node) => {
                node.visual_style = style;
                Self::TextInput(node)
            }
            Self::Column(mut node) => {
                node.visual_style = style;
                Self::Column(node)
            }
            Self::Row(mut node) => {
                node.visual_style = style;
                Self::Row(node)
            }
        }
    }

    /// Returns this node's visual style override.
    #[must_use]
    pub fn visual_style(&self) -> &VisualStyle {
        match self {
            Self::Label(node) => &node.visual_style,
            Self::Button(node) => &node.visual_style,
            Self::TextInput(node) => &node.visual_style,
            Self::Column(node) => &node.visual_style,
            Self::Row(node) => &node.visual_style,
        }
    }

    /// Marks this node (and, for containers, its native realization) as
    /// disabled. A disabled control stops accepting native input focus and
    /// input events, is skipped by Tab/Shift+Tab traversal, and is styled
    /// using the theme's `ControlState::Disabled` variant.
    #[must_use]
    pub fn disabled(self, disabled: bool) -> Self {
        match self {
            Self::Label(mut node) => {
                node.disabled = disabled;
                Self::Label(node)
            }
            Self::Button(mut node) => {
                node.disabled = disabled;
                Self::Button(node)
            }
            Self::TextInput(mut node) => {
                node.disabled = disabled;
                Self::TextInput(node)
            }
            Self::Column(mut node) => {
                node.disabled = disabled;
                Self::Column(node)
            }
            Self::Row(mut node) => {
                node.disabled = disabled;
                Self::Row(node)
            }
        }
    }

    /// Returns whether this node is disabled.
    #[must_use]
    pub fn is_disabled(&self) -> bool {
        match self {
            Self::Label(node) => node.disabled,
            Self::Button(node) => node.disabled,
            Self::TextInput(node) => node.disabled,
            Self::Column(node) => node.disabled,
            Self::Row(node) => node.disabled,
        }
    }

    /// Returns this node's accessibility metadata.
    #[must_use]
    pub fn accessibility(&self) -> &AccessibilityInfo {
        match self {
            Self::Label(node) => node.accessibility(),
            Self::Button(node) => node.accessibility(),
            Self::TextInput(node) => node.accessibility(),
            Self::Column(node) => node.accessibility(),
            Self::Row(node) => node.accessibility(),
        }
    }

    /// Returns this node's identity.
    #[must_use]
    pub fn id(&self) -> NodeId {
        match self {
            Self::Label(label) => label.id(),
            Self::Button(button) => button.id(),
            Self::TextInput(input) => input.id(),
            Self::Column(column) => column.id(),
            Self::Row(row) => row.id(),
        }
    }

    /// Returns this node's kind.
    #[must_use]
    pub fn kind(&self) -> NodeKind {
        match self {
            Self::Label(_) => NodeKind::Label,
            Self::Button(_) => NodeKind::Button,
            Self::TextInput(_) => NodeKind::TextInput,
            Self::Column(_) => NodeKind::Column,
            Self::Row(_) => NodeKind::Row,
        }
    }

    /// Returns this node's layout style.
    #[must_use]
    pub fn layout(&self) -> LayoutStyle {
        match self {
            Self::Label(label) => label.layout(),
            Self::Button(button) => button.layout(),
            Self::TextInput(input) => input.layout(),
            Self::Column(column) => column.layout(),
            Self::Row(row) => row.layout(),
        }
    }

    /// Returns this node's column style, if it is a [`Column`].
    #[must_use]
    pub fn column_style(&self) -> Option<ColumnStyle> {
        match self {
            Self::Column(column) => Some(column.style()),
            _ => None,
        }
    }

    /// Returns this node's row style, if it is a [`Row`].
    #[must_use]
    pub fn row_style(&self) -> Option<RowStyle> {
        match self {
            Self::Row(row) => Some(row.style()),
            _ => None,
        }
    }

    /// Walks this node and every descendant depth-first, calling `visitor`
    /// with each node, its parent's id (`None` for the root), and its index
    /// among its siblings.
    pub fn visit(&self, visitor: &mut impl FnMut(&Node, Option<NodeId>, usize)) {
        self.visit_with_parent(None, 0, visitor);
    }

    /// Returns whether `target` identifies this node or any descendant.
    #[must_use]
    pub fn contains_id(&self, target: NodeId) -> bool {
        let mut found = false;
        self.visit(&mut |node, _, _| {
            if node.id() == target {
                found = true;
            }
        });
        found
    }

    fn visit_with_parent(
        &self,
        parent: Option<NodeId>,
        index: usize,
        visitor: &mut impl FnMut(&Node, Option<NodeId>, usize),
    ) {
        visitor(self, parent, index);

        match self {
            Self::Column(column) => {
                for (index, child) in column.children().iter().enumerate() {
                    child.visit_with_parent(Some(column.id()), index, visitor);
                }
            }
            Self::Row(row) => {
                for (index, child) in row.children().iter().enumerate() {
                    child.visit_with_parent(Some(row.id()), index, visitor);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn validate_unique_ids(&self) -> Result<(), TreeError> {
        let mut count = HashMap::new();
        self.visit(&mut |node, _, _| {
            *count.entry(node.id()).or_insert(0usize) += 1;
        });

        if let Some((id, _)) = count.into_iter().find(|(_, count)| *count > 1) {
            return Err(TreeError::DuplicateNodeId(id));
        }

        Ok(())
    }

    /// Used only by the component runtime while scoping a freshly-rendered
    /// view's local keys into framework-wide identities (see
    /// `crate::component::tree::scope_component_node_ids`). Application code
    /// never needs to — and cannot, since node identity is otherwise
    /// immutable once constructed — assign a node's id directly.
    pub(crate) fn set_id(&mut self, id: NodeId) {
        match self {
            Self::Label(node) => node.id = id,
            Self::Button(node) => node.id = id,
            Self::TextInput(node) => node.id = id,
            Self::Column(node) => node.id = id,
            Self::Row(node) => node.id = id,
        }
    }
}

/// A UI node's realization kind, independent of any single node instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// See [`Node::Label`].
    Label,
    /// See [`Node::Button`].
    Button,
    /// See [`Node::TextInput`].
    TextInput,
    /// See [`Node::Column`].
    Column,
    /// See [`Node::Row`].
    Row,
}

/// Encodes the small amount of state genuinely shared by every leaf/
/// container node payload (`id`, `layout`, `accessibility`, `visual_style`,
/// `disabled`) without introducing a base "class" via a trait object, which
/// would give every node kind a payload-independent way to be constructed
/// with mismatched invariants. Each generated type keeps its own
/// kind-specific payload field(s) (`text`/`value`/`children`) as a plain,
/// explicit struct field instead.
macro_rules! leaf_node {
    (
        $name:ident,
        $payload_field:ident : $payload:ty,
        $payload_param:ident,
        role = $role:ident,
        focusable = $focusable:literal
    ) => {
        /// A leaf UI node (see [`Node`]).
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name {
            id: NodeId,
            $payload_field: $payload,
            layout: LayoutStyle,
            accessibility: AccessibilityInfo,
            visual_style: VisualStyle,
            disabled: bool,
        }

        impl $name {
            fn new(id: NodeId, $payload_param: impl Into<$payload>, layout: LayoutStyle) -> Self {
                Self {
                    id,
                    $payload_field: $payload_param.into(),
                    layout,
                    // A node's accessibility metadata starts out
                    // describing what the node *is*, rather than empty.
                    //
                    // Every leaf kind used to default to
                    // `AccessibilityRole::None` and `focusable: false`,
                    // which made the portable semantic model convey nothing
                    // unless an application filled it in by hand for every
                    // single node — and since a backend can only realize
                    // what the model says, that is the root of the standards
                    // audit's P1.16 finding that accessibility "is not
                    // actually fully realized". A button is an activatable
                    // control and a keyboard stop; a label is static text
                    // and is not; a text field is both editable and a
                    // keyboard stop. Those are properties of the node kind,
                    // known here, so they are the defaults here.
                    //
                    // `with_accessibility` still replaces the whole value,
                    // so an application that wants a decorative button
                    // outside the tab order, or a label with an overridden
                    // announced name, says so explicitly.
                    accessibility: AccessibilityInfo::new(crate::event::AccessibilityRole::$role)
                        .focusable($focusable),
                    visual_style: VisualStyle::default(),
                    disabled: false,
                }
            }

            /// Returns this node's identity.
            pub fn id(&self) -> NodeId {
                self.id
            }

            /// Returns this node's layout style.
            pub fn layout(&self) -> LayoutStyle {
                self.layout
            }

            /// Returns this node's accessibility metadata.
            pub fn accessibility(&self) -> &AccessibilityInfo {
                &self.accessibility
            }
        }
    };
}

leaf_node!(Label, text: String, text, role = Label, focusable = false);
leaf_node!(Button, text: String, text, role = Button, focusable = true);
leaf_node!(TextInput, value: String, value, role = TextInput, focusable = true);

impl Label {
    /// Returns the label's text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Button {
    /// Returns the button's text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl TextInput {
    /// Returns the text field's current value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// A macro-free way to keep [`Column`] and [`Row`] — the two container node
/// payloads — from drifting apart while still being distinct types (see the
/// same reasoning on `crate::layout::ColumnStyle`/`RowStyle`).
macro_rules! container_node {
    ($name:ident, $style:ty) => {
        /// A container UI node (see [`Node`]).
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name {
            id: NodeId,
            children: Vec<Node>,
            style: $style,
            layout: LayoutStyle,
            accessibility: AccessibilityInfo,
            visual_style: VisualStyle,
            disabled: bool,
        }

        impl $name {
            fn new(id: NodeId, children: Vec<Node>, style: $style, layout: LayoutStyle) -> Self {
                Self {
                    id,
                    children,
                    style,
                    layout,
                    accessibility: AccessibilityInfo::new(crate::event::AccessibilityRole::Group),
                    visual_style: VisualStyle::default(),
                    disabled: false,
                }
            }

            /// Returns this node's identity.
            pub fn id(&self) -> NodeId {
                self.id
            }

            /// Returns this container's children.
            pub fn children(&self) -> &[Node] {
                &self.children
            }

            /// Mutable child access for the component runtime's node-id
            /// scoping pass (`crate::component::tree::scope_component_node_ids`),
            /// which must rewrite each authored node's id in place after a
            /// component's `view`/`render` returns it. Not exposed further
            /// than `pub(crate)`: application code never needs to mutate an
            /// already-constructed `Node`.
            pub(crate) fn children_mut(&mut self) -> &mut [Node] {
                &mut self.children
            }

            /// Returns this container's layout-specific style.
            pub fn style(&self) -> $style {
                self.style
            }

            /// Returns this node's layout style.
            pub fn layout(&self) -> LayoutStyle {
                self.layout
            }

            /// Returns this node's accessibility metadata.
            pub fn accessibility(&self) -> &AccessibilityInfo {
                &self.accessibility
            }
        }
    };
}

container_node!(Column, ColumnStyle);
container_node!(Row, RowStyle);

/// A structural problem detected while validating a raw [`Node`] tree's
/// identities, independent of the component runtime (see
/// `crate::component::RenderError` for the analogous, component-runtime-
/// aware error this feeds into).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// Two nodes in the same tree share a [`NodeId`].
    DuplicateNodeId(NodeId),
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateNodeId(id) => write!(f, "duplicate UI node id: {}", id.get()),
        }
    }
}

impl std::error::Error for TreeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::SizeMode;

    #[test]
    fn button_defaults_to_focusable_accessible_control() {
        let button = Node::button("go", "Go");
        assert_eq!(button.accessibility().role(), crate::event::AccessibilityRole::Button);
        assert!(
            button.accessibility().is_focusable(),
            "a button is a keyboard stop unless an application says otherwise"
        );
    }

    #[test]
    fn every_node_kind_defaults_to_the_role_that_describes_it() {
        use crate::event::AccessibilityRole;
        assert_eq!(Node::label("l", "x").accessibility().role(), AccessibilityRole::Label);
        assert_eq!(Node::button("b", "x").accessibility().role(), AccessibilityRole::Button);
        assert_eq!(Node::text_input("t", "x").accessibility().role(), AccessibilityRole::TextInput);
        assert_eq!(
            Node::column("c", [] as [Node; 0]).accessibility().role(),
            AccessibilityRole::Group
        );
        assert_eq!(
            Node::row("r", [] as [Node; 0]).accessibility().role(),
            AccessibilityRole::Group
        );
    }

    #[test]
    fn only_interactive_node_kinds_default_to_being_keyboard_stops() {
        assert!(!Node::label("l", "x").accessibility().is_focusable());
        assert!(Node::button("b", "x").accessibility().is_focusable());
        assert!(Node::text_input("t", "x").accessibility().is_focusable());
        assert!(!Node::column("c", [] as [Node; 0]).accessibility().is_focusable());
    }

    #[test]
    fn with_accessibility_still_fully_replaces_the_default() {
        let decorative = Node::button("b", "x").with_accessibility(
            AccessibilityInfo::new(crate::event::AccessibilityRole::None).focusable(false),
        );
        assert!(
            !decorative.accessibility().is_focusable(),
            "an application must be able to take a button out of the tab order"
        );
    }

    #[test]
    fn disabled_flag_round_trips() {
        let button = Node::button("go", "Go").disabled(true);
        assert!(button.is_disabled());
        let button = button.disabled(false);
        assert!(!button.is_disabled());
    }

    #[test]
    fn duplicate_local_keys_are_detected() {
        let tree = Node::column("root", [Node::label("dup", "a"), Node::label("dup", "b")]);
        assert!(matches!(tree.validate_unique_ids(), Err(TreeError::DuplicateNodeId(_))));
    }

    #[test]
    fn distinct_keys_reused_across_unrelated_subtrees_are_fine_before_scoping() {
        // `Node::label` alone (pre-component-scoping) intentionally allows
        // the same *raw* key to appear in two places that are not siblings
        // of one another structurally at this layer; scoping in
        // `crate::component::tree` is what makes reuse across independent
        // components safe. This just documents that `Node` itself performs
        // no scoping — see `component::tree::scope_component_node_ids`.
        let tree = Node::column(
            "root",
            [
                Node::column("a", [Node::label("submit", "A submit")]),
                Node::column("b", [Node::label("submit", "B submit")]),
            ],
        );
        assert!(matches!(tree.validate_unique_ids(), Err(TreeError::DuplicateNodeId(_))));
    }

    #[test]
    fn container_style_accessors_round_trip() {
        let column = Node::column_with_layout(
            "root",
            [],
            LayoutStyle::new().width(SizeMode::Fixed(10)),
            ColumnStyle::new().gap(4),
        );
        assert_eq!(column.column_style().unwrap().gap, 4);
        assert!(column.row_style().is_none());
    }
}
