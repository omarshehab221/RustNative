# Certification posture

`PLAN.md` Milestone 51. What a team preparing a certification (a security
review, SOC 2 evidence, an accessibility conformance report, a medical or
industrial audit) gets from the framework, and what stays theirs.

| Evidence | Where it comes from |
|---|---|
| Software bill of materials (CycloneDX 1.5) | `rustnative compliance` → `sbom.cdx.json` |
| License inventory | `licenses.md`, `dependency-inventory.json`; `cargo deny` enforces the allowed set |
| Accessibility conformance | The Milestone 41 conformance suites; the application's own suite in `accessibility-report.json` |
| Privacy declarations | `privacy-manifest.json`, `permissions.md` |
| Requirement traceability | `traceability.json`: `// req: ID` annotations → the tests that verify them |
| Threat models | `docs/security/threat-model-*.md` |
| Secure development | The verification gate: formatting, pedantic lints with `unsafe` justifications, the full test suite, documentation, MSRV, and dependency policy on every change |

What stays the applicant's: their own threat model, their deployment's
controls, penetration testing, and the certification body's process. The
framework's evidence is generated, never hand-maintained, so it cannot
drift from the build it describes.
