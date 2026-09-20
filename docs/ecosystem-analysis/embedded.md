# Embedded archetypes — RTOS, bare-metal, and embedded devices

Read [`foundations.md`](foundations.md) first. Bottom-up analysis per
[`method-and-stack.md`](method-and-stack.md).

RustNative's targets here are `PLAN.md` Milestone 37 (embedded Linux,
RTOS-backed, and selected bare-metal profiles) and, on the same draw-list path,
Milestone 38 (terminal). This is the target class where the root layers matter
most and where almost nothing above L4 can be assumed: there may be no
allocator, no threads, no filesystem, no clock beyond a timer peripheral, no
display, and no user.

It is also the class where `PLAN.md`'s capability model stops being an
architectural preference and becomes the only honest way to describe reality.

Tags: `[E]` embedded/RTOS, `[X]` cross-cutting.

---

## E1 — The small preemptive RTOS kernel

**Root (L0–L2).** A few thousand lines of C providing tasks with priorities, a
preemptive scheduler, queues, semaphores, mutexes with priority inheritance,
timers, and optional tickless idle. Statically or pool-allocated. No UI layer
of any kind — L2 does not exist.

**Semantics and model (L3–L4).** The application is tasks and inter-task
primitives. Concurrency is preemptive and shared-memory, so the defect classes
are the classic ones: priority inversion, stack overflow, unbounded blocking,
and data races that appear only under a specific interrupt interleaving.

**Integration (L5–L6).** Whatever the vendor's HAL provides (E4) plus
middleware chosen per project (E6). Nothing is integrated for you.

**Loop and ship (L7–L8).** Debug probe, breakpoints, and a serial console.
Shipping is a flashed image.

**Project (L9).** Ubiquitous, long-lived, deeply documented, and present in an
enormous installed base of shipped products. Frequently the kernel inside
vendor SDKs, so teams use it without choosing it.

**Strengths.** Tiny footprint; deterministic and well-understood scheduling;
portable across nearly every microcontroller architecture; decades of field
evidence; certified variants available for safety work.

**Weaknesses.** Everything above the kernel is the application's problem;
shared-memory preemptive concurrency in C with no compile-time protection;
stack sizing by measurement and hope; and no application model, so every
product reinvents structure.

**Opportunities for RustNative.** The opening is exactly our substrate: a
compile-time-checked concurrency model on top of, or in place of, this kernel
removes the defect classes that dominate its bug reports. The practical shape
is not to replace the kernel but to *run on it* — our single-threaded executor
(`X-L1-3`) as one task, with the core's identity, reconciliation, and layout
layers in a `no_std` profile. `PLAN.md` already names that core work; it is the
prerequisite for this entire document.

**Threats.** Incumbency and inertia are absolute here. Teams do not replace a
working kernel; they add to it. Any strategy that requires displacing it fails.

**What we must ship.**

- `E-K-1` `[E]` A `no_std`-capable core profile: identity, node, reconciliation,
  and layout without `std`, with allocation behind an explicit, replaceable
  strategy and a documented static-memory mode.
- `E-K-2` `[E]` A single-threaded, non-`Send` executor profile running as one
  task under a host RTOS, with the same test suite as the threaded profile
  (= `X-L1-3`).
- `E-K-3` `[E]` Documented stack, heap, and worst-case timing characteristics
  for the core's hot paths, measured rather than estimated.

---

## E2 — The configuration-driven RTOS ecosystem

**Root (L0–L2).** A scalable kernel bundled with a device-description layer, a
driver model, subsystem APIs (networking, storage, sensors, power, Bluetooth),
a configuration system, and a purpose-built build system. The distinguishing
root choice is that hardware is *described in data* and the build assembles the
image from that description.

**Semantics and model (L3–L4).** Portable subsystem APIs mean application code
is written against capabilities rather than against a specific chip — the same
idea as `PLAN.md` 2.5, executed at the driver level.

