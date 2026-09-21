# Concepts IV — constrained and device targets (L0–L8 on embedded)

Continues [`concepts-core.md`](concepts-core.md) (format, inclusion rule,
scoring). Requirement identifiers are `Cnn-k`.

Embedded concepts are gathered in one document rather than spread across the
layer-ordered ones because on these targets the layers collapse into each
other: an allocation strategy (L1) is a product decision (L8), and a storage
layout (L6) is a field-update decision (L8). Within the document the order is
still bottom-up.

---

# Part A — Hardware, scheduling, and memory

## C73 — Hardware described as data

**Introduced by.** E2 (device descriptions and configuration fragments), E4
(register description files and graphical configuration tools), E5 (typed
peripheral-access crates generated from register descriptions).

**Mechanism.** Boards, pins, peripherals, memory maps, and clock trees are
described in data files. Build tools generate initialization, linker layout,
and — in the Rust ecosystem — a complete typed register-access layer from
vendor register descriptions. Configuration tools check pin and clock
conflicts before code is written.

**Strengths.** Hardware portability by editing data; typed, complete
peripheral access without hand-written register code; conflicts caught at
configuration time.

**Weaknesses.** Description quality varies by vendor; generated layers are
large.

**Opportunities.** `rustnative` must know about boards to build firmware
(Milestone 37). Consuming existing descriptions — rather than defining a new
format — keeps us inside the ecosystem (`E-RS-3`) and lets board metadata drive
capability answers (which displays, inputs, and radios a board has).

**Threats.** A framework-specific board format would be one more thing every
board needs, and boards would be missing.

**Position.** **Absent.**

**Requirements.**

- `C73-1` `[E]` Board metadata consumed from existing device and register
  descriptions, used by `rustnative` for build configuration and to derive
  capability answers; no framework-specific hardware format.

## C74 — Typestate and compile-time resource checking for hardware

**Introduced by.** E5.

**Mechanism.** Peripherals and pins are values whose types encode their mode;
a peripheral is *taken* once, so two drivers cannot own the same pin; clock
and DMA configurations are validated by types. Misconfiguration is a compile
error.

**Strengths.** A whole class of hardware bugs — double use, wrong mode, missing
clock — removed before flashing.

**Weaknesses.** Generic-heavy APIs and long type errors.

**Opportunities.** This is `C21` applied to hardware, and it already exists in
the ecosystem. The framework's input and display drivers should accept these
typed peripherals rather than raw handles, so our layer inherits the
guarantees instead of erasing them.

**Threats.** Low.

**Position.** **Absent** (no embedded backend yet).

**Requirements.**

- `C74-1` `[E]` Framework display, input, and storage drivers take ownership of
  typed peripherals from the ecosystem's traits, never raw register handles.

## C75 — Static-priority scheduling with a stack resource policy

**Introduced by.** E5's interrupt-driven concurrency framework.

**Mechanism.** Tasks are bound to hardware interrupt priorities; shared
resources are declared, and each is given a ceiling priority; locking a
resource raises the priority to its ceiling. The result is provably free of
deadlock and unbounded priority inversion, runs every task on one stack, and
costs almost nothing at runtime — with resource access checked at compile time.

**Strengths.** Hard real-time behaviour with mathematical guarantees; minimal
memory; no kernel.

**Weaknesses.** Static structure — tasks and priorities fixed at compile time;
unfamiliar model.

**Opportunities.** The single-threaded executor profile (`E-K-2`) should be able
to run *as a task* in such a system, at a declared priority, with UI work below
real-time control work. This is how a device with both a control loop and a
display should be structured, and saying so explicitly prevents the UI from
ever preempting control.

**Threats.** A framework that assumes it owns all execution is unusable on
real-time devices.

**Position.** **Absent.**

**Requirements.**

- `C75-1` `[E]` The executor profile runs as one task at a declared priority
  inside a static-priority system, with the rule that framework work never runs
  above the application's real-time tasks, and an example with a control loop
  and a display.

## C76 — Executors that sleep by default

**Introduced by.** E5's async executor.

**Mechanism.** When no task is ready, the executor puts the processor to sleep
until an interrupt wakes a task; timers are backed by a low-power hardware
timer; there is no polling loop. Low power is the default behaviour rather than
an optimization.

**Strengths.** Battery life without effort; responsiveness preserved because
wake-ups are interrupt-driven.

**Weaknesses.** Wake latency from deep sleep; peripherals must support
interrupt-driven operation.

