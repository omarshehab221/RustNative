# Method: the stack, from the root up

## Why this document exists before the archetype documents

A framework is not a product feature list. It is a stack of decisions, and the
decisions at the bottom constrain everything above them. A framework that
chose a garbage-collected runtime at layer 0 cannot have a deterministic
destruction guarantee at layer 6, no matter how good its API looks. A
framework that chose immediate-mode rendering at layer 2 cannot have a real
accessibility tree at layer 5 without building a retained model it had
deliberately refused.

So this analysis is ordered the way the stack is built, not the way a brochure
is written. Every archetype is examined starting at layer 0 — what it actually
is at the bottom — and climbed from there. A weakness at layer 7 is a backlog
item. A weakness at layer 1 is permanent.

[`foundations.md`](foundations.md) analyses the root layers themselves — the
strategies available at each of L0–L4, independently of who chose them — and
is the document to read first. The platform documents then analyse concrete
archetypes as *paths through those choices*.

## The ten layers

```text
L9  governance, docs, stability, ecosystem
L8  ship and operate: packaging, deployment, updates, observability
L7  engineering loop: iteration speed, inspection, testing
L6  application services: data, navigation, persistence, capabilities
L5  host integration: accessibility, internationalization, system conventions
L4  application model: components, state, effects, lifetime
L3  semantic UI: identity, reconciliation, layout, styling, text
L2  realization: what a "widget" is and who draws it
L1  execution: event loop, threading, scheduling, async, memory discipline
L0  substrate: language, runtime, compilation model, FFI boundary
```

Read downward to predict a framework's failures; read upward to plan one.

### L0 — Substrate

The language and its runtime. Compiled or interpreted, statically or
dynamically typed, garbage-collected or deterministically freed, single
binary or runtime-plus-bundle, and what crossing into foreign code costs.

Everything about startup time, memory floor, artifact size, and where type
errors surface is decided here, and cannot be undone above.

### L1 — Execution

The event loop and its relationship to the host's. Which thread may touch host
objects, how background work gets back to it, how a task's lifetime is bounded,
how time is obtained, and whether the model is preemptive or cooperative.

Everything about responsiveness, cancellation correctness, and whether async
bugs are possible-but-rare or structurally impossible is decided here.

### L2 — Realization

What a UI element *is* when it exists. A host-owned object, an object the
framework draws itself, a document node the host renders, or nothing at all
between frames. Who owns the pixels, who owns the hit test, who owns the text
caret.

Everything about host fidelity, accessibility reach, per-platform cost, and
visual consistency is decided here. This is the layer where the
fidelity/uniformity trade is made, and it is irreversible.

### L3 — Semantic UI

Identity across time, the algorithm that turns a new description into host
mutations, the layout algorithm, the styling model, the text pipeline (shaping,
wrapping, bidirectional ordering, measurement), and the surface a developer
actually writes the description in.

Everything about update cost, layout correctness under translation and
accessibility text scaling, and whether identity survives a rerender is decided
here. So is a softer but decisive property: whether the authoring surface a
framework offers is one surface done well, or several of unequal quality.

### L4 — Application model

Components, props, local and shared state, effects, lifecycle, messages, and
the rules binding background work to the lifetime of the thing that started it.

Everything about how a developer reasons — and about which bug classes exist at
all — is decided here.

### L5 — Host integration

Accessibility trees and their bridges, input methods and complex-script text,
locale and bidirectional layout, and the system conventions a user expects:
contrast, reduced motion, text scale, colour scheme, right-click and long-press,
scroll physics, focus rings.

This layer is where "cross-platform" frameworks are found out, and it is the
most common place for a technically impressive framework to be rejected.

### L6 — Application services

Data fetching and caching, forms and validation, routing and navigation,
persistence and restoration, and the capability-guarded services an application
needs from its host.

This is the layer where most teams' day-to-day code lives, and the layer most
often left to third-party libraries.

### L7 — Engineering loop

Iteration speed, live inspection, debugging, profiling, and every level of
testing.

Decides adoption more often than any layer below it.

### L8 — Ship and operate

Packaging, signing, store and distribution rules, deployment shapes, updates
after ship, performance budgets enforced in CI, security and supply chain, and
production observability.

Decides whether the framework survives contact with a real release.

### L9 — The project

Documentation, examples, API reference, versioning and deprecation policy,
migration tooling, decision records, contribution path, third-party ecosystem,
interoperability with existing code, and machine-readable descriptions for
code-generating tools.

