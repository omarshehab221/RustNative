# Gap plan — from the matrix to the roadmap

[`parity-matrix.md`](parity-matrix.md) says where we stand. This document says
what to build, in what order, and why that order.

It does not restate the backend work already in `PLAN.md` sections 8 and 9.
Everything here is either missing from the plan entirely, or present only as a
bullet under "eventually" when the analysis says it decides adoption.

## The sequencing rule that governs everything below

**Anything that is a per-backend obligation must land before the second
backend exists.** A conformance suite, a capability shape, a layout property,
or an embedding contract costs once when there is one backend and N times when
there are N. This is the same argument `PLAN.md` 2.4 makes about widening
contracts rather than adding conditionals, applied to schedule instead of
structure.

Everything else is ordered by what it unblocks.

## Four tiers

```text
Tier 0  before the second backend      — per-backend obligations and the seams
Tier 1  alongside the backend work     — the loop, the guarantees, the budgets
Tier 2  before any public release      — the application layer users expect
Tier 3  with and after the web track   — the server, deployment, ecosystem
```

Tiers are not strict phases: Tier 1 runs continuously the way `PLAN.md`
section 9 does. Milestone numbers below are identities, not an order —
`PLAN.md` section 8 already establishes that convention.

---

# Tier 0 — Before the second backend

## Milestone 53 — The markup syntax

**Why.** `foundations.md` F3.5: a markup surface is how the largest population
of UI developers arrives at a framework at all, and it decays into a trap
unless its equality with the builder surface is enforced by a test. Carried as
a source dialect — the form that population already writes — it carries a
second obligation: the compile step must be invisible in use. It is Tier 0
because the authoring surface is what every later example, template, guide,
doc test, and conformance case is written in — retrofitting a second syntax
through that corpus later costs more than every other Tier 0 item, and grows
with each backend.

**Covers.** `X-L3-8`, `X-L3-9`, `X-L3-10`, `X-L3-11`.

**Scope.**

- `X-L3-8` **Capability equality by construction.** Markup is a compile-time
  front end that lowers to builder calls and nothing else; every node kind is
  an element, every builder method or style field an attribute, `..expr`
  reaches any `Node -> Node` function including an application's own
  extensions, and `{expr}` splices any builder expression into markup. An
  equivalence suite asserts both spellings of every node kind and modifier are
  equal values, with the markup compiled through both carriers.
- **One grammar, two carriers.** `.rsx` files, where an element is an
  expression written anywhere Rust accepts one, for code that is mostly UI;
  and `rsx!`, for markup in `.rs` files, crates without a build step, and
  runnable documentation. The `.rsx` compile step only wraps markup in
  `rsx!`, so parsing, lowering, and diagnostics exist once, and the no-bare-text
  rule keeps the grammar identical in both.
- Structural constructs markup is good at — conditional and repeated children,
  fragments, component elements with typed props (their context inferred from
  the enclosing function in `.rsx` files, named explicitly in `rsx!`) —
  without narrowing the builder form to match, or the reverse.
- `X-L3-9` Diagnostics at compiler quality, held by a compile-failure suite
  run through both carriers.
- `X-L3-10`, `X-L3-11` Tooling parity and an invisible compile step:
  diagnostics from `rustnative build`, `check`, and `test` reported at the
  `.rsx` source; a language-server proxy mapping positions both ways; a
  whole-file formatter; an expansion view; source-map round trips under test.
- Both syntaxes in every document, template, and doc example, side by side;
  project templates that require a choice rather than defaulting to one.

**Done when.** The equivalence suite covers every node kind and modifier
through both carriers, the diagnostics suite covers every listed error through
both carriers, a `.rsx` diagnostic lands at its source position in the CLI and
the editor, both templates build, and no documented example exists in only one
syntax.

## Milestone 39 — Portable-surface obligations

**Why.** Three archetype families fail in the same place: a portable API shaped
by the first host it was written for (`desktop.md` D6, D10; `mobile.md` M3).
The cost of fixing it rises with each backend.

**Covers.** `D-PT-1`, `D-PT-2`, `D-TW-1`, `X-L3-3`, `M-OB-1`, `M-OB-3`,
`X-L5-2`, `X-L4-4`, `X-L1-2`, `X-L0-5`, `X-L0-6`.

**Scope.**

- A desktop-class affordance audit of the portable API — window management,
  menus, shortcut maps, hover and cursors, drag-and-drop, multi-window state —
  each expressed as a capability rather than assumed present.
- A reverse audit: which parts of the current API encode a Windows assumption,
  resolved by widening the contract (2.4), not by conditionals.
