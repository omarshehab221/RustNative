# Native Rust Framework — Master Development Plan

## 1. Vision

Build a Rust-first application framework that lets developers write the application model, state, components, layout, and business logic once while using the operating system's native primitives instead of emulating an operating system on top of a custom renderer.

The long-term target platforms are:

- Windows
- macOS
- Linux
- Android
- iOS
- Web (WebAssembly + browser DOM/Web APIs)
- Embedded Linux
- RTOS / selected bare-metal embedded targets

The framework follows the React Native philosophy of native host controls, but is Rust-native rather than JavaScript-native:

```text
Application
    │
    ├── shared Rust state / business logic
    ├── component tree
    ├── declarative UI tree
    ├── layout / events / scheduling
    └── platform services
             │
             ▼
      platform adapter
             │
             ▼
      native OS APIs
```

The framework must not become a lowest-common-denominator abstraction that hides the unique capabilities of each operating system. Portable semantics should be consistent, while platform-specific capabilities remain available through explicit capability APIs and native escape hatches.

---

## 2. Core architectural principles

### 2.1 Rust owns application semantics

Rust owns:

- application state;
- component state;
- props;
- messages and callbacks;
- declarative UI trees;
- reconciliation;
- scheduling;
- task ownership;
- layout semantics;
- cross-platform service contracts.

### 2.2 The OS owns the native UI

The framework should use native host controls and platform services wherever a native equivalent exists.

The Windows backend, for example, creates real Win32 `HWND`s. Future backends should use the corresponding native APIs rather than painting a facsimile of an operating-system UI.

### 2.3 The Web is a first-class platform, not a fallback

The Web must be treated as a first-class target alongside desktop, mobile, and embedded systems. It is not acceptable to build the native platforms first and then emulate them in a generic canvas-based browser runtime.

The Web backend should use browser-native primitives wherever appropriate:

- DOM elements for semantic controls and containers;
- CSS for browser-native layout and visual mechanics where the framework can map its layout semantics safely;
- browser event listeners and event propagation;
- browser focus and selection APIs;
- browser accessibility semantics through HTML/ARIA;
- Web APIs for storage, networking, clipboard, notifications, media, sensors, permissions, and other capabilities;
- WebAssembly for the shared Rust runtime;
- JavaScript bindings only at the browser boundary where required by Web APIs.

The Web adapter therefore follows the same principle as the native adapters:

```text
Rust application
    ↓
framework-core
    ↓
framework-web
    ↓
WASM + browser bindings
    ↓
DOM / CSS / Web APIs
```

The framework must also explicitly account for browser-specific constraints: the single-threaded main-thread model for DOM access, asynchronous Web APIs, browser lifecycle, URL/history/navigation, page visibility, storage quotas, user-gesture restrictions, hydration, and browser security boundaries.

### 2.4 Platform-independent core, platform-specific adapters

`framework-core` must never depend on Windows, macOS, Android, iOS, GTK, AppKit, UIKit, WinUI, JNI, Objective-C, or other platform APIs.

Platform crates implement the contracts exposed by the core.

### 2.5 Capabilities over platform conditionals

Prefer capability contracts such as:

```text
HasClipboard
HasCamera
HasNotifications
HasBluetooth
HasStorage
```

over application-wide checks such as `if platform == Android`.

### 2.6 Native escape hatch

A serious cross-platform framework must allow advanced applications to access native platform objects and APIs when the portable abstraction is insufficient.

The escape hatch must be explicit and isolated so platform-specific code does not leak into the portable core.

### 2.7 Stable identity is fundamental

Framework node IDs and component keys provide stable identity across rerenders. Native objects must be reused when identity remains stable.

### 2.8 Declarative tree is the source of truth

The application produces a declarative tree. Native objects are a realization of that tree, not an independent source of truth.

### 2.9 Transient interaction state is not a rerender

Examples such as scrolling, focus transitions, pointer movement, and other high-frequency native interaction should update native runtime state directly when possible instead of rebuilding the entire component tree.

### 2.10 Layout is platform-independent

The core calculates semantic geometry, constraints, clipping, intrinsic measurement requests, and coordinate spaces. A platform backend applies the resulting geometry to native objects and supplies native measurement capabilities.

### 2.11 Async work must respect component lifetime

Background work must never mutate component state directly from a worker. Results return through the framework scheduler/event loop, and task lifetime must be tied to component lifetime through structured task scopes.

---

# 3. Completed milestones

The following milestones are implemented in the current codebase. The current production backend is Windows/Win32; the Web target is planned but not yet implemented.

## Milestone 1 — Framework foundation and native Windows label

Implemented:

- Cargo workspace;
- `framework-core` crate;
- `framework-windows` crate;
- example application crate;
- `Platform` abstraction;
- native Win32 window creation;
- native Win32 `STATIC` label;
- Windows-specific unsafe/FFI code isolated in the Windows backend.

## Milestone 2 — Stable node identity + native-object ownership + UI tree

Implemented:

- declarative `Node` tree;
- stable `NodeId` keys;
- native object registry;
- `NodeId -> NativeObject -> HWND` ownership;
- duplicate-ID detection;
- native object reuse based on stable identity;
- logical containers separated from native ownership.

## Milestone 3 — Application state + initial reconciliation

Implemented:

```text
State -> view() -> Node tree -> renderer -> native objects
```

State updates rebuild the declarative tree while stable IDs preserve native controls.

## Milestone 4 — Components + events + proper tree diffing

Implemented:

- `Component` abstraction;
- component `view()` and `update()`;
- semantic framework events;
- explicit tree snapshots;
- structural tree diffing;
- insert/update/remove/move operations;
- stable node identity during updates.

## Milestone 5 — Native container ownership + hierarchical layout

Implemented:

- native `Column` containers;
- native parent/child HWND hierarchy;
- nested containers;
- recursive layout;
- native reparenting for moved nodes;
- hierarchical event propagation.

## Milestone 6 — Deterministic layout architecture

Implemented:

- layout as a separate phase after reconciliation;
- deterministic sibling ordering;
- explicit coordinate-space contract;
- child rectangles expressed relative to native parents;
- layout result separated from native application.

## Milestone 7 — General layout model

Implemented:

- `Auto` sizing;
- `Fixed` sizing;
- `Fill` sizing;
- alignment;
- padding;
- margins;
- gaps;
- column layout;
- row layout foundations.

## Milestone 8 — Intrinsic measurement + `Row` + layout invalidation

Implemented:

- platform `IntrinsicMeasurer` contract;
- native Windows text measurement;
- `Row` container;
- horizontal flow;
- layout invalidation detection;
- bounded intrinsic measurement support.

## Milestone 9 — Constraints + text wrapping

Implemented:

- min/max width constraints;
- min/max height constraints;
- bounded text measurement;
- multiline height calculation;
- Windows text wrapping using native GDI measurement/drawing APIs;
- constraint-aware final geometry.

## Milestone 10 — Overflow, clipping, and scrolling

Implemented:

- `Visible`, `Clip`, and `Scroll` overflow semantics;
- scroll ranges;
- scroll offsets;
- clipping regions;
- proper native viewport/content-host architecture;
- stable child coordinates during scrolling;
- scroll updates that do not rerender the component tree;
- native event-loop wheel handling;
- regression protections against scroll/layout feedback loops.

The native scroll hierarchy is:

```text
Scrollable container
└── Viewport HWND
    └── Content HWND
        ├── child HWND
        ├── child HWND
        └── ...
```

## Milestone 11 — Focus, keyboard input, and accessibility semantics

Implemented:

- framework focus model;
- focus gained/lost events;
- keyboard abstraction independent of Win32 virtual-key constants;
- key modifiers;
- keyboard event routing;
- Tab / Shift+Tab focus traversal foundations;
- accessibility roles;
- accessibility names;
- accessibility descriptions;
- focusability semantics;
- native controls continue to provide their normal platform accessibility behavior.

A full custom Windows UI Automation provider is intentionally still future work.

## Milestone 12 — Text input controls + controlled component state

Implemented:

- native Windows `EDIT` control;
- `TextInput` node;
- controlled value model;
- `TextChanged` framework event;
- native-to-Rust-to-native synchronization;
- suppression of feedback loops caused by programmatic native value updates;
- preservation of stable native input identity.

## Milestone 13 — Component composition + lifecycle

Implemented:

- reusable child components;
- explicit mount/update/unmount lifecycle;
- `ComponentHost` ownership primitive;
- lifecycle stability across rerenders;
- lifecycle replacement semantics;
- child state persistence across parent rerenders.

Key invariant:

```text
rerender != unmount + remount
```

## Milestone 14 — Framework-managed component tree

Implemented:

- framework-owned child component entries;
- stable keyed child identity;
- automatic child creation/reuse/removal;
- automatic lifecycle transitions;
- automatic event ownership routing;
- `NodeId -> ComponentId` ownership mapping;
- framework-managed component lifetime.

Application code no longer needs to manually route every child event or manually maintain child component hosts for ordinary composition.

## Milestone 15 — Component props / parent-to-child data flow

Implemented:

- typed `Component::Props`;
- `new(props)`;
- `props()`;
- `set_props()`;
- `props_changed()`;
- typed `child_with_props(...)` construction;
- prop equality checks;
- prop updates without remounting;
- child state preservation across prop updates.

