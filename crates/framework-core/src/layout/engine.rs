//! The platform-independent layout pass.
//!
//! Layout runs as a separate phase after reconciliation (see
//! `crate::reconcile`), consuming a [`crate::reconcile::TreeSnapshot`] and
//! producing absolute-within-parent rectangles, clip regions, content sizes,
//! and scroll ranges. A platform backend applies the result to native
//! objects; nothing in this module talks to a platform.
//!
//! # Overflow safety
//!
//! Every arithmetic operation in this module is saturating rather than
//! wrapping or panicking. An application can put arbitrarily large values
//! into `LayoutStyle`/`Constraints` (deliberately or through a bug), and a
//! production layout engine must degrade to a clamped, still-rectangular
//! geometry rather than let a single pathological node crash the whole
//! layout pass — see the standards audit's P1.19 finding, "integer-overflow
//! hazards".
//!
//! # A note on this module's `i32`/`u32` casts
//!
//! This module works in `i32` internally (negative intermediate values —
//! e.g. "how much space is left after subtracting margins" before it's
//! floored at zero — are a normal, meaningful part of the arithmetic) but
//! exchanges `u32` at its [`crate::layout::geometry::Size`] boundary. Every
//! `as u32`/`as i32` cast in this file is preceded by an explicit
//! `.max(0)`/`.min(i32::MAX as _)` that establishes the value is in range
//! for the target type — an invariant this module's own tests
//! (`extreme_constraints_do_not_panic_or_produce_negative_geometry`) check
//! directly, but that `clippy::cast_sign_loss`/`cast_possible_wrap`/
//! `cast_possible_truncation` cannot see through a `.max(0)` call to prove
//! statically. Allowed at the module level rather than per cast site, since
//! re-litigating the same already-tested invariant at every one of this
//! module's several dozen casts would be pure noise, not added safety.
#![allow(clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::cast_possible_truncation)]

use std::collections::HashMap;

use super::constraints::ResolvedContainerStyle;
use super::geometry::{Alignment, EdgeInsets, Overflow, Point, Rect, Size, SizeMode};
use super::measure::{DefaultIntrinsicMeasurer, IntrinsicMeasurer};
use crate::identity::NodeId;
use crate::node::NodeKind;
use crate::reconcile::{TreeNode, TreeSnapshot};

/// Whether the most recent tree diff requires a full relayout.
///
/// Currently a coarse on/off flag; see [`crate::reconcile::TreeDiff`] for the
/// change-classification (layout-affecting vs. paint-only vs.
/// accessibility-only) that decides when `full` is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayoutInvalidation {
    /// Whether every node's geometry must be recomputed from scratch.
    pub full: bool,
}

impl LayoutInvalidation {
    /// No relayout is required.
    #[must_use]
    pub const fn none() -> Self {
        Self { full: false }
    }
    /// A full relayout is required.
    #[must_use]
    pub const fn full() -> Self {
        Self { full: true }
    }
}

/// The complete output of one layout pass.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LayoutResult {
    /// Every node's rectangle, in its native parent's local coordinate
    /// space (children never inherit an ancestor's absolute position; see
    /// the module-level coordinate-space contract this mirrors from the
    /// original single-file design).
    pub rects: HashMap<NodeId, Rect>,
    /// Clip regions for containers whose overflow is not `Visible`.
    pub clips: HashMap<NodeId, Rect>,
    /// Each container's *unscrolled* content size — this must never be
    /// affected by the container's current scroll offset, or scrolling
    /// would feed back into its own available range.
    pub content_sizes: HashMap<NodeId, Size>,
    /// `content_size - viewport_size` (floored at zero) for every
    /// container, i.e. how far it can scroll along each axis.
    pub scroll_ranges: HashMap<NodeId, Size>,
}

/// Computes absolute-within-parent geometry for a [`TreeSnapshot`]; see
/// the module-level documentation for the full contract.
#[derive(Debug, Default)]
pub struct LayoutEngine;

impl LayoutEngine {
    /// Creates a layout engine.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Lays out `snapshot` within `size` using the default intrinsic-size
    /// measurer, returning each node's rectangle.
    #[must_use]
    pub fn layout(&self, snapshot: &TreeSnapshot, size: Size) -> HashMap<NodeId, Rect> {
        self.layout_with(snapshot, size, &DefaultIntrinsicMeasurer)
    }

