//! An immutable, indexed snapshot of one rendered [`crate::Node`] tree.
//!
//! [`TreeSnapshot`] is what [`super::diff::TreeDiff`] compares two of, and
//! what [`crate::layout::LayoutEngine`] and a platform backend read
//! structure from. It pre-computes a children index and a depth index at
//! construction time specifically so that later operations —
//! `children_of`, `ordered_nodes`, depth lookups during diffing — are O(1)/
//! O(children) instead of rescanning every node in the tree, which is what
//! the standards audit's P1.11 finding ("avoidable O(n²) work") flags.

use std::collections::HashMap;

use crate::event::AccessibilityInfo;
use crate::identity::NodeId;
use crate::layout::{ColumnStyle, LayoutStyle, RowStyle};
use crate::node::{Node, TreeError};
use crate::style::{ControlState, Theme, VisualStyle};

/// A resolved, backend-facing view of one node.
///
/// Fields are public and read directly by every platform backend (this is
/// the primary contract `framework-windows`, and any future backend, reads
/// structure from). Unlike types such as [`crate::style::VisualStyle`],
/// encapsulating this behind accessor methods would not protect any
/// invariant — nothing outside `crate::reconcile` ever constructs or
/// mutates a `TreeNode` — it would only add call-site indirection for a
/// type whose whole purpose is to be read, so this crate makes a deliberate
/// exception to the "prefer private fields" default here (see the
/// standards audit's P1.21, "a good boundary... not... one file per
/// concern").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub id: NodeId,
    pub kind: crate::node::NodeKind,
    pub parent: Option<NodeId>,
    pub index: usize,
    pub text: Option<String>,
    pub layout: LayoutStyle,
    pub column_style: Option<ColumnStyle>,
    pub row_style: Option<RowStyle>,
    pub accessibility: AccessibilityInfo,
    /// The node's unresolved visual override, exactly as the application
    /// authored it. Kept alongside the theme-resolved `visual_style` below
    /// so a platform backend can re-resolve a single node's style against a
    /// live-only interaction state (hover/press/focus) without losing the
    /// application's own customization — see [`Self::visual_style`]'s doc
    /// comment for the two fields' respective roles.
    pub style_override: VisualStyle,
    /// The fully theme-resolved style: [`Self::style_override`] merged onto
    /// the active [`Theme`]'s default for this node's kind, in this node's
    /// `Normal`/`Disabled` state (see
    /// [`TreeSnapshot::from_node_with_theme`]). A backend applies *this*
    /// field for an ordinary render; it re-derives a fresh value from
    /// `style_override` only when synchronizing a transient interaction
    /// state (hover/press/focus) that a declarative rerender never observes.
    pub visual_style: VisualStyle,
    pub disabled: bool,
}

impl TreeNode {
    fn from_node(node: &Node, parent: Option<NodeId>, index: usize) -> Self {
        let text = match node {
            Node::Label(label) => Some(label.text().to_owned()),
            Node::Button(button) => Some(button.text().to_owned()),
            Node::TextInput(input) => Some(input.value().to_owned()),
            Node::Column(_) | Node::Row(_) => None,
        };

        Self {
            id: node.id(),
            kind: node.kind(),
            parent,
            index,
            text,
            layout: node.layout(),
            column_style: node.column_style(),
            row_style: node.row_style(),
            accessibility: node.accessibility().clone(),
            style_override: node.visual_style().clone(),
            visual_style: node.visual_style().clone(),
            disabled: node.is_disabled(),
        }
    }
}

/// An immutable, indexed snapshot of one rendered [`Node`] tree.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeSnapshot {
    nodes: HashMap<NodeId, TreeNode>,
    children: HashMap<Option<NodeId>, Vec<NodeId>>,
    depths: HashMap<NodeId, usize>,
}

impl TreeSnapshot {
    /// Builds an immutable snapshot of `root` for the layout/hit-testing
    /// passes to read from.
    ///
    /// # Errors
    ///
    /// Returns `TreeError::DuplicateNodeId` if two nodes in `root` share the
    /// same `NodeId` — every node key must be unique within a single view.
    pub fn from_node(root: &Node) -> Result<Self, TreeError> {
        root.validate_unique_ids()?;

        let mut nodes = HashMap::new();
        let mut children: HashMap<Option<NodeId>, Vec<NodeId>> = HashMap::new();
        let mut depths = HashMap::new();
        root.visit(&mut |node, parent, index| {
            let id = node.id();
            let depth = parent
                .and_then(|parent| depths.get(&parent).copied())
                .map_or(0usize, |parent_depth: usize| parent_depth.saturating_add(1));
            nodes.insert(id, TreeNode::from_node(node, parent, index));
            children.entry(parent).or_default().push(id);
            depths.insert(id, depth);
        });

        Ok(Self { nodes, children, depths })
    }

