# Embedded: what we reuse, not rebuild

`PLAN.md` Milestone 52. The embedded Rust ecosystem already has good
executors, hardware abstraction layers, probe tooling, and board
descriptions. The framework consumes them. It never writes a second copy.
This page states that policy, and says which parts are implemented and
which are owed with the embedded backends (Milestone 37 and the embedded
profiles).

## The policy

| Area | What the framework does | What it never does |
|---|---|---|
| Async execution | The core's `Executor` contract is implemented over an existing embedded executor (an `embassy`-style executor) | Ship its own interrupt-driven executor |
| Peripherals | Takes `embedded-hal` traits as inputs: a display is a `DrawTarget` or an SPI device, and input is a `digital::InputPin` | Define its own GPIO, SPI, or I2C traits |
| Probing and flashing | `rustnative` calls `probe-rs` for flashing and RTT logging | Implement a debug probe protocol |
| Board descriptions (`C73`) | Reads an existing hardware description (a board's device tree or its crate's `memory.x`) to derive memory, display, and input configuration | Keep its own board database |
| RTOS (`C79`) | Provides an executor and clock adapter for a standard RTOS interface (Zephyr's, through its Rust bindings), and maps capabilities from the RTOS's configuration | Wrap the RTOS in a framework-specific layer |

A framework crate that needs something in this table depends on the
ecosystem crate. Where no suitable crate exists, the framework proposes
the missing piece upstream before carrying a local one, and a local one
is deleted when upstream lands.

## Status

- **Implemented now**
  - The core's `Executor` and `Clock` contracts.
  - The capability model that an RTOS configuration maps into.
  - The headless backend, which runs the core with no operating-system
    services at all.
- **Owed with Milestone 37 and the embedded profiles**
  - The executor adapter demonstrated on hardware.
  - Consuming `embedded-hal` peripherals.
  - `probe-rs` integration.
  - Board metadata consumption.
  - The Zephyr adapter.
  - The device half of the span example: `examples/span` runs on Windows
    and headless today. This repository has no device target yet, so
    running on one is not claimed.