**Opportunities.** `E-OB-1` requires no periodic wake without a pending
deadline; implementing the executor seam on top of an executor that already
sleeps satisfies it by construction, and the scheduler's frame pacing must also
stop entirely when nothing is animating.

**Threats.** A UI framework that redraws on a timer drains batteries and is
rejected.

**Position.** **Absent.**

**Requirements.**

- `C76-1` `[E]` Frame pacing stops completely when no animation, input, or
  pending work exists, and the executor seam, when implemented by a sleeping
  executor, adds no periodic wake of its own — verified by measured idle
  current.

## C77 — Selectable allocation strategies and arenas

**Introduced by.** E1 (a choice of heap implementations), E12 (static tensor
arenas), E3.

**Mechanism.** The allocator is a build-time choice: none (fully static), a
bump allocator that never frees, fixed-size pools, or a general heap. Large
workloads get a dedicated *arena* sized up front, so their peak is known and
fragmentation cannot occur.

**Strengths.** Memory behaviour known at build time; no fragmentation failures
in the field; certification friendliness.

**Weaknesses.** Sizing is manual; the wrong choice wastes memory or fails.

**Opportunities.** `E-K-1` already requires allocation behind a replaceable
strategy. Arenas map naturally onto framework structures with known maxima — a
node pool sized for the largest tree, a draw-list arena per frame — and the
tree's size is observable, so the framework can *report* the arena size an
application needs.

**Threats.** Field failures from heap exhaustion are the most expensive bugs in
embedded products.

**Position.** **Absent.**

**Requirements.**

- `C77-1` `[E]` Pool and arena allocation for framework structures (nodes,
  layout data, draw lists) with capacities declared at build time.
- `C77-2` `[E]` A high-water-mark report per structure from simulator and
  on-device runs, so capacities are set from measurement.

## C78 — Memory protection and supervised isolation

**Introduced by.** E2 (userspace threads with memory domains), E3 (microkernel
message passing, process restart supervision, adaptive time partitioning).

**Mechanism.** On microcontrollers with a memory protection unit, application
threads run unprivileged in memory domains and cannot corrupt the kernel or each
other. On microkernel systems, drivers and services are separate processes
communicating by messages; a supervisor restarts a failed service without
rebooting; time partitions guarantee each subsystem a CPU share.

**Strengths.** Fault containment on devices that cannot be rebooted casually;
a failing UI cannot take down control; safety arguments become possible.

**Weaknesses.** Configuration effort; memory and switching overhead; limited
region counts on small parts.

**Opportunities.** The UI is precisely the component that should run in the
least privileged, most restartable partition. A framework that documents how
to run in an unprivileged domain or as a supervised process — and that restores
its state on restart (Milestone 30) — makes the device architecture that safety
reviewers want easy to build.

**Threats.** Devices where a UI bug can disable a safety function are not
certifiable.

**Position.** **Absent.**

**Requirements.**

- `C78-1` `[E]` The framework can run as an unprivileged task in a
  memory-protected domain, or as a supervised process, and recovers its state
  after a supervised restart, with an example on one protected target.

## C79 — Standardized kernel interfaces and POSIX on microcontrollers

**Introduced by.** E4 (a standard RTOS interface layered over several
kernels), and RTOS kernels offering a POSIX-compatible environment on small
devices.

**Mechanism.** Application code is written against a standard interface —
threads, mutexes, message queues, timers — that many kernels implement, or
against POSIX, so code moves between kernels and between microcontroller and
Linux targets.

**Strengths.** Kernel independence; reuse of existing Unix code on small
devices; easier team skill transfer.

**Weaknesses.** Lowest-common-denominator interfaces hide kernel strengths;
POSIX layers cost memory.

**Opportunities.** Our `Executor` and host-clock contracts are the framework's
equivalent. Providing adapters for a standard kernel interface — so one adapter
covers many kernels — is cheaper than one adapter per kernel.

**Threats.** Low.

**Position.** **Absent.**

**Requirements.**

- `C79-1` `[E]` An executor and clock adapter targeting a standard RTOS
  interface, covering every kernel that implements it, in addition to any
  kernel-specific adapters.

---

# Part B — Storage, boot, and field updates

## C80 — Power-loss-resilient storage

**Introduced by.** E6 (fail-safe, wear-levelling flash filesystems; key-value
stores in flash partitions), E4 (partition tables and encrypted non-volatile
storage), E2 (a settings subsystem).

