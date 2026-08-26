# Build Status — Native Rust Framework

## Current milestone

**Structured task scopes** is the latest completed milestone.

The framework currently has a Rust-first component/runtime architecture with a native Win32 backend. Component-owned asynchronous task scopes now automatically cancel outstanding tasks when the component leaves the framework-managed component tree.

## Latest architecture state

```text
Application
  └── Framework-managed ComponentTree
       ├── keyed component identity
       ├── typed props
       ├── child → parent callbacks/messages
       ├── lifecycle
       ├── structured TaskScope per component
       └── declarative Node tree
            ↓
         TreeDiff
            ↓
         LayoutEngine
            ↓
         Windows native realization
```

## Completed milestones

1. Framework foundation + native Windows label
2. Stable node identity + native object ownership + UI tree
3. Application state + reconciliation
4. Components + events + structural tree diffing
5. Native containers + hierarchical layout
6. Deterministic layout phase + coordinate-space contract
7. General layout model (`Auto`, `Fixed`, `Fill`, alignment, padding, margin, gap)
8. Intrinsic measurement + `Row` + layout invalidation
9. Constraints + text wrapping
10. Overflow + clipping + scrolling with native viewport/content-host architecture
11. Focus + keyboard input + accessibility semantics
12. Text input + controlled component state
13. Component composition + lifecycle
14. Framework-managed component tree
15. Typed component props + parent-to-child data flow
16. Child-to-parent callbacks + shared message channels
17. Async tasks + framework scheduler
18. Structured task scopes

## Structured task-scope guarantees

- Every managed component receives a persistent task scope.
- Rerenders do not recreate the scope.
- Prop changes do not recreate the scope.
- `ComponentContext::spawn()` uses the component-owned scope.
- `ComponentContext::task_scope()` exposes the same ownership boundary explicitly.
- Scope destruction cancels outstanding tasks.
- Component removal cancels the scope before `unmounted()`.
- Worker threads never mutate component state directly.
- Task completions return through the framework scheduler/event loop.
- Completions targeting removed components are ignored.

## Latest user-reported Windows verification

The user reported successful compilation/testing through the structured-task-scope milestone after the final scope fix. Before that final fix, the task-scope suite exposed the render-time task-scope lookup bug; the corrected implementation passes the intended architecture by carrying the component-owned `TaskScope` directly through `ComponentContext` instead of requiring a live map lookup during rendering.

The authoritative commands on Windows remain:

```powershell
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo run -p hello-label
```

## Local environment limitation

The current execution environment does not provide `cargo`, `rustc`, or `rustfmt`, so the workspace cannot be compiled or tested locally here. Source/zip integrity can be checked, but Windows compilation and runtime behavior must be verified on a Windows Rust toolchain.

## Current next milestone

**Effects + reactive invalidation**

Web is now a first-class planned platform target. It is not implemented yet; the roadmap covers WASM, semantic DOM/CSS realization, browser events and accessibility, Web APIs/capabilities, Workers, routing/history, SSR/hydration, service workers/PWA, and browser packaging/testing/deployment.

Goals:

- dependency-aware effects;
- effect cleanup;
- effect reruns when relevant state/props change;
- integration with structured task scopes;
- explicit invalidation;
- cancellation of obsolete effect work.

## Long-range roadmap

The complete roadmap is in `PLAN.md`. The remaining major stages are:

```text
Effects + reactive invalidation
        ↓
Resource/service system
        ↓
Theme + styling system
        ↓
Platform capability abstraction
        ↓
Native dialogs/menus/system integration
        ↓
Window lifecycle + multi-window
        ↓
Advanced input
        ↓
Full accessibility bridge
        ↓
Animations/transitions
        ↓
Virtualized lists
        ↓
Graphics/custom rendering escape hatch
        ↓
Persistence/navigation
        ↓
CLI/project tooling
        ↓
Packaging/deployment
        ↓
Web (WASM + DOM + Web APIs)
        ↓
macOS / Linux / Android / iOS / Embedded backends
```