- Right-to-left as a layout-model property: start/end throughout, mirroring
  applied by the core, with the backend applying host mirroring where it has
  it.
- Safe areas, display cutouts, foldable hinges, and split-screen as layout
  properties.
- Permission states as a capability enum (not-asked, granted, limited, denied,
  permanently-denied) with a portable request flow.
- A gesture arbitration contract describing how our recognizers coexist with a
  host's.
- Per-target panic and teardown policy, thread-affinity enforcement, one
  ownership module per backend, and the escape-hatch contract, all written down
  as the rules a new backend is held to.

**Done when.** A new-backend conformance checklist exists, the Windows backend
passes it, and each item names the test that proves it.

## Milestone 40 — Interoperability and incremental adoption

**Why.** The one asymmetry that runs against us is accumulated ecosystem
(`method-and-stack.md`), and the only strategy that has ever beaten it is being
usable *inside* what already exists. Every archetype that grew fast grew this
way. It is Tier 0 because embedding constrains how a backend realizes its root,
and retrofitting it means rewriting each backend's root.

**Covers.** `X-INTEROP-1`, `D-LG-1`, `D-LG-2`, `M-AS-1`, `M-AS-2`, `M-MP-1`,
`M-MP-2`, `D-GX-1`, `M-EN-1`, `W-CL-2`, `W-MS-1`, `E-HAL-1`.

**Scope.**

- `X-INTEROP-1` **Embedding, both directions.** Our tree realized into a
  caller-supplied host window, view, or DOM node; and a foreign host object
  adopted as a leaf of our tree, laid out and clipped by our layout model.
- Library-only mode: application model, state, scheduler, services, and data
  layer compiled into an existing native application through a generated typed
  interface, with no UI dependency.
- Surface handoff: a node owning a host-native rendering surface, with
  documented lifetime, resize, DPI, and present semantics.
- Guest-runtime mode: our runtime driven from someone else's `main` and
  initialization, owning neither startup nor the loop (required for vendor
  embedded SDKs and for mounting inside an existing HTTP service).
- A documented adoption ladder — library, embedded subtree, full application —
  with a worked example at each rung.

**Done when.** A sample existing application on each shipped backend hosts a
RustNative subtree, and a sample RustNative application hosts a foreign
control, both under test.

---

# Tier 1 — Alongside the backend work

These run continuously, like `PLAN.md` section 9, and each new backend is
expected to satisfy them as part of being called complete.

## Milestone 41 — Guarantees and conformance suites

**Why.** Our root-layer advantages (`foundations.md` F0.4, F1.4, F2.1, F3.3)
are currently implementation properties, not guarantees. An unproven guarantee
is marketing; a tested one is a moat. This milestone converts each one.

**Covers.** `X-L3-1`, `X-L3-2`, `X-L3-4`, `X-L3-6`, `X-L3-8`, `X-L2-1`,
`X-L2-2`, `X-L1-1`, `X-L1-4`, `X-L0-2`, `X-L5-1`, `W-FG-1`, `D-FP-1`,
`D-FP-2`, `M-FP-1`, `D-WV-1`, `D-SD-2`, `X-L2-3`.

**Scope.**

- Syntax equivalence as a standing guarantee: the suite Milestone 53 creates
  lives here permanently, so a node kind or modifier added in one syntax only
  fails the build.
- A documented invalidation contract with tests that fail on
  over-invalidation, plus render-cause tracing that names the state, prop,
  resource, or effect responsible.
- The transient-state fast path (2.10) as a contract with tests.
- Scope-bound cancellation as a public guarantee: no task observes or mutates
  state after its owner unmounts, proven per target.
- Native-object lifetime as a guarantee, with leak detection as a CI gate.
- Host-object reuse and modal-operation conformance (resize, menu tracking,
  native dialogs, drag loops) per backend.
- A fidelity conformance checklist per backend, measured against the host's own
  first-party applications.
- A text conformance suite: complex scripts, bidirectional text, grapheme
  clusters and emoji sequences, font fallback, spaceless line breaking, caret
  and selection geometry.
- A layout conformance suite at multiple text scales with pseudo-localized
  strings, asserting no clipping, overlap, or lost targets.
- Accessibility assertions in CI plus a recorded screen-reader pass per
  backend.
- The published comparison methodology and results against self-drawing and
  webview archetypes.

**Done when.** Each guarantee has a named test, and no backend is called
complete without passing the suite.

## Milestone 42 — Budgets

**Why.** Every performance claim in this analysis is an assertion until it is
measured, and the archetypes we beat on footprint (`web.md` W3, W10, W11;
`desktop.md` D7a; `mobile.md` M3) are beaten only with numbers.

