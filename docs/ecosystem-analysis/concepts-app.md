# Concepts II — UI system, data and sync, server and API, distributed execution, platform surfaces (L3, L5, L6)

Continues [`concepts-core.md`](concepts-core.md), which explains the format,
the inclusion rule, and the scoring. Requirement identifiers are `Cnn-k`.

---

# Part A — The UI system

## C22 — Adaptive layout: size classes, breakpoints, container queries, posture

**Introduced by.** M1, M2, D1, W14's styling systems, and the web platform's
own container queries.

**Mechanism.** Layout decisions keyed to *available space* rather than to
device type: coarse size classes (compact, medium, expanded) per axis, named
breakpoints, and container queries that let a component adapt to the space its
*parent* gives it rather than to the window. Foldable and dual-screen devices
add *posture* (flat, half-open, hinge position). Navigation patterns switch
with the class — bottom bar, rail, sidebar.

**Strengths.** One layout for phone, tablet, desktop window, and split-screen;
components reusable in any container; resilient to window resizing, which is
now common on every host.

**Weaknesses.** Breakpoint sprawl; testing every class; designers and
engineers disagreeing on the boundaries.

**Opportunities.** The layout model is platform-independent (2.11), so size
classes and container-relative decisions can be computed once, in the core,
from the constraints the layout pass already has — container queries are
essentially free in a constraint-based model. Posture and hinge geometry are
host traits and belong in the environment (`C15`).

**Threats.** Mobile hosts now require resizable windows on large screens and
desktop-class devices; applications that assume a phone-sized fixed canvas are
flagged by store quality programs.

**Position.** **Absent** as a portable vocabulary.

**Requirements.**

- `C22-1` `[X]` Size classes per axis and container-relative layout decisions in
  the portable layout model, available to any component from its own
  constraints.
- `C22-2` `[M]` `[D]` Posture and hinge geometry as environment values, with
  layout able to avoid the hinge.
- `C22-3` `[X]` An adaptive navigation component that switches between bottom
  bar, rail, and sidebar by size class, host-conventionally per backend.

## C23 — Platform-adaptive components

**Introduced by.** M5, D3, M4's shared-UI variant, D1's design languages.

**Mechanism.** One component declaration that renders the host's own idiom:
a switch that is a toggle on one host and a checkbox on another; a date picker
that is a wheel, a calendar, or a text field by host; alert dialogs with button
order per host convention; navigation with back-button placement per host.

**Strengths.** Portable code with host-appropriate results; removes per-host
conditionals from applications.

**Weaknesses.** In frameworks that *draw* their controls, adaptivity is an
imitation that drifts from the host; genuine behavioural differences leak.

**Opportunities.** This is simply what host-native realization does (2.2) — for
free, and correctly, because the host draws its own idiom. The concept to
extract is the *documented per-host mapping* for each portable control and the
places where host idioms genuinely differ in behaviour (button order, dialog
dismissal, destructive-action placement), so applications do not second-guess
it.

**Threats.** Low; this is an advantage to state rather than a gap to close.

**Position.** **Partial** — true by construction on Windows; per-host mapping
not documented.

**Requirements.**

- `C23-1` `[X]` A per-control, per-backend idiom table in the component library
  documentation, including behavioural differences (ordering, dismissal,
  confirmation), maintained alongside the fidelity conformance suite.

## C24 — Per-property native mapping customization