Invariant:

```text
same component key + changed props
    !=
component replacement
```

## Milestone 16 — Child-to-parent communication / callbacks / shared channels

Implemented:

- typed `Callback<M>`;
- cloneable callback endpoints;
- shared framework-managed message queues;
- child-to-parent message delivery;
- parent message handling through the component tree;
- callback-driven parent rerenders;
- child state preservation during callback-driven updates.

This completes the initial two-way data-flow model:

```text
parent props -> child state -> callback/message -> parent state
```

## Milestone 17 — Async tasks + framework scheduler

Implemented:

- `Scheduler`;
- `TaskId`;
- `TaskHandle`;
- async task spawning;
- task completion queue;
- framework message delivery for task results;
- `SleepFuture` foundation;
- Windows native event-loop wake-up using a private `WM_APP` message;
- completion pumping on the event-loop thread;
- explicit task cancellation;
- ignoring completions targeting components that no longer exist.

Worker threads do not mutate component state directly.

## Milestone 18 — Structured task scopes

Implemented:

- persistent component-owned `TaskScope`;
- `ComponentContext::task_scope()`;
- `ComponentContext::spawn()` using the component scope;
- task ownership tied to managed component lifetime;
- task scope persistence across rerenders and prop updates;
- automatic cancellation when a component is removed;
- cancellation occurring before `unmounted()`;
- explicit `TaskHandle::cancel()` for early cancellation;
- completed task results ignored after component removal.

Current structured-task invariant:

```text
component rerender      -> task survives
props update            -> task survives
component unmount      -> outstanding tasks cancel
worker completion       -> result re-enters framework scheduler
```

## Milestone 19 — Effects + reactive invalidation

Implemented:

- keyed, dependency-aware effects declared from `ComponentContext`;
- effects committed after the declarative tree render;
- cleanup functions on dependency change, removal, and tree teardown;
- effect-owned task scopes, separate from the component-wide task scope;
- automatic cancellation of tasks started by obsolete effects;
- stable effects across rerenders when dependencies have not changed;
- duplicate effect-key detection within a component render.

Effect lifecycle:

```text
state / props change
      ↓
dependency change
      ↓
cleanup previous effect + cancel its tasks
      ↓
run replacement effect
```

## Milestone 20 — Resource and service system

Implemented:

- application-owned `Services` registry available from `ComponentContext`;
- typed asynchronous contracts for HTTP, storage, clipboard, file dialogs, and system operations;
- portable request/response and error types;
- deterministic `MemoryStorage` and `MemoryClipboard` implementations for tests and previews;
- explicit service injection through `Application::with_services`;
- service sharing across all window component roots.

## Milestone 21 — Theme + styling system

Implemented:

- theme colors, typography, spacing, and radius tokens;
- component defaults for labels, buttons, text inputs, and containers;
- node-level visual-style overrides;
- `disabled` as a first-class node flag (`Node::disabled`), realized natively
  as `EnableWindow` and excluded from Tab/Shift+Tab focus traversal;
- deterministic theme/override resolution;
- state-aware style variants for normal, hover, focus, pressed, and disabled
  controls, defined in the theme model;
- `TreeSnapshot::from_node_with_theme` resolves every node's style (theme
  default merged with override, in its `Normal`/`Disabled` state) before it
  reaches a backend — mirroring how layout geometry is already fully
  resolved in the core rather than left to each backend to compute;
- Windows realization: native fonts (`CreateFontIndirectW` + `WM_SETFONT`)
  and colors (`WM_CTLCOLORSTATIC`/`WM_CTLCOLOREDIT`/`WM_CTLCOLORBTN` for
  controls, `WM_ERASEBKGND` for containers), with GDI resources owned and
  freed per node;
- live Windows hover, pressed, and focus state repaints, using
  `TrackMouseEvent`/`WM_MOUSELEAVE` plus native focus synchronization, without
  rebuilding the declarative component tree.

## Milestone 22 — Platform capability abstraction

Implemented:

- portable `Capability` enumeration (including `Menus`) and
  `PlatformCapabilities` discovery set;
- capability reporting through every platform adapter;
- Windows capability declarations for its available system integration
  surface, kept honest: a capability is only advertised once this backend
  actually realizes it (`capabilities_only_advertise_realized_backend_features`
  asserts this for every `Capability` variant);
- explicit `Platform::native_extension()` escape hatch for backend-specific APIs.

## Milestone 23 — Native dialogs, menus, and system integration

Implemented:

- portable async contracts for clipboard, notifications, URL launching, and file dialogs;
- file-dialog request model with open/save/folder selection and filters;
- system-service contract usable by native backends for notifications and URL launching;
- capability flags for file dialogs, system appearance, drag-and-drop, system sharing, and window management;
- a portable `MenuBar`/`MenuItem` model (actions, submenus, separators,
  enabled/checked state) attached to a `Window` via `Window::with_menu`, and
  `Event::MenuAction` for selection, routed like a window-lifecycle event to
  the window's root component;
- Windows realization: `WindowsFileDialogs` (open/save via
  `GetOpenFileNameW`/`GetSaveFileNameW`, folder picking via
  `SHBrowseForFolderW`, both COM-apartment-initialized per call since shell
  extensions are COM-backed), real notifications via `Shell_NotifyIconW`
  (add-then-immediately-delete a tray icon so a one-shot balloon leaves no
  permanent tray presence), and a native menu bar (`CreateMenu`/
  `CreatePopupMenu`/`AppendMenuW`/`SetMenu`, `WM_COMMAND` routed to
  `Event::MenuAction` via a per-window command-id → `NodeId` table).

Drag-and-drop, system sharing, and system-appearance-change notifications
remain portable contracts/flags only — not realized, and correspondingly not
advertised as supported capabilities — preserving the core's platform
independence for the parts still deferred. A menu's contents are static at
window-creation time, matching a `Window`'s title and size, which are
likewise not yet reactively updatable after the window opens.

## Milestone 24 — Window lifecycle + multi-window support

Implemented:

- stable `WindowId` identities and a primary-window convention;
- multiple independently owned window/component roots;
- open/close operations and optional modal-parent relationship;
- per-window view, render, dispatch, task, service, and theme ownership;
- portable resize, move, close-request, and presentation-change events;
- window state for position, size, visibility, normal/minimized/maximized/fullscreen presentation;
- opening and closing windows at **runtime**, from inside a running
  component, not just before the platform's event loop starts:
  `ComponentContext::windows()` returns a `WindowRequests` handle
  (`.open(component, window, modal_parent)` / `.close(id)`) that queues a
  deferred `WindowCommand`, applied by `Application` right after the
  requesting window's own dispatch/task-pump finishes — so a component never
  mutates the window registry while it is itself mid-render;
- Windows realization: a `WindowRegistry` that syncs live native windows
  against `Application::window_ids()` after every dispatch and task pump,
  creating a new `HWND` for a window the application gained and — for one
  the application lost — posting that window itself a `WM_CLOSE` rather than
  destroying it inline. The deferral matters: a `Runtime` can be mid-dispatch
  (and so borrowed by a caller further up the native call stack) at the
  exact moment its own window is asked to close, and tearing it down inline
  would free memory that caller still holds a reference to. Every `Runtime`
  — closed or not — is kept alive until the whole native event loop returns,
  so this is sound even when a window closes itself.

## Milestone 25 — Advanced input system

Implemented:

- portable payloads in `framework-core::input`: `PointerEvent` (mouse,
  touch, pen; per-contact ids; buttons; modifiers; pressure; monotonic
  timestamps), `WheelDelta` (1/120-notch lines, or pixels), `Gesture`,
  `Composition`, `ClipboardAction`, `DragData`/`DropEffect`, and gamepad
  `GamepadState`/`GamepadInput`, plus `KeyUp`, navigation/function keys, and
  the meta modifier;
- **opt-in delivery**: a node declares `InputInterest` (pointer, wheel,
  gestures, drop target, gamepad) and only interested nodes — found by
  walking up from the node under the pointer — receive those streams, so a
  moving mouse is never a rerender storm;
- portable **gesture recognition** (`GestureRecognizer`: tap, long press,
  pan, two-finger pinch) driven from the pointer stream with an injected
  clock, and portable **gamepad diffing** (`GamepadPoller` over a
  `GamepadSource`) — logic that exists once for every future backend;
- deferred **input requests** (`ComponentContext::input()` →
  `InputRequests`): pointer capture/release and drag-feedback answers,
  scoped to framework-wide node ids and applied by the backend after the
  dispatch that made them;
- Windows realization: mouse (all five buttons, hover enter/leave,
  horizontal wheel), touch and pen through `WM_POINTER*` with implicit
  per-contact capture (touch-promoted mouse messages recognized and
  skipped), mouse capture on the top-level window with lost capture
  delivered as `PointerCancel`, IMM32 composition for focusable custom
  containers, Ctrl/Shift clipboard shortcuts plus `WM_CLIPBOARDUPDATE`,
  an OLE `IDropTarget` per window (files and text; source-allowed effects
  negotiated), and `XInput` controllers polled only while a node asks and
  delivered only to the active window;
- `Capability::{DragAndDrop, Touch, Pen, Gamepad, Ime}` advertised.