Decides whether anything above ever gets a second project.

## The parity bar per layer

A framework is production ready **for a target** when it clears every line
below for that target. The lines are deliberately falsifiable: each one can be
tested, measured, or shown to be absent.

| Layer | The bar |
| --- | --- |
| L0 | Startup, memory floor, and artifact size are declared numbers, not emergent ones; the foreign-code boundary has stated ownership rules |
| L1 | One documented thread affinity rule; cancellation is deterministic; no work outlives the scope that owns it; time comes from the host clock |
| L2 | Host objects reused across updates by stable identity; nothing that exists visually is invisible to the host's accessibility and automation layers |
| L3 | Layout survives text scaling, translation growth, and right-to-left mirroring; text is shaped and measured by the host's own stack; update cost is proportional to what changed; every authoring surface offered reaches the whole API, with equality proven rather than claimed |
| L4 | Illegal UI states unrepresentable where the type system allows; effects and tasks bounded by the lifetime of their component; state local by default |
| L5 | Verified with the host's own screen reader; IME and complex scripts work; locale formatting, plurals, collation, and bidirectional text supported; system settings honoured without application code |
| L6 | Data caching, invalidation, optimistic updates, and offline behaviour are framework concerns; routing typed and host-conventional; persistence crash-safe and migratable; capabilities answered honestly |
| L7 | Code change visible in seconds with state preserved; a live tree inspector with layout and re-render tracing; component tests without a host and end-to-end tests with one; deterministic time and async in tests |
| L8 | One command produces the signed artifact each host demands; every real deployment shape is a mode, not a rewrite; an update path after ship; budgets enforced in CI; crash capture with symbolication |
| L9 | Generated API reference and task-oriented guides; runnable example per subsystem; stability policy with a support window and automated migration; incremental adoption inside existing applications |

## Scoring

| Score | Meaning |
| --- | --- |
| **Met** | Shipped, verified on real hardware for at least one target, and covered by a test that fails if it regresses |
| **Partial** | Shipped for some targets or some cases; the contract exists but the coverage or the verification does not |
| **Planned** | Specified in `PLAN.md` at implementable depth, not built |
| **Absent** | Not specified anywhere; a genuine gap this analysis is surfacing |

"Verified" carries `PLAN.md` section 2.13's meaning exactly: run on the real
host, recorded in `BUILD_STATUS.md`. Work reasoned through but not run is never
scored above **Planned**.

## How each archetype is analysed

Bottom-up, in the same order, every time:

1. **Root (L0–L2)** — what it is at the substrate, how it executes, what a
   widget physically is. The irreversible part.
2. **Semantics and model (L3–L4)** — identity, update algorithm, layout, state.
3. **Integration (L5–L6)** — what it gives the host back, and what it leaves to
   the application.
4. **Loop and ship (L7–L8)** — the daily experience and the release.
5. **Project (L9)** — why it wins or loses regardless of the above.
6. **SWOT** — strengths, weaknesses, opportunities, threats, derived from the
   climb rather than asserted before it.
7. **What we must ship** — falsifiable requirements, tagged by target, each one
   written so a milestone can be derived and a test can prove it.

Requirement tags: `[X]` cross-cutting, `[W]` web, `[D]` desktop, `[M]` mobile,
`[E]` embedded/RTOS, `[T]` terminal.

## The four asymmetries

Three are structural weaknesses in the competition that come directly from
root-layer choices, and one is a structural weakness of ours.

1. **The dynamic-runtime tax (L0).** An interpreted or just-in-time runtime is
   carried into every artifact: startup cost, memory floor, size, a second
   collector, a second type system, and a boundary where types stop being
   checked. Compensated with tooling; never removed.

2. **The fidelity/uniformity trade (L2).** Draw your own widgets and you get
   identical output everywhere and lose the host — accessibility re-implemented
   per platform and perpetually behind, system settings re-implemented or
   ignored, every host release a chase. Use host widgets and you get fidelity
   and pay per host. Only the first debt compounds against the framework's own
   users.

3. **The seam count (L0–L9, assembled).** Most stacks are an assembly of
   independently versioned parts, each with its own idea of errors, async, and
   time. Every seam loses types, stack traces, and guarantees. Owning the whole
   path is only an advantage if the ownership is *used* to make guarantees no
   assembly can.

4. **Accumulated ecosystem (L9), against us.** Not closable by building faster.
   The only strategy that has ever worked is interoperability and incremental
   adoption — being usable *inside* what already exists.