    /// Lays out `snapshot` within `size` using `measurer` for any node
    /// whose size depends on measuring its own content (e.g. `Auto`-sized
    /// text), returning each node's rectangle. See
    /// [`Self::layout_result_with`] for the complete [`LayoutResult`]
    /// (clip regions, content sizes, and scroll ranges as well).
    pub fn layout_with<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        size: Size,
        measurer: &M,
    ) -> HashMap<NodeId, Rect> {
        self.layout_result_with(snapshot, size, measurer, &HashMap::new()).rects
    }

    /// Runs a full layout pass. `_scroll_offsets` is accepted for API
    /// symmetry with how a backend tracks per-container scroll position, but
    /// intentionally unused here: scroll offset is a viewport transform a
    /// backend applies when placing native objects, not an input to how much
    /// content there is or how large it naturally wants to be (see
    /// [`LayoutResult::content_sizes`]'s doc comment).
    pub fn layout_result_with<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        size: Size,
        measurer: &M,
        _scroll_offsets: &HashMap<NodeId, Point>,
    ) -> LayoutResult {
        let mut result = LayoutResult::default();
        let Some(root) = snapshot.ordered_nodes().into_iter().next() else {
            return result;
        };

        let root_rect = Rect::new(
            0,
            0,
            size.width.min(i32::MAX as u32) as i32,
            size.height.min(i32::MAX as u32) as i32,
        );

        self.layout_node(snapshot, root, root_rect, &mut result, measurer);
        result
    }

    fn layout_node<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        rect: Rect,
        result: &mut LayoutResult,
        measurer: &M,
    ) {
        // Rectangles are always expressed in the coordinate space of the
        // node's native parent. Descendants therefore start from (0, 0)
        // inside their own native container instead of inheriting the
        // parent's absolute position.
        result.rects.insert(node.id, rect);
        let content_rect = Rect::new(0, 0, rect.width, rect.height);

        let children = snapshot.children_of(node.id).collect::<Vec<_>>();

        match node.kind {
            NodeKind::Column => {
                let style = node.column_style.unwrap_or_default();
                if !matches!(style.overflow, Overflow::Visible) {
                    result.clips.insert(node.id, Rect::new(0, 0, rect.width, rect.height));
                }
                let content_size = self.layout_column(
                    snapshot,
                    content_rect,
                    &children,
                    style.into(),
                    result,
                    measurer,
                );
                result.content_sizes.insert(node.id, content_size);
                result.scroll_ranges.insert(
                    node.id,
                    Size::new(
                        content_size.width.saturating_sub(rect.width.max(0) as u32),
                        content_size.height.saturating_sub(rect.height.max(0) as u32),
                    ),
                );
            }
            NodeKind::Row => {
                let style = node.row_style.unwrap_or_default();
                if !matches!(style.overflow, Overflow::Visible) {
                    result.clips.insert(node.id, Rect::new(0, 0, rect.width, rect.height));
                }
                let content_size = self.layout_row(
                    snapshot,
                    content_rect,
                    &children,
                    style.into(),
                    result,
                    measurer,
                );
                result.content_sizes.insert(node.id, content_size);
                result.scroll_ranges.insert(
                    node.id,
                    Size::new(
                        content_size.width.saturating_sub(rect.width.max(0) as u32),
                        content_size.height.saturating_sub(rect.height.max(0) as u32),
                    ),
                );
            }
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput => {}
        }
    }

    fn layout_column<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        rect: Rect,
        children: &[&TreeNode],
        style: ResolvedContainerStyle,
        result: &mut LayoutResult,
        measurer: &M,
    ) -> Size {
        let ResolvedContainerStyle { padding, gap, align_items } = style;
        let content = inner_rect(rect, padding);
        if children.is_empty() {
            return Size::new(padding.horizontal().max(0) as u32, padding.vertical().max(0) as u32);
        }

        let gap_total = gap.max(0).saturating_mul(child_gap_count(children.len()));
        let usable_height = content.height.saturating_sub(gap_total).max(0);
        let preferred_height = children
            .iter()
            .map(|child| {
                self.preferred_height(snapshot, child, measurer, preferred_width_hint(child))
            })
            .fold(0, i32::saturating_add);
        let fill_count =
            children.iter().filter(|child| matches!(child.layout.height, SizeMode::Fill)).count();
        let distributable = usable_height.saturating_sub(preferred_height).max(0);
        let fill_count_i32 = fill_count.min(i32::MAX as usize) as i32;
        let share = if fill_count == 0 { 0 } else { distributable / fill_count_i32 };
        let remainder = if fill_count == 0 { 0 } else { distributable % fill_count_i32 };

        let mut y = content.y;
        let mut fill_index = 0;
        for (index, child) in children.iter().enumerate() {
            let margin = child.layout.margin;
            y = y.saturating_add(margin.top);
            let height = match child.layout.height {
                SizeMode::Fixed(value) => value.max(0),
                SizeMode::Auto => {
                    self.preferred_height(snapshot, child, measurer, preferred_width_hint(child))
                }
                SizeMode::Fill => {
                    let extra = if fill_index == 0 { remainder } else { 0 };
                    fill_index += 1;
                    (self
                        .preferred_height(snapshot, child, measurer, preferred_width_hint(child))
                        .saturating_add(share)
                        .saturating_add(extra))
                    .max(0)
                }
            };

            let available_width = content.width.saturating_sub(margin.horizontal()).max(0);
            let alignment = child.layout.align_self.unwrap_or(align_items);
            let width = match (alignment, child.layout.width) {
                (Alignment::Stretch, SizeMode::Auto | SizeMode::Fill) => available_width,
                (_, SizeMode::Auto) => {
                    self.preferred_width(snapshot, child, measurer).min(available_width)
                }
                (_, mode) => resolve_width(mode, available_width),
            };
            let width =
                child.layout.constraints.clamp_width(width.max(0)).min(available_width.max(0));
            let height = child.layout.constraints.clamp_height(height.max(0));
            let x = aligned_start(content.x, margin.left, available_width, width, alignment);
            let child_rect = Rect::new(x, y, width, height);
            self.layout_node(snapshot, child, child_rect, result, measurer);

            y = y.saturating_add(height).saturating_add(margin.bottom);
            if index + 1 < children.len() {
                y = y.saturating_add(gap.max(0));
            }
        }

        // Content size must be computed from the unscrolled layout. Scrolling
        // changes the viewport position of children; it must never change the
        // size of the content itself or the available scroll range.
        let natural_height = y.saturating_add(padding.bottom).max(rect.height);
        let natural_width = children
            .iter()
            .map(|child| {
                result.rects.get(&child.id).map_or(0, |r| {
                    r.x.saturating_add(r.width).saturating_add(child.layout.margin.right)
                })
            })
            .max()
            .unwrap_or(0)
            .max(rect.width);

        Size::new(natural_width.max(0) as u32, natural_height.max(0) as u32)
    }

    fn layout_row<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        rect: Rect,
        children: &[&TreeNode],
        style: ResolvedContainerStyle,
        result: &mut LayoutResult,
        measurer: &M,
    ) -> Size {
        let ResolvedContainerStyle { padding, gap, align_items } = style;
        let content = inner_rect(rect, padding);
        if children.is_empty() {
            return Size::new(padding.horizontal().max(0) as u32, padding.vertical().max(0) as u32);
        }

        let gap_total = gap.max(0).saturating_mul(child_gap_count(children.len()));
        let usable_width = content.width.saturating_sub(gap_total).max(0);
        let preferred_width = children
            .iter()
            .map(|child| self.preferred_width(snapshot, child, measurer))
            .fold(0, i32::saturating_add);
        let fill_count =
            children.iter().filter(|child| matches!(child.layout.width, SizeMode::Fill)).count();
        let distributable = usable_width.saturating_sub(preferred_width).max(0);
        let fill_count_i32 = fill_count.min(i32::MAX as usize) as i32;
        let share = if fill_count == 0 { 0 } else { distributable / fill_count_i32 };
        let remainder = if fill_count == 0 { 0 } else { distributable % fill_count_i32 };

        let mut x = content.x;
        let mut fill_index = 0;
        for (index, child) in children.iter().enumerate() {
            let margin = child.layout.margin;
            x = x.saturating_add(margin.left);
            let width = match child.layout.width {
                SizeMode::Fixed(value) => value.max(0),
                SizeMode::Auto => self.preferred_width(snapshot, child, measurer),
                SizeMode::Fill => {
                    let extra = if fill_index == 0 { remainder } else { 0 };
                    fill_index += 1;
                    self.preferred_width(snapshot, child, measurer)
                        .saturating_add(share)
                        .saturating_add(extra)
                        .max(0)
                }
            };

            let available_height = content.height.saturating_sub(margin.vertical()).max(0);
            let alignment = child.layout.align_self.unwrap_or(align_items);
            let height = match (alignment, child.layout.height) {
                (Alignment::Stretch, SizeMode::Auto | SizeMode::Fill) => available_height,
                (_, SizeMode::Auto) => self
                    .preferred_height(snapshot, child, measurer, Some(width))
                    .min(available_height),
                (_, mode) => resolve_width(mode, available_height),
            };
            let width = child.layout.constraints.clamp_width(width.max(0));
            let height =
                child.layout.constraints.clamp_height(height.max(0)).min(available_height.max(0));
            let y = aligned_start(content.y, margin.top, available_height, height, alignment);
            let child_rect = Rect::new(x, y, width, height);
            self.layout_node(snapshot, child, child_rect, result, measurer);

            x = x.saturating_add(width).saturating_add(margin.right);
            if index + 1 < children.len() {
                x = x.saturating_add(gap.max(0));
            }
        }

        // As with Column, measure the unscrolled content first. The scroll
        // offset is a viewport transform and must not feed back into the
        // intrinsic content size.
        let natural_width = x.saturating_add(padding.right).max(rect.width);
        let natural_height = children
            .iter()
            .map(|child| {
                result.rects.get(&child.id).map_or(0, |r| {
                    r.y.saturating_add(r.height).saturating_add(child.layout.margin.bottom)
                })
            })
            .max()
            .unwrap_or(0)
            .max(rect.height);

        Size::new(natural_width.max(0) as u32, natural_height.max(0) as u32)
    }

    fn preferred_width<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        measurer: &M,
    ) -> i32 {
        let base = match node.kind {
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput => {
                measurer.measure(node.kind, node.text.as_deref(), None).width as i32
            }
            NodeKind::Column | NodeKind::Row => {
                self.container_preferred_width(snapshot, node, measurer)
            }
        };
        node.layout.constraints.clamp_width(base.saturating_add(node.layout.margin.horizontal()))
    }

    fn preferred_height<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        measurer: &M,
        max_width: Option<i32>,
    ) -> i32 {
        let height = self
            .preferred_content_height(snapshot, node, measurer, max_width)
            .saturating_add(node.layout.margin.vertical());
        node.layout.constraints.clamp_height(height)
    }

    fn preferred_content_height<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        measurer: &M,
        max_width: Option<i32>,
    ) -> i32 {
        match node.kind {
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput => {
                measurer.measure(node.kind, node.text.as_deref(), max_width).height as i32
            }
            NodeKind::Column => {
                let style = node.column_style.unwrap_or_default();
                let children = ordered_children(snapshot, node.id);
                style
                    .padding
                    .vertical()
                    .saturating_add(
                        children
                            .iter()
                            .map(|child| {
                                self.preferred_height(snapshot, child, measurer, max_width)
                            })
                            .fold(0, i32::saturating_add),
                    )
                    .saturating_add(
                        style.gap.max(0).saturating_mul(child_gap_count(children.len())),
                    )
            }
            NodeKind::Row => {
                let style = node.row_style.unwrap_or_default();
                let children = ordered_children(snapshot, node.id);
                style.padding.vertical().saturating_add(
                    children
                        .iter()
                        .map(|child| {
                            self.preferred_height(
                                snapshot,
                                child,
                                measurer,
                                preferred_width_hint(child).or(max_width),
                            )
                        })
                        .max()
                        .unwrap_or(0),
                )
            }
        }
    }

    fn container_preferred_width<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        measurer: &M,
    ) -> i32 {
        let children = ordered_children(snapshot, node.id);
        match node.kind {
            NodeKind::Column => {
                let style = node.column_style.unwrap_or_default();
                style.padding.horizontal().saturating_add(
                    children
                        .iter()
                        .map(|child| self.preferred_width(snapshot, child, measurer))
                        .max()
                        .unwrap_or(0),
                )
            }
            NodeKind::Row => {
                let style = node.row_style.unwrap_or_default();
                style
                    .padding
                    .horizontal()
                    .saturating_add(
                        children
                            .iter()
                            .map(|child| self.preferred_width(snapshot, child, measurer))
                            .fold(0, i32::saturating_add),
                    )
                    .saturating_add(
                        style.gap.max(0).saturating_mul(child_gap_count(children.len())),
                    )
            }
            _ => 0,
        }
    }
}