**Introduced by.** D3 (its newest generation's handler architecture).

**Mechanism.** Each cross-platform control is connected to its host control by
a *handler*, and each property is applied by an entry in a *mapper* — a table
from property name to a function that sets it on the host object. Applications
may append, prepend, or replace mapper entries globally or per instance, so a
single host-specific tweak (disable autocorrect underline, change a native
drawable) does not require subclassing or forking the control.

**Strengths.** Targeted, low-cost access to host-specific behaviour; the
escape hatch at property granularity rather than control granularity; a
clean structure for backend authors.

**Weaknesses.** Global mapper changes are action at a distance; ordering of
multiple customizations is subtle.

**Opportunities.** This is the right *shape* for our escape hatch (2.6,
`X-L0-6`) at the granularity applications actually need, and it is Tier 0
because it dictates how every backend structures the code that applies
properties to host objects. With typed property keys it is also safe: a mapper
entry for a host type can only be written against that host's control type.

**Threats.** Without it, the only escape hatch is "take the whole host object
and manage it yourself", which is too coarse and pushes teams to fork.

**Position.** **Absent.** The Windows backend applies properties directly.

**Requirements.**

- `C24-1` `[X]` Every backend applies properties through a per-control mapper
  of typed property appliers.
- `C24-2` `[X]` Applications may add or replace appliers per backend, globally
  or per instance, with ordering defined and the customization visible in the
  inspector.

## C25 — Shared-element and view transitions

**Introduced by.** M1, M2, D1, and the web platform's view-transition
mechanism adopted by W3.

**Mechanism.** When navigation or a state change moves a logical element from
one place to another — a thumbnail becoming a header image — the element is
matched by identity across the two states and animated continuously between
its old and new geometry, while the rest of the scene cross-fades. The web
platform version snapshots old and new states and animates between them with
style rules.

**Strengths.** Spatial continuity that users read as quality; cheap once
identity exists; now a host-level feature on several platforms.

**Weaknesses.** Needs stable identity across two different trees; interacts
with clipping and z-order; easy to overuse.

**Opportunities.** Stable identity across renders is foundational here (2.7),
and animations run through a portable timeline (Milestone 27). A matched
geometry transition is "same identity, different layout result, interpolate" —
a direct application of mechanisms that already exist, and a place where
host-native transitions can be used on hosts that provide them.

**Threats.** Medium: it is now expected in content-heavy mobile applications.

**Position.** **Absent** — Milestone 27 animates properties of one node; not
matched geometry across navigation.

**Requirements.**

- `C25-1` `[X]` Matched-geometry transitions keyed by a declared shared
  identity across navigation and state changes, using the host's own
  transition mechanism where one exists, and respecting reduced motion.

## C26 — Model/view, proxy models, and identity-based list snapshots

**Introduced by.** D5 (item models with roles and chainable sort/filter
proxies), M2 (snapshot-based list data sources with identity diffing,
compositional list layouts), D1 (virtualizing item controls).

**Mechanism.** Lists and tables are driven by a *model* that exposes items with
stable identity; *proxy models* sort, filter, and group without copying; the
view applies *snapshot diffs* computed from identities, so animations of
inserts, deletes, and moves are correct by construction. Layout is
*compositional*: sections with their own layout (grid, list, carousel),
headers, and supplementary views.

**Strengths.** Large data sets without copying; correct, animated updates;
complex screens from one scrolling surface; separation of data order from
presentation.

**Weaknesses.** API complexity; performance traps when identities are unstable.

**Opportunities.** Milestone 28 already virtualizes lists with stable keys. The
concepts to add are proxy transforms (sort, filter, group as views over a
source, not copies), identity-diffed animated updates, and sectioned
compositional layout — all portable, and all consumers of existing identity.

**Threats.** Tables and grids are where business applications live; weak list
infrastructure disqualifies a framework for line-of-business work quickly.

**Position.** **Partial** — virtualization and keys exist; proxies, animated
diffs, and compositional sections do not.

**Requirements.**

- `C26-1` `[X]` Sort, filter, and group as non-copying views over a list
  source, composable, driving virtualized lists.
- `C26-2` `[X]` Identity-diffed animated insert, delete, and move in
  virtualized lists.
- `C26-3` `[X]` Sectioned, compositional list layout — per-section list, grid,
  or carousel, with headers — on the virtualization infrastructure.

## C27 — Document-based application architecture

**Introduced by.** D1 (the macOS document architecture in particular), D5,
and productivity applications generally.

**Mechanism.** The framework models the *document*: open, save, save as,
revert, autosave, version browsing, recently opened files, dirty state
reflected in the window, an undo manager per document with grouped and named
actions, file coordination so external changes are detected, and a
document-per-window lifecycle.

**Strengths.** Correct, host-conventional document behaviour without writing
it; users' work is protected by autosave and versions; undo integrated with
menus and commands.

**Weaknesses.** Opinionated; awkward for applications that are not
document-shaped.

**Opportunities.** Multi-window (Milestone 24), dialogs (Milestone 23),
persistence (Milestone 30), commands (`C20`), and undo history (`X-L4-2`)
together are most of a document architecture already; composing them into an
explicit document model makes an entire application category first-class.

**Threats.** Desktop productivity is a category where native frameworks are
still preferred, and a framework without a document story forfeits it.

**Position.** **Absent.**

**Requirements.**

- `C27-1` `[D]` A document model: open, save, save as, revert, autosave, dirty
  state, recent documents, per-document undo integrated with the command model,
  external-change detection, and one-window-per-document lifecycle, mapped to
  each host's conventions.

## C28 — Host content controls

**Introduced by.** Every native archetype (D1, M1, M2), and the embedding
parts of D7b and M5.

**Mechanism.** Some content is only rendered well by the host: embedded web
content, audio and video playback with system controls and picture-in-picture,
maps, camera preview, document and PDF viewers, rich text editing. Frameworks
expose these as controls that participate in layout and lifecycle.

**Strengths.** Hardware decoding, DRM, system media controls, and accessibility
come from the host.

**Weaknesses.** Each is a large per-host integration with its own lifecycle and
threading rules.

**Opportunities.** Host-native realization makes these ordinary nodes rather
than composited foreign surfaces, which is exactly where self-drawing
archetypes struggle (they must punch holes in their own rendering). They are
also the most common reason an application reaches for the escape hatch, so
first-party versions of the most-used ones remove the most escape-hatch use.

**Threats.** An application that needs video or web content and cannot get it
does not adopt the framework, however good the rest is.

**Position.** **Absent.**

**Requirements.**

- `C28-1` `[X]` Host content controls for embedded web content, media playback
  (with system media controls and picture-in-picture where the host has them),
  and camera preview, as capability-guarded nodes.
- `C28-2` `[X]` A documented pattern — built on `D-GX-1` and `X-INTEROP-1` — for
  adding further host content controls outside the framework.

## C29 — Data-state contracts and colocated data requirements

**Introduced by.** W3 and a full-stack archetype's data "cells", and the
client-specified query lineage in W14.

**Mechanism.** Two ideas. First, a component that loads data declares its
*states* as a contract — loading, empty, failure, success — and must render
each; the framework drives the transitions. Second, a component declares *the
data it needs* next to its code (a fragment of a query); parents compose their
children's requirements into one request, and each component can see only the
data it asked for (*data masking*), so changing a child's needs never breaks a
sibling.

**Strengths.** The empty and failure states stop being forgotten; one request
per screen instead of waterfalls; components remain independently changeable.

**Weaknesses.** Tied to a query language or code generator in most
implementations; masking adds indirection.

**Opportunities.** Rust enums make the four-state contract a type the component
must match exhaustively — forgetting the empty state becomes a compile error.
Colocated requirements can be composed at compile time from typed query
fragments without a separate code generator.

**Threats.** Waterfall loading and missing empty states are among the most
common quality defects in data-driven applications.

**Position.** **Absent.**

**Requirements.**

- `C29-1` `[X]` The data layer (`X-DATA-1`) exposes query results as an
  exhaustive state type — loading, empty, failure, success, plus
  stale-while-refreshing — that components must match.
- `C29-2` `[X]` Colocated, composable data requirements: a component declares
  what it needs, an ancestor batches descendants' needs into one request, and
  each component receives only its own slice.

---

# Part B — Data, synchronization, and real time

## C30 — Stale-while-revalidate, specified

**Introduced by.** W14's asynchronous data caches, M1.

**Mechanism.** The details that distinguish a real data cache from a memoized
fetch: separate *freshness* and *retention* lifetimes (data may be stale yet
still kept); revalidation triggered by window focus, network reconnection,
mount, and interval; *structural sharing* (unchanged parts of new results keep
their identity, so dependent components do not re-render); query keys with
hierarchical invalidation; prefetching; request cancellation when the last
observer leaves; and garbage collection of unobserved entries.

**Strengths.** Correct freshness with minimal network; stable identities
minimize re-renders; prefetch and cancellation make navigation feel instant.

**Weaknesses.** Many knobs; developers misconfigure the two lifetimes.

**Opportunities.** Structural sharing and identity are exactly what the
reconciler already optimizes for; revalidation triggers map onto visibility-
and lifecycle-aware work (`C02`) and onto network capability changes; request
cancellation maps onto scope cancellation.

**Threats.** `X-DATA-1` without these details is a cache that users will
replace with their own.

**Position.** **Absent** (the data layer itself is absent; see `X-DATA-1`).

**Requirements.**

- `C30-1` `[X]` Separate freshness and retention lifetimes; revalidation on
  focus, reconnect, mount, and interval as declared policy; hierarchical key
  invalidation; prefetch; cancellation when unobserved; retention-based
  collection.
- `C30-2` `[X]` Structural sharing of query results so unchanged parts keep
  identity and do not invalidate dependents.

## C31 — Live queries and the database as the source of truth

**Introduced by.** M1 (persistence libraries whose queries return observable
streams, and paging that fills a local database from the network), D1's object
graph persistence with fetched-results controllers.

**Mechanism.** The local database is the single source of truth for the UI.
Queries are *live*: they emit a new result whenever the underlying tables
change, and lists observe them directly. The network never updates the UI; it
updates the database, and the UI follows (a *repository* or *remote mediator*
pattern). Paging reads pages from the database and asks the network to fill
gaps.

**Strengths.** Offline behaviour by construction; one path for UI updates;
consistency between screens; paging that survives process death.

**Weaknesses.** A local database becomes mandatory; schema migrations matter
more; write amplification.

**Opportunities.** Live queries are a stream source bound to component scope
(`C12`); a live query driving a virtualized list with identity diffs (`C26`) is
the full pattern, built from pieces this plan already contains.

**Threats.** Offline-capable mobile applications are now expected in many
categories, and this is how the native archetypes deliver them.

**Position.** **Absent.**

**Requirements.**

- `C31-1` `[X]` A live-query contract for local storage services: a query
  result stream that re-emits on relevant changes, collectable by components,
  and usable as a virtualized list source.
- `C31-2` `[X]` A documented repository pattern — network writes to local
  storage, UI reads from local storage — with a paging source that fills gaps
  from the network.

## C32 — Local-first applications and sync engines

**Introduced by.** Backend-as-a-service platforms (W15) with offline
persistence and sync; D1's cloud-synced persistence; a growing generation of
sync-engine libraries.

**Mechanism.** The application's data lives on the device first. Reads and
writes are local and instant; a sync engine replicates changes to a server and
to other devices in the background; conflicts are resolved by policy —
last-writer-wins, server authority, or conflict-free replicated data types
(CRDTs) that merge concurrent edits deterministically. The server can push
changes, and partial replication limits what each device holds.

**Strengths.** Instant UI, offline by default, real-time collaboration, fewer
loading states, resilience to flaky networks.

**Weaknesses.** Conflict semantics are hard to explain; authorization on
replicated data is hard; schema evolution across devices at different versions
is hard; storage growth.

**Opportunities.** This is reconciliation — the framework's core competency —
applied to data instead of to UI: a desired state and an observed state
brought into agreement. The same machinery serves device fleets (`C84`).
Rust's type system makes CRDT merge functions checkable and pure. And a
compiled core means the same sync engine runs on the client, the server, and
an embedded device.

**Threats.** Collaborative and offline-first applications increasingly choose
their framework *because of* the sync layer.

**Position.** **Absent.** `X-DATA-2` covers offline mutation queueing only.

**Requirements.**

- `C32-1` `[X]` A sync service contract: local-first reads and writes,
  background replication, server push, partial replication, and a declared
  conflict policy per collection (server authority, last-writer-wins, or
  merge function), with at least one adapter.
- `C32-2` `[X]` CRDT-backed collaborative types (text, list, map, counter) as an
  optional layer, with merge functions tested for commutativity,
  associativity, and idempotence.
- `C32-3` `[X]` Schema versioning for replicated data across clients at
  different application versions.

## C33 — Persistent-connection server-driven UI, channels, and presence

**Introduced by.** The persistent-connection server archetype (W16), and W9's
server-interactive component model.

**Mechanism.** The UI's state lives on the server in a long-lived process per
connected client. Events travel from the browser to the server over a
persistent connection; the server re-renders and sends minimal diffs back.
Around it: publish/subscribe channels for broadcasting, *presence* (who is
connected, replicated with a conflict-free structure), and automatic
reconnection with state recovery.

**Strengths.** Rich interactivity with almost no client code; no API layer; the
server is the single source of truth; real-time collaboration and live
dashboards are trivial.

**Weaknesses.** Latency on every interaction (optimistic client hooks mitigate
it); server memory per connected client; reconnection and deploys must restore
state; offline is impossible.

**Opportunities.** The server-driven mode (`W-HM-1`) already applies typed
fragments to the realized tree. A persistent-connection variant is the same
mechanism with a different transport, and because the diff is produced by the
same reconciler that runs on the client, the client patch is ordinary
reconciliation rather than a bespoke DOM patcher. Per-client state is a
component tree with scope-bound tasks — the lifetime model already exists.

**Threats.** Teams using this archetype are some of the most productive in the
industry for internal and real-time applications, and it is exactly the kind
of application a batteries-included server model (`W-SF-*`) would otherwise
win.

**Position.** **Absent.**

**Requirements.**

- `C33-1` `[W]` A server-interactive mode: per-connection component trees on
  the server, events over a persistent connection, reconciler-produced diffs
  applied on the client, reconnection with state recovery, and deployment
  draining.
- `C33-2` `[X]` Publish/subscribe channels and presence as service contracts,
  usable by server-interactive and client-side applications alike.
- `C33-3` `[W]` Optimistic client-side hooks for server-interactive components
  so latency-sensitive interactions do not wait for the round trip.

## C34 — Declarative HTTP clients

**Introduced by.** M1 and M2's networking libraries, W14.

**Mechanism.** An API is declared as an interface — methods annotated with
paths, verbs, headers, and body types — and the client is generated. Around it,
an *interceptor chain* handles authentication, token refresh, logging, retries,
caching, and request adaptation; connection pooling, HTTP/2 multiplexing, and
response caching are transparent; certificate pinning restricts trusted
certificates for sensitive applications.

**Strengths.** Typed calls instead of string building; cross-cutting concerns in
one place; security features (pinning) available without expertise.

**Weaknesses.** Generated clients hide what is sent; interceptor ordering bugs.

**Opportunities.** The HTTP service (Milestone 20) is typed already; an
interceptor chain and pinning are straightforward, and typed server functions
(`W-MF-1`) plus contract generation (`C35`) produce clients with no annotation
step at all for the application's own server.

**Threats.** Teams will bring their favourite HTTP stack and bypass the service
contract, losing testability and capability reporting.

**Position.** **Partial** — typed HTTP contract exists; interceptors, pinning,
and declared endpoints do not.

**Requirements.**

- `C34-1` `[X]` An interceptor chain on the HTTP service (authentication and
  token refresh, retry, logging, caching), ordered and testable.
- `C34-2` `[X]` Certificate pinning as a declared policy, with a rotation story.
- `C34-3` `[X]` Declared endpoint interfaces producing typed clients.

## C35 — API contracts generated from types

**Introduced by.** W8's typed-API members, W9, and the typed-RPC and
client-specified-query lineages in W14.

**Mechanism.** Handler signatures and data types *are* the API definition. The
framework derives a machine-readable schema from them, publishes interactive
documentation, validates requests against it, generates clients in other
languages, and runs contract tests between versions. Typed-RPC variants skip the
schema for same-language clients by sharing the type directly.
Client-specified query languages let the client request exactly the fields it
needs, with the schema as the contract.

**Strengths.** Documentation that cannot drift; free validation; generated
clients for every consumer; breaking changes detected before deploy.

**Weaknesses.** Schema expressiveness limits the types used; generated-client
quality varies.

**Opportunities.** Rust types carry more than these schemas can express, so
derivation is lossless in the direction that matters. Typed server functions
already share types in-language (`W-MF-1`); generating a schema from the same
definitions serves every *other* consumer — mobile clients on other stacks,
partners, and test tools — at no extra cost.

**Threats.** A server model without a published API contract is rejected by
teams whose server has more than one client.

**Position.** **Absent.**

**Requirements.**

- `C35-1` `[W]` A machine-readable API schema derived from server handler and
  server-function types, with generated documentation, request validation, and
  generated clients for at least one other language.
- `C35-2` `[W]` Contract tests that fail the build when a change breaks a
  published version of the API.

## C36 — Validation pipelines and changesets

**Introduced by.** W16's runtime lineage, W7.

**Mechanism.** A *changeset* is the unit between raw input and stored data: it
casts raw parameters into typed fields, runs validations, collects errors per
field, and — crucially — maps *storage* constraint violations (a unique index,
a foreign key) back onto the field that caused them. Forms render the
changeset, so validation messages from client, server, and database all arrive
in one place.

**Strengths.** One pipeline for all validation sources; database constraints
become user-facing messages instead of 500 errors; forms and storage share one
description.

**Weaknesses.** A second model alongside the entity; ceremony for trivial
forms.

**Opportunities.** The shared forms model (`W-SF-7`) needs exactly this shape,
and typestate (`C21`) turns "validated" into a type that storage accepts and
raw input does not.

**Threats.** Without it, constraint violations surface as generic errors, and
validation logic is duplicated three times.

**Position.** **Absent.**

**Requirements.**

- `C36-1` `[X]` A changeset type in the forms model: typed casting, per-field
  errors, storage-constraint errors mapped to fields, and a validated output
  type distinct from raw input.

---

# Part C — Server and backend

## C37 — Middleware pipelines and typed extractors

**Introduced by.** W7, W8, W9, and the Rust members of W8.

**Mechanism.** Request handling as a pipeline of composable layers — the
*onion* model, where each layer can act before and after the inner ones — with
the same abstraction for timeouts, compression, authentication, tracing, and
rate limiting. Handlers declare what they need as typed parameters
(*extractors*): a path parameter, a JSON body, the authenticated user, a
database connection; the framework extracts and validates them or rejects the
request before the handler runs.

**Strengths.** Cross-cutting concerns written once; handlers that document
their requirements in their signatures; rejection before business logic runs.

**Weaknesses.** Deep type errors when extractor composition fails; middleware
ordering subtleties.

**Opportunities.** The compiled-language ecosystem already has a mature,
composable service abstraction and extractor pattern. The server model
(`W-SF-1`) should adopt the ecosystem's abstraction rather than invent one,
which also delivers mountability inside existing services (`W-MS-1`) for free.

**Threats.** Inventing a parallel abstraction would split the ecosystem this
project most needs to join.

**Position.** **Absent** (no server model yet).

**Requirements.**

- `C37-1` `[W]` The server model is built on the established service and
  middleware abstraction of the Rust server ecosystem, with typed extractors
  for framework values (session, authenticated principal, request-scoped
  services), rather than a framework-specific pipeline.

## C38 — Migrations generated from the model

**Introduced by.** W7.

**Mechanism.** The developer edits the model definitions; the framework diffs
them against the recorded schema history and *generates* a migration, which is
reviewed, committed, and applied in order. Data migrations can be attached.
Squashing collapses long histories.

**Strengths.** Schema and code cannot drift; migrations are reviewable
artifacts; no hand-written DDL for common changes.

**Weaknesses.** Generated migrations for renames and complex changes need
human judgement; histories grow long.

**Opportunities.** Compile-time-checked queries (`W-SF-3`) plus generated
migrations close the loop: the model, the migration, and every query agree, and
disagreement is a build failure.

**Threats.** Teams from this archetype consider hand-written migrations a
regression.

**Position.** **Absent.**

**Requirements.**

- `C38-1` `[W]` Migration generation from model changes, with rename detection
  prompts, attached data migrations, a dry run, and squashing — also applied to
  persisted client state (`X-DATA-3`).

## C39 — Backend-as-a-service primitives

**Introduced by.** W15.

**Mechanism.** The backend is provided rather than written: a hosted database
with an API *generated from its schema*; authorization expressed as
*row-level policies* evaluated by the database for every query; realtime
subscriptions to table changes; managed authentication, file storage, and
functions — all reachable directly from the client with the policies as the
security boundary.

**Strengths.** Days instead of months to a working product; security enforced
where the data lives rather than in every endpoint; realtime for free.

**Weaknesses.** Policy logic in the database is hard to test and review;
vendor coupling; complex business logic eventually needs a real server anyway.

**Opportunities.** The concept to extract is *policy at the data layer*:
authorization rules declared once, next to the schema, enforced for every query
and every subscription regardless of which endpoint issued it — expressible as
typed policies in the server model (`W-SF-2`). A client-side adapter for
existing hosted backends also lets applications adopt RustNative's UI without
changing their backend.

**Threats.** For small teams, this archetype plus any UI layer beats writing a
backend, and "no backend needed" is a stronger pitch than "a better backend".

**Position.** **Absent.**

**Requirements.**

- `C39-1` `[W]` Declarative data-layer authorization policies in the server
  model, enforced for queries and subscriptions alike, with a test harness that
  evaluates a policy against fixtures.
- `C39-2` `[X]` Client-side service adapters for at least one hosted backend
  (data, auth, storage, realtime) behind the portable contracts.

## C40 — Convention-based auto-configuration

**Introduced by.** W9, W7.

**Mechanism.** Adding a dependency enables a capability with sensible defaults:
the presence of a database driver configures a connection pool, health check,
and metrics; *starter* bundles group compatible dependencies. Every default is
overridable, and a report explains what was configured and why.

**Strengths.** Near-zero configuration for common setups; consistent
operational behaviour across services.

**Weaknesses.** Magic; debugging why something was configured requires the
report.

**Opportunities.** Rust features and build-time resolution allow the same
convenience without runtime scanning, and the explanation report can be
emitted at build time by `rustnative`.

**Threats.** Low.

**Position.** **Absent.**

**Requirements.**

- `C40-1` `[W]` Feature-driven defaults for server services (pool, health,
  metrics, tracing) with a build-time report of what was configured and why.

## C41 — Web metadata and discoverability

**Introduced by.** W3, W4.

**Mechanism.** Per-route head management (title, description, canonical URL,
language alternates), social preview cards, structured data, generated
sitemaps and robots rules, and locale-prefixed routing — all declared in the
route definition, rendered server-side, and updated on client navigation.

**Strengths.** Search and social visibility without hand-written head
manipulation; correct alternates for localized sites.

**Weaknesses.** Easy to get subtly wrong (duplicate canonicals, missing
alternates) without validation.

**Opportunities.** Route definitions are typed (Milestone 30), so metadata can be
typed too and validated at build time; localized routing joins the
localization milestone.

**Threats.** A web framework that cannot be indexed properly is not used for
anything public.

**Position.** **Absent.**

**Requirements.**

- `C41-1` `[W]` Typed per-route metadata — head, social cards, structured data —
  rendered server-side and updated client-side, with generated sitemaps and
  build-time validation.
- `C41-2` `[W]` Locale-aware routing with generated language alternates.

## C42 — Resource loading optimization and user-centric performance metrics

**Introduced by.** W3, W4.

**Mechanism.** The framework owns the loading path: images are resized,
converted to modern formats, served responsively, lazy-loaded, and the most
important one prioritized; fonts are subset, preloaded, and given a fallback
whose metrics are adjusted to match, so text does not shift when the real font
arrives; links prefetch their route on hover or on entering the viewport.
Success is measured by user-centric metrics: time to the largest content,
responsiveness of interactions, and cumulative layout shift.

**Strengths.** Good defaults deliver good user-centric metrics without
expertise; search ranking and conversion follow.

**Weaknesses.** Build-time asset processing adds build cost; prefetch wastes
data if too eager.

**Opportunities.** Intrinsic measurement is a core concept here (Milestones 8,
9); declaring intrinsic dimensions for images and reserving space is the same
mechanism and removes layout shift structurally. Prefetch on intent maps onto
the router and the data layer's prefetch (`C30`).

**Threats.** Poor user-centric metrics are visible to anyone with a browser's
developer tools, and they are how web frameworks are compared in public.

**Position.** **Absent** as a named concern.

**Requirements.**

- `C42-1` `[W]` An image pipeline: resizing, modern formats, responsive
  sources, lazy loading, priority hints, and reserved intrinsic dimensions.
- `C42-2` `[W]` Font optimization: subsetting, preload, and metric-adjusted
  fallbacks.
- `C42-3` `[W]` Route and data prefetch on hover and on viewport entry, bounded
  by a data-use policy.
- `C42-4` `[W]` User-centric metrics — largest content paint, interaction
  responsiveness, layout shift — as budgets in Milestone 42, with a zero
  layout-shift target for framework-controlled content.

## C43 — Micro-frontends and framework-agnostic components

**Introduced by.** W14's module-federation tooling, and the web platform's
custom elements and shadow DOM (used by the web-component library lineage).

**Mechanism.** Large organizations split one web application into
independently deployed parts composed at runtime, sharing dependencies.
Framework-agnostic *custom elements* let a component written with one framework
be used in any page or any other framework.

**Strengths.** Team autonomy; incremental migration between frameworks;
reusable design-system components across stacks.

**Weaknesses.** Duplicate runtimes, version skew, inconsistent UX, and
operational complexity; shadow DOM complicates styling and accessibility
relationships.

**Opportunities.** Exporting a RustNative component as a custom element is the
web half of incremental adoption (`X-INTEROP-1`): it lets a RustNative
component live inside any existing web application without that application
adopting anything else. Runtime composition of separately built RustNative
modules is lower priority.

**Threats.** Without a custom-element export, web adoption requires a
whole-page decision.

**Position.** **Absent.**

**Requirements.**

- `C43-1` `[W]` Export of a RustNative component as a custom element, with
  typed attributes and properties, events surfaced as DOM events, and
  accessibility relationships preserved.

---

# Part D — Serverless and distributed execution

## C44 — Event-driven triggers beyond HTTP

**Introduced by.** W10, W13.

**Mechanism.** Functions are invoked by events, not only requests: queue
messages, stream records, storage object changes, database change feeds,
schedules, and platform events — with a standard event envelope so handlers
and routing are portable, batching, partial-batch failure reporting, retries,
and dead-letter destinations.

**Strengths.** Decoupled, scalable pipelines; background work without servers;
a portable event format.

**Weaknesses.** Distributed debugging; at-least-once delivery demands
idempotency; ordering guarantees differ per source.

**Opportunities.** Web milestone K is entirely request-shaped. The same
per-invocation discipline (stateless, scope bounded by the invocation,
configuration from the environment) applies unchanged to events; what is
missing is the entry point, the envelope, and idempotency support.

**Threats.** Most serverless workloads are not HTTP; a serverless mode that
only serves pages is a small fraction of the category.

**Position.** **Absent.**

**Requirements.**

- `C44-1` `[W]` Event handlers as a serverless entry point, using a standard
  event envelope, with batching, partial-failure reporting, retries, and
  dead-letter routing — and the same invocation-bounded task scope as requests.
- `C44-2` `[W]` Idempotency keys and deduplication helpers for at-least-once
  delivery.

## C45 — Durable execution and workflows

**Introduced by.** W10's orchestration services, W12's application frameworks,
and dedicated durable-execution engines.

**Mechanism.** A long-running process — minutes to months — is written as
ordinary sequential code. The engine records each step's result in a durable
log; on failure or restart, the function is *replayed* from the log, skipping
completed steps, so execution resumes exactly where it stopped. Timers, waits
for external signals, human approvals, compensation for sagas, and versioning
of in-flight workflows are part of the model.

**Strengths.** Reliability for business processes without hand-written state
machines, queues, and retry tables; readable code for complex flows.

**Weaknesses.** Determinism constraints on workflow code (no clocks, no random,
no direct I/O outside steps); versioning in-flight executions is hard; the
engine is infrastructure.

**Opportunities.** Rust async functions are state machines already, and
structured scopes already model step lifetime. Determinism can be *enforced*:
workflow code receives a context that provides the only clock, randomness, and
I/O, so non-deterministic calls do not compile — a guarantee the dynamic
substrates can only lint for.

**Threats.** Business applications (onboarding, payments, provisioning) need
this; without it, the server model covers only request/response work.

**Position.** **Absent.**

**Requirements.**

- `C45-1` `[W]` A durable workflow contract — steps with recorded results,
  replay on restart, durable timers, external signals, compensation — with
  determinism enforced by the workflow context type, and at least one engine
  adapter.
- `C45-2` `[W]` Workflow versioning rules for in-flight executions, with tests.

## C46 — Single-instance stateful actors at the edge

**Introduced by.** W11.

**Mechanism.** An addressable object with an identity; exactly one instance
exists globally at a time, placed near its users; it has private durable
storage and processes requests one at a time. It is used for coordination —
chat rooms, collaborative documents, rate limiters, game sessions — without a
database round trip for every operation.

**Strengths.** Strong consistency per object without distributed locking; low
latency coordination; a natural home for real-time sessions.

**Weaknesses.** Platform-specific; single-object throughput limits; migration
of stored state.

**Opportunities.** Actor-shaped components with scope-bound tasks (`C17`) are
the natural programming model; the server-interactive mode (`C33`) and sync
engines (`C32`) need exactly this coordination point.

**Threats.** Real-time collaborative features increasingly assume it exists.

**Position.** **Absent.**

**Requirements.**

- `C46-1` `[W]` A stateful-actor service contract (identity, single-instance
  serialized execution, private durable storage, alarms) with an edge adapter
  and a single-process local implementation for development.

## C47 — Cold-start mitigation, revisions, and traffic splitting

**Introduced by.** W10, W13.

**Mechanism.** Startup is reduced by *snapshotting* an initialized process and
restoring it instead of booting, or by keeping instances provisioned. Each
deployment creates an immutable *revision*; traffic is split between revisions
by percentage for canary releases and instant rollback; unused revisions scale
to zero.

**Strengths.** Predictable latency; safe, gradual releases; rollback without
redeploying.

**Weaknesses.** Snapshots must avoid capturing unique state (random seeds,
connections); provisioned capacity costs money.

**Opportunities.** Our cold start should make snapshotting unnecessary, which
is itself the argument (`W-SL-1`). Revisions and traffic splitting belong in the
deployment adapter contract (`W-DP-2`), and are the web half of progressive
delivery alongside feature flags (`C50`).

**Threats.** Low, provided the budgets are met.

**Position.** **Absent.**

**Requirements.**

- `C47-1` `[W]` Immutable revisions and percentage traffic splitting in the
  deployment adapter contract.
- `C47-2` `[W]` A documented rule that nothing unique is initialized at startup
  outside a per-invocation scope, so snapshot-restore hosts are safe.

## C48 — Declarative resource bindings

**Introduced by.** W11, W10's tooling, W12.

**Mechanism.** The resources a function uses — a key-value namespace, a queue,
a bucket, a database, a secret — are declared in configuration and injected
into the handler's environment as typed handles, so code never constructs
clients from connection strings and infrastructure knows exactly what the code
touches.

**Strengths.** Least privilege by construction; infrastructure derivable from
declarations; test doubles injected the same way.

**Weaknesses.** Platform-specific binding types.

**Opportunities.** Service contracts (Milestone 20) plus typed configuration
(`W-EP-1`) are bindings already; declaring them in `rustnative.toml` lets
`rustnative deploy` derive infrastructure and permissions (`W-DP-1`) from what
the application actually uses.

**Threats.** Low.

**Position.** **Absent.**

**Requirements.**

- `C48-1` `[W]` Resource bindings declared in project metadata, injected as
  typed service handles, and used by deployment adapters to derive
  infrastructure and least-privilege permissions.

---

# Part E — Platform surfaces and product services

## C49 — Surfaces beyond the main window

**Introduced by.** M1, M2, D1 (and every first-party native archetype).

**Mechanism.** Modern hosts run parts of an application *outside* its main UI:
home-screen and lock-screen widgets with their own constrained rendering and
timeline; ongoing-activity surfaces and live notifications; quick-settings
tiles and control-center controls; share, action, and keyboard extensions;
lightweight instant versions of an application launched from a link or code;
companion applications on wearables, televisions, and vehicles; and on
desktop, tray and menu-bar extras, dock and jump-list menus, and taskbar
progress. Each runs in its own process or sandbox with its own lifecycle,
memory limit, and update model.

**Strengths.** Presence where the user already is; engagement without opening
the application; host-integrated features users expect from quality
applications.

**Weaknesses.** Each surface is a separate target with a restricted API,
tight limits, and shared-data plumbing to the main application.

**Opportunities.** Several surfaces (widgets especially) use a *restricted
declarative UI* the host renders — which a declarative tree can target
directly, with the capability model expressing each surface's limits. The same
application model reaching a widget, a watch, and a tray menu is a strong
demonstration of "one application, every target".

**Threats.** Cross-platform frameworks are commonly rejected because these
surfaces require dropping to native code; mobile quality programs reward them.

**Position.** **Absent.** Milestone 23 covers menus and dialogs within the
application only.

**Requirements.**

- `C49-1` `[X]` A surface vocabulary in the capability model — widget, live
  activity, tile, extension, instant application, companion device, tray or
  menu-bar extra, jump list, taskbar progress — so an application asks which
  surfaces a host offers (Tier 0: vocabulary only).
- `C49-2` `[M]` `[D]` Realization of widgets and of tray/menu-bar extras on the
  backends that have them, from a restricted subset of the portable tree, with
  data shared with the main application through a declared store.
- `C49-3` `[M]` Share and action extensions receiving typed payloads.

## C50 — Feature flags, remote configuration, and experimentation

**Introduced by.** Platform services commonly paired with M1–M5 and W3.

**Mechanism.** Behaviour is controlled by flags evaluated at runtime against
rules (user, cohort, percentage, version), fetched from a service and cached;
configuration values change without releasing; experiments assign variants and
measure outcomes; kill switches disable broken features instantly.

**Strengths.** Decoupling deploy from release; safe rollout; instant
mitigation; product experimentation.

**Weaknesses.** Flag debt; combinatorial testing; privacy considerations.

**Opportunities.** Flags are typed values in the environment (`C15`), with
defaults compiled in and invalidation when they change — typed flags remove the
"string key typo disables the feature" class entirely. Combined with revisions
(`C47`) and over-the-air updates (`M-BR-1`) they complete progressive delivery.

**Threats.** Product teams expect it and will bolt on an untyped client if the
framework offers nothing.

**Position.** **Absent.**

**Requirements.**

- `C50-1` `[X]` A typed feature-flag and remote-configuration service contract
  with compiled defaults, caching, offline behaviour, environment integration
  and invalidation, and a local override for development and tests.

## C51 — In-application commerce

**Introduced by.** M1, M2, and every host store's own rules.

**Mechanism.** Purchases and subscriptions through each host store's own
billing system, with product catalogues, entitlement checks, receipt
validation (ideally server-side), restoration, grace periods, and refunds —
under rules that forbid alternative billing for digital goods in many
jurisdictions.

**Strengths.** Monetization with store trust and payment handling.

**Weaknesses.** Different APIs, rules, and edge cases per store; receipt
validation is easy to get wrong.

**Opportunities.** A portable entitlement model over per-store adapters, with
validation integrated with the server model, is exactly the kind of
capability-guarded service the plan is built for.

**Threats.** Consumer mobile applications without commerce support will not
adopt the framework.

**Position.** **Absent.**

**Requirements.**

- `C51-1` `[M]` `[D]` A commerce service contract — catalogue, purchase,
  entitlements, restoration, subscription state — with per-store adapters and
  server-side receipt validation in the server model.

## C52 — Secure storage and identity

**Introduced by.** M1, M2, D1, W7.

**Mechanism.** Secrets live in the host's protected store (keychain, keystore,
credential vault), optionally hardware-backed and gated by biometrics or
device passcode; passkeys replace passwords using platform authenticators;
tokens are refreshed and revoked centrally.

