# Threat model

## Scope and assets

The protected assets are signing authority, decrypted or authenticated Nostr
data, exact user approvals, runtime/NMP store integrity, canonical Nostr state,
component-scoped storage, workspace state, verified executable bytes, native OS
capabilities, and availability of unrelated runtime sessions.

Trust boundaries are:

1. untrusted manifest, relay, gateway, and blob inputs -> artifact verifier;
2. verified but untrusted napplet bytes -> sandboxed iframe;
3. sandboxed iframe -> trusted local shell through `postMessage`;
4. trusted shell -> native bridge using an opaque native-created session;
5. runtime policy -> NMP public facade and native capability providers;
6. runtime store -> NMP canonical store, which remains a separate authority.

The model assumes napplet code, publisher metadata, relays, gateways, blob
servers, and component updates may be malicious. It does not claim protection
from a compromised OS, WebView engine, native host, or all timing side channels.

## Security invariants

- Executed bytes are exactly verified artifact bytes plus the runtime-owned
  compatibility prelude.
- The iframe is `sandbox="allow-scripts"` without `allow-same-origin`.
- The iframe receives no native bridge, `window.nostr`, key material, raw
  signer, ambient network, sibling storage, or unrestricted native API.
- Inbound authority derives from `MessageEvent.source` and native session state,
  never caller-supplied identity, origin, principal, or session fields.
- Grants and component storage bind to
  `(manifestAuthor, dTag, aggregateHash)`.
- Unknown message types are ignored; known messages are validated before
  provider dispatch.
- NMP remains the only canonical Nostr state and durable-write owner.
- Artifact redirects are followed only by the Rust policy loop, with the same
  URL, DNS, address, TLS/SNI, effective-URL, byte, and deadline checks repeated
  independently for every hop.
- Every resource class is finite, refuses observably, and tears down
  deterministically.

## Threats and mitigations

| Threat | Required mitigation | Falsifier |
| --- | --- | --- |
| Manifest signature or tag forgery | verify signed event and pinned kinds before cache/execute | invalid signature/tag fixture is rejected |
| Blob substitution | SHA-256 every path and recompute aggregate | one-byte mutation fails baseline hash gate |
| Gateway or redirect injection | gateway is untrusted; raw transport auto-follow is disabled; Rust follows only 301/302/303/307/308 through at most five manually revalidated, credential-free and query-free HTTPS hops | a public redirect reaches verified bytes, while an unsafe target, sixth hop, unapproved status, or inexact effective URL is refused observably before retention |
| DNS rebinding or proxy confusion during acquisition | resolve each hop afresh, admit only public addresses, pin the connection to those addresses under the requested host's TLS/SNI, and use no ambient proxy | a hop resolving to a non-public address or connecting/reporting a different effective URL is refused |
| Remote subresource escape | private verified materialization plus deny-by-default CSP | fetch, WebSocket, and remote asset fixture is denied |
| Bridge discovery from iframe | bridge only in trusted top-level shell | iframe global inspection finds no bridge |
| Source-window spoofing | exact `MessageEvent.source` mapping | sibling iframe's valid privileged envelope is dropped |
| Caller-supplied principal spoofing | bridge discards payload identity fields | spoofed session/principal has no effect |
| Ambient signing | no `window.nostr`; native exact-draft approval; NMP freezes author/body | key/signer inspection and account-switch write tests |
| Cross-build grant/storage reuse | exact-build principal | build B cannot read A or inherit sensitive grants |
| Malicious signed update | show hash/capability diff; explicit grant carry; rollback | new aggregate starts without sensitive grants |
| Capability escalation | inject only granted domain objects; session profile fixed | renderer outbox request reaches no provider |
| Private data leakage from shared NMP cache | provider output filtered by principal and access context | ungranted authenticated rows never cross boundary |
| Second Nostr truth | no runtime event/replacement/deletion/write cache | dependency/doctrine audit |
| Flood/starvation | per-session and global byte/rate/concurrency limits | offender is refused while sibling remains responsive |
| Slow consumer backlog | newest snapshot or bounded composed transition | memory does not scale with skipped revisions |
| WebView crash loop | invalidate only session; bounded reload; preserve binding/NMP/write | repeated crash stops reload and resources return |
| Log disclosure | redact payload content, keys, ciphertext/plaintext, tokens, and URLs with credentials | secret-pattern scan and activity-schema tests |

## Resource classes

M0 ratifies the existence, not final measured values, of finite ceilings for
envelope bytes, rate/burst, active requests, subscriptions, filters/authors/IDs/
tags, state-frame bytes, collection windows, storage bytes/keys, resource
streams/bytes, uploads, devices/media, WebViews, CPU unresponsiveness, and
global/per-principal work. A provider cannot ship until concrete values and
typed refusal facts are present in its contract tests and diagnostics.

Silent truncation is forbidden. Refusal must identify the exhausted class
without exposing unrelated activity.

## Sensitive lifecycle

- Revocation cancels active non-durable provider work immediately.
- Teardown closes callbacks, subscriptions, streams, and session mappings.
- A durable NMP write is transferred to NMP ownership before its originating
  component may disappear; destroying the WebView does not cancel it.
- Account changes cannot retarget an accepted write.
- Reset closes runtime and NMP before destructive store operations and clearly
  separates runtime component data from canonical NMP data and account vaults.

## Review gate

Security ratification requires release-build proof of artifact verification,
CSP/network denial, source binding, bridge absence, storage/grant isolation,
revocation, overload isolation, crash recovery, and teardown counters. A clean
compile or debug-only test is insufficient.
