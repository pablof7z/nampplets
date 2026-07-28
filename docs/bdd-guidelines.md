# BDD guidelines

Status: living document, pilot stage. This describes how strict
Given/When/Then scenario coverage is adopted across the NMP native runtime.
It reflects what we know today; expect it to change as more crates and
platform packages adopt scenarios and we learn what actually holds up. When
a rule here stops matching how the codebase actually works, fix the rule,
not just the code.

## Why

Unit tests describe *how* code works. BDD scenarios describe *what the
runtime is contractually supposed to do*, in language a reviewer can check
against the product/security requirements without reading the
implementation. This repository already has strict compatibility and
architecture discipline (see `AGENTS.md`, `docs/adr/`); BDD scenarios are
another artifact in that same spirit — a durable, readable spec that tests
are checked against, not the other way around.

Strict BDD here means: scenarios are real Gherkin (`Given`/`When`/`Then`) in
`.feature` files, executed by step definitions, not just descriptively-named
unit tests. A scenario should be readable and reviewable by someone who has
not read the Rust or Swift implementation.

## Scope and rollout

This is adopted incrementally, crate by crate / package by package, starting
with `crates/runtime-app` (the kernel: sessions, grants, permission review,
provider push, lifecycle). A crate or package is not required to have BDD
coverage until it is explicitly brought into scope; existing `#[test]` /
`Swift Testing` suites remain the primary safety net everywhere until then.

Bringing a new crate/package into scope means:

1. Picking a first slice of already-tested, well-understood behavior (not
   the newest or most speculative code) and writing scenarios for it.
2. Wiring the scenario runner into that crate/package's existing test/build
   commands so `cargo test --workspace` (Rust) or the relevant Xcode
   scheme/`swift test` (Swift) picks it up automatically — no separate gate
   to remember to run.
3. Adding an entry to the table below.

| Crate / package | Status | Runner |
| --- | --- | --- |
| `crates/runtime-app` | Pilot | `cargo test -p nmp-native-runtime-app --test bdd` (also runs under `cargo test --workspace`) |
| `crates/runtime-ffi` | Pilot | `cargo test -p nmp-native-runtime-ffi --test bdd` (also runs under `cargo test --workspace`) |
| `crates/provider-lists` | Pilot | `cargo test -p nmp-native-provider-lists --test bdd` (also runs under `cargo test --workspace`) |
| `crates/performance-harness` | Pilot | `cargo test -p nmp-native-performance-harness --test bdd` (also runs under `cargo test --workspace`) |
| `apps/workbench-macos/RuntimeWorkbenchPackage` | Pilot | Quick/Nimble in `RuntimeWorkbenchFeatureTests` (runs under `swift test` and the shared `RuntimeWorkbench` scheme) |

## Rust: cucumber

- Dependency: [`cucumber`](https://crates.io/crates/cucumber), pinned in
  `[workspace.dependencies]`.
- Each crate in scope gets its own `tests/features/*.feature` directory and
  a `tests/bdd.rs` runner registered as a `harness = false` test target in
  that crate's `Cargo.toml`.
- Step definitions live in `tests/bdd.rs` and drive the crate's existing
  integration-test fixtures — do not build a second, parallel bootstrap
  path. In `crates/runtime-app`, both `tests/kernel_*.rs` (`#[test]`) and
  `tests/bdd.rs` (cucumber) share the exact same `Rig` harness from
  `tests/support/mod.rs`, so a scenario and a unit test exercise identical
  setup and dispatch code. The performance harness follows the same rule:
  its Cucumber scenarios and ordinary `ResourceTracker` exemplar share
  `tests/support/mod.rs`, including the authoritative Python v1 validator
  invocation.
- A scenario is a *port* of already-verified behavior until the pilot
  proves the tooling; do not let scenario coverage get ahead of what is
  independently known to be correct. Expand past that once the approach
  has held up.
- Prefer `Background:` for shared setup across a feature's scenarios over
  repeating `Given` steps.
- Step text should read as product behavior ("the review is scoped to that
  exact build's principal"), not implementation detail ("the
  `PermissionReviewView.principal` field equals..."). The assertion in the
  step definition is where implementation detail belongs.

## Swift: Quick/Nimble

`apps/workbench-macos/RuntimeWorkbenchPackage` owns the first Swift BDD
pilot. Its `Performance/PerformanceEvidenceSpec.swift` owner uses the same
Quick/Nimble target and the single `ApplePerformanceRig` to exercise real
`NativeRuntimeProfile` evidence; release/nonparallel expansion must reuse that
rig rather than add another runner. When another Swift package is brought into
scope:

- Add `Quick` and `Nimble` as SPM dependencies scoped to the test target.
- Use `describe`/`context`/`it` for scenario structure and Nimble matchers
  for assertions.
- Existing `Swift Testing` (`@Test`) and any remaining `XCTest` files
  continue to cover what Quick/Nimble has not yet absorbed; there is no
  requirement to migrate a file just because it is touched incidentally.

## What "strict" does not mean here

- It does not mean every test becomes a scenario. Fast, narrow unit tests
  (parsing, validation, single-function edge cases) stay as `#[test]` /
  `@Test` — BDD is for behavior a reviewer would want to check against a
  requirement, not for exhaustive input coverage.
- It does not mean scenarios replace the existing required gates in
  `AGENTS.md`. They run alongside `cargo test --workspace` and the Apple
  scheme tests, not instead of them.
- It does not mean introducing scenario coverage justifies restructuring
  production code. The pilot in `crates/runtime-app` added a shared test
  fixture module and nothing else outside `tests/`.

## Open questions (revisit as we learn)

- Whether `.feature` files should be considered product-facing
  documentation reviewed by non-engineers, or engineering-only specs. This
  affects how much narrative detail belongs in `Background:` sections.
- Whether cross-crate scenarios (spanning `runtime-app` and `runtime-ffi`,
  for example) need a home, and where.
- Whether the Swift side should mirror the Rust `tests/support` pattern for
  shared fixtures once Quick/Nimble is wired in.
