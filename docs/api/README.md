# The API reference and the machine-readable description

`PLAN.md` Milestone 52. Both are generated from the source, so neither can
say something the code does not.

## The reference

```sh
cargo doc --workspace --no-deps --open
```

- **Every public item is documented.** Each crate denies missing
  documentation. The verification gate builds the reference with warnings
  denied, so a broken link or an undocumented item fails the build.
- **Examples in the reference compile.** They are doctests, run with the
  test suite. `compile_fail` examples show what the compiler refuses.

## The machine-readable description

```sh
rustnative describe --json > framework.json
```

`docs/api/framework.json` is its committed output. A code-generating tool
should read it before writing code for this framework. It lists:

| Key | What |
|---|---|
| `elements` | Every markup element: the builder constructor it lowers to, whether it takes children, and each attribute with its kind, whether it is required, and the builder method it calls |
| `utilities` | Every utility class the default vocabulary knows (one example per spacing and sizing scale), with the style properties it sets |
| `variants` | The variant prefixes (`hover:`, `dark:`, `md:`, …) |
| `capabilities` | Every `Capability` a backend can answer |
| `events` | Every `Event` variant |
| `services` | What an application registers on `Services` |
| `component` | The `Component` contract and its rules |
| `layout` | The layout semantics |

- **Staleness.** `crates/rustnative/tests/describe.rs` fails when the
  committed file differs from what the tool generates. Regenerate it with
  the command above.
- **Events.** A test checks the event list against the `Event` enum's
  source.