**Strengths.** Secrets never in plain storage; phishing-resistant sign-in;
user-visible, host-standard prompts.

**Weaknesses.** Per-host semantics differ (access groups, synchronization,
invalidation on biometric change).

**Opportunities.** A capability-guarded secure-store contract with honest
per-host answers (hardware-backed or not, biometric-gated or not) is
straightforward and closes a security gap applications otherwise fill badly.

**Threats.** Applications storing tokens in plain preferences is a common audit
finding, and the framework is blamed for it if it offers no alternative.

**Position.** **Absent.**

**Requirements.**

- `C52-1` `[X]` A secure-storage contract backed by each host's protected store,
  with capability answers for hardware backing and biometric gating.
- `C52-2` `[X]` Passkey sign-in through platform authenticators, integrated
  with the server model's authentication (`W-SF-2`).

## C53 — Dynamic delivery and application thinning

**Introduced by.** M1, M2, and the host stores.

**Mechanism.** The installed application contains only what the device needs:
per-device slices of assets and code, on-demand resources and feature modules
downloaded when first used, and asset packs for large content — all delivered
by the store rather than by the application's own servers.

**Strengths.** Smaller downloads and installs, which measurably improve
conversion; large content without bloating the base install.

**Weaknesses.** Complexity; features unavailable offline until downloaded.

