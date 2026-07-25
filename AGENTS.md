# NMP Native Runtime repository rules

These rules apply to the whole repository.

## Product boundary

- This repository is a native application runtime built above NMP.
- `/Users/pablofernandez/Work/nmp` is reference-only. Never edit, stage, reset,
  clean, stash, commit, or otherwise mutate that checkout from this repository.
- Consume only the supported NMP facade (`nmp::Engine`, the `NMP` Swift package,
  or the supported Kotlin wrapper). Mechanism crates and generated UniFFI types
  are not application APIs.
- Dependency direction is one way: platform/app -> runtime packages -> NMP
  facade. NMP never depends on this runtime.

## Ownership and architecture

- Rust owns product state machines, policy, validation, lifecycle, persistence,
  routing decisions, compatibility decisions, limits, and error semantics.
- Native code owns rendering, accessibility, platform lifecycle integration,
  and bounded execution of OS capabilities. It reports raw results to Rust.
- NMP is the only canonical Nostr event, replacement, deletion, routing, signer,
  pending-row, write-intent, and receipt owner.
- Runtime persistence may own installs, exact-build grants, component KV,
  workspaces, artifact indexes, and bounded activity facts. It must never become
  a second Nostr truth.
- The untrusted WebView iframe never receives the native bridge, `window.nostr`,
  key material, raw signer objects, unrestricted storage, or direct network.
- Every queue, stream, subscription, message, state frame, and resource class
  has a finite limit and observable refusal. Polling and sleep-check loops are
  prohibited.

## Code size discipline

- Keep source files at or under 300 lines; 600 lines is a hard ceiling.
  Split a file once it passes the soft limit instead of continuing to grow
  it in place.
- A file already past 600 lines is not license to keep piling onto it: put
  new code in a new module/file, and any nontrivial change touching such a
  file should shrink it (extract, split) rather than add net lines.
- This is enforced as a one-way ratchet, not just a guideline: a file at or
  over 600 lines must not grow versus its prior committed size (it may shrink
  freely, even while staying over the ceiling), and a file under 600 lines
  must not cross it. `scripts/ci/check_file_growth.py` implements the check;
  it runs in CI (`file-growth` job) and locally via the pre-commit hook
  installed by `scripts/setup-git-hooks.sh`.

## Workstream boundaries

- `conformance/`, `docs/`, and `compatibility.lock` own the pinned compatibility
  and security contract.
- `crates/artifact` owns verified artifact resolution and immutable bytes.
- `crates/runtime-core` owns principals, grants, sessions, quotas, and lifecycle.
- `crates/nap-bridge` owns NAP envelopes and provider dispatch.
- `crates/runtime-store` owns non-Nostr runtime persistence.
- `crates/surface` owns private surface descriptors, bindings, revisions, and
  typed actions.
- `crates/test-harness` owns deterministic service implementations; scenario
  contracts live under `conformance/test-services`.
- `apps/workbench-macos` owns the macOS reference shell and native presentation.
- `apps/workbench-ios` owns the iOS reference shell and native presentation; it
  shares `RuntimeWorkbenchFeature` and `platforms/apple` with the macOS shell
  rather than redefining them.

Do not redefine another workstream's public envelope, principal, persistence
schema, or lifecycle state machine. Coordinate changes at the owning boundary.

## Compatibility discipline

- `compatibility.lock` is authoritative. Upstream movement requires a dedicated
  compatibility change, regenerated fixtures and inventories, a new report, and
  explicit owner/security/NMP signoff.
- Existing accepted napplets must run without source or build changes.
- Unsupported domains are absent, not simulated by placeholder providers.
- Unknown well-formed message types are ignored at the compatibility boundary.
- Grants bind to `(manifest author, dTag, aggregateHash)`.
- Legacy compatibility becomes green before the private surface extension may
  be described as supported.

## Behavior scenarios

- `docs/bdd-guidelines.md` is the living spec for strict Given/When/Then
  scenario coverage: which crates/packages are in scope, tooling
  (`cucumber` for Rust, Quick/Nimble for Swift), and how scenarios relate
  to the existing test suites. Read it before adding or changing scenario
  coverage; harden it as adoption expands rather than treating it as
  settled.

## Git workflow

- Always make changes in a dedicated git worktree, never in the base checkout.
- Keep the base checkout's `main` synced with `origin/main` (fetch and
  fast-forward) instead of letting it linger stale. Other agents merge PRs
  concurrently; a stale local `main` produces false diffs, stale build
  artifacts, and misleading "what's on main" answers.
- Always open a pull request for the change; never push or merge directly into
  `main` from the base checkout.
- Agents MUST clean up their worktree immediately once they are done with it
  and the pull request has been published (even if it is still awaiting CI or
  review) — do not hold a worktree open "just in case" while a PR sits
  unmerged. Do not leave finished worktrees lying around.

## Required gates

Run the narrow gate while iterating, then all applicable gates before handoff:

```sh
python3 -m unittest discover -s conformance/tests -p 'test_*.py'
python3 conformance/scripts/verify_baseline.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For Apple changes, build and test the shared `RuntimeWorkbench` scheme in the
macOS destination, and build the `RuntimeWorkbenchiOS` scheme in an iOS
Simulator destination.
