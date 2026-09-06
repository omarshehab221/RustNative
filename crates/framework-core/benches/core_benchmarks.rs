//! Benchmarks for the hot paths the standards audit's P1.11/P1.19 findings
//! were about: tree reconciliation, layout, and scheduling. Run with
//! `cargo bench -p framework-core`.
//!
//! These exist to make a future regression in these paths *visible* (a
//! reconciliation change that reintroduces O(n²) child lookups, a layout
//! change that adds allocation churn) rather than to assert a specific
//! absolute number — this crate does not currently gate CI on benchmark
//! output, matching `criterion`'s own guidance that meaningful thresholds
//! depend on the machine running them.
//!
//! `unwrap`/`expect` are allowed throughout this file for the same reason
//! `clippy.toml` allows them in tests: a benchmark's setup code asserts its
//! own preconditions, and a panic there is the assertion. Clippy's
//! `allow-unwrap-in-tests` does not reach a `benches/` target, which is a
//! separate crate with no `cfg(test)`, so the exemption is stated here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};

use framework_core::{
    Component, ComponentContext, ComponentTree, Event, LayoutEngine, LayoutStyle, Node, RowStyle,
    Size, SizeMode, TreeDiff, TreeSnapshot,
};

/// Builds a moderately wide-and-deep tree: `width` labelled children per
/// level, `depth` levels, which is representative of a real settings/form
/// screen rather than either extreme (a single deep chain, or a single wide
/// list) — see the individual benchmark functions below for where each
/// shape matters more.
fn build_tree(width: usize, depth: usize, text_prefix: &str) -> Node {
    fn build(width: usize, depth: usize, path: &str) -> Node {
        if depth == 0 {
            return Node::label(path, format!("{path}-label"));
        }
        let children =
            (0..width).map(|i| build(width, depth - 1, &format!("{path}-{i}"))).collect::<Vec<_>>();
        Node::column(path, children)
    }
    build(width, depth, text_prefix)
}

fn bench_snapshot_construction(c: &mut Criterion) {
    let tree = build_tree(8, 3, "n");
    c.bench_function("tree_snapshot_from_node/8x3", |b| {
        b.iter(|| black_box(TreeSnapshot::from_node(black_box(&tree)).unwrap()));
    });
}

fn bench_diff_no_changes(c: &mut Criterion) {
    let tree = build_tree(8, 3, "n");
    let previous = TreeSnapshot::from_node(&tree).unwrap();
    let next = TreeSnapshot::from_node(&tree).unwrap();
    c.bench_function("tree_diff_between/8x3/no_changes", |b| {
        b.iter(|| black_box(TreeDiff::between(black_box(&previous), black_box(&next))));
    });
}

fn bench_diff_leaf_text_change(c: &mut Criterion) {
    fn build(width: usize, depth: usize, path: &str, override_path: &str) -> Node {
        if depth == 0 {
            let text =
                if path == override_path { "changed".to_string() } else { format!("{path}-label") };
            return Node::label(path, text);
        }
        let children = (0..width)
            .map(|i| build(width, depth - 1, &format!("{path}-{i}"), override_path))
            .collect::<Vec<_>>();
        Node::column(path, children)
    }

    let previous_tree = build_tree(8, 3, "n");
    // Perturb exactly one leaf's text, which is the common case a real
    // application re-render produces: this benchmark's value is in showing
    // that a single-node change stays cheap regardless of overall tree
    // size, which an accidentally-reintroduced O(n^2) full-tree rescan
    // would not.
    let next_tree = build(8, 3, "n", "n-0-0-0");
    let previous = TreeSnapshot::from_node(&previous_tree).unwrap();
    let next = TreeSnapshot::from_node(&next_tree).unwrap();
    c.bench_function("tree_diff_between/8x3/single_leaf_text_change", |b| {
        b.iter(|| black_box(TreeDiff::between(black_box(&previous), black_box(&next))));
    });
}