**Integration (L5–L6).** Very broad: connectivity stacks, power management,
file systems, sensor abstractions, logging, shell, and a device-management and
update story.

**Loop and ship (L7–L8).** A real build system with configuration fragments and
board overlays; emulator targets; unit testing on host; and a bootloader with
A/B image slots as part of the ecosystem rather than an afterthought.

**Project (L9).** Vendor-neutral governance, broad silicon support, and an
active community — the most credible modern general-purpose embedded platform.

**Strengths.** Hardware portability through data-described boards; enormous
driver and subsystem coverage; testable on host; integrated update and
bootloader story; genuine multi-vendor support.

**Weaknesses.** Steep learning curve and a build system that is a skill of its
own; large configuration surface; footprint larger than E1; and a C API that
carries none of the guarantees our substrate can.

**Opportunities for RustNative.** This archetype is the *right host* for us on
constrained targets, and the integration story is concrete: our application
model as a task, its drivers behind our service contracts, and its board
description feeding our build metadata so `rustnative` does not invent a second
hardware-description format. Its capability-shaped subsystem APIs map cleanly
onto our capability contracts, which is unusual and valuable.

**Threats.** It is also a plausible *competitor* for the application layer if
it grows one; and its build system will resist being driven by ours.

**What we must ship.**

- `E-EC-1` `[E]` A supported integration with at least one configuration-driven
  RTOS ecosystem: our executor as a task, its subsystem APIs behind our service
  contracts, and its board description consumed by `rustnative` rather than
  duplicated.
- `E-EC-2` `[E]` Capability mapping for embedded subsystems — networking,
  storage, sensors, power, connectivity — so an application asks what exists
  rather than assuming.

---

## E3 — The commercial and safety-certified RTOS

**Root (L0–L2).** A commercially supported kernel with hard real-time
guarantees, memory protection or full process isolation, and — the decisive
property — certification evidence packages for safety and security standards.

**Semantics and model (L3–L4).** Partitioned execution, time and space
isolation, deterministic scheduling with bounded worst-case latencies, and
tooling to demonstrate it.

**Integration (L5–L6).** Certified networking, filesystem, and graphics stacks,
each with its own evidence package, plus long-term support measured in decades.

**Strengths.** The only archetype that can be used where certification is
mandatory; determinism that is documented and provable; vendor liability and
long-term support; mature tooling for timing analysis.

**Weaknesses.** Licence cost; closed source in most cases; slow adoption of new
language toolchains; and an ecosystem constrained by what has been certified.

**Opportunities for RustNative.** Certification is out of reach for now and
should be stated as such rather than implied. What *is* reachable, and what
matters later, is the groundwork: a `no_std` core with bounded allocation, no
hidden global state, documented worst-case behaviour, traceable requirements,
and a testing story with coverage evidence. Rust's growing acceptance in
safety-adjacent work makes this a real long-term position — but only if the
groundwork exists before it is asked for.

**Threats.** In regulated device markets, an uncertified stack is not
considered at all, regardless of technical merit.

**What we must ship.**

- `E-SF-1` `[E]` A stated certification posture: what we do and do not claim,
  which practices are in place today (traceability, bounded allocation, no
  hidden global state, coverage evidence), and what a future certification
  effort would require.
- `E-SF-2` `[X]` Requirement traceability from this analysis and `PLAN.md`
  through to tests, maintained as build output rather than prose.

---

## E4 — Vendor HALs and chip SDKs

**Root (L0–L2).** The chip vendor's peripheral abstraction, startup code,
linker scripts, clock configuration, and board support — often with a graphical
configuration tool that generates initialization code. Frequently bundles E1 as
its kernel.

**Strengths.** The fastest path from a chip to blinking hardware; complete
peripheral coverage including the parts nobody else implements; vendor support;
reference designs and examples for every board.

**Weaknesses.** Chip-vendor lock-in by construction; generated code that is
hard to own; C APIs with inconsistent error handling; quality that varies by
vendor and by peripheral; and a tendency to assume it owns `main`.