**Mechanism.** Flash storage survives power loss at any instant: copy-on-write
metadata, atomic commits, and wear levelling spread writes to extend flash
life. Flash is divided into partitions (bootloader, application slots,
settings, data). A settings subsystem stores small typed values with
namespacing; sensitive partitions are encrypted with device-unique keys.

**Strengths.** No corrupted configuration after brown-outs; long flash life;
predictable layout.

**Weaknesses.** Write amplification; partition sizing is permanent once devices
ship.

**Opportunities.** Milestone 30's state store is already crash-safe on Windows;
the embedded equivalent is a state-store adapter on a power-loss-resilient
filesystem or key-value store, with the same atomicity contract stated in
power-loss terms and tested by cutting power in a loop.

**Threats.** Bricked devices after a power cut are the worst possible field
failure.

**Position.** **Absent.**

**Requirements.**

- `C80-1` `[E]` A state-store adapter on a power-loss-resilient store with the
  atomicity contract restated for power loss and verified by a power-cut test
  rig or simulator.
- `C80-2` `[E]` Partition layout declared in project metadata and generated,
  with encrypted partitions where the hardware supports it.

## C81 — Multi-image builds and the secure boot chain

**Introduced by.** E2, E4, E6.

**Mechanism.** A device image is several images — bootloader, application,
network co-processor firmware, sometimes a recovery image — built and signed
together. Each stage verifies the next (secure boot), flash contents can be
encrypted, and anti-rollback counters prevent downgrading to vulnerable
versions.

**Strengths.** A verifiable chain of trust from reset; protection against
physical and downgrade attacks.

**Weaknesses.** Key management is hard and permanent; mistakes brick devices.

**Opportunities.** `E-MW-1` and `E-MW-2` cover updates and posture; the concept
to add is that `rustnative` builds the *whole* multi-image set and signs it
consistently, rather than producing one application image and leaving the rest
to the team.

**Threats.** Security regulation for connected devices increasingly mandates
secure boot and update integrity.

**Position.** **Absent.**

**Requirements.**

- `C81-1` `[E]` Multi-image build and signing driven by `rustnative`, with
  anti-rollback versioning and a documented key-management procedure.

## C82 — Hardware-in-the-loop test matrices

**Introduced by.** E2 (a test runner across many boards and emulators), E9 (on-
target unit testing), E5 (probe-driven test runners).

**Mechanism.** The same test suite runs on emulated targets, on a simulator,
and on a rack of real boards connected to CI, with results aggregated per
board. Unit tests run *on the target* through the debug probe.

**Strengths.** Board-specific regressions caught before release; confidence in
hardware support claims.

**Weaknesses.** Rack maintenance; flaky hardware.

**Opportunities.** 2.13 says a platform is supported only after it runs on real
hardware; a hardware-in-the-loop matrix is how that rule becomes continuous
rather than a one-time check.

**Threats.** Supported-board claims without continuous hardware testing decay.

**Position.** **Absent.**

**Requirements.**

- `C82-1` `[E]` A test runner that executes the framework's suites on
  emulators, the simulator, and connected boards through debug probes, with
  per-board results recorded as the basis of "supported" claims.

## C83 — Boot-to-UI time and direct-to-display rendering

**Introduced by.** D5's embedded variant, E8, E7.

**Mechanism.** On embedded Linux, the UI renders directly to the display
(kernel mode setting or framebuffer) without a window system, and the boot
sequence is trimmed so the first frame appears in about a second; on
microcontrollers, a splash is shown by the bootloader and the UI takes over
without a blank frame.

**Strengths.** Appliance-like startup; lower memory without a compositor.

**Weaknesses.** Only one application owns the display; input handling moves
into the application.

**Opportunities.** The embedded Linux profile may reuse the Linux toolkit
backend (Milestone 37); a direct-to-display profile on the draw-list path is
the lighter alternative. Boot-to-first-frame belongs in the budget file.

**Threats.** Consumer and industrial devices are judged on power-on time.

**Position.** **Absent.**

**Requirements.**

- `C83-1` `[E]` A direct-to-display embedded Linux profile on the draw-list
  path, with no window system.
- `C83-2` `[E]` Boot-to-first-frame as a budget in Milestone 42, measured on a
  reference board.

---

# Part C — Fleets, connectivity, and interoperability

## C84 — Desired-state reconciliation for devices

**Introduced by.** E10's cloud device services (device twins and shadows).

**Mechanism.** Each device has a *desired* state document written by the
cloud and a *reported* state document written by the device. The device
reconciles itself toward desired state and reports what it achieved; the
difference is the pending work. Offline devices catch up on reconnection.

