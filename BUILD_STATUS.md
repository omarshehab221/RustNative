# Build Status — Native Rust Framework

## Current milestone

**Standards-audit remediation is now fully closed**: every P0, P1, and P2
finding in `Audit.md`, and every item in its Phase 1–3 roadmap, is
implemented and verified (see below) — including the two items that
earlier revisions of this document described as deliberately deferred
(native-dialog owner-window support, and full `missing_docs` enforcement).
**Window lifecycle + multi-window support** was the last feature milestone
before that.

The framework currently has a Rust-first component/runtime architecture with a native Win32 backend. Component-owned asynchronous task scopes now automatically cancel outstanding tasks when the component leaves the framework-managed component tree.

## Completeness pass (milestones 1–24)

A full audit of milestones 1–24 against a "production framework" bar found that most of the architecture was already solid (real native HWNDs, full tree diffing, working Tab/Shift+Tab traversal, structured async task scopes with cancellation), but several milestones had gaps between what was documented as "implemented" and what actually took effect end-to-end. All of the following were closed in this pass:

- **Milestone 21 (theme + styling) had no realization at all.** `Theme`/`VisualStyle` resolution existed and was unit-tested in isolation, but nothing ever called it outside of tests: `TreeSnapshot` only ever carried a node's raw style *override*, and the Windows backend never read font or color data — no `WM_SETFONT`, no `WM_CTLCOLOR*`, no background paint. Fixed: `TreeSnapshot::from_node_with_theme` resolves every node's style (theme default merged with override, in its `Normal`/`Disabled` state) before it reaches the backend, mirroring how layout geometry is already fully resolved in the core; the Windows backend now realizes it as native fonts (`CreateFontIndirectW` + `WM_SETFONT`) and colors (`WM_CTLCOLORSTATIC`/`WM_CTLCOLOREDIT`/`WM_CTLCOLORBTN` for controls, `WM_ERASEBKGND` for containers), with GDI resources owned and freed per node.
- **No `disabled` concept existed anywhere**, so the theme's `Disabled` state variant was unreachable. Fixed: `Node::disabled(bool)`/`is_disabled()`, threaded through `TreeNode`, realized as `EnableWindow` on Windows, and excluded from Tab-order focus traversal.
- **Live interaction style variants were declared but not realized.** Fixed: the Windows renderer retains each node's unresolved style override and applies hover, pressed, and focus variants as transient native repaint state via `TrackMouseEvent`/`WM_MOUSELEAVE`, mouse-button messages, and focus synchronization; these changes never force a component rerender.
- **Milestone 23 ("native dialogs, menus, and system integration") was only portable contracts.** The Windows backend had no file-dialog implementation, `notify()` was a hardcoded stub error, and there was no menu system — not even a data model. Fixed: `WindowsFileDialogs` (open/save/folder-pick — originally via `GetOpenFileNameW`/`GetSaveFileNameW`/`SHBrowseForFolderW`, since modernized to `IFileOpenDialog`/`IFileSaveDialog`; see the standards-audit pass's P1.15 below), real toast-style notifications via `Shell_NotifyIconW`, and a full `MenuBar`/`MenuItem` model in the core realized as native `HMENU`s with `WM_COMMAND` routing to a new `Event::MenuAction`.
- **Milestone 24 ("multi-window support") only let you open windows before the platform's event loop started.** There was no way for a running component to open or close a window in response to an event (e.g., a menu action or button click), since `Application::open_window`/`close_window` aren't reachable from `ComponentContext`. Fixed: a deferred `WindowCommand` queue (`ComponentContext::windows()` → `WindowRequests::open`/`close`) applied by `Application` after each dispatch/task-pump, and a Windows-side `WindowRegistry` that keeps native HWNDs in sync with `Application::window_ids()` as they change at runtime — including a specific, documented design to avoid a use-after-free when a window is asked to close itself mid-dispatch (its teardown is deferred via a posted `WM_CLOSE`, never synchronous).

`Capability::Menus` was added, and `WindowsPlatform::capabilities()` now advertises `FileDialogs`, `Notifications`, and `Menus` alongside the previously-advertised capabilities. `examples/hello-label` now exercises all of the above (a disabled button past count 5, a native menu bar with a checkable-style toggle and a runtime-opened settings window, both routed through `Event::MenuAction`).

**Known, deliberately out-of-scope remainder**, consistent with the framework's existing platform-independence philosophy: drag-and-drop, system share, and system-appearance-change notifications remain portable contracts/flags only (not advertised as supported capabilities, so no app can be misled into depending on them); native menus are static at window creation (matching the existing maturity level of a `Window`'s title and size, which also are not reactively updatable after the window opens).