Deliberately not done: mouse input is not moved into the pointer stack
(`EnableMouseInPointer` is process-global and changes standard-control
behavior); Windows' own `WM_GESTURE` is not used (it is mutually exclusive
with `WM_POINTER`).

## Milestone 26 — Full accessibility bridge

Implemented:

- a portable model in `framework-core::accessibility`: 29 roles (including
  headings with levels), text and range values, checked/expanded/selected/
  read-only/required/busy states, declared actions (`AccessibleActionKind`)
  and invoked ones (`AccessibleAction`, delivered as
  `Event::AccessibilityAction`), labelled-by/described-by/controls
  relationships scoped per component like node keys, live regions,
  position-in-set for virtualized children, stable automation ids, and
  **virtual elements** — semantic children with no native object of their
  own, for custom-drawn content;
- `AccessibilityTree`, the portable projection every backend needs:
  role-less structure flattened away, relationships resolved to nodes that
  exist, and accessible names computed (explicit name, then the labelling
  node, then visible text) with label cycles terminated;
- Windows: real UI Automation server-side providers for every realized node
  (containers answer `WM_GETOBJECT` directly; `BUTTON`/`EDIT`/`STATIC` are
  subclassed), merged with each control's native proxy so only what the
  model states is overridden; fragment roots and fragments for virtual
  elements; Invoke, Value, RangeValue, Toggle, ExpandCollapse,
  SelectionItem, and ScrollItem patterns that turn into component events;
  property-changed, structure-changed, and live-region events raised from a
  posted message, never inside a runtime borrow; providers disconnected
  when their node or element goes away. The MSAA annotations from the
  standards-audit pass remain, for MSAA-only clients.

Planned platform targets not built here (macOS, iOS, Android, Linux) consume
the same portable model when their backends exist (Milestones 33–36).

---

## Milestone 27 — Animations and transitions

Implemented:

- a portable animation model in `framework_core::animation`: animatable
  properties (position, size, translation, opacity, background, foreground),
  typed animated values, `Transition` (a duration and easing curve, or a real
  mass/stiffness/damping spring, either with a start delay), `Animation`
  (explicit from/to, repeat counts, autoreverse, fill mode) and `Timeline`,
  the platform-free evaluator that turns elapsed time into per-property
  frames;
- **transitions**, declared on a node (`Node::with_transition`): when a
  rendered value changes, the backend animates from the previous value to the
  new one instead of jumping, then releases the property back to the tree;
- **interruption**: retargeting a running animation continues from the value
  and velocity it had, so a reversed drag or a second click bends the motion
  rather than restarting it;
- **cancellation**: per node and property, per owning component (a component
  that unmounts cancels its own animations), and per window;
- **frame scheduling**: one process-wide driver thread paced by `DwmFlush`,
  posting — never sending — a private frame message, coalesced per window, and
  asleep on a condition variable whenever nothing animates;
- **reduced-motion preference support**: the system preference
  (`SPI_GETCLIENTAREAANIMATION`) read at startup and tracked through
  `WM_SETTINGCHANGE`; each animation declares whether it is skipped or still
  run when motion is reduced;
- **declarative animation state**: nothing about a frame reaches the
  component tree — `Event::AnimationFinished` is the only thing a component
  hears, and it may ignore it.

Frames do not rerender. A frame applies the changed property to the native
object alone (`SetWindowPos` for geometry, a layered-window alpha for
opacity, an invalidation for colours); the tests assert the component's
render count does not move while values animate.

---

## Milestone 28 — Virtualized lists and large data sets

Implemented, reusing identity, layout, scrolling, and components rather than
a second list runtime — a virtual list is an ordinary scrollable column or
row (`Node::virtual_list`) that also declares how many items it logically
has:

- **visible-range calculation**: `VirtualRange::compute` from the scroll
  offset, viewport, per-item extents, and overscan (counted in items);
  `Event::VisibleRangeChanged` reaches the component only when the range
  changes, so scrolling inside a range renders nothing;
- **recycling/reuse**: on Windows, a render that removes rows of a virtual
  list and inserts others parks the removed `HWND`s and realizes the new rows
  on them (`rendering::pool`), whole item subtrees included; anything not
  reused is destroyed before the render ends;
- **stable item keys**: rows keep ordinary node keys, so a row that stays in
  range keeps its native window; `Node::with_item_index` says which item a
  row realizes, independent of its position among realized siblings;
- **incremental measurement**: `ItemExtent::Estimated` lists measure the rows
  they realize, report them as `LayoutResult::measured_items`, and record
  them in an `ExtentCache` (a Fenwick tree: `O(log n)` offsets and lookups);
  the backend lays out once more when a measurement moved an offset, then
  settles;
