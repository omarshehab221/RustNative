# Build Status — Native Rust Framework

## Current milestone

**Window lifecycle + multi-window support** is the latest completed milestone.

The framework currently has a Rust-first component/runtime architecture with a native Win32 backend. Component-owned asynchronous task scopes now automatically cancel outstanding tasks when the component leaves the framework-managed component tree.

## Completeness pass (milestones 1–24)

A full audit of milestones 1–24 against a "production framework" bar found that most of the architecture was already solid (real native HWNDs, full tree diffing, working Tab/Shift+Tab traversal, structured async task scopes with cancellation), but several milestones had gaps between what was documented as "implemented" and what actually took effect end-to-end. All of the following were closed in this pass:

- **Milestone 21 (theme + styling) had no realization at all.** `Theme`/`VisualStyle` resolution existed and was unit-tested in isolation, but nothing ever called it outside of tests: `TreeSnapshot` only ever carried a node's raw style *override*, and the Windows backend never read font or color data — no `WM_SETFONT`, no `WM_CTLCOLOR*`, no background paint. Fixed: `TreeSnapshot::from_node_with_theme` resolves every node's style (theme default merged with override, in its `Normal`/`Disabled` state) before it reaches the backend, mirroring how layout geometry is already fully resolved in the core; the Windows backend now realizes it as native fonts (`CreateFontIndirectW` + `WM_SETFONT`) and colors (`WM_CTLCOLORSTATIC`/`WM_CTLCOLOREDIT`/`WM_CTLCOLORBTN` for controls, `WM_ERASEBKGND` for containers), with GDI resources owned and freed per node.
- **No `disabled` concept existed anywhere**, so the theme's `Disabled` state variant was unreachable. Fixed: `Node::disabled(bool)`/`is_disabled()`, threaded through `TreeNode`, realized as `EnableWindow` on Windows, and excluded from Tab-order focus traversal.
- **Milestone 23 ("native dialogs, menus, and system integration") was only portable contracts.** The Windows backend had no file-dialog implementation, `notify()` was a hardcoded stub error, and there was no menu system — not even a data model. Fixed: `WindowsFileDialogs` (open/save via `GetOpenFileNameW`/`GetSaveFileNameW`, folder picking via `SHBrowseForFolderW`), real toast-style notifications via `Shell_NotifyIconW`, and a full `MenuBar`/`MenuItem` model in the core realized as native `HMENU`s with `WM_COMMAND` routing to a new `Event::MenuAction`.
- **Milestone 24 ("multi-window support") only let you open windows before the platform's event loop started.** There was no way for a running component to open or close a window in response to an event (e.g., a menu action or button click), since `Application::open_window`/`close_window` aren't reachable from `ComponentContext`. Fixed: a deferred `WindowCommand` queue (`ComponentContext::windows()` → `WindowRequests::open`/`close`) applied by `Application` after each dispatch/task-pump, and a Windows-side `WindowRegistry` that keeps native HWNDs in sync with `Application::window_ids()` as they change at runtime — including a specific, documented design to avoid a use-after-free when a window is asked to close itself mid-dispatch (its teardown is deferred via a posted `WM_CLOSE`, never synchronous).

`Capability::Menus` was added, and `WindowsPlatform::capabilities()` now advertises `FileDialogs`, `Notifications`, and `Menus` alongside the previously-advertised capabilities. `examples/hello-label` now exercises all of the above (a disabled button past count 5, a native menu bar with a checkable-style toggle and a runtime-opened settings window, both routed through `Event::MenuAction`).

**Known, deliberately out-of-scope remainder**, consistent with the framework's existing platform-independence philosophy: drag-and-drop, system share, and system-appearance-change notifications remain portable contracts/flags only (not advertised as supported capabilities, so no app can be misled into depending on them); native menus are static-at-window-creation (matching the existing maturity level of a `Window`'s title and size, which also aren't reactively updatable after creation); and dynamic hover/pressed native repaint (as opposed to the now-realized Normal/Disabled states) would need a mouse-tracking state machine (`TrackMouseEvent`/`WM_MOUSELEAVE`) that was judged lower priority than closing the gaps above.

## Latest architecture state

