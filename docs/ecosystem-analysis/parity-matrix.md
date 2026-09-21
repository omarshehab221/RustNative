# Parity matrix — where RustNative stands

Scored with the rules in [`method-and-stack.md`](method-and-stack.md):
**Met** (shipped, verified on real hardware, regression-tested), **Partial**
(shipped for some targets or cases), **Planned** (specified in `PLAN.md` at
implementable depth, not built), **Absent** (not specified anywhere).

Evidence is `PLAN.md` sections and milestones and `BUILD_STATUS.md` records.
"Verified" means `PLAN.md` 2.13's meaning: run on the real host.

Current reality: Milestones 1–32 complete and verified on Windows/Win32. Every
other backend is **Planned**. Scores below are therefore given as
*core contract* first and *coverage across targets* second, because those two
numbers differ enormously and averaging them would hide the real position.

---

## L0 — Substrate

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Compiled, no runtime floor, deterministic destruction | **Met** | Rust throughout; `framework-core` has no platform dependency (2.4) | — |
| Declared startup / memory / size budgets | **Absent** | Performance listed as "measure and eventually optimize" (section 9) | `X-L0-1`, `X-L0-3` |
| `no_std`-capable core profile | **Planned** | Named in Milestone 37 and the long-range roadmap | `E-K-1` |
| Foreign-boundary ownership rules documented per backend | **Partial** | Windows confines raw handles; rule stated but not generalized | `X-L0-5` |
| Escape-hatch contract (obtain, use, invalidate) | **Partial** | 2.6 states the principle; no contract document | `X-L0-6` |
| Native-object lifetime as a tested guarantee | **Partial** | Ownership implemented; leak diagnostics are "eventually" | `X-L0-2` |

## L1 — Execution

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Host-owned loop with scheduler wake-ups | **Met** (Windows) / **Planned** (rest) | Milestone 17; per-backend run-loop integration in section 8 | `X-L1-1` |
| Modal-operation conformance (resize, menus, dialogs, drag) | **Absent** | Not specified as a test anywhere | `X-L1-1` |
| Thread affinity enforced, not documented | **Partial** | Enforced in practice; not stated as a typed or asserted rule | `X-L1-2` |
| Executor seam | **Partial** | `Executor` contract exists (2.4); non-`Send` profile outstanding | `X-L1-3`, `E-K-2` |
| Structured task scopes bound to lifetime | **Met** (core) | Milestone 18 | `X-L1-4` (make it a stated guarantee + conformance test) |
| Host clock abstraction | **Planned** | Named in the long-range roadmap | `X-L1-5` |
| Deterministic test clock/executor | **Absent** | — | `X-L7-6` |

## L2 — Realization

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Host-native realization with identity and reuse | **Met** (Windows) | Milestones 2, 5; 2.7, 2.8 | — |
| Draw-list path for hosts without controls | **Met** (core) / **Planned** (targets) | Milestone 29; reused by Milestones 37, 38 | `E-GUI-1` |
| Per-backend fidelity conformance against first-party applications | **Absent** | Not specified | `X-L2-1`, `D-FP-1`, `M-FP-1` |
| Published fidelity/accessibility comparison methodology | **Absent** | — | `X-L2-2` |
| Host-window/view embedding (both directions) | **Absent** | Not specified for any backend | `D-LG-1`, `D-LG-2`, `M-AS-1`, `M-AS-2` |
| Host rendering-surface handoff | **Absent** | Milestone 29 accepts our draw list only | `D-GX-1`, `M-EN-1` |