- **scroll position preservation**: `ScrollAnchor` holds the item at the top
  of the viewport across a render, so items inserted or removed above it do
  not move what is on screen;
- **efficient insertion/removal/reordering**: the existing keyed diff; only
  rows that entered or left the range are inserted or removed.

Virtual-list items announce their position in the whole list to assistive
technology ("5,012 of 100,000") automatically.

---

## Milestone 29 — Graphics / custom rendering escape hatch

Implemented as two explicit, isolated paths; ordinary UI is still native
controls:

- **custom canvas**: `Node::canvas(key, DrawList, layout)`. A `DrawList` is a
  retained, portable display list — fills, strokes, rounded rectangles,
  ellipses, lines, paths (lines, quadratic and cubic Béziers), text,
  RGBA images, and nested transform / clip / opacity scopes — held in the
  node tree and diffed like any node (cheap clones, pointer-equality fast
  path). On Windows a canvas is its own child window painted by Direct2D
  (DirectWrite for text); clips and opacity are Direct2D layers, so a
  rotated clip clips exactly; a lost device (`D2DERR_RECREATE_TARGET`) is
  recovered on the next paint from the draw list;
- **custom drawing regions**: `DrawList::hit_region(id, rect)` declares
  regions tested under the transforms and clips in force where they were
  declared; pointer input on a canvas carries the region it landed in
  (`PointerEvent::region`);
- **GPU surface access**: `Node::native_surface(key, layout)` is a bare child
  window the framework positions and sizes and never paints.
  `Event::SurfaceResized { surface, size, scale_factor }` reports its size;
  `framework_windows::native_surface(surface)` returns a `SurfaceHandle`
  implementing `raw-window-handle` 0.6's `HasWindowHandle`/`HasDisplayHandle`,
  which wgpu, ash, and glutin accept directly;
- **integration with platform compositors**: both paths are real child
  windows composed by DWM like every other control.

Not built: shader-backed drawing inside a `DrawList`. An application that
needs shaders owns a native surface and brings its own GPU API, which is
what that path is for.

---

## Milestone 30 — Persistence and navigation

Implemented on the managed component tree, not beside it:

- **navigation stacks**: `NavigationStack<R>` is data in the showing
  component's state; each entry's screen is that component's keyed child
  (`EntryId::key`), and every entry is rendered with all but the top one
  `Node::hidden`, so pushing never remounts the screens below and popping
  finds them as they were. Screens navigate with a `Navigator`, which sends
  `NavigationCommand`s through an ordinary child-to-parent `Callback`;
- **routes**: `Route` patterns (`/users/:id`, trailing `*rest`) with
  percent-decoding, typed parameters, and `build` for the reverse; a
  `Router` of named routes;
- **deep links**: `Event::DeepLink` to the primary root, `Application::open_url`,
  and on Windows the launch URL from the command line plus single-instance
  handoff (`WindowsPlatform::with_app_id`): a second launch forwards its
  URL through `WM_COPYDATA` to the running instance and exits;
- **state restoration / persistent state**: `StateStore` (sync; `MemoryStateStore`
  in core, crash-safe `FileStateStore` on Windows), set on `Services` so it
  is present from the first render; `ComponentContext::persisted` keyed by
  the component's key path, which is stable across runs; navigation stacks
  are `Serialize` and restore the same way; the primary window's placement
  is saved on close and restored on open;
- **lifecycle-aware storage**: writes are buffered and flushed after 1.5 s
  of quiet, before `Lifecycle::Suspending` / `Terminating` are delivered
  (`WM_POWERBROADCAST`, `WM_QUERYENDSESSION`), and when the last window
  closes;
- **tab/navigation containers**: `Node::tab_bar` realized as the system
  `SysTabControl32` (`Event::TabSelected`, controlled selection), with every
  tab's content kept mounted and hidden unless selected; `Node::hidden`
  removes a subtree from layout, focus traversal, and the accessibility
  tree while keeping its native objects and state.

---

# 6. Rendering and interaction milestones

All three (Milestones 27–29) are complete; see section 3.

## Milestone 31 — Developer CLI and project tooling

Implemented as `crates/rf` (package `rf-cli`, binary `rf`), which
orchestrates the native toolchains rather than replacing them:

```text
rf new <name> [--path DIR] [--framework-path DIR]
rf build <platform> [--release]
rf run   <platform> [--release]
rf check [platform]
rf test  [cargo test arguments...]
rf doctor [--json]
```

- **project creation**: `rf new` writes a project that compiles — `src/main.rs`,
  `Cargo.toml`, `rf.toml`, `README.md`, `.gitignore` — depending either on
  published framework versions or, with `--framework-path`, on a checkout;
