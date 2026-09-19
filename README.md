# RustNative

A Rust-first, native-control-oriented cross-platform application framework.

The project is being developed around one core idea:

> **Write application semantics once in Rust; let each operating system provide the actual native UI and platform services.**

This is intentionally closer to the architectural philosophy of React Native than to a custom-rendered toolkit. The framework does not paint an imitation of every operating system. It maintains a declarative Rust UI/component model and realizes that model through native platform objects.

## Current status

The current working backend is Windows/Win32. The framework core is designed to remain platform-independent so Web, macOS, Linux, Android, iOS, and embedded targets can later be added as separate adapters. Web is a first-class planned target using WebAssembly, semantic DOM/CSS, browser events, accessibility, and Web APIs rather than a canvas emulator.

The latest completed milestone is **Milestone 27 — Animations and
Transitions** (a portable timeline whose frames touch native properties
alone, never the component tree), after Milestone 26's accessibility bridge,
Milestone 25's advanced input system, and the standards-audit remediation
pass (`Audit.md`). See `BUILD_STATUS.md` for what each pass
verified, what it found while doing so, and what is still open.

## Architecture

```text
                       Application
                           │
                    Component Runtime
                           │
        ┌──────────────────┼──────────────────┐
        │                  │                  │
     Components          Scheduler          Services
        │                  │                  │
        ├── state          ├── tasks          └── future platform APIs
        ├── props          ├── scopes
        ├── effects        └── wakeups
        ├── lifecycle
        └── messages
        │
        ▼
                  Declarative UI Tree
                           │
                       Tree Diff
                           │
                         Layout
                           │
                    Native realization
                           │
              ┌────────────┴────────────┐
              ▼                         ▼
          framework-core        framework-windows
                                          │
                                          ▼
                                    Windows / Win32
```

The runtime also carries theme/style data and portable capability discovery;
platform-specific APIs remain behind explicit service and native-extension boundaries.

## Crate boundaries

```text
RustNative/
├── .github/
│   └── workflows/
│       └── ci.yml
├── Cargo.toml
├── Cargo.lock
├── .gitignore
├── .gitattributes
├── rust-toolchain.toml
├── rustfmt.toml
├── clippy.toml
├── deny.toml
├── LICENSE-APACHE
├── LICENSE-MIT
├── SECURITY.md
├── CONTRIBUTING.md
├── PLAN.md
├── BUILD_STATUS.md
├── README.md
│
├── crates/
│   ├── framework-core/
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs              (module map + flat public re-exports)
│   │   │   ├── identity.rs          (NodeId/ComponentId/WindowId + key interning)
│   │   │   ├── event.rs
│   │   │   ├── node.rs
│   │   │   ├── component/           (Component trait, context, effects, tree)
│   │   │   ├── reconcile/           (snapshot + diff)
│   │   │   ├── layout/              (geometry, constraints, measure, engine)
│   │   │   ├── style/               (theme + the two style phases)
│   │   │   ├── scheduler/           (Scheduler, TaskScope, pluggable Executor)
│   │   │   ├── services/            (service contracts + in-memory impls)
│   │   │   ├── capability.rs
│   │   │   ├── menu.rs
│   │   │   ├── window.rs
│   │   │   ├── panic.rs             (component-panic policy)
│   │   │   ├── application.rs
│   │   │   └── platform.rs
│   │   ├── tests/
│   │   │   ├── component_lifecycle.rs
│   │   │   └── property_tests.rs
│   │   └── benches/
│   │       └── core_benchmarks.rs
│   │
│   └── framework-windows/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs              (module map + flat public re-exports)
│           ├── error.rs             (Error + NativeContext + Win32Category)
│           ├── platform.rs          (WindowsPlatform: the Platform impl)
│           ├── ffi.rs               (shared string/memory helpers)
│           ├── services/            (clipboard, dialogs, notifications, system)
│           └── native/              (the Win32 window backend)
│               ├── context.rs       (the only place a raw handle becomes a reference)
│               ├── win32.rs         (Win32 return-value classification)
│               ├── message_loop.rs  (window class, message loop, WNDPROC)
│               ├── runtime.rs       (per-window Runtime + WindowRegistry)
│               ├── container.rs     (container WNDPROC)
│               ├── registry.rs      (NodeId <-> native object, both directions)
│               ├── rendering/       (realization, controls, styling,
│               │                     accessibility, scrolling)
│               ├── input/           (keys, focus, pointers/capture/gestures,
│               │                     IME, clipboard events, OLE drop target,
│               │                     XInput controllers)
│               ├── measure.rs       (GDI text measurement)
│               ├── menu.rs          (MenuBar -> HMENU, with RAII)
│               ├── user_data.rs     (typed GWLP_USERDATA accessors)
│               ├── window_handles.rs
│               ├── harness.rs       (test-only bounded message pump)
│               ├── integration.rs   (test-only native scenarios)
│               └── input_integration.rs (test-only advanced-input scenarios)
│
├── examples/
│   ├── hello-label/
│   │   ├── Cargo.toml
│   │   └── src/main.rs
│   └── gdi-font-diagnostic/         (standalone GDI handle-lifetime probe)
│       ├── Cargo.toml
│       └── src/main.rs
│
└── tools/
    ├── windows-cross-test.sh        (mingw + Wine cross-compile/run)
    └── win_probes/                  (C ground-truth probes for Win32 behavior)
```