**Opportunities for RustNative.** We should never compete here — we should
*consume* these. The requirement is that our core never assumes ownership of
startup, clocks, or `main`, so it can be dropped into a vendor-generated
project. That is an inversion-of-control property and it is cheap now,
expensive later.

**Threats.** None competitively, but a vendor SDK that insists on owning the
event loop is a real integration hazard and must be designed around (F1.1).

**What we must ship.**

- `E-HAL-1` `[E]` A guest integration mode: our runtime driven from an existing
  vendor project's `main` and its initialization, with no assumption of owning
  startup, clocks, interrupts, or the loop.
- `E-HAL-2` `[E]` Peripheral access left to the platform's existing trait
  ecosystem rather than re-abstracted, with our service contracts adapting to
  it.

---

## E5 — Rust embedded concurrency frameworks

**Root (L0–L2).** Two established shapes in Rust: an async executor with
hardware-driven wakers, timers, and peripheral drivers written as `async`
functions; and an interrupt-driven framework where tasks are bound to hardware
priorities with compile-time-checked resource sharing. Both sit on a shared
trait ecosystem for peripheral access.

**Semantics and model (L3–L4).** Compile-time concurrency correctness: shared
resources are checked, priorities are static, and data races are prevented by
the type system rather than by discipline. Allocation is often absent entirely.

**Integration (L5–L6).** Growing driver ecosystem built on the shared traits;
networking, USB, and storage available with varying maturity.

**Loop and ship (L7–L8).** Deferred-formatting logging over a debug probe —
where formatting happens on the host, so the device pays almost nothing — plus
probe-based flashing and run tooling. This is the best embedded debug loop
available in any language and the bar for our own tooling on these targets.

**Project (L9).** Smaller than the C ecosystems and growing quickly, with a
strong culture of compile-time guarantees.

**Strengths.** The defect classes that dominate E1's bug reports are eliminated
at compile time; no allocator required; async without a kernel; tiny binaries;
and excellent, low-overhead diagnostics.

**Weaknesses.** Driver coverage is far behind vendor SDKs; peripheral support
varies by chip family; and it competes with vendor tooling teams already know.

**Opportunities for RustNative.** These are **allies, not competitors**, and
the single most important integration decision in this document is not to
duplicate them. Our executor seam (`X-L1-3`) should be implementable *by* an
existing embedded executor, and our service contracts should adapt to the
existing trait ecosystem rather than replacing it. Their deferred-formatting
diagnostic approach is also exactly what our inspection protocol (`X-L7-3`)
needs on constrained targets, where a full inspector transport is impossible.

**Threats.** Only one: that we build a parallel, incompatible embedded stack
and split a small ecosystem. That outcome is avoidable and must be stated as a
constraint.

**What we must ship.**

- `E-RS-1` `[E]` The `Executor` contract implementable by existing embedded
  async executors, demonstrated with at least one.
- `E-RS-2` `[E]` A constrained-target diagnostic channel using deferred
  host-side formatting over a debug probe, carrying the same inspection
  protocol as richer targets in a reduced form.
- `E-RS-3` `[E]` A stated non-duplication policy: peripheral drivers, HAL
  traits, and probe tooling are consumed, not reimplemented.

---

## E6 — Embedded middleware stacks

**Root.** Independent libraries filling specific needs: network stacks, TLS,
filesystems, USB device stacks, bootloaders with image verification and
rollback, GUI libraries, and constrained machine-learning runtimes. Each is
portable across kernels and HALs by design.

**Strengths.** Battle-tested, small, portable, and focused; a bootloader with
A/B slots and rollback is the difference between a fleet you can update and one
you can brick.

**Weaknesses.** Integration is the application's job; configuration surfaces
are large; and security-sensitive components (TLS, bootloader) demand
maintenance most product teams do not budget for.