- **project metadata**: `rf.toml` holds the application's identity (the same
  id its saved state, single-instance mutex, and package use), its display
  name, version, publisher, description, icon, and URL schemes. Every
  validation failure names the field it is about, and an unknown field is
  refused rather than ignored;
- **platform commands**: every platform on the roadmap is recognized; the
  ones without a backend fail with "no backend for `<platform>` yet — see
  PLAN.md, Milestone N" and exit code 3, never a silent build for Windows.
  Exit codes are part of the interface: 2 usage, 3 no backend, 4 a missing
  toolchain, 1 everything else;
- **`rf doctor`**: rustc against the MSRV, Cargo, git, the MSVC build tools
  (through `vswhere`), the newest installed Windows SDK and its `rc`, `mt`,
  `makeappx`, and `signtool`, plus per-platform readiness — as a table or,
  with `--json`, for a script.

The toolchains it drives, and the ones it will: Windows uses Cargo with the
MSVC build tools and the Windows SDK; macOS and iOS will use Xcode and the
Apple SDKs, Linux the system compiler, Android Gradle with the SDK and NDK,
and embedded targets their own toolchains through Cargo.

---

# 7. Application architecture milestones

## Milestone 32 — Packaging and deployment

Add reproducible packaging for each target:

- Windows installer / executable packaging;
- macOS app bundles;
- Linux packages/AppImage-style distribution where appropriate;
- Android APK/AAB;
- iOS application bundle;
- embedded firmware/images.

Signing, resource bundling, manifests, icons, and platform metadata belong here.

---

# 8. Additional platform backends

The Windows backend is the first production-oriented backend. New backends should be added only after the core contracts are stable enough to avoid duplicating accidental Windows assumptions.

## Milestone 33 — macOS backend

Native AppKit/Swift/Objective-C interoperability, native windows/controls, native text measurement, menus, accessibility, system services, and packaging.

## Milestone 34 — Linux backend

Start with one supported native toolkit/backend and keep the backend pluggable so additional Linux native backends can be added later.

Potential backend families may include GTK or another native toolkit depending on final architectural decisions.

## Milestone 35 — Android backend

Native Android view hierarchy, lifecycle integration, JNI/FFI boundary, Android text/input/accessibility, system services, Gradle integration, packaging.

## Milestone 36 — iOS backend

Native UIKit/Swift/Objective-C interoperability, lifecycle integration, native input/accessibility, system services, Xcode integration, packaging.

## Milestone 37 — Embedded backends

Separate embedded platform profiles from desktop/mobile assumptions.

Initial targets should distinguish:

- embedded Linux;
- RTOS-backed targets;
- selected bare-metal systems.

Capability-oriented design is critical here because embedded targets will not implement desktop concepts such as windows or accessibility.

---

# 9. Cross-cutting quality milestones

These should advance continuously rather than waiting for the end.

## Testing

Maintain:

- core unit tests;
- reconciliation tests;
- layout tests;
- scheduler tests;
- lifecycle tests;
- component-tree tests;
- platform integration tests;
- end-to-end example applications.

## Correctness boundaries

- keep `unsafe` localized;
- document every FFI ownership rule;
- minimize global mutable state;
- make task cancellation deterministic;
- keep native object lifetime explicit;
- preserve stable IDs across rerenders;
- never mutate component state from worker threads.

## Performance

Measure and eventually optimize:

- tree diff cost;
- layout cost;
- native object creation/destruction;
- text measurement;
- event dispatch;
- scheduler overhead;
- scrolling;
- virtualized list performance;
- startup and binary size.

## Tooling and diagnostics

Eventually add:

- tree inspection;
- component ownership diagnostics;
- native-object leak diagnostics;
- scheduler/task inspection;
- layout overlays;
- event tracing;
- accessibility inspection;
- platform capability diagnostics.

---

# 10. End-state architecture

The intended final architecture is:

```text
                       Application
                           │
                    Component Runtime
                           │
        ┌──────────────────┼──────────────────┐
        │                  │                  │
     Components          Scheduler          Services
        │                  │                  │
        ├── props          ├── tasks          ├── HTTP
        ├── state          ├── scopes         ├── storage
        ├── effects        └── wakeups        ├── clipboard
        ├── lifecycle                         └── platform APIs
        └── messages
        │
        ▼
                  Declarative UI Tree
                           │
                    Reconciliation
                           │
                       Layout
                           │
                    Native realization
                           │
       ┌───────────┬───────┼────────┬───────────┐
       ▼           ▼       ▼        ▼           ▼
    Windows      macOS   Linux    Android       iOS
       │           │       │        │           │
       └───────────┴───────┴────────┴───────────┘
                           │
                       Embedded
                 via capability subsets
```