**Covers.** `X-L0-1`, `X-L0-3`, `W-RS-1`, `W-RS-2`, `W-SL-1`, `W-ED-2`,
`M-BR-2`, `D-WS-1`, `E-GUI-4`, `W-SH-1`, `M-OB-4`.

**Scope.** Declared per-target budgets — cold start, resident memory, artifact
size, frame-time distribution including worst case, input latency, per-route
client payload, serverless cold start, edge CPU time per route, embedded RAM
and flash, container image size, and build time — measured in CI on every
commit, on a low-end reference device or profile where one exists, with
regressions failing the build and the numbers published.

**Done when.** A budget file exists per target, CI enforces it, and the public
documentation quotes the measured numbers rather than adjectives.

## Milestone 43 — The developer loop

**Why.** Our substrate's one structural disadvantage (`foundations.md` F7), and
the pillar most cited in framework selection. No competitor will fix it for us.

**Covers.** `X-L7-1`, `X-L7-2`, `M-BR-4`, `E-PR-1`.

**Scope.** Rebuild-and-restart with application state preserved from a
serialized snapshot, against a declared wall-clock budget; optional dynamic
library reload for the application crate where the platform allows; the loop
working on-device for mobile and embedded, not only on the development machine;
board and device quickstart in one command including flashing, logging, and
restart; and a measured first-hour target — project creation to running on a
device in three commands or fewer.

**Done when.** The loop's wall-clock time is in the budget file (M42) and is
met on every shipped backend.

## Milestone 44 — Inspection and diagnostics

**Why.** Dynamic substrates get inspection from reflection; we must expose it
deliberately. `PLAN.md` section 9 lists it as "eventually add", which
understates what it decides.

**Covers.** `X-L7-3`, `X-L7-4`, `X-L3-5`, `D-IM-1`, `E-RS-2`.

**Scope.** A runtime inspection protocol — declarative tree, realized host
objects and their mapping, state and props (readable and editable), layout with
per-node explanation, event and render tracing, task and scope inspection, host
object lifetimes — over a transport that works locally, on-device, and
remotely; an inspector client shipped with the CLI; an in-application overlay
on the draw-list path for hosts without a second screen; and a reduced form
using deferred host-side formatting for constrained targets.

**Done when.** Every backend answers the protocol, including terminal and
embedded in reduced form.

## Milestone 45 — Test infrastructure

**Why.** `PLAN.md` 2.13 caps verification at what hardware we have. A headless
backend lifts most of that cap for everything above L2, and it is the
prerequisite for testing the application layer built in Tier 2.

**Covers.** `X-L7-5`, `X-L7-6`, `X-L7-7`, `X-L7-8`, `M-OB-2`, `M-OB-4`,
`E-GUI-2`.

**Scope.** A headless reference backend realizing the tree into an inspectable
model; synthetic input dispatched through the real input path; a deterministic
test clock and executor; golden and visual regression tests over realized
output; a lifecycle conformance suite (process death and restoration,
configuration changes, deep-link entry during restoration, low-memory trim); a
device and emulator matrix in CI; and a host-side device simulator for
display-bearing embedded profiles.

**Done when.** Component, interaction, and golden tests for the full
application layer run on a machine with none of the target hardware.

---

# Tier 2 — Before any public release

The application layer every competing archetype has and we do not. These are
the reasons commercial software picks a framework.

## Milestone 46 — Internationalization and localization

**Why.** The largest single omission in the current plan
(`parity-matrix.md` L5). Every archetype at every maturity level has an answer.
A framework without one is not viable for commercial software, and retrofitting
it touches every string, every layout, and every backend.

**Covers.** `X-L5-3`, `X-L5-4`.

**Scope.** Typed message catalogues with plural and grammatical-gender
categories; compile-time-checked placeholders; locale-aware number, date,
currency, unit, and collation behaviour delegated to host facilities where they
exist; bidirectional text and locale-aware casing; runtime locale switching
that updates the tree; an extraction and merge workflow for translators with
context; and pseudo-localization wired into the dev loop and the layout
conformance suite (M41).

## Milestone 47 — State, resilience, and data

**Why.** Four **Absent** rows that together are most of what an application
actually does: shared state, error containment, the async data layer, and
forms.

**Covers.** `X-L4-1`, `X-L4-2`, `X-L4-3`, `X-DATA-1`, `X-DATA-2`, `X-DATA-3`,
`W-SF-7`, `M-FP-2`, `M-FP-3`, `D-MC-1`.

**Scope.**

- A shared/scoped state contract — typed, observable, no global mutable state,
  with defined update ordering and the same lifetime discipline as component
  state — plus a state history contract giving undo/redo.