**Opportunities for RustNative.** Consume, expose through capabilities, and
make the *update path* a first-class part of `rustnative` rather than a
per-product integration. Firmware update with A/B slots and rollback is the
embedded equivalent of the mobile over-the-air requirement (`M-BR-1`), and our
plan currently has neither.

**Threats.** None; the risk is under-scoping the update story, which is a
fleet-level liability rather than a feature gap.

**What we must ship.**

- `E-MW-1` `[E]` Firmware update as a first-class `rustnative` capability:
  signed images, A/B slots, verification, automatic rollback on failed boot,
  and staged fleet rollout, delegating to an existing bootloader rather than
  writing one.
- `E-MW-2` `[E]` Secure-boot and device-identity posture documented per profile
  — what the framework provides, what the hardware must provide, and what is
  out of scope.

---

## E7 — Embedded Linux build systems

**Root (L0–L2).** Not runtimes but *image assemblers*: cross-toolchain
construction, package recipes, dependency resolution, licence accounting, and
reproducible root filesystem generation for a device.

**Strengths.** Complete control over what is on the device; reproducible builds
and software bills of materials, which matter for compliance; licence
compliance artifacts generated automatically; long-term maintainability for
products with ten-year lifespans.

**Weaknesses.** Extremely steep learning curve; very long build times; and a
skill set that is scarce and expensive.

**Opportunities for RustNative.** Integrate rather than replace: `rustnative`
should emit a recipe or package that these systems consume, so an embedded
Linux product includes our application the way it includes anything else. Their
licence-accounting and bill-of-materials output is also the model for our own
compliance evidence (`W-EP-3`), and it is generated rather than written.

**Threats.** None; failing to integrate simply excludes us from embedded Linux
products.

**What we must ship.**

- `E-BL-1` `[E]` Build-system integration: `rustnative` emits a consumable
  recipe/package for at least one embedded Linux build system, with
  cross-compilation, sysroot, and toolchain inputs documented.
- `E-BL-2` `[X]` A generated software bill of materials and licence report per
  artifact, for every target (= `W-EP-3`).

---

## E8 — MCU graphical libraries

**Root (L0–L2).** Widget toolkits that draw into a framebuffer on a
microcontroller (F2.6): their own controls, their own layout, their own fonts,
optimized for partial refresh, limited RAM, and no GPU. The more modern members
add a declarative UI language and a designer tool, and compile the UI into the
firmware.

**Semantics and model (L3–L4).** Retained widget trees with invalidation and
partial redraw; layout is usually simple containers plus absolute placement;
text uses pre-generated bitmap or compact vector fonts.

**Integration (L5–L6).** Touch and encoder input, display drivers, and
animation. Accessibility does not exist, correctly, because the host has none.

**Loop and ship (L7–L8).** Desktop simulators are standard and are the reason
these are pleasant to develop with; designer tooling with live preview is the
differentiator among the modern members.

**Strengths.** Fits in tens of kilobytes of RAM; partial refresh makes cheap
displays look good; simulator-first development; declarative authoring with
real designer tooling in the newer members; and a genuine cross-device story
from microcontroller to embedded Linux.

**Weaknesses.** Their own widget and layout model, separate from anything the
team uses elsewhere; limited text shaping, so complex scripts are poorly served;
memory budgets that constrain design; and licensing models that vary.

**Opportunities for RustNative.** This is our most direct competitor on
`PLAN.md` Milestone 37's display-bearing profile, and the comparison is
favourable in one specific way: their UI model is *only* for embedded, while
ours is the same component model, layout, identity, and state the team already
uses on desktop, mobile, and web. `D-CT-1`'s one-stack argument lands harder
here than anywhere else. The features to match are concrete and bounded:
partial-refresh damage tracking, a desktop simulator, and compact font handling
— and the first two are already required by the terminal backend, which is the
same draw-list path.

**Threats.** Their designer tooling and their memory efficiency are real
advantages, and their simulator-first loop is exactly the L7 experience we must
match rather than explain away.

**What we must ship.**

