# Contributing

## Development checks

Run the following before opening a change:

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --no-deps
cargo deny check
cargo audit
```

Changes to `framework-windows` also need verification on a real Windows
runner. Native callback, HWND, COM, menu, dialog, and GDI changes should add
or update an executable integration test where practical.

## Design rules

- Preserve the core/platform dependency boundary.
- Keep unsafe code contained and document the invariant that makes it safe.
- Use component-local keys; platform node IDs are runtime-scoped and opaque.
- Keep async work owned by a task scope and never mutate component state from
  an executor thread.
- Add tests for behavior changes, especially lifecycle and reentrancy cases.