The final framework should feel like a native application framework first and a cross-platform abstraction second: one Rust application model, native operating-system behavior, explicit platform capabilities, and strong compile-time/lifetime guarantees wherever Rust can provide them.

## Web platform roadmap (first-class target)

Web support is a full architectural track rather than a final packaging step. It must cover the browser environment end-to-end.

### Web milestone A — WASM runtime and browser host

Build a dedicated `framework-web` adapter that:

- compiles the shared Rust runtime to WebAssembly;
- owns browser-side initialization and lifecycle;
- bridges Rust to JavaScript/Web APIs only at the platform boundary;
- creates and tracks DOM/native handles without exposing browser types to `framework-core`;
- integrates with the browser event loop and microtask/task model.

### Web milestone B — Native DOM realization

Map framework nodes to semantic DOM elements rather than a canvas renderer:

```text
Button  → <button>
TextInput → <input>/<textarea>
Label    → <span>/<p>/<label>
Column   → <div>
Row      → <div>
Dialog   → <dialog> or an appropriate semantic composition
```

The adapter must retain stable node identity and native DOM ownership just like the desktop backends retain native HWND ownership.

### Web milestone C — CSS/layout integration

Introduce a deliberate mapping between the framework layout model and browser layout mechanics. The framework must define which semantics are computed in Rust and which are delegated to CSS, avoiding two competing layout engines for the same subtree.

This includes:

- intrinsic measurement;
- constraints;
- wrapping;
- overflow and scrolling;
- clipping;
- transforms;
- viewport sizing;
- responsive/container sizing;
- device-pixel-ratio and viewport changes.

### Web milestone D — Browser input/focus/accessibility

Integrate:

- pointer and mouse events;
- keyboard events;
- composition/IME events;
- text input and selection;
- pointer capture;
- touch and pointer cancellation;
- focus/blur;
- browser Tab order;
- clipboard events;
- native HTML accessibility;
- ARIA roles/properties/states where a semantic HTML element is insufficient.

The framework accessibility model must map to real browser semantics rather than emulate accessibility behavior itself.

### Web milestone E — Browser services/capabilities

The capability system must include browser implementations for, where supported:

- HTTP/fetch;
- WebSocket;
- Web Storage;
- IndexedDB;
- Cache Storage;
- Clipboard;
- Notifications;
- File System Access;
- Web Share;
- geolocation;
- camera/microphone/media devices;
- Bluetooth/Web Bluetooth where available;
- sensors where available;
- permissions;
- downloads;
- URL/history/navigation;
- service workers;
- web workers.

Capability availability must be explicit because browser support varies by browser, security context, permissions, user gesture, and deployment mode.

### Web milestone F — Async/runtime scheduling

The scheduler must support browser constraints without blocking the browser main thread. Tasks should map to appropriate WASM/browser mechanisms, and DOM operations must remain on the browser thread where required.

Long-running CPU work should have a path to Web Workers when the capability is available, with a message bridge back into the framework scheduler.

### Web milestone G — Routing, navigation, persistence, and browser lifecycle

Add browser-native lifecycle concepts:

- URL routing;
- History API integration;
- back/forward navigation;
- deep links;
- page visibility;
- online/offline state;
- before-unload/pagehide/unload boundaries where applicable;
- persistence and restoration;
- storage-backed application state.

### Web milestone H — SSR, hydration, and progressive enhancement

The framework should support server-rendered HTML as an optional deployment mode, followed by Rust/WASM hydration on the client. This requires deterministic tree identity and a hydration-safe native ownership model.

The Web roadmap therefore includes:

```text
Rust server/runtime
      ↓
HTML output
      ↓
browser loads page
      ↓
WASM runtime
      ↓
hydrate existing DOM
      ↓
normal reconciliation
```

Hydration must preserve semantic HTML, support accessibility before hydration where practical, and detect/handle server-client tree mismatches deterministically.

### Web milestone I — Service workers and offline applications

Support service-worker-backed application architectures for offline caching, background synchronization where available, update/version management, and installable Progressive Web Apps.

### Web milestone J — Web packaging, testing, and deployment

The CLI/package system must eventually support:

- WASM builds;
- JavaScript/WASM glue generation;
- static assets;
- source maps;
- development server;
- browser hot reload/development workflow;
- production bundling;
- SSR deployment;
- PWA manifests;
- service-worker packaging;
- browser test execution;
- accessibility testing;
- feature/capability detection.

Web support is complete only when an application can be developed, tested, packaged, deployed, and updated through the same framework tooling rather than merely compiled to WASM.

---


## Long-range roadmap
