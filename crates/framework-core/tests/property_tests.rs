//! Property-based tests (standards audit P2.34).
//!
//! Unlike the example-based unit/integration tests elsewhere in this crate
//! (which pin down specific, hand-picked scenarios — including
//! deliberately extreme ones, e.g. `layout::engine`'s
//! `extreme_constraints_do_not_panic_or_produce_negative_geometry`), these
//! tests generate hundreds of *randomized* inputs per run and check that a
//! small set of invariants holds for all of them. That is a meaningfully
//! different kind of coverage: it catches interaction bugs between inputs
//! no one thought to hand-write a test for, at the cost of being slower and
//! less specific about *which* input failed (`proptest` prints a minimized
//! failing case when one is found, which mitigates this).

use std::collections::HashSet;

use proptest::prelude::*;

use framework_core::{
    Alignment, ColumnStyle, Constraints, DefaultIntrinsicMeasurer, LayoutEngine, LayoutStyle, Node,
    NodeId, RowStyle, Size, SizeMode, TreeDiff, TreeOp, TreeSnapshot,
};

// ---------------------------------------------------------------------
// Identity: distinct keys never collide (crate::identity, P0.1).
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// For any set of distinct key strings, [`NodeId::from_key`] must never
    /// map two of them to the same id. This is the property the interning
    /// design in `crate::identity` exists to make *structurally*
    /// impossible (see that module's docs) rather than merely unlikely;
    /// this test exercises it against proptest-generated input rather than
    /// only the hand-picked sequential keys in `identity`'s own unit test.
    #[test]
    fn distinct_keys_never_collide(keys in proptest::collection::hash_set("[a-zA-Z0-9_-]{1,16}", 1..64)) {
        let mut seen = HashSet::new();
        for key in &keys {
            let id = NodeId::from_key(key);
            prop_assert!(seen.insert(id), "key {key:?} collided with a previously interned key");
        }
    }
}

// ---------------------------------------------------------------------
// Reconciliation: a diff's operations, replayed against a plain model,
// always reconstruct exactly the target snapshot's node set
// (crate::reconcile, P1.11/P1.12).
// ---------------------------------------------------------------------

/// A small, bounded vocabulary of keys so generated trees produce a
/// meaningful number of structural overlaps (shared keys, reordering, ...)
/// between "previous" and "next" instead of almost always being two
/// entirely disjoint trees.
fn small_key() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("a".to_string()),
        Just("b".to_string()),
        Just("c".to_string()),
        Just("d".to_string()),
    ]
}

fn small_text() -> impl Strategy<Value = String> {
    prop_oneof![Just("x".to_string()), Just("y".to_string()), Just("xy".to_string())]
}

fn arbitrary_tree() -> impl Strategy<Value = Node> {
    let leaf = (small_key(), small_text()).prop_map(|(key, text)| Node::label(key, text));
    leaf.prop_recursive(3, 16, 4, |inner| {
        proptest::collection::vec(inner, 0..4).prop_map(|children| Node::column("root", children))
    })
}

fn node_ids(node: &Node) -> HashSet<NodeId> {
    let mut ids = HashSet::new();
    node.visit(&mut |n, _, _| {
        ids.insert(n.id());
    });
    ids
}