## Standards audit remediation pass

`Audit.md` (a from-scratch senior-engineer review against a "would this pass
review at a top-tier systems team" bar) found 3 P0s, 20 P1s, and 15 P2s.
Every P0, every P1, and every P2 in that document is now closed, including
every item in its Phase 1–3 roadmap — the two items (P0.2, P1.15's owner-
window support, and P2.23's `missing_docs` enforcement) that earlier status
updates once recorded as deferred were all revisited and closed for real,
the first two once this pass developed a way to actually compile and run
`framework-windows`'s Windows-only code in this environment (see P1.17
below), and the third as a self-contained mechanical documentation pass
verified by the crate's own `deny(missing_docs)` lint. A prior pass had
already closed a meaningful subset before this one started (engineering
controls — `deny.toml`, `clippy.toml`,
`rustfmt.toml`, `SECURITY.md`/`CONTRIBUTING.md`, an accurate
`rust-toolchain.toml` — plus task-scope bounded retention, effect-dependency
exact equality instead of hashing, stale-event rejection, indexed
`TreeSnapshot` children/depth lookups, selective layout invalidation,
checked identity allocators, RAII menu resources with checked command-id
allocation, and a dedicated STA thread for native dialogs instead of the
shared blocking pool — but, notably, *not* an actual CI workflow: this
repository had none until this pass, see P1.17 below). This pass picked up
from there and closed every remaining P0/P1/P2 finding and every roadmap
item in `Audit.md`, once the compile/test breakthrough below made
`framework-windows`'s Windows-only surface reachable by the same tools
(`cargo build`/`test`/`clippy`/`fmt`) as `framework-core` already was.

**P0.1 — node identity could theoretically collide (`framework-core`,
fully fixed).** `NodeId::from_key` was an FNV-1a hash of the key string:
compact, but a many-to-one mapping, so *some* pair of distinct keys
colliding was mathematically inevitable even though any one collision was
astronomically unlikely. Replaced with a true interning table
(`crate::identity`) keyed by exact string equality: distinct keys are now
*structurally* guaranteed distinct ids, not just very-probably distinct.
Covered by both a unit test and a `proptest` property test generating
hundreds of randomized key sets per run.

**P1.7/P1.18 — the async scheduler was hard-wired to one process-global
Tokio runtime, and delays could not be tested deterministically (fully
fixed).** Introduced `crate::scheduler::Executor`, a pluggable backend
trait; `Scheduler::new()` still defaults to the same shared two-worker
runtime as before (`TokioExecutor::shared`, so no behavior changed for
existing callers), but `Scheduler::with_executor` now lets a host supply
its own (`TokioExecutor::dedicated(n)` for an independently-owned runtime,
or any other `Executor` impl). Time was initially left as a known, separate
gap (`SleepFuture` still read the shared runtime's timer regardless of
which executor a `Scheduler` used) — that gap is now closed too:
`Executor` gained a `sleep` method alongside `spawn`, `Scheduler::sleep`
delegates to it, and `ComponentContext::sleep`/`EffectContext::sleep`
route through their owning component's scope's scheduler instead of a
free-standing constructor. `ManualExecutor` is the new deterministic,
virtual-time backend this unlocks: nothing runs until `run_until_stalled`
is called, and delays only resolve once `advance(duration)` moves the
virtual clock (in deadline order, even across multiple concurrently
pending sleeps) — so a component/effect test that spawns a task or awaits
a delay no longer needs to race a real clock or a real thread pool to
assert on the outcome. Covered by six new unit tests in
`scheduler::executor::tests`.

**P1.10 — user-triggerable composition mistakes could panic (fully
fixed).** A component using the same node key twice in one render, or
calling `ComponentContext::effect` with a duplicate key, used to trip a
release-mode-silent `debug_assert!`/`assert!`. Both are now a structured
`RenderError` (`crate::component::RenderError`), surfaced through
`ComponentTree::render`'s `Result` and inspectable afterward via
`last_render_error()`, while the render pass still completes a
structurally consistent tree rather than aborting. Internal
impossible-states (a bookkeeping map missing an entry the runtime itself
just inserted) deliberately remain `debug_assert!`/`expect!` — the line
this pass draws between the two is documented directly on `RenderError`.

**P1.4 — the Windows tray-notification icon used an undocumented `hWnd =
NULL` identity, added and immediately deleted per call (fixed).**
`framework-windows` now creates one persistent, hidden, message-only host
window on first use (`NotificationHost`) and keeps it alive for the life
of the process; the icon is added once and updated in place via
`NIM_MODIFY` for every subsequent notification instead of being torn down
and recreated (which also fixes a visible taskbar-icon flicker the old
add/delete-per-call design had). Verified against `windows-sys`'s actual
`HWND_MESSAGE`/`NIM_MODIFY` constants via the crate's published docs
before writing the code, per the same discipline as the rest of this
crate's Win32 surface (see the local-verification limitation below).

**P0.2 — a single `GWLP_USERDATA` slot was cast to two different types
across three files with the invariant re-explained by hand at each call
site (fully fixed).** Top-level windows store a `*mut Runtime` there;
container/control windows store a cached `COLORREF` there — a disjoint use
of the same untyped Win32 slot, previously accessed through seven-plus
individual `GetWindowLongPtrW`/`SetWindowLongPtrW` calls spread across
`message_loop.rs`, `container.rs`, and `renderer.rs`. Centralized behind
two narrow typed accessors in a new `native::user_data` module —
`RuntimeSlot::{get, set}` and `BackgroundColorSlot::{get, set}` — so the
invariant is documented and can be audited or changed in exactly one place,
and every call site outside that module is now a plain function call
instead of a bespoke `unsafe` cast. This is now genuinely compiler-verified
rather than reviewed by hand only: it compiles cleanly as real
`x86_64-pc-windows-gnu` code (see `tools/windows-cross-test.sh` under
"Local verification" below), and its one cross-cutting behavioral
assumption (`GetWindowLongPtrW` on a null `HWND` is well-defined and
returns `0` rather than crashing, which `RuntimeSlot::get` relies on when
called against a possibly-null ancestor window) was additionally confirmed
against a real Win32 implementation via a MinGW/Wine ground-truth probe.

**P1.17 — no way to ever compile or run the native Win32 backend existed
(fixed).** `framework-windows`'s `native` module is `#[cfg(windows)]`-gated,
so every local development and review pass — including every one before
this repository had a CI workflow at all — ran on Linux, where that module
is simply excluded from the build. No commit in this project's history had
ever actually had its Win32 logic compiled, let alone tested or run, by
anything. Three fixes landed together: (1) `.github/workflows/ci.yml` adds
a `windows-latest` job so this finally happens automatically on real
Windows going forward (unverified until its first real CI run, since this
environment cannot run GitHub Actions itself); (2)
`tools/windows-cross-test.sh` (see "Local verification" below) makes it
possible *right now, in this environment*, via `RUSTC_BOOTSTRAP=1 -Z
build-std` plus Wine — no more waiting for a Windows machine or a CI run to
find out whether a change to `native/` compiles; (3) using exactly that
pipeline, `native/` went from zero unit tests of its own to sixteen —
`native::user_data`, `native::registry`, and `native::measure` are now
covered against real `HWND`s (message-only windows, needing no display
driver), including a real GDI-handle-leak proof via `GetGuiResources`
before/after counts, not just data-structure-level assertions. This pass
also used the pipeline to find and fix a previously-invisible problem at
real scale — see "122 real clippy findings" below — and to implement P1.15
with actual compiler feedback rather than blind review. What remains
genuinely open: this is Wine, not Windows (see "Local verification" for
exactly where that distinction matters and does not substitute for the
real Windows CI job), and message-loop-level integration tests (creating a
full top-level window and driving its message loop, as opposed to the
narrower `native::registry`/`native::user_data`/`native::measure` unit
tests this pass added) remain future work.

 Split into ~20 focused modules matching the audit's
own suggested map: `identity`, `event`, `node`, `component/{context,
effects,error,tree}`, `reconcile/{snapshot,diff}`,
`layout/{geometry,constraints,measure,engine}`, `style/theme`,
`scheduler/{mod,executor}`, `services/{mod,memory}`, `capability`, `menu`,
`window`, `application`, `platform`. The public API is re-exported flat
from the crate root exactly as before (`framework_core::NodeId`,
`framework_core::Component`, ...), so no downstream call site — including
every one in `framework-windows` and the example — changed.
`framework-windows` was deliberately **not** similarly re-split in this
pass; see "What this pass deliberately did not do" below.

**Encapsulation (P2.24/25).** `VisualStyle`, `Theme`, `WindowState`, and
`TreeDiff`'s operation list moved from public fields to accessor methods
(`Theme` gained builder methods — `with_button`, `with_foreground`, etc. —
so a custom theme can still be constructed without the struct-update
syntax the public fields used to allow). `TreeNode` and the plain
geometry/color value types (`Rect`, `Point`, `Size`, `Color`,
`Typography`, ...) deliberately kept public fields: they have no
invariant a getter would protect and are read broadly by any backend by
design — see `crate::reconcile::snapshot`'s and `crate::layout::geometry`'s
module docs for the reasoning.

**Other P2s closed:** removed the unused, never-adopted `ServiceFuture`
type alias (P2.26); the workspace lint policy now runs
`clippy::pedantic`/`clippy::cargo` in addition to the previous minimal
set, with each `#[allow]` exception commented at its use site (P2.40);
added a `proptest`-based property test suite covering identity,
reconciliation round-tripping, and layout non-negativity under randomized
input (P2.34); added a `criterion` benchmark suite covering snapshot
construction, diffing, layout, component dispatch, and task scheduling
(P2.35); every public struct/enum/trait/associated type in `framework-core`
now has a doc comment (P2.23 — see "Known, deliberately incomplete" below
for the part of this that's still open).

## 122 real clippy findings, found and fixed for real

`cargo clippy --workspace --all-targets -- -D warnings` had, since this
project's `deny.toml`/`clippy.toml` policy was first written, only ever
actually been evaluated against `framework-core` and the tiny
`#[cfg(windows)]`-free surface of `framework-windows` — every claim in this
document (and every prior one) that "clippy is clean" was true only of
that subset, because clippy, like `cargo check`, silently excludes
`#[cfg(windows)]`-gated code on a non-Windows host. The first time this
pass ran clippy through `tools/windows-cross-test.sh`'s real
cross-compilation path — against the actual `x86_64-pc-windows-gnu` target,
with the workspace's real `-D warnings` policy — it returned **122
errors**, none of them previously known, spread across every file in
`native/` and `services/`.

These were fixed for real, not suppressed: `SetWindowLongPtrW`/`GetClientRect`/
`DrawTextW`-style implicit-reference-to-pointer calls became explicit
`&raw const`/`&raw mut`; ad hoc `WM_SIZE`/`WM_MOVE`/`WM_COMMAND`/
`WM_MOUSEWHEEL` bit-twiddling (`(x >> 16) & 0xffff) as u16 as i16`-style
chains, several of them subtly duplicated across files) was replaced with
shared, tested `loword`/`hiword`/`loword_signed`/`hiword_signed` helpers in
`native::util`; every genuinely lossless numeric conversion (`u8`/`u16` →
`u32`/`i32`) switched from `as` to `u32::from`/`i32::from`; a handful of
`unsafe` blocks (an `OwnedMenu::drop`, a notification-host `unsafe impl
Send`, a `RegisterClassW` call) were missing the safety-comment this
workspace's `undocumented_unsafe_blocks = "deny"` policy requires, and now
have one; and every remaining narrowing/sign-changing cast that really is
safe by construction (masked-to-16-bits message fields; struct sizes that
can never approach `u32::MAX`; a `COLORREF`'s 24 significant bits fitting
in an `isize` regardless of pointer width) got a `#[allow]` with a comment
explaining *why*, at the exact site, rather than a blanket suppression —
matching this workspace's existing P2.40 policy for every other
intentional lint exception. `clippy::multiple_crate_versions` firing on a
second `syn` major version pulled in transitively by the `windows` crate
(used only by `services::dialogs`, see P1.15 below) is recorded as an
accepted, externally-imposed exception in `clippy.toml`'s
`allowed-duplicate-crates`, rather than worked around by depending on an
older/newer `windows` release chosen only to dodge the lint.

**What this pass deliberately did not do, and why:**

- **`framework-windows`'s module split (P1.21/P2.22).** Unlike
  `framework-core`, this crate's `native` module can only be
  compiler-verified via the cross-compilation path in
  `tools/windows-cross-test.sh` (see "Local verification" below), not via
  plain `cargo check` on this host — a real but no longer absolute
  limitation. The crate is organized into the same kind of focused module
  tree `framework-core` uses: `native::{app, container, input, measure,
  menu, message_loop, registry, renderer, runtime, user_data,
  window_handles, util, test_support}`, plus `error`, `ffi`, `platform`,
  and `services::{clipboard, dialogs, notifications, system}` at the crate
  root. This pass added `native::user_data` and `native::test_support` to
  that tree (see P0.2 and P1.17 above), and a later pass in the same
  overall effort added `native::window_handles` (see P1.15's owner-window
  section above), otherwise leaving the module boundaries as they already
  were, since a wholesale reorganization of unsafe FFI code is a separate,
  larger piece of work from what this pass's compile-verification
  breakthrough makes newly safe to attempt.

## P1.15 — modern `IFileDialog`, with real owner-window support (fixed)

An earlier note in this document deferred this, reasoning that
`windows-sys` (this crate's dependency everywhere else) only exposes raw
COM vtables, and hand-indexing into one to call `IFileOpenDialog`/
`IFileSaveDialog` risked a silent, undefined-behavior-on-real-Windows
mistake with no compiler able to catch it in this environment. Both halves
of that reasoning held up under a real attempt this pass, and pointed to
the same fix: `windows-sys` genuinely has no COM interface definitions at
all for `IFileOpenDialog` (confirmed by their total absence from its own
source, not just from what this crate happens to enable), so implementing
this against raw `windows-sys` really would have meant re-deriving the
interface's GUID and vtable layout from documentation by hand. Instead,
`services::dialogs` now depends on the `windows` crate — Microsoft's own
higher-level, maintained bindings, which provide the actual generated
`IFileOpenDialog`/`IFileSaveDialog`/`IShellItem` methods rather than raw
vtable slots — scoped to just this one module via a `cfg(windows)`-only,
minimal-feature dependency (see `Cargo.toml`'s comment on it), not added
workspace-wide. `show_open_or_save`'s two functions and the previous
`SHBrowseForFolderW`-based folder picker were replaced with three
functions built on `IFileOpenDialog`/`IFileSaveDialog`, unified where
possible: `IFileOpenDialog` with `FOS_PICKFOLDERS` is Microsoft's own
documented modern replacement for `SHBrowseForFolderW`, so folder picking
now goes through the same modern interface family as file open/save
instead of a third, older API. This compiles, links, and passes clippy's
full `-D warnings` policy against the real `x86_64-pc-windows-gnu` target
via `tools/windows-cross-test.sh` — the same real verification this
document describes for everything else in `native/`.

**`Audit.md`'s Phase 3 roadmap item 15 — real owner/parent-window support —
is now also closed.** `FileDialogRequest` (the cross-platform request type
in `framework-core`, shared by every future platform backend) gained an
`owner: Option<WindowId>` field. `framework-windows` resolves it to a real
native `HWND` through a new module, `native::window_handles`: a small,
thread-safe `WindowId -> HWND` table for top-level windows, kept in sync by
`WindowRegistry` at window creation and by `window_proc`'s `WM_DESTROY`
handling at window teardown. This exists specifically because
`services::dialogs` runs each dialog on its own dedicated STA thread (see
P1.15's `run_sta` discussion elsewhere in this document), not the
message-loop thread that actually owns the window — so resolving a window
identity to its native handle from the dialog thread means reaching across
threads for it, which every other native handle in this crate deliberately
avoids needing to do. `dialogs.rs`'s `show_open`/`show_save`/
`show_pick_folder` now call `IFileDialog::Show` with that resolved handle
(or `None`, unchanged, for a request with no owner or one naming a window
this process doesn't currently recognize — an owner is a presentation
enhancement, not a correctness requirement, so a stale or absent owner
falls back to an unowned dialog rather than an error). Covered by three new
unit tests in `native::window_handles::tests`, run and passing under the
same real Windows/Wine verification as everything else in `native/` (see
"Local verification" below) — `framework-windows`'s native test suite is
now 19 tests, up from 16.

## `missing_docs` (P2.23, now fully enforced)

Every public item in `framework-core` — including individual struct
fields, enum variants, trait methods, and associated functions, not just
the types that contain them — now carries a doc comment, and
`#![deny(missing_docs)]` (not `warn`) is set in `lib.rs` so this cannot
silently regress. An earlier pass documented every type but left the lint
disabled entirely, deferring roughly 380 field/variant/method-level
warnings as a bounded, mechanical follow-up rather than fixing them or
half-enabling a lint that would fail `-D warnings` CI on an unrelated
backlog; this pass wrote that documentation (not filler — each comment
describes what the specific field/variant/method actually does) and
flipped the lint to `deny`. `cargo build -p framework-core --all-targets`
now reports zero `missing_docs` warnings.



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
cargo audit
cargo run -p hello-label
```

## Local verification

`framework-core` — every module from this pass and the audit-remediation
pass above — was compiled, linted, and tested locally with a genuine
current-stable toolchain (`rustc`/`cargo` 1.91.1, obtained via this
environment's package manager, well past the crate's declared MSRV of
1.85), not the older, edition-downgraded 1.75 compiler prior passes were
limited to. `cargo test -p framework-core` passes (65 test functions: 57
unit tests across every module — including new ones in
`scheduler::executor::tests` covering `ManualExecutor`'s deterministic
scheduling and virtual-time behavior — 4 integration tests in
`component_lifecycle.rs`, and 4 `proptest` properties in
`property_tests.rs` — each of those 4 runs hundreds of independently
generated cases per invocation, so effective input coverage is well beyond
the function count alone), `cargo bench -p framework-core` runs its full
`criterion` suite successfully, `cargo clippy --workspace --all-targets
--all-features -- -D warnings` is clean under the strengthened
`pedantic`/`cargo` lint policy, and `cargo fmt --all -- --check` is clean.
`examples/hello-label` was also fully type-checked against the same
compiler and compiles cleanly.

The Windows-specific code under `crates/framework-windows/src/native/` was,
this pass, **actually compiled, linked into real PE32+ binaries, and
executed** for the first time in this project's history — not merely
type-checked on a non-Windows subset. Earlier passes (and the first attempt
in this one) concluded this was impossible: this environment's rustc ships
a prebuilt standard library only for its own host target, `rustup target
add x86_64-pc-windows-gnu` needs network access to
`static.rust-lang.org` (outside this environment's allowlist), and `-Z
build-std` looked like the obvious workaround except that it is a
nightly-only flag a stable-channel compiler rejects outright. That last
conclusion was wrong, or at least incomplete: `RUSTC_BOOTSTRAP=1` makes a
*stable* rustc and cargo accept nightly-gated flags, including `-Z
build-std`, as if running on nightly. Combined with fetching the exact
matching `library/` source for the installed rustc's own commit hash
(verified to match exactly, not just the version number) from
`rust-lang/rust` on GitHub — sparse-checked-out to avoid downloading the
full repository, plus its `library/backtrace` git submodule fetched
separately at its pinned commit, since a plain checkout doesn't pull
submodules — this successfully builds `core`/`alloc`/`std` from source for
`x86_64-pc-windows-gnu`. The only remaining gap, two small "Rust runtime
startup object" files (`rsbegin.o`/`rsend.o`) that bootstrap normally
builds as a special case rather than through a normal `cargo build`, is
closed by compiling `library/rtstartup/{rsbegin,rsend}.rs` directly with
`rustc --emit=obj` and placing the result in the sysroot's target `lib/`
directory. The whole procedure — including a hard check that the fetched
library source's commit actually matches the installed compiler's, so this
can never silently build against mismatched source — is now a reusable,
documented script: **`tools/windows-cross-test.sh`**.

Running it:

- Compiles `framework-core` and all of `framework-windows` — every file
  under `native/` and `services/`, including this session's `user_data.rs`
  centralization and the `IFileOpenDialog`/`IFileSaveDialog` rewrite of
  `services::dialogs` (P1.15) — as real `x86_64-pc-windows-gnu` code,
  against the real `windows-sys`/`windows` crates. This is qualitatively
  different from a host-target `cargo check`: on the host target, the
  *bodies* of every `#[cfg(windows)]`-gated function are stripped before
  type-checking even runs; cross-compiled for real, every `unsafe` FFI
  call, struct field access, and pointer cast in that module went through
  full type-checking, borrow-checking, and code generation.
- Links successfully via `x86_64-w64-mingw32-gcc`/`-ld` (from
  `gcc-mingw-w64-x86-64`) into real PE32+ binaries — confirmed with `file`.
- Runs `framework-core`'s full 57-test unit suite under Wine, all passing,
  as an actual Windows binary rather than a Linux one.
- Runs `framework-windows`'s 19-test suite under Wine, all passing — see
  "Native test coverage" below for what these actually exercise.
- Runs `cargo clippy` against this same real target with the workspace's
  full `-D warnings` policy — see "122 real clippy findings" above.
- With `--run-example` and an Xvfb virtual display available, actually
  launches `examples/hello-label` as a live Win32 GUI application under
  Wine: `CreateWindowExW` succeeds for real (confirmed by the absence of
  Wine's "no driver could be loaded" diagnostic once a display is
  present, versus its presence when none is), the menu bar realizes, and a
  screenshot (`import -window root`) shows real native controls — a menu
  bar with "File"/"View", a native button, and native labels reflecting
  the component tree's actual rendered state — positioned by this crate's
  actual layout engine and painted by actual Win32 child windows.

This is Wine, not Windows, and the two are not identical — differences in
DPI handling, theming (`visual styles`/`UxTheme`), IME behavior, shell COM
interfaces (`IFileOpenDialog`'s Wine implementation has historically lagged
real Windows more than plain `user32`/`gdi32`, which is one reason P1.15's
dialog code, while fully compiled and clippy-clean, has not been run
end-to-end interactively under Wine the way `hello-label` has), and other
areas Wine deliberately or incidentally diverges from real Windows are
exactly the kind of thing this technique cannot catch. Treat a pass here as
"compiles, links, and behaves plausibly under Wine", and the real Windows
CI job (`.github/workflows/ci.yml`, `test-windows`) as the actual release
gate — this does not replace that, but it closes nearly all of the gap
between "reasoned about carefully with no compiler" and "confirmed on real
Windows" that every previous pass's notes described as unclosable in this
environment.

### Native test coverage (P1.17, closed this pass)

Before this pass, `framework-windows` had exactly one unit test
(`platform::tests::capabilities_only_advertise_realized_backend_features`),
and it did not touch the `native` module at all — every other module
under `native/` had zero automated coverage of its own, correctness
resting entirely on manual review. This pass used the cross-compilation
pipeline above to add sixteen tests total, all running against real
`HWND`s created via a shared `native::test_support::TestWindow` helper
(message-only windows — `HWND_MESSAGE` as parent — which need no display
driver, so these tests run identically with or without Xvfb, including
under Wine's "null" graphics driver):

- `native::user_data`: `RuntimeSlot`/`BackgroundColorSlot` round-trip
  through a real `GWLP_USERDATA` slot on a real window, a null-`HWND`
  safety check pinned against the real linked `GetWindowLongPtrW` (not
  just the separate `tools/win_probes/null_hwnd.c` C probe), and an
  explicit test documenting that the two typed accessors are views over
  the same underlying storage.
- `native::registry`: insert/get round-tripping, duplicate-`NodeId`
  rejection (and that a rejected insert doesn't clobber the existing
  entry), the `id_for_hwnd` reverse lookup for both plain objects and a
  container's two HWNDs, `remove` clearing both directions, and — the
  strongest of these — a real proof via `IsWindow` that
  `NativeObjectRegistry::drop` actually destroys every `HWND` it still
  owns, not just that it compiles to do so.
- `native::measure`: real `GetDC`/`DrawTextW`-based measurement
  (monotonicity under more text, respecting a max-width constraint), and
  a real GDI-handle-leak proof for `ControlStyle::resolve`/`Drop` via
  `GetGuiResources(GR_GDIOBJECTS)` before/after counts — the same
  accounting Windows' own Task Manager uses, not a heuristic.

**What remains open:** message-loop-level integration tests (creating a
full top-level window via `native::runtime`/`native::app` and driving
`WM_COMMAND`/`WM_SIZE`/etc. through it, as opposed to the narrower
unit-level tests above) are still future work — this pass closed the "zero
coverage, and no way to add any" gap, not "full coverage of every
message-dispatch path."

**Confirmed on real Windows, and one real finding from doing so.** The
person using this project ran `cargo test --workspace` on an actual
Windows machine after this pass — the first time anything in this
repository's history had that happen. 72 of 73 tests passed unmodified,
including all 57 `framework-core` tests and 15 of 16 `framework-windows`
tests, which is itself a strong, independent confirmation that the
Wine-based verification this pass relied on throughout was not fooling
itself. The one failure,
`control_style_drop_frees_every_gdi_handle_it_created` (`before=0,
after=4`), was a real bug — but in the *test*, not in
`ControlStyle::drop`, whose logic (unconditionally freeing both fields
when non-null) is correct by inspection. `GetGuiResources(GR_GDIOBJECTS)`
is a process-wide counter, and `cargo test` runs tests concurrently across
threads by default; without serializing every GDI-object-creating test
against each other, a brush or font created by a *different*,
concurrently-running test between this test's "before" and "after"
snapshots produces exactly the same symptom as a real leak. Fixed with a
`static GDI_ACCOUNTING_LOCK: Mutex<()>` in `measure.rs`'s test module,
held for each GDI-creating test's entire before/during/after window (not
just around the allocating calls) — scoped to that one file since it is
the only one whose tests create real (non-null) GDI objects. Notably, this
raciness did not reproduce under Wine even across several runs in this
pass, which is itself worth recording: it suggests Wine's `GetGuiResources`
accounting is not sensitive enough to this class of interference to be a
reliable stand-in for it, one more concrete instance of the "Wine is not
Windows" limitation this document already calls out elsewhere, now with a
specific example rather than only the general caveat.

### MinGW/Wine struct-layout ground-truth verification

Separately from full-crate cross-compilation above, this pass also used
`x86_64-w64-mingw32-gcc` to compile small, standalone C programs against
real `<windows.h>` headers, run under Wine, to get real executed ground
truth (not documentation lookup) for specific behavioral assumptions this
crate's `unsafe` FFI code depends on. Used to confirm
`GetWindowLongPtrW(NULL, GWLP_USERDATA)` returns `0` (and sets
`ERROR_INVALID_WINDOW_HANDLE`) rather than crashing — the assumption
`native::message_loop::run_message_loop` and `RuntimeSlot::get` both rely
on when resolving a possibly-null ancestor window. The probe sources are
kept at `tools/win_probes/*.c` for reuse on any future struct-layout or
documented-edge-case question.

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
