# Protocol and ecosystem model

## The shortest correct model

```text
Nostr and NIP-5A       signed events, paths, hashes, relays, Blossom
        |
NIP-5D                 proposed web projection and napplet manifest profile
        |
NAP capability seam    what a trusted runtime can do for untrusted code
        |
projection             web postMessage, native IPC/FFI, WASM imports, ...
        |
runtime/product        providers, policy, grants, composition, UX, limits
```

NIP-5D is not the whole Napplets ecosystem. It is currently a proposed web
projection plus a napplet artifact/manifest profile. NAPs own concrete
capability domains. Products decide which NAPs to implement and how to mediate
them.

## Product thesis

A napplet is a small focused application that does one thing well. The host
composes napplets; napplets do not build their own universal shell. A feed,
composer, profile, relay manager, game tool, and media controller are usually
separate napplets.

This decomposition is a product heuristic, not a license to make every button a
separate iframe. Use coarse product surfaces with meaningful lifecycle and
authority boundaries.

## Current proposed web projection

At the observed PR head:

- Napplet code runs in an iframe loaded with `srcdoc`.
- The iframe uses `sandbox="allow-scripts"` without `allow-same-origin`.
- The shell injects `window.napplet` before any napplet-authored classic or
  module script runs.
- Only exposed NAP domain objects appear in the namespace.
- The shell does not expose `window.nostr`.
- Messages are plain objects with a `type` discriminant in
  `domain.action` form.
- Unknown message types are silently ignored for forward compatibility.
- The shell binds each iframe `Window` to its identity and checks
  `MessageEvent.source` on every inbound message.
- `event.origin` cannot identify an opaque-origin napplet; `postMessage` uses
  the `'*'` target origin.

Unknown-message ignore is a compatibility rule. It does not justify silent
provider failures, swallowed quota refusals, or missing diagnostics for known
operations.

## Manifest and artifact model

The current proposal defines napplet-specific kinds:

| Kind | Shape |
| --- | --- |
| `5129` | immutable snapshot |
| `15129` | replaceable root napplet |
| `35129` | addressable named napplet with `d` |

It adopts NIP-5A's `path`, aggregate `x`, `server`, title, description, and
source tag model. A current proposed napplet is one self-contained
`/index.html`. A manifest declares hard capability dependencies with bare
`["requires", "<domain>"]` tags.

Keep three identifiers distinct:

- event identity: Nostr event id or address
- napplet protocol identity: currently `(dTag, aggregateHash)`
- product security principal: often strengthened to
  `(manifestAuthor, dTag, aggregateHash)`

The manifest author's inclusion in a product grant key is prudent runtime
policy, but do not call it NIP-5D protocol identity unless the selected pin says
so.

## Verification and loading

The proposed trust chain is:

1. Resolve the napplet manifest from relays.
2. Verify the Nostr event signature and manifest shape.
3. Fetch every referenced blob, using server hints or other allowed discovery.
4. Verify every blob SHA-256 against its `path` tag.
5. Recompute the aggregate from sorted NIP-5A path lines.
6. Check the aggregate against any manifest `x` tag.
7. Assemble the verified self-contained HTML.
8. Compute/register identity from the verified bytes.
9. Inject trusted bootstrap/CSP outside the signed artifact identity.
10. Load through `srcdoc`, map the iframe window, then execute.

A gateway or cache may accelerate fetching. It is not an identity authority.
Hash-matching bytes do not become less authentic because they came through a
redirect or different host; transport policy still must prevent SSRF,
credential leakage, downgrade, and resource exhaustion.

## Availability and capability contracts

There are two different questions:

- Load-time requirement: does the manifest's `requires` set fit this runtime?
- Runtime availability: is a domain object present for this session?

Hard requirements should refuse before code runs when unavailable. Optional
features test presence and degrade. Presence signals availability, not protocol
version, operation coverage, policy grant, or semantic compatibility.

The NAP registry's `NAP-SHELL` adds a foundational lifecycle and capability
surface. Package and runtime revisions have drifted around `shell.ready`,
`shell.init`, `shell.supports`, and simple domain presence. Verify the chosen
lock before recommending one. Never invent a private readiness handshake.

## What a NAP owns

A NAP owns one runtime-provided API domain:

- valid operations and message types
- request, result, event, and refusal shapes
- correlation and lifecycle
- provider behavior
- limits or semantics that must interoperate
- dependency on other NAP domains

A NAP is transport-neutral when possible. `relay.publish` means the same
contract whether web carries it over `postMessage`, a native host uses IPC, or a
WASM host uses imports.

Higher-level domains should preserve intent. `outbox`, `common`, `lists`,
`count`, and `dm` can let the runtime own routing, signing, encryption, and
replacement semantics. The lower-level `relay` domain remains an escape hatch
for explicitly relay-local work such as a group relay or diagnostic.

## Projection, convention, and archetype

Do not force all cross-component meaning into NAPs.

- A projection binds the same NAP seam to a host environment.
- A convention is a message meaning napplets agree on.
- A NAAT/archetype is a canonical role such as `note`, `profile`, `feed`,
  `composer`, or `dm`.
- `NAP-INTENT` lets a runtime resolve an archetype to a user's selected handler.

The current convention URI shape is
`napplet:<archetype>/<intent>[?params]`. The stable identity excludes the query;
the web binding may transpose unique query fields into a shallow text payload.
Structured data belongs in an explicit payload. Check the current convention
and NAP-INTENT texts before relying on this shape.

## Trust and data ownership

The napplet owns rendering, local transient UI state, and expressed user intent.
It does not own:

- signing keys or raw signer handles
- relay routing and canonical replacement/deletion decisions
- ambient browser or native storage
- unrestricted network or OS capabilities
- another napplet's data
- runtime grants or policy

The runtime owns mediation, policy, bounded provider execution, per-build
storage/grants, lifecycle, diagnostics, and host composition. An underlying
Nostr engine remains the canonical owner of protocol truth when the product
uses one.

## Native projection

Native does not mean executing downloaded native code. A safe native Napplets
runtime can:

- own policy and state machines in a native core;
- render portable HTML/CSS/JS in WKWebView or Android WebView;
- place a trusted local shell between untrusted content and the native bridge;
- translate validated NAP envelopes into typed native provider calls;
- expose native permission, signing, file, media, navigation, and notification
  UX;
- preserve the same NAP contracts while changing the transport below them.

The untrusted frame never sees the native message handler directly. Native
bridges accept opaque session handles created by native code, not napplet-
asserted identity.

An additive host-owned state/actions surface can coexist with legacy napplets,
but it is product or extension work until standardized. Prove unchanged legacy
compatibility before claiming such an extension is supported.