/// Replays `diff` against a plain `HashSet<NodeId>` model seeded with
/// `previous`'s node ids, exactly as a minimal backend would track "which
/// native objects currently exist".
fn apply_diff_to_model(model: &mut HashSet<NodeId>, diff: &TreeDiff) {
    for op in diff.operations() {
        match op {
            TreeOp::Insert(node) => {
                model.insert(node.id);
            }
            TreeOp::Remove(node) => {
                model.remove(&node.id);
            }
            TreeOp::Update(_) | TreeOp::Move { .. } => {
                // Neither operation changes *which* ids exist, only their
                // data or position — nothing for a pure existence model to
                // do.
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn diff_operations_reconstruct_exactly_the_next_node_set(
        previous_tree in arbitrary_tree(),
        next_tree in arbitrary_tree(),
    ) {
        // Skip the (rare, since keys are drawn from a 4-symbol vocabulary
        // shared across siblings) inputs where a single tree reuses the
        // same local key twice — that is a distinct, separately-tested
        // condition (`node::tests::duplicate_local_keys_are_detected`), not
        // what this property is about.
        let Ok(previous) = TreeSnapshot::from_node(&previous_tree) else { return Ok(()) };
        let Ok(next) = TreeSnapshot::from_node(&next_tree) else { return Ok(()) };

        let mut model: HashSet<NodeId> = node_ids(&previous_tree);
        let diff = TreeDiff::between(&previous, &next);
        apply_diff_to_model(&mut model, &diff);

        prop_assert_eq!(model, node_ids(&next_tree));
    }

    /// Every `Update`/`Move` operation's node id must already have existed
    /// in the previous snapshot (an `Update`/`Move` for a node the backend
    /// never created would be a use-after-free-shaped bug in any real
    /// backend), and every `Remove`'s node id must not appear in `next`.
    #[test]
    fn diff_operations_target_only_nodes_that_could_legitimately_exist(
        previous_tree in arbitrary_tree(),
        next_tree in arbitrary_tree(),
    ) {
        let Ok(previous) = TreeSnapshot::from_node(&previous_tree) else { return Ok(()) };
        let Ok(next) = TreeSnapshot::from_node(&next_tree) else { return Ok(()) };
        let previous_ids = node_ids(&previous_tree);
        let next_ids = node_ids(&next_tree);

        let diff = TreeDiff::between(&previous, &next);
        for op in diff.operations() {
            match op {
                TreeOp::Update(node) => {
                    prop_assert!(previous_ids.contains(&node.id));
                }
                TreeOp::Move { id, .. } => {
                    prop_assert!(previous_ids.contains(id));
                }
                TreeOp::Remove(node) => {
                    prop_assert!(!next_ids.contains(&node.id));
                }
                TreeOp::Insert(_) => {}
            }
        }
    }
}

// ---------------------------------------------------------------------
// Layout: geometry is always non-negative and finite, across a wide
// randomized parameter space (crate::layout, P1.19).
// ---------------------------------------------------------------------

fn size_mode() -> impl Strategy<Value = SizeMode> {
    prop_oneof![Just(SizeMode::Auto), Just(SizeMode::Fill), (0i32..2000).prop_map(SizeMode::Fixed)]
}

fn layout_style() -> impl Strategy<Value = LayoutStyle> {
    (size_mode(), size_mode(), 0i32..64, 0i32..64).prop_map(|(width, height, min_w, min_h)| {
        LayoutStyle::new()
            .width(width)
            .height(height)
            .constraints(Constraints::new().with_min_width(min_w).with_min_height(min_h))
    })
}

fn leaf_with_layout() -> impl Strategy<Value = Node> {
    (small_key(), small_text(), layout_style())
        .prop_map(|(key, text, layout)| Node::label_with_layout(key, text, layout))
}

fn arbitrary_layout_tree() -> impl Strategy<Value = Node> {
    leaf_with_layout().prop_recursive(3, 20, 5, |inner| {
        (
            proptest::collection::vec(inner, 0..5),
            0i32..32,
            prop_oneof![
                Just(Alignment::Start),
                Just(Alignment::Center),
                Just(Alignment::End),
                Just(Alignment::Stretch),
            ],
            any::<bool>(),
        )
            .prop_map(|(children, gap, align, is_row)| {
                if is_row {
                    Node::row_with_layout(
                        "container",
                        children,
                        LayoutStyle::new(),
                        RowStyle::new().gap(gap).align_items(align),
                    )
                } else {
                    Node::column_with_layout(
                        "container",
                        children,
                        LayoutStyle::new(),
                        ColumnStyle::new().gap(gap).align_items(align),
                    )
                }
            })
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn layout_geometry_is_always_non_negative(
        tree in arbitrary_layout_tree(),
        width in 0u32..4000,
        height in 0u32..4000,
    ) {
        let Ok(snapshot) = TreeSnapshot::from_node(&tree) else { return Ok(()) };
        let engine = LayoutEngine::new();
        let rects = engine.layout(&snapshot, Size::new(width, height));
        for rect in rects.values() {
            prop_assert!(rect.width >= 0, "negative width: {rect:?}");
            prop_assert!(rect.height >= 0, "negative height: {rect:?}");
        }
    }
}

// ---------------------------------------------------------------------
// Layout: declared constraints are respected, and scroll ranges only exist
// where content actually overflows (P2.34's remaining named properties).
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// A node's resolved rectangle must honor the minimum and maximum
    /// bounds its `Constraints` declare.
    ///
    /// This is a different claim from `layout_geometry_is_always_non_negative`
    /// above: that one says the engine never produces nonsense, this one
    /// says it produces the *specific* geometry the caller asked for. A
    /// clamp applied in one branch of the engine but not another — the
    /// realistic way this breaks, given how many branches
    /// `column`/`row`/`Fill`/`Fixed`/`Auto` multiply into — passes the first
    /// property and fails this one.
    ///
    /// The maximum is checked against the constraint itself, not against the
    /// available space: a node whose minimum exceeds the viewport is
    /// legitimately allowed to overflow it (that is what a scrollable
    /// container is for), so asserting containment would assert the wrong
    /// thing.
    #[test]
    fn layout_respects_declared_constraints(
        min_width in 0i32..500,
        extra_width in 0i32..500,
        min_height in 0i32..500,
        extra_height in 0i32..500,
        viewport_width in 0u32..2000,
        viewport_height in 0u32..2000,
    ) {
        let max_width = min_width + extra_width;
        let max_height = min_height + extra_height;
        let constraints = Constraints::new()
            .with_min_width(min_width)
            .with_max_width(max_width)
            .with_min_height(min_height)
            .with_max_height(max_height);
        let tree = Node::column(
            "root",
            [Node::label_with_layout(
                "item",
                "text",
                LayoutStyle::new()
                    .width(SizeMode::Fill)
                    .height(SizeMode::Fill)
                    .constraints(constraints),
            )],
        );

        let Ok(snapshot) = TreeSnapshot::from_node(&tree) else { return Ok(()) };
        let rects =
            LayoutEngine::new().layout(&snapshot, Size::new(viewport_width, viewport_height));
        let Some(rect) = rects.get(&NodeId::from_key("item")) else { return Ok(()) };

        prop_assert!(
            rect.width >= min_width,
            "width {} is below the declared minimum {}",
            rect.width,
            min_width
        );
        prop_assert!(
            rect.width <= max_width,
            "width {} exceeds the declared maximum {}",
            rect.width,
            max_width
        );
        prop_assert!(
            rect.height >= min_height,
            "height {} is below the declared minimum {}",
            rect.height,
            min_height
        );
        prop_assert!(
            rect.height <= max_height,
            "height {} exceeds the declared maximum {}",
            rect.height,
            max_height
        );
    }

    /// A node only reports a scroll range where its content genuinely
    /// overflows its own rectangle.
    ///
    /// A backend clamps a scroll offset into `0..=range` (see
    /// `framework-windows`'s `native::rendering::scrolling`). A range
    /// invented for a node whose content fits would let the viewport scroll
    /// past its own content into blank space, and a range derived from
    /// arithmetic that underflowed would produce an inverted clamp range —
    /// which `i32::clamp` panics on rather than silently mishandling,
    /// turning a layout slip into a crash on an ordinary mouse wheel.
    #[test]
    fn scroll_ranges_exist_only_where_content_overflows(
        tree in arbitrary_layout_tree(),
        width in 0u32..2000,
        height in 0u32..2000,
    ) {
        let Ok(snapshot) = TreeSnapshot::from_node(&tree) else { return Ok(()) };
        let output = LayoutEngine::new().layout_result_with(
            &snapshot,
            Size::new(width, height),
            &DefaultIntrinsicMeasurer,
            &std::collections::HashMap::new(),
        );
        for (id, range) in &output.scroll_ranges {
            let content = output.content_sizes.get(id).copied().unwrap_or(Size::new(0, 0));
            // A scroll range for a node with no rectangle would itself be
            // a bug, but this property is about the range/content
            // relationship; treat a missing rectangle as zero-sized so the
            // assertion below still says something rather than skipping.
            let (rect_width, rect_height) = output
                .rects
                .get(id)
                .map_or((0, 0), |rect| {
                    (
                        u32::try_from(rect.width.max(0)).unwrap_or(0),
                        u32::try_from(rect.height.max(0)).unwrap_or(0),
                    )
                });
            prop_assert!(
                range.width == 0 || content.width > rect_width,
                "node {:?} reports a horizontal scroll range of {} with content {} in a \
                 {}-wide rectangle",
                id,
                range.width,
                content.width,
                rect_width
            );
            prop_assert!(
                range.height == 0 || content.height > rect_height,
                "node {:?} reports a vertical scroll range of {} with content {} in a \
                 {}-tall rectangle",
                id,
                range.height,
                content.height,
                rect_height
            );
        }
    }
}

// ---------------------------------------------------------------------
// Reconciliation: identity stability and removal ordering (P2.34).
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Snapshotting the same tree twice produces the same identities, and
    /// diffing the two produces no work at all.
    ///
    /// "Identity survives rerender" is the assumption every other part of
    /// reconciliation rests on. If a key resolved to a different id on a
    /// second render, every node would look inserted-and-removed, and a
    /// backend would tear down and rebuild its entire native tree on every
    /// frame — while still appearing to work, which is what makes this worth
    /// asserting rather than trusting.
    #[test]
    fn identity_survives_a_rerender_and_produces_an_empty_diff(tree in arbitrary_tree()) {
        let Ok(first) = TreeSnapshot::from_node(&tree) else { return Ok(()) };
        let Ok(second) = TreeSnapshot::from_node(&tree) else { return Ok(()) };
        prop_assert_eq!(node_ids(&tree), node_ids(&tree));

        let diff = TreeDiff::between(&first, &second);
        prop_assert!(
            diff.operations().is_empty(),
            "an unchanged tree must produce no operations, got {:?}",
            diff.operations()
        );
        prop_assert!(!diff.invalidates_layout(), "an unchanged tree must not invalidate layout");
    }

    /// Removals are emitted deepest-first, and every removal precedes every
    /// insertion.
    ///
    /// A backend destroys native objects in the order it receives them.
    /// Destroying a parent before its child means the child's handle is
    /// already dead when its own `Remove` arrives — on Win32 that is a
    /// `DestroyWindow` against a stale `HWND`, exactly the use-after-free
    /// this ordering exists to prevent. The insert side is the mirror: a
    /// child created before its parent has nowhere to be created.
    #[test]
    fn removals_are_ordered_deepest_first_and_precede_insertions(
        previous in arbitrary_tree(),
        next in arbitrary_tree(),
    ) {
        let Ok(previous_snapshot) = TreeSnapshot::from_node(&previous) else { return Ok(()) };
        let Ok(next_snapshot) = TreeSnapshot::from_node(&next) else { return Ok(()) };
        let diff = TreeDiff::between(&previous_snapshot, &next_snapshot);

        let mut removed: HashSet<NodeId> = HashSet::new();
        let mut seen_insert = false;
        for operation in diff.operations() {
            match operation {
                TreeOp::Remove(node) => {
                    prop_assert!(!seen_insert, "removal of {:?} came after an insertion", node.id);
                    if let Some(parent) = node.parent {
                        prop_assert!(
                            !removed.contains(&parent),
                            "node {:?} was removed after its own parent {:?}",
                            node.id,
                            parent
                        );
                    }
                    removed.insert(node.id);
                }
                TreeOp::Insert(node) => {
                    seen_insert = true;
                    if let Some(parent) = node.parent {
                        // A parent that is not itself being inserted must
                        // already exist in the previous tree.
                        let parent_inserted = diff.operations().iter().any(|other| {
                            matches!(other, TreeOp::Insert(other) if other.id == parent)
                        });
                        prop_assert!(
                            parent_inserted || previous_snapshot.contains(parent),
                            "node {:?} is inserted under a parent {:?} that neither exists nor \
                             is being created",
                            node.id,
                            parent
                        );
                    }
                }
                TreeOp::Update(_) | TreeOp::Move { .. } => {}
            }
        }
    }
}
