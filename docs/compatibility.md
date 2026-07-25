# Compatibility baseline report

Baseline `native-runtime-compat-v1` was captured on 2026-07-24. It is
machine-readable in [`compatibility.lock`](../compatibility.lock) and remains
**unratified**. Product-owner direction is recorded as `pablof7z`;
compatibility, security, and NMP-boundary review remain unsigned.

## Pinned authorities

| Authority | Pin |
| --- | --- |
| NIP-5D | `nostr-protocol/nips#2303` head `78efc118278e3ed42201eba9b60530b65835d7ed` |
| NAP registry | `napplet/naps@6461e4b37c29dc09a20dff35d9515889c4433874` |
| napplet packages | `napplet/web@b335c40c77f55547f23af81d6d999e2e4e3a3623` |
| Kehto corpus | `kehto/web@bb3929b3523b75356fd65f658f9bd14c7ff697e4` |
| NMP | `pablof7z/nmp@005dc2a5f12aa414961b313d05ebb021934e385c` |

Package versions are `@napplet/core` 0.28.0, `@napplet/shim` 0.26.8,
`@napplet/sdk` 0.24.4, `@napplet/nap` 0.28.0, and
`@napplet/conformance` 0.13.0. The lock records both npm tarball SHA-256 values
and source-tree object IDs so published bytes and repository bytes are
independently pinned.

The NMP checkout at `/Users/pablofernandez/Work/nmp` was inspected read-only.
It was at `68310f88a31bf80e6b73d018b1374e73efda0041` with unresolved/user-owned
changes, so it is not used as reproducible baseline evidence. The clean remote
default-branch commit above is the lock authority. Its Rust facade and UniFFI
component snapshots are separately hashed in the lock.

## Deliberate drift decisions

### Artifact shape

The pinned NIP-5D draft says a napplet is one self-contained `/index.html`.
The pinned `@napplet/vite-plugin` and existing ecosystem tooling accept both
`single-file` and `external-assets`. This baseline deliberately accepts both,
but only when every path hash, the required `/index.html`, and the aggregate
hash verify and every subresource is materialized from verified bytes.

This is compatibility support, not permission to navigate the iframe to a
remote URL or allow runtime network fetches.

### Redirected artifact acquisition

Artifact redirects are supported, but transport-library auto-follow is not.
For each approved source, Rust accepts only 301, 302, 303, 307, or 308 and
follows at most five hops. Every target is parsed again as a credential-free,
query-free, fragment-free HTTPS URL, resolved again, admitted only when every
reported address is public, and connected to only through those approved
addresses while preserving the target hostname for certificate validation and
SNI. Ambient proxy configuration is not used.

Each raw response must report the exact URL requested for that hop. Each
request has a finite byte ceiling and a 15-second default deadline. A public
redirect does not weaken artifact identity: no bytes become retained or
executable until every manifest path SHA-256 and the aggregate hash verify.
Unsafe targets, unapproved redirect statuses, missing locations, a sixth hop,
source confusion, deadline/byte exhaustion, and verification failure are
typed, observable refusals.

### NAP-SHELL

The pinned NAP registry defines `shell.ready` and `shell.init`. The pinned
`@napplet/core` domain union and `@napplet/conformance` envelope validator do
not include a `shell` domain, and the pinned shim explicitly installs no
generic shell object.

The baseline therefore tracks those two envelopes as
`registry-only-handshake`: they need a dedicated compatibility adapter and
validator before they can be advertised. Ordinary domain-object presence
remains the NIP-5D availability signal.

### Registry and package domain breadth

The pinned registry contains NAP-IDENTITY, NAP-INC, NAP-INTENT, NAP-SHELL, and
NAP-THEME. The pinned `@napplet/nap` package exposes 22 domains. Eighteen
package domains therefore have no matching document in the pinned registry:
`relay`, `storage`, `keys`, `media`, `notify`, `config`, `resource`, `cvm`,
`outbox`, `upload`, `ble`, `webrtc`, `link`, `count`, `lists`, `serial`,
`common`, and `dm`.

The executable envelope inventory records all package-active types, but this
does not make the package-only domains ratified protocol. M0 advertises none.
Each future provider needs an explicit compatibility decision, package contract
tests, and platform matrix promotion.

### Conformance depth

Pinned `@napplet/conformance` checks kind/d-tag shape, a hashed `/index.html`,
known `requires`, sandbox/prelude boot, emitted envelope structure,
no-capability degradation, and listener teardown. It does not by itself prove
the event signature, every path hash, aggregate recomputation, duplicate
critical tags, bounded per-hop redirect revalidation, or full external-asset
closure.

The runtime compatibility gate is therefore the pinned suite **plus** the
stronger artifact, malicious-input, and native bridge tests in this repository.
Passing the upstream suite alone is never an installation verdict.

### Principal identity

NIP-5D defines the protocol identity as `(dTag, aggregateHash)`. The runtime
security principal is deliberately stronger:
`(manifestAuthor, dTag, aggregateHash)`. Protocol-visible identity remains
unchanged; the author is an internal grant/storage isolation dimension.

The pinned draft also permits snapshot kind `5129` and root kind `15129` while
forbidding a `d` tag on those kinds. That conflicts with its own mandatory
`(dTag, aggregateHash)` identity definition. The runtime therefore verifies
and caches those signed artifacts, but currently refuses to execute them with
the typed `unsupported_manifest_identity` reason. It does not silently invent
a dTag or let a caller choose one. Enabling those kinds requires an explicit
compatibility decision for a collision-free typed identity scope and all four
baseline signoffs. Named kind `35129` has an unambiguous signed dTag and is the
only executable manifest identity until then.

### NMP facade

The runtime uses NMP's public facade only. Current supported application nouns
are live queries and write intents/receipts, plus identity and diagnostics.
The runtime must not depend on mechanism crates or claim native Android support:
the pinned Kotlin package is desktop JVM, not a qualified Android AAR.

Known public gaps relevant to the product:

- no public receipt enumeration after a process loses an accepted receipt ID;
- Swift/Kotlin receipt consumption has no native observer-detach handle;
- public rows do not expose typed pending intent or receipt IDs;
- Swift/Kotlin omit some Rust configuration ceilings;
- secure standard Keychain/Keystore signer persistence is not shipped;
- scoped evidence must not be collapsed into global completeness.

## Upgrade gate

A baseline change is accepted only with:

1. a dedicated compatibility issue;
2. old/new NIP, NAP, package, corpus, and provider diffs;
3. regenerated source snapshots, envelope inventory, and corpus hashes;
4. an explicit newly-accepted/no-longer-accepted behavior list;
5. migration or dual-support behavior;
6. product, compatibility, security, and NMP-boundary signoff.

Patch or minor releases cannot silently drop a declared baseline.