    /// Builds a snapshot whose `visual_style` on every node is fully resolved
    /// against `theme`: the theme's per-kind default merged with that node's
    /// own override, in the node's `Normal` (or `Disabled`, when the node is
    /// marked disabled) state. A platform backend can apply the resulting
    /// colors and typography directly without needing to know about `Theme`
    /// merge order itself, mirroring how layout geometry is already fully
    /// resolved in the core before a backend applies it.
    ///
    /// Interactive states that only the backend can observe live — hover,
    /// press, and focus — remain the backend's responsibility: it already
    /// tracks focus for Tab traversal and can re-resolve a single focused
    /// node's style against the same theme when focus changes.
    ///
    /// # Errors
    ///
    /// Returns `TreeError::DuplicateNodeId` under the same condition as
    /// [`Self::from_node`].
    pub fn from_node_with_theme(root: &Node, theme: &Theme) -> Result<Self, TreeError> {
        let mut snapshot = Self::from_node(root)?;
        for node in snapshot.nodes.values_mut() {
            let state = if node.disabled { ControlState::Disabled } else { ControlState::Normal };
            node.visual_style = theme.resolve(node.kind, state, &node.visual_style);
        }
        Ok(snapshot)
    }

    #[must_use]
    pub fn get(&self, id: NodeId) -> Option<&TreeNode> {
        self.nodes.get(&id)
    }
    #[must_use]
    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }
    pub fn nodes(&self) -> impl Iterator<Item = &TreeNode> {
        self.nodes.values()
    }

    /// Returns a node's children in declarative sibling order without
    /// rescanning the complete snapshot (backed by the children index built
    /// in [`Self::from_node`]).
    pub fn children_of(&self, parent: NodeId) -> impl Iterator<Item = &TreeNode> {
        self.children.get(&Some(parent)).into_iter().flatten().filter_map(|id| self.nodes.get(id))
    }

    /// Returns nodes in declarative preorder, preserving sibling indices.
    #[must_use]
    pub fn ordered_nodes(&self) -> Vec<&TreeNode> {
        let mut ordered = Vec::with_capacity(self.nodes.len());
        if let Some(root) = self.children.get(&None).and_then(|roots| roots.first()).copied() {
            collect_ordered_nodes(self, root, &mut ordered);
        }
        ordered
    }

    /// O(1) depth lookup backed by the index built in [`Self::from_node`],
    /// rather than walking the parent chain on every call.
    pub(crate) fn depth(&self, id: NodeId) -> usize {
        self.depths.get(&id).copied().unwrap_or(0)
    }
}

fn collect_ordered_nodes<'a>(
    snapshot: &'a TreeSnapshot,
    id: NodeId,
    output: &mut Vec<&'a TreeNode>,
) {
    let Some(node) = snapshot.get(id) else {
        return;
    };
    output.push(node);
    for child in snapshot.children_of(id) {
        collect_ordered_nodes(snapshot, child.id, output);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_flag_round_trips_into_tree_snapshot() {
        let node = Node::label("a", "hello").disabled(true);
        let snapshot = TreeSnapshot::from_node(&node).unwrap();
        assert!(snapshot.get(NodeId::from_key("a")).unwrap().disabled);
    }

    #[test]
    fn from_node_with_theme_resolves_normal_and_disabled_states() {
        // Use a theme with an explicit `disabled` override for buttons so
        // this test actually proves state selection, rather than merely
        // confirming that resolution doesn't panic: `Theme::default()`'s
        // button style defines no `disabled` override at all, so a
        // disabled button would legitimately resolve to the same style as
        // an enabled one there.
        let base = Theme::default();
        let button_style = crate::style::ComponentStyle {
            normal: base.button().normal.clone(),
            disabled: Some(VisualStyle::new().foreground(crate::style::Color::rgb(9, 9, 9))),
            ..Default::default()
        };
        let theme = base.with_button(button_style);

        let node = Node::column(
            "root",
            [Node::button("go", "Go"), Node::button("stop", "Stop").disabled(true)],
        );
        let snapshot = TreeSnapshot::from_node_with_theme(&node, &theme).unwrap();
        let go = snapshot.get(NodeId::from_key("go")).unwrap();
        let stop = snapshot.get(NodeId::from_key("stop")).unwrap();
        assert_eq!(
            go.visual_style.foreground_override(),
            theme.button().normal.foreground_override()
        );
        assert_eq!(
            stop.visual_style.foreground_override(),
            Some(crate::style::Color::rgb(9, 9, 9))
        );
        assert_ne!(stop.visual_style.foreground_override(), go.visual_style.foreground_override());
    }

    #[test]
    fn themed_snapshot_retains_node_override_for_live_platform_states() {
        let theme = Theme::default();
        let override_style = VisualStyle::new().foreground(crate::style::Color::rgb(9, 9, 9));
        let node = Node::button("go", "Go").with_style(override_style.clone());
        let snapshot = TreeSnapshot::from_node_with_theme(&node, &theme).unwrap();
        let go = snapshot.get(NodeId::from_key("go")).unwrap();
        assert_eq!(go.style_override, override_style);
    }

    #[test]
    fn children_of_and_ordered_nodes_use_the_precomputed_index() {
        let node = Node::column("root", [Node::label("a", "a"), Node::label("b", "b")]);
        let snapshot = TreeSnapshot::from_node(&node).unwrap();
        let children: Vec<_> =
            snapshot.children_of(NodeId::from_key("root")).map(|n| n.id).collect();
        assert_eq!(children, vec![NodeId::from_key("a"), NodeId::from_key("b")]);
        assert_eq!(snapshot.ordered_nodes().len(), 3);
    }
}