```text
Application
  └── Window registry
       └── Framework-managed ComponentTree per window
       ├── keyed component identity
       ├── typed props
       ├── child → parent callbacks/messages
       ├── lifecycle
       ├── structured TaskScope per component
       ├── dependency-aware effects + cleanup
       ├── injected Services + capabilities
       ├── deferred window-open/close requests
       ├── theme/style resolution (resolved before reaching the backend)
       └── declarative Node tree (+ menu bar, disabled state)
            ↓
         TreeDiff
            ↓
         LayoutEngine
            ↓
         Windows native realization
         (controls, fonts/colors, native menus, dynamic window registry)
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
19. Effects + reactive invalidation
20. Resource and service system
21. Theme + styling system, resolved and realized as native fonts/colors
22. Platform capability abstraction
23. Native dialogs, menus, and system integration — realized (file dialogs, notifications, native menu bar), not just contracts
24. Window lifecycle + multi-window support, including opening/closing windows at runtime

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

## Local verification

`framework-core` (all platform-independent logic, including every addition in
this completeness pass — `disabled`, theme resolution into snapshots, the
`MenuBar`/`MenuItem` model, `Event::MenuAction`, and the window-request queue)
has been compiled and tested locally with `cargo test -p framework-core`
(37/37 passing) using a real — if older (1.75, edition-2021-downgraded for
local compatibility only) — `rustc`, not just read for plausibility.
`examples/hello-label` was also locally type-checked in full against that
same compiler and compiles cleanly.

The Windows-specific code added in this pass (native menu realization, the
dynamic window registry, font/color realization, file dialogs, and
notifications, all in `crates/framework-windows/src/lib.rs`) could **not**
be compiled locally: this environment has no Windows target, and
`windows-sys`'s `raw-dylib` linking is rejected by rustc for any non-Windows
target even under `cargo check`, before reaching type-checking. That code
was instead written and manually cross-checked line-by-line against the
actual `windows-sys 0.61.2` source (struct field names/order, exact function
signatures, constant types) rather than from memory, and was kept to
well-established, decades-stable Win32 APIs (classic common dialogs and
`Shell_NotifyIconW` rather than the newer `IFileDialog`/WinRT toast COM
surfaces) specifically to minimize the risk of an uncompilable mistake. It
still requires a real Windows interactive verification run before being
trusted the way the rest of the workspace now is — same as the native Win32
runtime behavior noted below, but with less confidence than that prior,
directly-user-verified baseline until it gets one.

## Post-delivery fix: unbounded window creation on real Windows

The user ran the completeness-pass build on a real Windows 10 machine.
`cargo check --workspace` and `cargo test --workspace` passed, but
`cargo run -p hello-label` spun up new windows continuously until the
process crashed — exactly the class of bug flagged as a risk above, since
this specific code path had no compiler and no runtime available to catch
it here.

**Root cause:** `WindowRegistry::create_window` called
`self.runtimes.insert(id, runtime)` as its *last* step, after
`CreateWindowExW` and `ShowWindow`. `ShowWindow` (and, in principle,
`CreateWindowExW` itself) can synchronously deliver `WM_SIZE` to the new
window's own `window_proc` before either call returns. `Runtime::dispatch`
calls `sync()` at the end of every dispatch — including this one — and
`sync()` create-loop checks `!self.runtimes.contains_key(&id)`. Since the
window hadn't been inserted yet, it looked "missing" from its own
perspective and `sync()` created a second native window for the same
`WindowId`, which showed itself, which delivered its own synchronous
`WM_SIZE`, which created a third — recursing until the stack overflowed.
This is precisely the kind of native-callback reentrancy bug that cannot be
caught by `cargo check`/`cargo test`, only by running the real event loop.

**Fix:** two layers, both in `crates/framework-windows/src/lib.rs`:

1. `create_window` now inserts the new `Runtime` into `self.runtimes`
   immediately after `CreateWindowExW` succeeds — before `SetMenu`, the
   waker, `render()`, or `ShowWindow` — so any message synchronously
   delivered during those later calls sees the window as already present.
2. A `creating: HashSet<WindowId>` reentrancy guard on `WindowRegistry`,
   checked by both `sync()`'s create-loop and `create_window` itself, as
   defense-in-depth in case a message is ever delivered synchronously even
   earlier than that (from inside `CreateWindowExW` before it returns, which
   is not believed to happen without `WS_VISIBLE` but — again — was
   unverifiable here).

Traced by hand through the actual nested reentrant call stack that opening
both the primary and the auxiliary example window produces (each window's
own `ShowWindow` can trigger the other window's creation from partway
inside the first window's own creation call), which now terminates
correctly: every window is created exactly once, and `self.creating` and
`self.runtimes` end each `sync()` pass consistent with
`Application::window_ids()`.

`cargo test -p framework-core` still passes 37/37 and `cargo check -p
hello-label` still compiles cleanly after this fix; the fix itself remains
subject to the same Windows-target compile-check limitation described below
— it is reasoned through carefully, but a second real-Windows run is what
actually confirms it.



**Advanced input system**

Web is now a first-class planned platform target. It is not implemented yet; the roadmap covers WASM, semantic DOM/CSS realization, browser events and accessibility, Web APIs/capabilities, Workers, routing/history, SSR/hydration, service workers/PWA, and browser packaging/testing/deployment.

Milestones 20–24 add service injection/mocks, theme tokens and style resolution,
capability discovery with a native escape hatch, portable system-integration
contracts, and independently owned multi-window component roots.

## Long-range roadmap

The complete roadmap is in `PLAN.md`. The remaining major stages are:

```text
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

**Advanced input system**

Web is now a first-class planned platform target. It is not implemented yet; the roadmap covers WASM, semantic DOM/CSS realization, browser events and accessibility, Web APIs/capabilities, Workers, routing/history, SSR/hydration, service workers/PWA, and browser packaging/testing/deployment.

Milestones 20–24 add service injection/mocks, theme tokens and style resolution,
capability discovery with a native escape hatch, portable system-integration
contracts, and independently owned multi-window component roots.

## Long-range roadmap

The complete roadmap is in `PLAN.md`. The remaining major stages are:

```text
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