- `E-GUI-1` `[E]` Damage-tracked partial redraw on the draw-list path, with
  dirty-region coalescing, shared with the terminal backend's cell-damage
  tracking rather than implemented twice.
- `E-GUI-2` `[E]` A host-side device simulator: the same draw list rendered on
  a development machine with simulated display size, colour depth, and input,
  so most application work happens without hardware (`PLAN.md` Milestone 37
  names this; it should be a deliverable, not a note).
- `E-GUI-3` `[E]` A constrained text profile: pre-shaped or compact font
  handling with an explicit statement of which scripts a given profile
  supports, so the limitation is declared rather than discovered.
- `E-GUI-4` `[E]` Declared RAM, flash, and frame-budget profiles per device
  class, enforced in CI like every other budget.

---

## E9 — Prototyping and hobbyist platforms

**Root (L0–L2).** Two shapes: a simplified compiled layer with a setup/loop model
over an enormous library ecosystem, and interpreted-language runtimes on
microcontrollers with a live REPL and filesystem-based code editing.

**Strengths.** The lowest barrier to entry in all of embedded; a library for
every sensor and module, usually within days of the hardware appearing; live
REPL iteration that no compiled workflow matches; and an education pipeline
that produces the next generation of embedded developers.

**Weaknesses.** Performance and memory overhead (severe in the interpreted
variants); weak concurrency models; limited debugging; and library quality that
ranges from excellent to abandoned.

**Opportunities for RustNative.** Not a competitor, but a lesson about
*approachability*, which is the axis compiled embedded stacks lose on. The
transferable parts are a board-and-sensor quickstart path, a live loop that
feels immediate, and an examples corpus per board. This is also where
`X-L7-1`'s on-device rebuild-and-restart budget matters most, because the
comparison is against a REPL.

**Threats.** Low commercially, high in mindshare: developers learn here and
carry their expectations into professional work.

**What we must ship.**

- `E-PR-1` `[E]` A board quickstart path: one command from a supported board to
  a running application, with flashing, logging, and restart included.
- `E-PR-2` `[E]` A per-board example corpus covering display, input, sensors,
  storage, and connectivity.

---

## E10 — IoT connectivity and device management

**Root.** Protocol clients and brokers for lightweight messaging, an
interoperability standard for smart-home devices with its own certification,
cloud provider device SDKs with provisioning and shadow state, and flow-based
visual integration tools.

**Strengths.** Standardized connectivity with real interoperability;
provisioning, identity, and fleet management solved; visual tooling that lets
non-specialists integrate devices.

**Weaknesses.** Heavy on constrained devices; certification cost for the
interoperability standard; cloud SDKs that pull provider lock-in onto the
device itself; and security postures that vary widely.

**Opportunities for RustNative.** Connectivity is a service contract, not a
framework feature, and treating it that way keeps us out of a protocol arms
race. The high-value piece is *device identity and provisioning* as a
capability, because it is where teams get security wrong and where fleet
operations begin.

**Threats.** None directly; the risk is scope creep into protocol
implementation.

**What we must ship.**

- `E-IOT-1` `[E]` Messaging, provisioning, and device-identity service
  contracts with at least one working adapter each, explicitly delegating
  protocol implementation to existing libraries.

---

## E11 — Robotics middleware

**Root (L0–L2).** A distributed node graph over a publish/subscribe transport
with discovery, typed message definitions, quality-of-service policies, and
real-time-capable execution. The application is a graph of processes, not a
single program.

**Strengths.** Composition across languages and processes; an enormous library
of sensor drivers, planners, and controllers; excellent visualization,
recording, and replay tooling; simulation integration; and a real-time story.

**Weaknesses.** Heavy; complex configuration; discovery behaviour that is
difficult to debug; and a steep learning curve.

**Opportunities for RustNative.** Robotics systems need operator interfaces —
on-device panels, teach pendants, and control stations — and today those are
built with desktop toolkits entirely separate from the robot's own stack. A
framework that spans an embedded panel and a desktop control station with one
model is a strong fit. The integration is a transport adapter and typed message
mapping, not a competing middleware.