## L3 — Semantic UI

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Stable identity, keyed reconciliation, host-object reuse | **Met** | Milestones 2, 4 | — |
| Portable layout with host intrinsic measurement | **Met** (Windows) | Milestones 6–9, `IntrinsicMeasurer`, 2.11 | — |
| Transient-state fast path | **Partial** | 2.10 is a principle, not a contract with tests | `W-FG-1` |
| Invalidation contract + over-invalidation tests | **Absent** | — | `X-L3-1` |
| Render-cause tracing | **Absent** | — | `X-L3-2` |
| Right-to-left as a layout-model property | **Absent** | Layout model has no start/end mirroring story | `X-L3-3` |
| Text-scale and translation-growth conformance | **Absent** | — | `X-L3-4` |
| Layout explanation in tooling | **Planned** | "layout overlays" under section 9 tooling | `X-L3-5` |
| Text conformance (complex scripts, bidi, clusters, fallback, breaking, caret) | **Partial** | Host measurement delegated; no conformance suite | `X-L3-6` |
| Theming | **Met** (Windows) | Milestone 21 | — |
| Design-token pipeline | **Absent** | — | `X-L3-7` / `X-UI-2` |
| Builder authoring surface | **Met** | Every node kind and modifier since Milestone 1; 2.9 | — |
| Markup authoring surface at equal capability, in `.rsx` files and `rsx!` | **Planned** | 2.9; Milestone 53 | `X-L3-8` |
| Markup diagnostics at compiler quality, through both carriers | **Planned** | Milestone 53 compile-failure suite | `X-L3-9` |
| Markup tooling parity (format, editor, expansion view) | **Planned** | Milestones 53, 43 | `X-L3-10` |
| `.rsx` compile step invisible in use (source-mapped diagnostics, language-server proxy, whole-file formatter) | **Planned** | Milestones 53, 43 | `X-L3-11` |

## L4 — Application model

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Components, props, callbacks, lifecycle | **Met** | Milestones 13–16 | — |
| Local state, effects, reactive invalidation | **Met** | Milestones 3, 19 | — |
| Scope-bound async with no post-unmount mutation | **Met** (core) | Milestones 17, 18; 2.12 | `X-L1-4` |
| Shared/scoped state contract without globals | **Absent** | Props and callbacks only; no shared-state story | `X-L4-1` |
| Undo/redo and state history | **Absent** | — | `X-L4-2` |
| Error boundaries with fallback and retry | **Absent** | — | `X-L4-3` |
| Per-target panic and teardown policy | **Partial** | Stated only for the terminal backend (Milestone 38) | `X-L4-4` |

## L5 — Host integration

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Portable accessibility model with per-host bridge | **Met** (Windows) / **Planned** (rest) | Milestones 11, 26; section 8 per backend | — |
| Accessibility assertions in CI + screen-reader pass per backend | **Absent** | Verification described per backend, not automated | `X-L5-1` |
| Advanced input, focus, IME | **Met** (Windows) / **Planned** (rest) | Milestones 11, 12, 25 | — |
| Gesture arbitration with host recognizers | **Partial** | Stated for one mobile backend only | `X-L5-2` |
| **Localization and internationalization** | **Absent** | **Not mentioned anywhere in `PLAN.md` or `README.md`** | `X-L5-3`, `X-L5-4` |
| System settings honoured (contrast, reduced motion, text scale, colour scheme) | **Partial** | Reduced motion and appearance appear per backend; no portable contract or conformance | `D-FP-1`, `M-FP-1` |

## L6 — Application services

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Service/resource system with typed contracts | **Met** | Milestone 20 | — |
| Capability model | **Met** | Milestone 22; 2.5 | — |
| Permission states as capability answers | **Partial** | Mentioned for Android; no portable enum | `M-OB-1` |
| Navigation and routing | **Met** (Windows) | Milestone 30 | — |
| Persistence and restoration | **Met** (Windows) | Milestone 30 | — |
| State-schema migration | **Absent** | Not specified | `X-DATA-3` (see gap plan) |
| **Async data layer (cache, dedupe, invalidate, paginate, optimistic)** | **Absent** | Resources exist; caching and invalidation do not | `X-DATA-1` |
| Offline mutation queueing | **Absent** | — | `X-DATA-2` |
| **Forms and validation** | **Absent** | Text input exists; no forms model | `W-SF-7` |
| Background/scheduled work with host constraints | **Absent** | — | `M-FP-2` |
| Image/asset loading with caching | **Absent** | — | `M-FP-3` |
| Component library | **Absent** | Primitives only | `X-UI-1` |
| Server application model (auth, data, jobs, admin) | **Absent** | Web track covers rendering, not the backend | `W-SF-1`…`W-SF-6` |