- Error boundaries: a subtree may fail, be contained, present a fallback, and
  be retried, with the failure reported through the diagnostic channel.
- `X-DATA-1` the async data layer: typed queries keyed by identity, declared
  cache lifetimes, deduplication, background revalidation, retries with
  backoff, pagination and infinite scrolling, optimistic updates with rollback,
  and invalidation composed with the component lifecycle.
- `X-DATA-2` offline mutation queueing with a conflict policy.
- `X-DATA-3` schema migration for persisted state, with up/down and a dry run.
- One validation and forms model shared by client and server — one schema, one
  set of messages, checked at compile time — with two-way binding ergonomics,
  dirty tracking, and a submission lifecycle.
- Constrained background work as a portable service contract, and asset/image
  loading with decode, downscale, caching, and lifetime-bound cancellation.

## Milestone 48 — Components, tokens, and visualization

**Why.** Primitives are not a component set, and the design-to-code path is how
applications are actually built (`web.md` W14; `desktop.md` D3).

**Covers.** `X-UI-1`, `X-UI-2` / `X-L3-7`, `X-VIZ-1`, `D-SD-1`, `E-GUI-3`.

**Scope.** A component library covering the controls applications need,
realized natively per backend with documented accessibility semantics for each;
a design-token pipeline into the theme system with a documented schema and an
explicit split between semantic roles mapped to host appearance and absolute
brand values; charting and visualization on the draw-list path with an
accessible alternative for every visual encoding; a documented hybrid pattern
for custom-drawn subtrees inside a natively realized tree; and a constrained
text profile declaring which scripts each embedded profile supports.

---

# Tier 3 — With and after the web track

## Milestone 49 — The server application model

**Why.** The largest uncontested opening identified anywhere in this analysis
(`web.md` W7): nobody offers a batteries-included application backend in a
compiled, statically typed language with a native UI story attached. It is
Tier 3 because it depends on Web milestone H's render path and on M47's data
and forms work.

**Covers.** `W-SF-1`…`W-SF-6`, `W-MF-1`, `W-EP-1`, `W-MS-1`.

**Scope.** A server application model built on the same component, scheduler,
and service contracts as the client — typed request handling, middleware,
sessions, error pages; authentication and authorization with session and token
strategies, password and passkey handling, provider federation, and a typed
policy model; a data layer with compile-time-checked queries, migrations with
up/down and dry run, pooling, and transaction scoping bound to request
lifetime; durable background jobs with retries, backoff, idempotency keys, and
inspection; secure-by-default request handling; a generated administrative
surface derived from schema and policy; typed server functions with one
definition checked at both call sites; layered typed configuration; and
mountability inside an existing compiled-language HTTP service.

## Milestone 50 — Deployment, updates, and fleet operations

**Why.** Shipping and updating is where frameworks are judged after the demo,
and we currently have no answer for updates on any target
(`mobile.md` M3; `embedded.md` E6).

**Covers.** `W-MF-6`, `W-MF-7`, `W-DP-1`, `W-DP-2`, `W-DP-3`, `W-SL-2`,
`W-SL-3`, `W-SL-4`, `W-ED-1`, `W-ED-3`, `W-SH-1`, `M-BR-1`, `E-MW-1`,
`E-MW-2`, `E-BL-1`, `W-IS-1`, `W-IS-3`, `W-HM-1`, `W-HM-2`.

**Scope.** Deployment adapters as a stable documented contract with local
emulators for each shape; `rustnative deploy` plus export of standard
infrastructure descriptions so existing pipelines consume our output; preview
deployments, staged rollout, and one-command rollback; host-limit capabilities
and request-scoped cancellation on per-request hosts; edge storage and cache
contracts preserving each platform's semantics; container and embedded-Linux
package output; mobile over-the-air updates within each host's rules with
signing, staged rollout, rollback, and version pinning; firmware update with
A/B slots, verification, and automatic rollback, delegating to an existing
bootloader; zero-payload static routes and a progressive-enhancement baseline;
and single-artifact server deployment with embedded assets.

## Milestone 51 — Observability, security, and compliance

**Why.** What makes a framework acceptable to buyers who never read a benchmark
(`web.md` W9; `desktop.md` D7a; `embedded.md` E3).

**Covers.** `X-OBS-1`, `W-EP-2`, `W-EP-3`, `D-WS-2`, `E-SF-1`, `E-SF-2`,
`E-BL-2`, `E-OB-1`, `E-OB-2`, `E-OB-3`, `E-IOT-1`, `E-AI-1`, `E-RB-1`,
`D-CT-2`.