**Threats.** None; attempting to compete with the middleware itself would be a
scope error.

**What we must ship.**

- `E-RB-1` `[E]` A transport adapter pattern for node-graph middleware: typed
  messages mapped to our message and state model, with an operator-interface
  example spanning an embedded panel and a desktop station.

---

## E12 — Edge AI runtimes

**Root.** Constrained inference runtimes: quantized model execution with a
restricted operator set on microcontrollers, portable graph runtimes with
hardware-accelerated backends on larger devices, classical vision libraries,
and end-to-end platforms that handle data collection, training, and deployment.

**Strengths.** Inference within kilobytes of RAM in the constrained variants;
hardware acceleration where it exists; mature model tooling; and end-to-end
platforms that make the workflow accessible to non-specialists.

**Weaknesses.** Operator coverage gaps that force model redesign; quantization
accuracy loss; toolchain complexity; and memory planning that becomes the
dominant design constraint.

**Opportunities for RustNative.** Inference is a *service*, and the framework's
job is the part these runtimes do not do: keeping inference off the UI path,
bounding its latency, tying its lifetime to the component that requested it,
and presenting results without dropping frames. That is our scheduler and task
scopes applied to a workload that is increasingly on every device.

**Threats.** None; this is an integration surface, not a competitive one.

**What we must ship.**

- `E-AI-1` `[E]` An inference service contract with bounded-latency scheduling,
  cancellation tied to component lifetime, and a documented policy for keeping
  inference off the frame path.

---

## The embedded obligations every archetype must satisfy

**No allocator, or a bounded one.** Static allocation must be possible; where
allocation exists, it must be bounded and its failure handled rather than
aborting.

**No hidden global state**, because there is no process boundary to contain it
and no supervisor to restart it.

**Power.** Sleep modes, wake sources, and tickless idle dominate battery life,
and a framework that polls or wakes on a timer it does not need is unusable
regardless of how good its UI is.

**Watchdogs and recovery.** A hung task must be detected and the device
recovered; a panic must leave hardware in a safe state (`X-L4-4`).

**Determinism.** Worst-case timing matters more than average timing, and
worst-case allocation matters more than average allocation.

**Fleet reality.** Devices ship for years and are updated in the field or not
at all (`E-MW-1`), and diagnosing them means whatever fits down a serial line
(`E-RS-2`).

- `E-OB-1` `[E]` A power-aware scheduler profile: no periodic wake without a
  pending deadline, documented wake sources, and a measured idle current figure
  on a reference board.
- `E-OB-2` `[E]` Watchdog integration and a safe-state panic path per profile,
  verified by a deliberate hang and a deliberate panic.
- `E-OB-3` `[E]` Bounded-allocation mode with allocation failure as a handled
  outcome, not an abort.

---

## Summary: the embedded opening, ranked

1. **The core profile work (`E-K-1`, `E-K-2`).** Not an opportunity but a gate:
   nothing else in this document is reachable without a `no_std` core and a
   non-`Send` executor, and both are already named in `PLAN.md`'s roadmap.
2. **One stack from device to desktop (`D-CT-1`, `E-GUI-1`, `E-GUI-2`).** The
   MCU GUI archetype's UI model stops at the device; ours does not, and the
   draw-list path already serves terminal and embedded from one mechanism.
3. **Fleet updates and recovery (`E-MW-1`, `E-OB-2`).** The difference between
   a demonstration and a product, and absent from our plan today.
4. **Alliance rather than duplication (`E-RS-1`, `E-RS-3`, `E-HAL-1`,
   `E-EC-1`).** The embedded Rust ecosystem is small; splitting it would cost
   us more than any feature gains.
5. **Approachability (`E-PR-1`, `E-PR-2`, `E-GUI-2`).** The axis compiled
   embedded stacks always lose, and the one the simulator and quickstart work
   directly addresses.