fn bench_layout_wide_row(c: &mut Criterion) {
    let children = (0..200)
        .map(|i| {
            Node::label_with_layout(
                format!("item-{i}"),
                "item",
                LayoutStyle::new().width(SizeMode::Fixed(80)),
            )
        })
        .collect::<Vec<_>>();
    let tree = Node::row_with_layout("root", children, LayoutStyle::new(), RowStyle::new().gap(4));
    let snapshot = TreeSnapshot::from_node(&tree).unwrap();
    let engine = LayoutEngine::new();
    c.bench_function("layout/row_of_200_fixed_width_leaves", |b| {
        b.iter(|| black_box(engine.layout(black_box(&snapshot), Size::new(4000, 200))));
    });
}

fn bench_layout_nested_columns(c: &mut Criterion) {
    let tree = build_tree(6, 4, "n");
    let snapshot = TreeSnapshot::from_node(&tree).unwrap();
    let engine = LayoutEngine::new();
    c.bench_function("layout/nested_columns_6x4", |b| {
        b.iter(|| black_box(engine.layout(black_box(&snapshot), Size::new(1200, 1200))));
    });
}

// --- Component tree / scheduler ------------------------------------------

#[derive(Clone, PartialEq, Default)]
struct NoProps;

struct Counter {
    count: u32,
}

impl Component for Counter {
    type Props = NoProps;
    type Message = ();

    fn new(_props: Self::Props) -> Self {
        Self { count: 0 }
    }
    fn props(&self) -> &Self::Props {
        &NoProps
    }
    fn set_props(&mut self, _props: Self::Props) {}

    fn view(&self) -> Node {
        Node::column(
            "root",
            [Node::label("count", self.count.to_string()), Node::button("increment", "+")],
        )
    }

    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { .. }) {
            self.count += 1;
        }
    }
}

fn bench_component_dispatch(c: &mut Criterion) {
    c.bench_function("component_tree/dispatch_click_and_rerender", |b| {
        b.iter_batched(
            || ComponentTree::new(Counter::new(NoProps)),
            |mut tree| {
                black_box(tree.dispatch(Event::Click {
                    target: framework_core::NodeId::from_key("increment"),
                }));
            },
            BatchSize::SmallInput,
        );
    });
}

struct SpawnsManyTasks;

impl Component for SpawnsManyTasks {
    type Props = NoProps;
    type Message = u32;

    fn new(_props: Self::Props) -> Self {
        Self
    }
    fn props(&self) -> &Self::Props {
        &NoProps
    }
    fn set_props(&mut self, _props: Self::Props) {}

    fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
        context.effect("spawn-many", (), |ctx| {
            for i in 0..100u32 {
                let _ = ctx.spawn(async move { i });
            }
            Box::new(|| {})
        });
        Node::label("root", "root")
    }
    fn view(&self) -> Node {
        unreachable!()
    }
    fn update(&mut self, _event: Event) {}
}

fn bench_scheduler_spawn_and_drain(c: &mut Criterion) {
    // Exercised through `ComponentTree`'s public `pump_tasks` rather than
    // `Scheduler::drain` directly: the latter is `pub(crate)` (an
    // application only ever observes task completion through a component's
    // `message`, per `crate::scheduler`'s design), so this benchmark
    // measures the same code path a real application actually takes.
    c.bench_function("component_tree/spawn_100_tasks_then_pump", |b| {
        b.iter_batched(
            || ComponentTree::new(SpawnsManyTasks::new(NoProps)),
            |mut tree| {
                // Give the executor a chance to actually complete the
                // (trivially ready) tasks before pumping.
                std::thread::sleep(std::time::Duration::from_millis(20));
                black_box(tree.pump_tasks())
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_snapshot_construction,
    bench_diff_no_changes,
    bench_diff_leaf_text_change,
    bench_layout_wide_row,
    bench_layout_nested_columns,
    bench_component_dispatch,
    bench_scheduler_spawn_and_drain,
);
criterion_main!(benches);