**Opportunities.** The code-splitting mechanism required for the web
(`W-RS-1`) and asset handling in packaging (Milestone 32) extend naturally to
store-delivered modules.

**Threats.** Install size thresholds affect store ranking and conversion.

**Position.** **Absent.**

**Requirements.**

- `C53-1` `[M]` Per-device asset slicing and on-demand asset and feature packs
  through each store's own delivery mechanism, driven from project metadata.

## C54 — Push notification infrastructure

**Introduced by.** M1, M2, W3 (browser push), and platform services.

**Mechanism.** Device registration and token rotation; topic subscription;
rich notifications with media and actions; background delivery that wakes the
application briefly; notification channels and categories; and server-side
sending with per-host payloads.

**Strengths.** Re-engagement and timely information; actionable notifications
without opening the application.

**Weaknesses.** Per-host payload formats and limits; permission prompting
strategy matters.

**Opportunities.** Notifications are already a capability; the full path —
registration, token lifecycle, actions routed into the message model, and
server-side sending from the server model — is a portable contract with
per-host adapters.

**Threats.** Push is non-optional for most consumer mobile applications.

**Position.** **Partial** — local notifications are a capability; remote push
is not specified.

**Requirements.**

- `C54-1` `[M]` `[W]` `[D]` Remote push: registration, token rotation, topics,
  rich and actionable notifications with actions delivered as messages,
  background delivery, and a server-side sending service in the server model.

---

## Part summary

**Largest openings.** Local-first sync (`C32`) and persistent-connection
server-driven UI (`C33`) are both *reconciliation problems*, which is the core
competency this framework is built on — the same machinery that reconciles a
UI tree reconciles data between devices and a server tree with a browser.
Durable execution (`C45`) is the server concept where enforced determinism
gives a guarantee the originators can only lint for.

**Tier 0 implications.** Per-property mapping (`C24`) and the surface
vocabulary (`C49-1`) shape how every backend is structured; both land with
Milestone 39.

**Adoption-critical expectations.** Adaptive layout (`C22`), host content
controls (`C28`), secure storage (`C52`), push (`C54`), commerce (`C51`),
feature flags (`C50`), and web discoverability (`C41`, `C42`) — each is a
common reason an evaluation ends early.