**Strengths.** Declarative fleet management; robust to intermittent
connectivity; auditable.

**Weaknesses.** Conflict handling when device-local changes and cloud changes
race; document size limits.

**Opportunities.** This is the framework's own core idea — declare desired
state, reconcile the real thing towards it — applied to device configuration.
The sync machinery (`C32`) and the typed state model fit directly, and a device
UI that shows desired versus reported state falls out naturally.

**Threats.** Fleet operators expect it from anything that runs on connected
devices.

**Position.** **Absent.**

**Requirements.**

- `C84-1` `[E]` A typed desired/reported state contract with reconciliation on
  the device, conflict policy, offline catch-up, and at least one cloud adapter
  — sharing machinery with the sync service (`C32`).

## C85 — Messaging semantics, standard device models, and commissioning

**Introduced by.** E10.

**Mechanism.** Lightweight messaging with explicit delivery guarantees
(at-most-once, at-least-once, exactly-once), retained last values for new
subscribers, a *last will* published when a client disconnects uncleanly, and
persistent sessions. Interoperability standards define device *types* as data
models (clusters of attributes, commands, and events), and a *commissioning*
flow that securely adds a device to a network and can grant control to several
controllers at once.

**Strengths.** Predictable delivery on unreliable networks; interoperable
devices from different vendors; secure onboarding.

**Weaknesses.** Certification costs for the standards; protocol stacks are
large.

**Opportunities.** A device data model is a typed state tree with commands and
events — structurally the same thing as the framework's component model. A
control application can render a device's model directly, and a device
application can *expose* its state as such a model. Protocol stacks themselves
stay delegated (`E-IOT-1`).

**Threats.** Smart-home and industrial buyers require interoperability-standard
support.

**Position.** **Absent.**

**Requirements.**

- `C85-1` `[E]` Messaging service contract with delivery guarantee, retained
  values, last-will, and persistent-session semantics exposed explicitly.
- `C85-2` `[E]` A mapping between typed application state and standard device
  data models, with commissioning delegated to existing stacks.

## C86 — Flow-based visual integration

**Introduced by.** E10.

**Mechanism.** Integrations are wired visually as flows of nodes that transform
and route messages, edited in a browser and deployed live.

**Strengths.** Non-programmers integrate devices and services; fast
prototyping.

**Weaknesses.** Flows become unmaintainable at scale; weak typing and testing.

**Opportunities.** None as a framework feature — recorded as a non-goal in
`C72`. Integration is through messaging contracts (`C85-1`) that such tools
already speak.

**Position.** **Rejected**, with reason.

---

# Part D — Robotics and edge inference

## C87 — Long-running actions, managed lifecycles, and launch descriptions

**Introduced by.** E11.

**Mechanism.** Beyond request/response, *actions* are goals that run for a
long time, stream feedback, and can be cancelled or pre-empted, with a result
at the end. Nodes have a *managed lifecycle* — unconfigured, inactive, active,
finalized — with explicit transitions so a system can be brought up and down in
order. *Launch descriptions* compose many nodes with parameters into a system.

**Strengths.** Explicit, observable control of long-running work; orderly
bring-up and shutdown; reproducible system composition.

**Weaknesses.** Verbosity; lifecycle state machines everywhere.

**Opportunities.** A long-running action is a task with progress and
cancellation — structured scopes plus a progress stream (`C12`), which the
framework can make first-class for *every* target: file uploads, exports,
device firmware updates, and robot motions all share the shape. Managed
lifecycles map onto statecharts (`C10`).

**Threats.** Low outside robotics; the concept is valuable everywhere.

**Position.** **Absent.**

**Requirements.**

- `C87-1` `[X]` A long-running operation primitive — goal, progress stream,
  cancellation, pre-emption, result — bound to a task scope and displayable by
  standard progress components.

## C88 — Recording and replaying data streams, with visualization

**Introduced by.** E11, E12's data pipelines.

**Mechanism.** All messages on a system's buses are recorded to files with
timestamps; recordings are replayed into the same system for debugging,
testing, and algorithm development; visualization tools render sensor data,
transforms, and state over time.

**Strengths.** Field problems reproduced in the lab; algorithm regression tests
from real data; shared understanding through visualization.

**Weaknesses.** Storage volume; privacy in recordings.

**Opportunities.** Record/replay (`C61`) generalizes this to UI; on device
targets, recording sensor and service inputs as well as UI input lets a device
session be replayed in the simulator (`E-GUI-2`).

**Threats.** Low.

**Position.** **Absent.**

**Requirements.**

