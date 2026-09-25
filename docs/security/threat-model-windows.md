# Threat model: Windows applications

`PLAN.md` Milestone 51. What a RustNative application on Windows exposes,
who might attack it, and what stops them. Each mitigation names the code
or test that holds it.

## Assets

- The person's data, held in the application's state store and files.
- Their credentials and tokens (`SecureStorage`, Milestone 57).
- The integrity of the application itself: its binary, its updates, and
  the code it loads.

## Entry points and mitigations

| Entry point | Threat | Mitigation |
|---|---|---|
| Deep links (`url-schemes`) and a second launch's arguments | A crafted link drives the application | Links arrive as `Event::OpenUrl`; the application decides; nothing runs from a link by itself (Milestone 30) |
| Files the person opens | A malformed file exploits a parser | Parse untrusted formats in an `IsolatedWorker`: low integrity, a job with a memory cap and no UI access, killed with its owner (`native::isolated`, tested) |
| Network responses | A hostile server or a network attacker | `WinHttp` uses the system trust store; certificate pinning per host (`CertificatePins`, Milestone 47) |
| Updates | A forged update | Ed25519-signed manifests with the key pinned at build; digest checked before anything switches; side-by-side install with rollback (`update`, tested) |
| Third-party capability packages | A package reaching beyond its purpose | Services only through `ScopedServices` and the package's declared grants; HTTP origins and file paths checked at each call (`NotGranted`, tested) |
| Foreign native objects and escape hatches | Code outside the framework's model | Adoption is explicit (Milestone 40) and scoped to a node; nothing is ambient |
| Inspection (`RUSTNATIVE_INSPECT`) | A local process reading or editing state | Off unless the variable is set; loopback only; per-process token (Milestone 44) |
| Crash reports | Personal data in a dump | Reports stay on the machine; sending them is the application's choice, under telemetry consent |
| Telemetry | Silent collection | Off by default; consent recorded; the OTLP exporter sends nothing without it (`docs/telemetry-policy.md`) |

## Out of scope

- A local administrator, or malware already running at the person's
  integrity level: the operating system's boundary, not the application's.
- Physical access to an unlocked session.