### `framework-core`

Contains the portable runtime and UI model:

- component model;
- keyed identity;
- props;
- lifecycle;
- callbacks/messages;
- framework events;
- declarative nodes;
- tree snapshots/diffing;
- native-object-independent layout;
- constraints;
- intrinsic measurement contracts;
- overflow/clipping/scroll semantics;
- focus/keyboard abstractions;
- advanced input: pointer/touch/pen events with capture, wheels, portable
  gesture recognition, IME composition, clipboard actions, drag-and-drop, and
  gamepad diffing, all opt-in per node;
- accessibility semantics;
- scheduler;
- task scopes;
- dependency-aware effects and effect-owned task scopes.
- injected async services and deterministic mock services;
- theme tokens, component styles, and node-level visual overrides;
- platform capabilities and native-extension escape hatch;
- multiple independently owned window roots and window lifecycle state.

It must not import Windows or any other operating-system API.

### `framework-windows`

Owns Windows-specific implementation details:

- Win32 window creation;
- native `HWND` ownership;
- native controls;
- Win32 event translation;
- native text measurement;
- native focus APIs;
- advanced input (`WM_POINTER*`, mouse capture, IMM32, OLE drag-and-drop,
  `XInput`);
- native scrolling/viewport implementation;
- Windows event-loop wakeups;
- the accessibility bridge (`WS_TABSTOP`, and MSAA dynamic annotation for
  name/description/role);
- platform-specific unsafe code.

This is the only place where Windows APIs should leak into the framework implementation.

Two modules inside `native/` exist specifically to keep the unsafe surface
small, and are worth reading before anything else there:

- `native::context` is the **only** place a raw Win32 handle or stored raw
  pointer becomes a Rust reference. The lifetime argument that makes every
  such conversion sound is stated there once, rather than re-derived in a
  `SAFETY` comment at each call site.
- `native::win32` classifies Win32 return values — `must_succeed`,
  `best_effort`, `informational`, `ignored_by_contract` — so that ignoring
  one is a deliberate, readable decision rather than an omission.

## Current component data flow

The runtime now supports both directions of typed data flow:

```text
Parent
  │
  │ typed Props
  ▼
Child
  │
  │ Callback<Message>
  ▼
Parent
```

A changed prop updates an existing keyed component rather than recreating it. A child callback sends a typed message through a framework-managed channel and the parent rerenders through normal reconciliation.

## Current asynchronous model

Async work follows:

```text
Component
   │
   ▼
TaskScope
   │
   ▼
Scheduler
   │
   ▼
Worker/task execution
   │
   ▼
Completion queue
   │
   ▼
Native event-loop wakeup
   │
   ▼
Component message/update
   │
   ▼
Render → diff → layout → native update
```

Each framework-managed component owns one persistent `TaskScope`. Effects use a
separate scope per keyed dependency run, so replacing an effect cancels only the
work owned by that effect.

### Lifetime rules

- Rerendering does not cancel tasks.
- Prop changes do not cancel tasks.
- Explicit `TaskHandle::cancel()` cancels a task early.
- Removing a component cancels all outstanding tasks in its scope.
- Cancellation occurs before `unmounted()`.
- Completed results for removed components are ignored.
- Effects run after rendering, are retained for unchanged dependencies, and run
  cleanup before replacement or component removal.

## Native object identity