**Scope.** `X-OBS-1` crash capture with per-target symbolication, structured
logging, metrics, and tracing spanning the client/server boundary, with opt-in
privacy-respecting telemetry and a published data policy, and the ability to
reconstruct the tree state at failure from the inspection protocol; a stated
threat model per target with capability scoping and escape-hatch reach;
generated compliance evidence (dependency inventory and licences, software bill
of materials, accessibility conformance, privacy and permission manifests); a
stated certification posture with requirement traceability maintained as build
output; power-aware scheduling, watchdog integration, safe-state panic paths,
and bounded-allocation mode on embedded profiles; and service contracts for
messaging, provisioning and device identity, bounded-latency inference, and
node-graph transports.

## Milestone 52 — The project around the framework

**Why.** L9 decides whether anything above gets a second project, and two items
here — the stability policy and the machine-readable description — are direct
answers to the loudest complaint about the incumbent archetype and to how a
growing share of code is now written.

**Covers.** `W-MF-8`, `X-DOC-1`, `X-DOC-2`, `X-ECO-1`, `X-ECO-2`, `E-RS-3`,
`E-PR-2`, `D-CT-1`, `E-RS-1`, `E-EC-1`, `E-EC-2`, `E-HAL-2`, `E-K-3`.

**Scope.**

- A published stability and deprecation policy with a support window, plus
  automated migration for every breaking change.
- `X-DOC-1` task-oriented guides and a published generated API reference;
  `X-DOC-2` a runnable example per subsystem and per supported board.
- `X-ECO-1` a third-party capability package contract: a community-authored
  service or control implementing a portable contract with per-backend code,
  discoverable and versioned, with no framework fork required.
- `X-ECO-2` a machine-readable description of the framework — component and
  service contracts, capabilities, events, and layout semantics — so
  code-generating tools produce correct code rather than plausible code.
- A stated non-duplication policy for the embedded Rust ecosystem, with the
  `Executor` contract demonstrated against an existing embedded executor,
  subsystem capability mapping for a configuration-driven RTOS ecosystem, and
  peripheral access left to the existing trait ecosystem.
- The one-stack span demonstrated by a single application genuinely built for
  desktop and for a device, and documented core cost characteristics (stack,
  heap, worst-case timing).

---

# Dependency order

```text
M53 markup syntax ─────────────────┐
M39 portable-surface obligations ──┼─→ every later backend
M40 interoperability ──────────────┘

M53 equivalence suite ──→ owned by M41 thereafter

M41 guarantees        ─┐
M42 budgets           ─┼─ continuous, gate each backend's completion
M43 developer loop    ─┤
M44 inspection        ─┤
M45 test infrastructure┘ ──→ required by M46–M48

M46 internationalization ─┐
M47 state, resilience, data ─┼─→ M49 server model
M48 components and tokens  ─┘        ↓
                              M50 deployment and updates
                                     ↓
                              M51 observability and compliance
                                     ↓
                              M52 the project
```

`E-K-1`, `E-K-2`, `X-L1-3`, and `X-L1-5` (the `no_std` core profile, the
non-`Send` executor, and the host clock) are already in `PLAN.md`'s long-range
roadmap as shared core work and remain where they are: before the targets that
need them.

---

# The differentiation statement

Parity on the matrix is the price of being considered. This is the argument for
being chosen, and every clause is a claim the work above is designed to make
testable rather than rhetorical.

1. **Host-native realization on every target, from one application model.**
   Competitors have one or the other. The ones with host fidelity are
   single-platform; the ones that are portable draw their own widgets and
   inherit a permanent accessibility, input-method, and system-settings deficit
   (`foundations.md` F2.1 versus F2.2). Proven by M41's conformance suites and
   M41's published comparison, not asserted.

2. **No runtime tax anywhere on the range.** One substrate from a browser
   sandbox to a microcontroller, with declared and CI-enforced startup, memory,
   and size budgets (M42). The archetypes that lead on ergonomics cannot follow
   here, because the tax is their substrate.

3. **Correctness properties that are guarantees rather than conventions.**
   Scope-bound task cancellation, deterministic native-object lifetime, typed
   boundaries where competitors serialize, and compile-time checking across the
   client/server seam (M41, M49). Each with a conformance test behind it.

4. **One seam count of one.** UI, state, layout, routing, persistence, data,
   forms, services, packaging, and deployment versioned as one product with one
   stability policy (M46–M52) — answering both the assembly problem and the
   migration complaint that dominates the incumbent's feedback.

5. **Adoptable inside what already exists.** Library, then embedded subtree,
   then application (M40). This is the only clause that addresses the one
   asymmetry running against us, and it is therefore the one that must not slip.