- `C88-1` `[E]` Recordings on device targets include sensor and service inputs,
  replayable in the host-side simulator.

## C89 — Pluggable accelerators and models as versioned assets

**Introduced by.** E12.

**Mechanism.** Inference runtimes execute a model graph on the best available
hardware through pluggable *delegates* or *execution providers* (GPU, neural
accelerator, DSP), falling back to CPU per operation. Models are versioned
artefacts, quantized for target hardware, shipped and updated independently of
the application, and produced by a data-to-deployment pipeline.

**Strengths.** Portable models with hardware acceleration where present;
independent model updates; reproducible pipelines.

**Weaknesses.** Operator coverage varies per accelerator; accuracy changes with
quantization.

**Opportunities.** Accelerators are capabilities; models are assets with their
own update channel — the over-the-air and firmware update paths (`M-BR-1`,
`E-MW-1`) should carry model assets as a first-class payload type with
compatibility checks.

**Threats.** Low as a framework concern beyond scheduling (`E-AI-1`).

**Position.** **Absent.**

**Requirements.**

- `C89-1` `[X]` Accelerator availability as capability answers, and model
  assets as a versioned payload type in the update paths, with compatibility
  checked before activation.

## C90 — Board managers and component registries

**Introduced by.** E9, E2, E4, and RTOS ecosystems with online package
indices.

**Mechanism.** A board manager installs toolchains and board definitions on
demand; a component registry indexes drivers and middleware with compatibility
metadata; a single tool builds, flashes, and debugs across hundreds of boards.

**Strengths.** Zero-setup onboarding for new hardware; discoverable drivers.

**Weaknesses.** Registry quality varies.

**Opportunities.** `rustnative` should install and select toolchains and board
definitions on demand (`E-PR-1`), and the capability-package registry (`C71`)
should carry board and driver compatibility for embedded packages.

**Threats.** Setup friction is where embedded evaluations end.

**Position.** **Absent.**

**Requirements.**

- `C90-1` `[E]` On-demand installation of toolchains and board support by
  `rustnative`, and board compatibility in package metadata (`C71-1`).

## C91 — Programmable I/O and peripheral offload

**Introduced by.** E4 (programmable I/O state machines on some parts),
direct-memory-access-driven peripherals generally.

**Mechanism.** Small programmable state machines or DMA engines drive
peripherals — displays, LED chains, custom protocols — independently of the
CPU, with deterministic timing.

**Strengths.** Precise timing without CPU load; displays refreshed without
stealing cycles from the UI.

**Weaknesses.** Chip-specific.

**Opportunities.** Display drivers on the draw-list path should push damaged
regions (`E-GUI-1`) through DMA or programmable I/O where available, so frame
transfer does not occupy the CPU. This is a driver concern consumed from the
ecosystem, not a framework feature.

**Position.** **Absent**; handled by consuming ecosystem drivers.

**Requirements.**

- `C91-1` `[E]` The display driver contract supports asynchronous region
  transfer so DMA- or programmable-I/O-backed drivers can overlap transfer with
  the next frame's work.

## C92 — Kernel-aware debugging and trace recording

**Introduced by.** E1, E3, E2.

**Mechanism.** Debuggers understand the kernel: they list tasks, their stacks,
queues, and mutex owners; trace recorders capture scheduling events, interrupts,
and user events into a buffer for timeline visualization.

**Strengths.** Scheduling and timing bugs become visible; stack overflows found
before shipping.

**Weaknesses.** Tool licensing; trace buffer memory.

**Opportunities.** The reduced inspection form (`E-RS-2`) should include
scheduler and task events in a format existing trace viewers read, rather than
a framework-only viewer.

**Position.** **Absent.**

**Requirements.**

- `C92-1` `[E]` Framework scheduler, task, frame, and input events emitted to
  existing embedded trace formats and debugger kernel-awareness where
  available.

---

## Part summary

Two concepts here are the framework's own core idea in another domain:
desired-state reconciliation for devices (`C84`) and device data models as
typed state trees (`C85`). They are the strongest evidence that "one
application model, every target" can reach beyond screens into fleets.

The rest are disciplines that make the embedded profiles *shippable* rather
than demonstrable: allocation you can size (`C77`), power you can measure
(`C76`), storage that survives power cuts (`C80`), boot chains you can trust
(`C81`), isolation a safety reviewer accepts (`C78`), and hardware support
claims backed by continuous hardware testing (`C82`). None of them is new; all
of them are what the incumbent embedded stacks already provide, and the price
of being taken seriously there.
