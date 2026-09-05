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
    Alignment, ColumnStyle, Constraints, LayoutEngine, LayoutStyle, Node, NodeId, RowStyle, Size,
    SizeMode, TreeDiff, TreeOp, TreeSnapshot,
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
