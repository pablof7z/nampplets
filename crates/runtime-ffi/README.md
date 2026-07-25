# Runtime FFI boundary

`nmp-native-runtime-ffi` is the production Rust-to-Swift boundary. UniFFI owns
the generated ABI mechanics; application targets import the generated
`NMPNativeRuntime` Swift module and never import the C module directly.

## Authority

- `VerifiedArtifact` is sealed by Rust after Nostr event id/signature,
  coordinate, path digest, aggregate, source, redirect, count, and byte-limit
  verification. Swift cannot construct one.
- Install, installed-library filtering, exact-build uninstall, workspace
  assignment/clear, grant, launch, suspend, resume, revoke, stop, crash, and
  mapped-envelope methods are fire-and-observe commands. Their semantic
  failures appear in bounded runtime snapshots/events; they do not throw FFI
  operation errors.
- `RuntimeInstalledLibrarySnapshot` is the bounded Rust-owned library
  projection. Each row carries an exact `(publisher, dTag, aggregateHash)`
  coordinate, opaque verified manifest metadata, metadata-only versus
  sealed-exact-bytes-ready availability, active session ids, and current
  workspace assignments. Filtering is owned and bounded by `RuntimeApp`;
  native code does not maintain another installation index.
- Suspend and resume act on the session ids projected for an installed build;
  stale handles and illegal transitions are refused by the Rust lifecycle
  state machine.
- Uninstall stops only that exact build's sessions and removes runtime-owned
  installation, grant, component-value, and workspace-assignment state.
  Workspace definitions, activity evidence, retained NMP receipt ids,
  canonical NMP state, and outstanding durable writes are preserved.
  `RuntimeController` releases its in-process verifier handle only after the
  kernel confirms the build is absent. Cached artifact bytes are deliberately
  not deleted because the artifact owner does not yet expose an exact-build
  deletion API.
- The only throwing call is controller construction, where no runtime exists
  yet to own a semantic error state.
- Mapped envelopes accept only a Rust-issued active session id. Principal,
  profile, account, and grant claims in napplet bytes have no authority.
- Verified reads resolve an exact logical path against the sealed artifact for
  the active session. Native filesystem paths never cross the API.
- Catalog artifact acquisition is Rust-owned. Raw HTTP transport auto-follow is
  disabled, and Rust follows only 301/302/303/307/308 through at most five
  manually revalidated hops. Every hop repeats credential-free, query-free
  HTTPS parsing, fresh DNS/public-address admission, approved-address pinning
  under hostname TLS/SNI without ambient proxy, and exact effective-URL checks
  under a finite byte ceiling and per-request deadline.
- The bounded `ArtifactSource` callback still receives
  `redirects_allowed = false`; that is a raw-transport instruction, not a
  blanket runtime redirect policy. It must report a redirect rather than
  following invisibly so Rust can validate or refuse it. No callback or catalog
  response is retained or executed until Rust rechecks source/length, every
  path digest, and the aggregate.
- Theme and settings providers are registered only by constructors that
  receive real native callbacks. Native appearance reports raw OS traits;
  Rust maps and validates NAP-THEME. Native settings receives only bounded,
  validated schema/current values and commits edits back through Rust's
  exact-build config store.

## Permission-review boundary status

`RuntimeController.permission_review` returns one bounded projection for an
installed exact `(publisher, dTag, aggregateHash)` build. The projection comes
from `RuntimeApp` and includes the persisted capability-request inventory,
provider-owned sensitivity/dependencies/platform availability, effective live
or durable decision, Rust-owned requested default, and all four user decision
options with typed validity reasons. A missing provider is explicit `unknown`;
it is never reported as available and only denial is a valid decision.

`RuntimeController.apply_permission_decisions` accepts exactly one complete,
finite decision batch for that exact build. `RuntimeApp` validates the
principal, capability-set equality, duplicates, provider availability,
dependencies, managed-policy ownership, and required-domain consequences.
`GrantLedger` holds its exact-principal write boundary while
`RuntimeStore.set_grants_atomic` commits SQLite; memory changes only after the
store transaction succeeds. Revocation, resource cancellation, provider-push
teardown, and activity facts happen after that combined commit. One
`PermissionBatchApplied` event is the success outcome. The call never launches
the napplet.

`RuntimeController.set_grant` and `RuntimeController.revoke` remain individual
fire-and-observe maintenance commands. Swift must not sequence them to simulate
a review transaction.

The pinned NIP-5D manifest declares required domains only. Production
installation never scans JavaScript or accepts native-authored policy. One
legacy exception is machine-readable in `compatibility.lock`: the unchanged,
published Good Morning exact build predates signed `requires` tags, so Rust
attaches its pinned required identity/INC/outbox and optional
resource/theme/link profile only when author, dTag, and aggregate hash all
match. Launch rederives the same required set from the sealed handle. Any
publisher, dTag, or byte change loses the profile and receives no grant
inheritance.

## Verification

```sh
cargo test -p nmp-native-runtime-ffi
cargo clippy -p nmp-native-runtime-ffi --all-targets -- -D warnings
scripts/tests/test-build-runtime-swift-xcframework.sh
scripts/build-runtime-swift-xcframework.sh --universal --check-bindings
xcodebuildmcp swift-package test \
  --package-path "$PWD/Packages/NMPNativeRuntime" \
  --output text
```

The Swift tests cross the actual ABI for open/snapshot/close, a callback-backed
signed artifact install/launch/read, and conflated observation teardown.
The clean-checkout procedure and exact Rust/Xcode pins are documented in
[`docs/build-from-clean-checkout.md`](../../docs/build-from-clean-checkout.md).