## L7 — Engineering loop

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Project scaffolding and CLI | **Met** (Windows) | Milestone 31 | — |
| **State-preserving reload / fast iteration** | **Absent** | Only a browser hot-reload bullet under Web milestone J | `X-L7-1`, `X-L7-2` |
| **Runtime inspection protocol and inspector** | **Planned** (vaguely) | Section 9 "tooling and diagnostics — eventually add…" | `X-L7-3`, `X-L7-4`, `D-IM-1` |
| Headless reference backend for tests | **Absent** | Platform integration tests need real hosts | `X-L7-5` |
| Synthetic input through the real input path | **Absent** | — | `X-L7-7` |
| Core/reconciliation/layout/scheduler unit tests | **Met** | Section 9; `BUILD_STATUS.md` | — |
| Golden/visual regression tests | **Absent** | — | `X-L7-8` (see gap plan) |
| Device/emulator matrix in CI | **Absent** | — | `M-OB-4` |

## L8 — Ship and operate

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Packaging and signing | **Met** (Windows) / **Planned** (rest) | Milestone 32; section 8 per backend | — |
| Deployment shapes (static / server / serverless / edge / container / firmware) | **Planned** | Web milestones H, K; Milestone 37 | `W-MF-6`, `W-SH-1`, `E-BL-1` |
| Deployment tooling (`deploy`, previews, rollback) | **Absent** | — | `W-DP-1`, `W-DP-2` |
| **Updates after ship (OTA, delta, firmware A/B)** | **Absent** | — | `M-BR-1`, `E-MW-1` |
| Performance budgets enforced in CI | **Absent** | Section 9 lists metrics to "measure and eventually optimize" | `X-L0-1` |
| Security threat model and supply-chain gates | **Partial** | `deny.toml`, `SECURITY.md`, `clippy.toml` exist; no stated threat model | `D-WS-2`, `E-SF-1` |
| Compliance evidence generated (SBOM, licences, accessibility, privacy) | **Absent** | — | `W-EP-3`, `E-BL-2` |
| Crash capture with symbolication, structured logs, metrics, tracing | **Absent** | — | `W-EP-2`, `X-OBS-1` (see gap plan) |

## L9 — The project

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Architecture documentation | **Met** | `PLAN.md`, `README.md`, `BUILD_STATUS.md`, `Audit.md` | — |
| Task-oriented guides and generated API reference | **Partial** | `missing_docs` enforced; no guide set or published reference | `X-DOC-1` |
| Runnable example per subsystem | **Partial** | `examples/` exists | `X-DOC-2` |
| Stability, deprecation, and migration policy | **Absent** | — | `W-MF-8` |
| Incremental adoption inside existing applications | **Absent** | — | `X-INTEROP-1` |
| Third-party extension ecosystem | **Absent** | — | `X-ECO-1` |
| Machine-readable framework description for code generation | **Absent** | — | `X-ECO-2` |
| Layered configuration and operational surface | **Absent** | — | `W-EP-1`, `W-EP-2` |

---

## Reading the matrix

**Where we are genuinely strong.** L0 through L4 on the core contracts, and L2
through L6 on Windows. The root choices (`foundations.md` F0.4, F1.4, F2.1,
F3.3) are made, implemented, and verified on one host. That is a real
foundation, and it is the expensive half.

**Where the plan is complete but unbuilt.** Every other backend. That is a
scheduling and hardware fact (2.13), not a gap in this analysis's sense, and it
is not what this document is for.

**Where the plan itself is missing something.** Nine items scored **Absent**
that are not backend work and would be missing even if every backend shipped
tomorrow:

1. Localization and internationalization (`X-L5-3`) — the largest single
   omission; no commercial application ships without it.
2. The async data layer (`X-DATA-1`) — the most-used third-party layer in every
   competing ecosystem.
3. The engineering loop: fast iteration and runtime inspection (`X-L7-1`,
   `X-L7-3`) — our substrate's structural disadvantage, currently answered by a
   bullet list under "eventually".
4. Forms and validation (`W-SF-7`).
5. Shared/scoped state (`X-L4-1`) and error boundaries (`X-L4-3`).
6. Updates after ship — mobile OTA and firmware A/B (`M-BR-1`, `E-MW-1`).
7. Performance budgets enforced in CI (`X-L0-1`) — without which every
   performance claim we make is an assertion.
8. The server application model (`W-SF-*`) — the largest uncontested market
   opening identified anywhere in this analysis.
9. Interoperability and incremental adoption (`X-INTEROP-1`) — the only
   strategy that has ever worked against an accumulated ecosystem.

Those nine, plus the conformance suites that turn our root-layer advantages
from claims into tested guarantees, are what [`gap-plan.md`](gap-plan.md)
schedules.