/// `count - 1` gaps between `count` siblings, saturated at zero and clamped
/// into `i32` range so a pathologically large sibling count cannot overflow
/// the later `saturating_mul` by itself producing a wrapped *negative* count.
fn child_gap_count(sibling_count: usize) -> i32 {
    sibling_count.saturating_sub(1).min(i32::MAX as usize) as i32
}

fn preferred_width_hint(node: &TreeNode) -> Option<i32> {
    match node.layout.width {
        SizeMode::Fixed(value) => Some(value.max(0)),
        SizeMode::Auto | SizeMode::Fill => node.layout.constraints.max_width(),
    }
}

fn ordered_children(snapshot: &TreeSnapshot, parent: NodeId) -> Vec<&TreeNode> {
    snapshot.children_of(parent).collect()
}

fn inner_rect(rect: Rect, padding: EdgeInsets) -> Rect {
    Rect::new(
        rect.x.saturating_add(padding.left),
        rect.y.saturating_add(padding.top),
        rect.width.saturating_sub(padding.horizontal()).max(0),
        rect.height.saturating_sub(padding.vertical()).max(0),
    )
}

fn aligned_start(
    origin: i32,
    margin_start: i32,
    available: i32,
    size: i32,
    alignment: Alignment,
) -> i32 {
    let remaining = available.saturating_sub(size).max(0);
    origin.saturating_add(margin_start).saturating_add(match alignment {
        Alignment::Start | Alignment::Stretch => 0,
        Alignment::Center => remaining / 2,
        Alignment::End => remaining,
    })
}

