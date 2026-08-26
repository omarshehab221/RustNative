# ReactNative

A Rust-first, native-control-oriented cross-platform application framework.

The project is being developed around one core idea:

> **Write application semantics once in Rust; let each operating system provide the actual native UI and platform services.**

This is intentionally closer to the architectural philosophy of React Native than to a custom-rendered toolkit. The framework does not paint an imitation of every operating system. It maintains a declarative Rust UI/component model and realizes that model through native platform objects.

## Current status

The current working backend is Windows/Win32. The framework core is designed to remain platform-independent so Web, macOS, Linux, Android, iOS, and embedded targets can later be added as separate adapters. Web is a first-class planned target using WebAssembly, semantic DOM/CSS, browser events, accessibility, and Web APIs rather than a canvas emulator.

The latest completed milestone is **Structured Task Scopes**.

## Architecture

```text
                       Application
                           │
                    Component Runtime
                           │
        ┌──────────────────┼──────────────────┐
        │                  │                  │
     Components          Scheduler          Services*
        │                  │                  │
        ├── state          ├── tasks          └── future platform APIs
        ├── props          ├── scopes
        ├── effects*       └── wakeups
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

`*` Effects and services are the next major runtime milestones; their architecture is planned but not yet implemented.

## Crate boundaries

```text
native-rust-framework/
├── Cargo.toml
├── PLAN.md
├── BUILD_STATUS.md
├── README.md
│
├── crates/
│   ├── framework-core/
│   │   └── src/lib.rs
│   │
│   └── framework-windows/
│       └── src/lib.rs
│
├── examples/
│   └── hello-label/
│       └── src/main.rs
│
└── windows.txt
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
- accessibility semantics;
- scheduler;
- task scopes.

It must not import Windows or any other operating-system API.

### `framework-windows`

Owns Windows-specific implementation details:

- Win32 window creation;
- native `HWND` ownership;
- native controls;
- Win32 event translation;
- native text measurement;
- native focus APIs;
- native scrolling/viewport implementation;
- Windows event-loop wakeups;
- platform-specific unsafe code.

This is the only place where Windows APIs should leak into the framework implementation.

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

Each framework-managed component owns one persistent `TaskScope`.

### Lifetime rules

- Rerendering does not cancel tasks.
- Prop changes do not cancel tasks.
- Explicit `TaskHandle::cancel()` cancels a task early.
- Removing a component cancels all outstanding tasks in its scope.
- Cancellation occurs before `unmounted()`.
- Completed results for removed components are ignored.

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

## Focus, keyboard, and accessibility

The core exposes semantic input events instead of Win32 virtual-key constants. The Windows backend translates native input to those events.

Accessibility semantics currently include:

- role;
- name;
- description;
- focusability.

Because the framework uses native controls, standard native accessibility behavior is retained. A full cross-platform accessibility bridge remains future work.

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
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo run -p hello-label
```

The current model has been developed incrementally with regression tests in `framework-core` covering reconciliation, layout, scrolling, input, components, callbacks, scheduling, and task scopes.

The environment used to package this repository does not provide `cargo`, `rustc`, or `rustfmt`, so the final Windows verification must be performed on a Windows Rust toolchain.

## Roadmap

The complete master roadmap—including completed milestones, architectural invariants, and all planned future stages—is maintained in [`PLAN.md`](PLAN.md).

The next implementation target is:

### Effects + reactive invalidation

We will add dependency-aware effects with cleanup and integration with structured task scopes. This will allow patterns such as:

```text
state/props change
      ↓
 effect dependency changes
      ↓
 cleanup previous effect
      ↓
 start new effect/task
      ↓
 result message
      ↓
 state update
```

After that, the roadmap proceeds through services/capabilities, styling, system integration, advanced input/accessibility, animations, virtualization, custom rendering, persistence/navigation, CLI/packaging, and additional native backends.