The framework keeps stable Rust IDs separate from native handles:

```text
NodeId
  ↓
NativeObjectRegistry
  ↓
NativeObject
  ↓
HWND
```

A stable node identity means an existing native control can be updated in place instead of destroyed/recreated.

## Planned Web backend

Web will be a first-class platform target with a dedicated `framework-web` adapter. The intended architecture is:

```text
framework-core
      ↓
framework-web
      ↓
WebAssembly + browser bindings
      ↓
DOM / CSS / browser events / Web APIs
```

The Web backend will use semantic DOM elements for native browser controls rather than rendering the application into a canvas. The planned Web scope includes:

- WASM runtime and browser host lifecycle;
- DOM ownership and reconciliation;
- CSS/layout integration;
- browser-native focus, text input, pointer, touch, keyboard, and IME handling;
- HTML/ARIA accessibility mapping;
- fetch/WebSocket/storage/IndexedDB/Cache Storage and other capability APIs;
- Web Workers for suitable CPU-bound tasks;
- URL/history/navigation and browser lifecycle;
- SSR and hydration;
- service workers and PWA support;
- browser development, testing, bundling, and deployment tooling.

The full Web roadmap is integrated into `PLAN.md`, not treated as a final add-on.

## Layout model

The layout engine is platform-independent and supports:

- `Auto`;
- `Fixed`;
- `Fill`;
- minimum/maximum constraints;
- alignment;
- padding;
- margins;
- spacing/gaps;
- `Column`;
- `Row`;
- intrinsic measurement requests;
- wrapped text measurement;
- overflow/clipping;
- scroll ranges and offsets.

The core returns geometry; the platform backend applies it to native controls.

For scrolling, the Windows backend uses:

```text
Scrollable container
└── Viewport HWND
    └── Content HWND
        ├── native child
        ├── native child
        └── ...
```

Scroll position is transient runtime state and does not itself trigger a component rerender.

## Advanced input

Beyond clicks, keys, text, and focus — which every node receives — a node opts
into the higher-frequency streams it actually wants:

```rust
Node::column("canvas", children)
    .with_input(InputInterest::new().pointer().gestures().wheel())
```

A backend delivers those streams only to interested nodes, walking up from
whatever is under the pointer to the nearest one that asked. A component that
needs every sample of a drag, even outside its bounds, captures the pointer
through a deferred request:

```rust
fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
    self.input = Some(context.input()); // keep for `update`
    self.view()
}
// in `update`, on `Event::PointerDown { pointer, .. }`:
// self.input.capture_pointer("canvas", pointer.pointer_id());
```

Gesture recognition (tap, long press, pan, pinch) and gamepad state diffing
live in `framework-core`, not in a backend, so every platform agrees on what a
tap is. The Windows backend realizes the rest natively: `WM_POINTER*` for
touch and pen, top-level mouse capture with lost capture reported as
`PointerCancel`, IMM32 composition for focusable custom containers, an OLE
`IDropTarget` per window, clipboard shortcuts and `WM_CLIPBOARDUPDATE`, and
`XInput` controllers polled only while some node asks for them.

## Focus, keyboard, and accessibility

The core exposes semantic input events instead of Win32 virtual-key constants. The Windows backend translates native input to those events.

The portable accessibility model (`framework_core::accessibility`) covers
roles (29, including headings with levels), names, descriptions, text and
range values, checked/expanded/selected/read-only/required/busy states,
declared and invoked actions, labelled-by/described-by/controls
relationships, live regions, position-in-set for virtualized children,
automation ids, focusability, and *virtual elements* — semantic children with
no native object of their own, for custom-drawn content.

Every node kind defaults to the role and focusability that describe it — a
button is an activatable control and a keyboard stop, a label is static text
and is not — so the model carries real information without an application
filling it in node by node. `Node::with_accessibility` replaces it wholesale
where that default is wrong.

The Windows backend realizes it as **UI Automation server-side providers**
(`native::uia`): every realized node's window answers `WM_GETOBJECT`, and its
provider is merged with the control's native proxy so only what the model
states is overridden. Virtual elements are fragments; control patterns
(Invoke, Value, RangeValue, Toggle, ExpandCollapse, SelectionItem,
ScrollItem) turn into `Event::AccessibilityAction` for the component to
honor; property, structure, and live-region changes are announced.
Focusability is also realized as `WS_TABSTOP`, and MSAA-only clients still get
names and roles through the Dynamic Annotation API.