fn resolve_width(mode: SizeMode, available: i32) -> i32 {
    match mode {
        SizeMode::Fixed(value) => value.max(0).min(available.max(0)),
        SizeMode::Auto | SizeMode::Fill => available.max(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{ColumnStyle, Constraints, LayoutStyle};
    use crate::node::Node;

    fn snapshot_of(node: &Node) -> TreeSnapshot {
        TreeSnapshot::from_node(node).expect("test tree must have unique ids")
    }

    #[test]
    fn layout_respects_fixed_height_and_gap() {
        let tree = Node::column(
            "root",
            [
                Node::label_with_layout("a", "a", LayoutStyle::new().height(SizeMode::Fixed(10))),
                Node::label_with_layout("b", "b", LayoutStyle::new().height(SizeMode::Fixed(20))),
            ],
        );
        let snapshot = snapshot_of(&tree);
        let engine = LayoutEngine::new();
        let rects = engine.layout(&snapshot, Size::new(200, 200));
        let a = rects[&NodeId::from_key("a")];
        let b = rects[&NodeId::from_key("b")];
        assert_eq!(a.height, 10);
        assert_eq!(b.height, 20);
        assert!(b.y > a.y);
    }

    #[test]
    fn nested_layout_coordinates_are_relative_to_parent() {
        let tree = Node::column(
            "root",
            [Node::column_with_layout(
                "inner",
                [Node::label("leaf", "leaf")],
                LayoutStyle::new(),
                ColumnStyle::new().padding(EdgeInsets::all(5)),
            )],
        );
        let snapshot = snapshot_of(&tree);
        let engine = LayoutEngine::new();
        let rects = engine.layout(&snapshot, Size::new(200, 200));
        let leaf = rects[&NodeId::from_key("leaf")];
        // Relative to `inner`'s own content rect, not the root's.
        assert_eq!(leaf.x, 5);
        assert_eq!(leaf.y, 5);
    }

    #[test]
    fn extreme_constraints_do_not_panic_or_produce_negative_geometry() {
        let tree = Node::column(
            "root",
            [Node::label_with_layout(
                "huge",
                "x".repeat(10_000),
                LayoutStyle::new()
                    .width(SizeMode::Fixed(i32::MAX))
                    .height(SizeMode::Fixed(i32::MAX))
                    .constraints(Constraints::new().with_max_width(i32::MAX)),
            )],
        );
        let snapshot = snapshot_of(&tree);
        let engine = LayoutEngine::new();
        let rects = engine.layout(&snapshot, Size::new(u32::MAX, u32::MAX));
        let huge = rects[&NodeId::from_key("huge")];
        assert!(huge.width >= 0);
        assert!(huge.height >= 0);
    }
}