Every UIA test runs a real `IUIAutomation` client on another thread, the way a
screen reader in another process reaches the application.

## Animations and transitions

A value can change over time without the component tree knowing. Two ways in:

```rust
// A transition: whenever this node's opacity changes, animate to the new
// value over 120 ms rather than jumping to it.
Node::column("panel", [])
    .with_opacity(if pressed { 0.6 } else { 1.0 })
    .with_transition(
        AnimatedProperty::Opacity,
        Transition::new(Duration::from_millis(120)),
    );

// An explicit animation, requested from a component: a spring back to the
// laid-out position, starting 24 px to the right of it.
context.animations().animate(
    "panel",
    Animation::new(
        AnimatedProperty::Translation,
        AnimatedValue::Offset(Point::new(0, 0)),
        Transition::spring(220.0, 14.0, 1.0),
    )
    .from(AnimatedValue::Offset(Point::new(24, 0))),
);
```

`framework_core::animation::Timeline` evaluates both, and is entirely
platform-free: it turns elapsed time into a per-property value and handles
repeats, autoreverse, fill modes, and — for springs — carries the current
velocity into a retarget, so interrupting a motion continues it rather than
restarting it. A finished animation releases its property, and the node goes
back to exactly what the rendered tree says.

**A frame is not a render.** The backend applies the frame's value to the
native object it belongs to and nothing else: `SetWindowPos` for geometry, a
layered-window alpha for opacity, an invalidation for colours. A component
hears only `Event::AnimationFinished`, and may ignore it; the animation tests
assert that render counts do not move while values animate.

Frames come from one process-wide driver thread paced by `DwmFlush` and
posted — never sent — to each animating window, coalesced so a busy UI thread
drops frames rather than queueing them. With nothing animating, that thread
sleeps on a condition variable and the application costs nothing.

Animations are cancelled per node and property, per component (unmounting
cancels a component's own animations), and per window. The system's
reduced-motion preference is read at startup and tracked live; each animation
says whether it is skipped (jump straight to the target) or still run when
motion is reduced.

## Text input

The current Windows backend uses a native Win32 `EDIT` control. Its value is controlled by component state:

```text
Native EDIT
   ↓
TextChanged event
   ↓
Component state
   ↓
view()
   ↓
Tree diff
   ↓
Native EDIT synchronization
```

Programmatic updates are applied only when the native value differs, avoiding unnecessary feedback loops and cursor disruption.

## Verification

On Windows, run:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo deny check
cargo audit
cargo run -p hello-label
```

CI (`.github/workflows/ci.yml`) runs all of the above, plus an MSRV check,
and — importantly — a `windows-latest` job. That job is what actually
compiles and runs `framework-windows`'s `native` module: it is
`#[cfg(windows)]`-gated, so a Linux-only pipeline silently excludes it.

`framework-core` is covered by unit tests, integration tests, `proptest`
property tests (identity, reconciliation, layout constraints, scroll ranges,
removal ordering), runnable doc examples on every major public API, and
`criterion` benchmarks including scaling sweeps at 10/100/1k/10k nodes.

`framework-windows` is covered against **real** Win32: `native::integration`
creates genuine top-level windows and drives them through the production
message loop (`native::harness` replaces only the outermost blocking
`GetMessageW` with a bounded `PeekMessageW` pump), covering window lifecycle,
reconciliation, reorder, text input, focus traversal, dynamic and modal
window lifecycle, menu dispatch, GDI resource lifetime, scheduler wakeups,
creation reentrancy, stale-event rejection, and every component-panic policy.

Those tests need an interactive window station — true on a developer machine
and on GitHub's `windows-latest` runner, not true under a service account.

A few tests need more than a window station: the real cursor (hover is
decided by where it actually is) or access to the system clipboard. Those are
`#[ignore]`d with a reason, so a restricted shell reports them as *not run*
rather than failing or quietly passing, and CI runs them explicitly:

```powershell
cargo test --workspace -- --ignored
```

## Roadmap

The complete master roadmap—including completed milestones, architectural invariants, and all planned future stages—is maintained in [`PLAN.md`](PLAN.md).

The next implementation target is **Milestone 28 — Virtualized lists and
large data sets**. The roadmap then proceeds through custom rendering,
persistence/navigation, CLI/packaging, and additional native backends.
