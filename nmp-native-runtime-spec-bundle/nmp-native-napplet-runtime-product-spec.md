# NMP Native Napplet Runtime

## Product, Architecture, Delivery, and Verification Specification

**Working product name:** NMP Native Runtime  
**Working repository name:** `nmp-native-runtime`  
**Document status:** Draft for implementation  
**Specification version:** 0.1  
**Date:** 2026-07-23  
**Primary audience:** implementation agents, reviewers, product owner, security reviewers, NMP maintainers, napplet ecosystem maintainers

---

## 0. Executive decision

Build a native application runtime that:

1. runs the runtime core, policy engine, lifecycle, state ownership, and Nostr machinery natively;
2. uses NMP as its canonical Nostr sync, routing, storage, signing, publication, and diagnostics engine;
3. renders portable user-facing modules in sandboxed WebViews;
4. runs existing NIP-5D napplets without source or build changes; and
5. adds an optional **surface profile** in which WebView modules render host-owned state and emit typed actions.

The product is not “Kehto with NMP swapped in.” Its new capability is:

> A native Nostr application can install, replace, and compose portable WebView product surfaces while retaining one native application model, one canonical NMP state substrate, one permission boundary, and one durable write lifecycle.

Existing napplets remain first-class. New surface components are additive and remain valid NIP-5D napplets.

---

## 1. Product definition

### 1.1 What the product is

The NMP Native Runtime is a native host/runtime for composable Nostr applications. It provides:

- a native Rust runtime core;
- native Apple and, later, Android host integrations;
- a secure NIP-5D WebView projection for existing napplets;
- a capability provider registry for NAP domains;
- an NMP adapter for live queries, write intents, identity, evidence, and diagnostics;
- a native workspace model that composes native screens and WebView surfaces;
- install, verification, permissions, updates, storage, activity inspection, and lifecycle management;
- an optional state-down/actions-up surface protocol for replaceable UI modules.

The runtime is a sibling product built **above** NMP. It consumes NMP’s public facade. It does not turn NMP Core into an application framework, add a third NMP noun, or make WebView lifecycle part of NMP’s engine contract.

### 1.2 What “native” means

The following execute natively:

- runtime session management;
- capability policy and grants;
- artifact verification and caching;
- lifecycle and resource accounting;
- binding and action routing;
- application state and workspace composition;
- NMP and its Nostr network/store/signing machinery;
- native permission, signing, file, media, notification, and navigation UX.

Napplet and surface presentation code still executes as JavaScript/HTML/CSS inside WebKit or Android WebView. The product is therefore a **native runtime with web-rendered modules**, not a compiler that converts web code into SwiftUI or Compose.

### 1.3 The new capability

Existing napplets ask the runtime to perform operations:

```text
napplet -> query / subscribe / publish / upload -> runtime
```

A surface component may instead receive host-owned state and emit typed intent:

```text
native binding -> revisioned state -> surface component
surface component -> typed action -> native action router
```

This allows the host to preserve state and NMP demand while changing renderers. A feed renderer can be replaced without reopening the feed’s NMP query. A native view and several WebView surfaces can consume the same binding. A composer can be destroyed while an NMP write intent continues and updates every consumer through canonical state.

---

## 2. Source and compatibility baseline

This document is based on the following project state as observed on 2026-07-23:

- `pablof7z/nmp`, especially `README.md`, `docs/VISION.md`, `docs/known-gaps.md`, and `docs/design/ui-components-strategy.md`;
- `napplet/naps`, especially the registry, NAP-SHELL, and the web projection;
- the open NIP-5D proposal and its NIP-5A-derived napplet manifest model;
- `napplet/web`, especially `@napplet/shim`, `@napplet/nap`, and `@napplet/conformance`;
- `kehto/web`, especially its runtime/shell separation, capability enforcement, and playground corpus;
- the supplied napplet transcript, particularly the operating-system analogy, native-runtime direction, and requirement that the host mediate sensitive operations.

NIP-5D and several NAPs are moving drafts. Therefore the first implementation task is not “follow main forever.” It is to establish a pinned, executable compatibility baseline.

### 2.1 Compatibility lock

The repository MUST contain a machine-readable `compatibility.lock` recording:

- NIP-5D source commit;
- NAP registry source commit;
- `@napplet/core` version;
- `@napplet/shim` version;
- `@napplet/sdk` version;
- `@napplet/nap` version;
- `@napplet/conformance` version;
- Kehto compatibility-corpus commit;
- NMP commit/version;
- supported manifest kinds and artifact modes;
- supported NAP domains and protocol versions by platform.

No baseline dependency may move without a dedicated compatibility issue, updated fixtures, a compatibility report, and explicit owner signoff.

---

## 3. Hard product invariants

The following are non-negotiable.

### I-01 — Existing napplets run unchanged

A napplet conforming to the pinned compatibility baseline MUST run without source changes, rebuild changes, runtime-specific imports, or a custom manifest.

This includes napplets that:

- use `window.napplet.*` directly;
- use `@napplet/sdk`;
- use domain-specific `@napplet/nap/*/sdk` helpers;
- rely on runtime injection before classic or module scripts execute;
- use currently accepted single-file or external-asset artifact shapes;
- declare only standard `requires` domains.

The ordinary capability rule still applies: a napplet whose required domain is unavailable on the target platform is rejected before code executes with a precise reason. It is never loaded into a partially compatible environment.

### I-02 — NIP-5D remains intact

The runtime MUST preserve the pinned NIP-5D web-projection behavior:

- sandboxed iframe;
- `sandbox="allow-scripts"` baseline;
- no `allow-same-origin`;
- `postMessage` JSON envelopes;
- source-window identity binding;
- verified manifest and artifact bytes;
- runtime-injected `window.napplet` before napplet scripts;
- no `window.nostr`;
- unknown message types ignored for forward compatibility;
- sensitive operations mediated by the runtime.

The native bridge is below the trusted web shell and is invisible to napplet code.

### I-03 — NMP Core remains an engine

The runtime uses only NMP’s supported public facade. NMP owns live queries, write intents, canonical Nostr state, routing, signing, durable publication, and diagnostics. The runtime owns application architecture, WebViews, permissions, workspace state, bindings, and action routing.

### I-04 — One canonical Nostr truth

The runtime MUST NOT maintain a second authoritative Nostr event cache, replacement winner table, deletion model, or write state machine. NMP remains canonical for supported Nostr state.

Runtime storage may contain component installation state, grants, component KV, workspace definitions, and derived presentation caches that can be discarded and rebuilt.

### I-05 — State authority stays native in surface mode

A renderer-profile surface MUST NOT become the authoritative owner of feed contents, account state, publication state, routing state, or protocol truth. It receives immutable or revisioned state and emits typed actions.

### I-06 — No ambient privilege

A napplet receives only the NAP domain objects granted to its exact runtime principal. It never receives keys, raw signer objects, unrestricted native APIs, direct runtime database access, or an unrestricted bridge.

### I-07 — Permissions bind to exact code

Security grants bind internally to:

```text
(manifest author, dTag, aggregateHash)
```

The NIP-5D protocol identity remains whatever the pinned spec requires; the stronger internal principal prevents grants from silently crossing publishers or builds. A new aggregate hash is a new executable principal.

### I-08 — Boundedness is structural

Every session-owned queue, message, subscription, state frame, storage scope, resource stream, and provider concurrency class has a finite, explicit limit and an observable refusal path. Silent truncation is forbidden.

### I-09 — Scoped evidence is not global truth

Surface state and compatibility providers MUST preserve NMP’s scoped evidence semantics. The runtime never invents a global `synced`, `complete`, or “authoritative empty” claim.

### I-10 — Renderer replacement preserves application state

Replacing a surface renderer in a live workspace slot MUST NOT implicitly close and reopen the slot’s NMP demand, lose canonical state, retarget a write, or reset the host’s navigation state.

---

## 4. Goals and success criteria

### 4.1 Product goals

1. Run the existing napplet ecosystem inside a native host.
2. Let native applications install long-tail features without shipping a new native binary for each feature.
3. Let users replace product surfaces independently of the native application substrate.
4. Share one NMP engine, canonical cache, relay plan, signer system, and durable outbox across native and WebView surfaces.
5. Keep untrusted web code behind a small, auditable, capability-mediated boundary.
6. Provide a practical macOS reference product and reusable runtime packages.
7. Define an additive surface model that can later be proposed upstream without making existing NIP-5D napplets obsolete.

### 4.2 Measurable success criteria

The desktop release is successful when:

- 100% of the accepted compatibility corpus runs without source or build changes;
- the pinned `@napplet/conformance` suite passes;
- all advertised NAP domains pass contract tests and no unimplemented domain is advertised;
- an existing legacy feed napplet, profile napplet, and composer napplet run in the native host;
- two surface feed renderers can swap over the same live host binding without reopening NMP demand;
- a native SwiftUI view and a surface component observe the same canonical update;
- a publication accepted through a component survives component destruction and application restart through an NMP receipt;
- a malicious napplet cannot access the native bridge, keys, ungranted domains, direct network, or another napplet’s storage;
- repeated mount/unmount, renderer swaps, and WebView crashes return runtime resources to a measured baseline;
- offline startup renders cached artifacts and NMP rows without falsely claiming network completeness;
- component activity, grants, state bindings, and NMP evidence are inspectable by the user or developer.

---

## 5. Non-goals

The first product does not attempt to:

- translate arbitrary HTML into SwiftUI or Compose;
- download and execute arbitrary Swift, Kotlin, Rust, dynamic libraries, or native plug-ins;
- dynamically install new Rust protocol modules into NMP;
- define a universal cross-platform pixel/layout IR;
- replace NMP’s native UI component ecosystem;
- make NMP Core own navigation, component manifests, WebViews, or renderer catalogs;
- implement a general desktop operating system;
- create a mandatory centralized app store;
- expose raw NMP mechanism crates to napplets;
- support mutually untrusted OS users in one NMP engine/store;
- promise every device-specific NAP on every platform where the underlying OS does not permit it;
- standardize the surface extension upstream before a working implementation and falsifier exist.

A future WASM service/runtime or dynamic protocol cartridge system may be explored separately after this product proves the native-hosted WebView model.

---

## 6. Users and primary journeys

### 6.1 End user

The user wants to assemble a Nostr client from trusted native infrastructure and independently authored features.

Primary journeys:

- install a napplet from a Nostr coordinate or discovery result;
- review its publisher, exact build hash, required capabilities, and requested permissions;
- launch it in a native window or workspace pane;
- choose which feed, profile, thread, composer, or tool handles a role;
- replace one renderer without losing current state;
- approve signing, uploads, device access, or external links through native UI;
- inspect what a component has read, requested, published, uploaded, or retained;
- revoke permissions, stop a component, roll back an update, or uninstall it.

### 6.2 Existing napplet developer

The developer wants their current napplet to run in the native runtime without changing it.

The runtime must accept the same manifest, artifact, SDK/global surface, and NAP envelopes as the pinned web baseline.

### 6.3 Surface component developer

The developer wants to build a portable renderer/controller that:

- remains a standard NIP-5D napplet;
- optionally declares a surface descriptor;
- receives typed host state;
- emits typed actions;
- can provide a legacy fallback when the surface domain is absent;
- can be tested against a reference surface harness.

### 6.4 Native host developer

The developer wants to embed the runtime into an existing NMP application or use the reference workspace app. They need:

- a native runtime API;
- a WebView host component;
- capability provider registration;
- binding and action handler registration;
- permission and activity UI primitives;
- deterministic conformance fixtures.

---

## 7. Reference application

The first complete product proof is a macOS application called **Runtime Workbench**.

```text
┌───────────────────────────────────────────────────────────────────┐
│ Native toolbar: account · search · install · activity · settings │
├──────────────┬──────────────────────────────┬─────────────────────┤
│ Native       │ Feed slot                    │ Detail slot         │
│ sidebar      │                              │                     │
│              │ native or WebView surface    │ profile/thread      │
│ Home         │                              │ native or WebView   │
│ Messages     │                              │                     │
│ Groups       ├──────────────────────────────┴─────────────────────┤
│ Streams      │ Composer slot — native, legacy napplet, or surface │
│ Tools        │                                                     │
├──────────────┴─────────────────────────────────────────────────────┤
│ Native diagnostics/activity drawer                                │
└───────────────────────────────────────────────────────────────────┘
```

The reference app demonstrates:

- native account and signer UX;
- existing napplets in ordinary compatibility windows;
- surface components in feed/detail/composer slots;
- native and web renderers over shared NMP state;
- runtime installation and update flows;
- NMP source evidence and durable receipt inspection;
- renderer switching without data restart;
- offline launch and recovery.

The workspace is intentionally coarse-grained. A WebView hosts a feed, profile panel, thread, composer, editor, player, or tool—not one avatar, button, or list row.

---

## 8. System architecture

### 8.1 Logical architecture

```text
Native application
│
├── Native product shell
│   ├── workspace / navigation / slots
│   ├── account and signer UX
│   ├── permission and approval UI
│   ├── native notifications / files / devices / media
│   └── activity and diagnostics UI
│
├── Native Runtime Core (Rust)
│   ├── installation and artifact registry
│   ├── principals, sessions, grants, quotas
│   ├── lifecycle and resource accounting
│   ├── NAP provider registry
│   ├── surface descriptors, bindings, action router
│   ├── runtime persistence
│   └── compatibility/version policy
│
├── NMP Adapter (Rust, public facade only)
│   ├── live queries and evidence
│   ├── write intents and receipts
│   ├── accounts / auth / signers
│   ├── protocol resources
│   └── diagnostics
│
├── Web Projection Adapter
│   ├── platform WebView wrapper
│   ├── trusted local shell document
│   ├── source-window -> session mapping
│   ├── pinned @napplet shim/prelude behavior
│   ├── postMessage <-> native bridge translation
│   └── sandboxed napplet iframe
│
└── External systems
    ├── Nostr relays and signers
    ├── Blossom / permitted resource rails
    └── OS services
```

### 8.2 Process and trust boundaries

| Component | Trust level | May hold keys? | May access network directly? | May authorize actions? |
|---|---:|---:|---:|---:|
| Native application | Trusted | Through approved providers | Yes, policy-bound | Yes |
| Runtime Core | Trusted | References/providers only | Through owned providers | Yes |
| NMP | Trusted subsystem | Through signer abstractions | Relays/signers | No user-facing approval UI |
| Trusted WebView shell | Minimal trusted adapter | No | No arbitrary network | No; forwards to native |
| Napplet iframe | Untrusted | Never | No | Never |
| Relay/gateway/blob source | Untrusted external | No | N/A | No |

### 8.3 Platform shape

#### Apple

- Rust runtime core and NMP linked through the supported FFI/Swift package path.
- SwiftUI/AppKit owns windows, navigation, approval UI, and `WKWebView` instances.
- A local trusted shell document is loaded into each coarse surface WebView.
- The shell creates one sandboxed napplet iframe and maps its `Window` to a native-created opaque session token.
- A single narrow script-message channel connects the trusted shell to native code.

#### Android

- Same Rust runtime core and compatibility fixtures.
- Kotlin/Compose owns the application and Android WebView.
- The bridge uses origin-scoped web messaging rather than a globally exposed JavaScript interface.
- Android work begins only after the required NMP Android packaging/runtime surface is qualified.

### 8.4 Repository shape

Recommended layout:

```text
nmp-native-runtime/
├── crates/
│   ├── runtime-core/          # principals, sessions, policy, lifecycle
│   ├── artifact/              # manifest resolution, hashes, CAS
│   ├── nap-bridge/            # envelope types, validators, provider registry
│   ├── nmp-adapter/           # public NMP facade integration
│   ├── surface/               # descriptor, bindings, actions, revisions
│   ├── runtime-store/         # grants, installs, workspace metadata, KV
│   └── test-harness/          # mock relays, fixtures, fault injection
├── web/
│   ├── trusted-shell/         # tiny bundled shell, source binding, bridge
│   ├── compatibility-fixtures/
│   └── surface-sdk/           # optional developer SDK and reference harness
├── platforms/
│   ├── apple/                 # Swift package and macOS/iOS hosts
│   └── android/               # Kotlin package and Android host
├── apps/
│   └── workbench-macos/
├── conformance/
│   ├── compatibility.lock
│   ├── napplet-corpus/
│   ├── bdd/
│   └── reports/
├── docs/
│   ├── adr/
│   ├── threat-model.md
│   ├── compatibility.md
│   └── provider-matrix.md
└── AGENTS.md
```

The runtime MAY begin inside the NMP repository for convenience, but dependency direction must remain one-way and independently enforceable. A separate repository is preferred.

---

## 9. Execution profiles

### 9.1 Legacy profile

The runtime loads an existing napplet exactly as a conformant NIP-5D shell would.

Characteristics:

- no surface descriptor required;
- current `window.napplet` domain APIs;
- napplet owns its local UI orchestration;
- data access occurs through granted NAPs;
- NAP-INTENT and NAP-INC remain available when supported;
- compatible with existing artifacts and SDKs.

### 9.2 Surface hybrid profile

The napplet remains able to call granted ordinary NAPs, and additionally receives the `surface` domain.

Use cases:

- an editor receives host context but performs specialized resource operations;
- a stream surface receives native selection and uses media/session capabilities;
- a complex tool combines host state with its own scoped queries.

### 9.3 Surface renderer profile

The component receives host-owned state and safe utility capabilities, but does not receive relay/outbox/query capabilities.

Typical available domains:

- `shell`;
- `surface`;
- `storage`;
- `theme`;
- `keys`;
- `resource` under explicit policy;
- `link`;
- `media` or `notify` when explicitly granted.

This profile is the cleanest proof of the new product model. The component renders; the host owns application data and Nostr behavior.

### 9.4 Profile selection

- If no surface descriptor exists, load in legacy profile.
- If a descriptor exists and `surface` is supported, honor the declared profile subject to host policy.
- If a descriptor declares a legacy fallback and `surface` is unavailable, load in legacy profile.
- During the private v0 phase, descriptor `fallback` controls behavior when `surface` is unavailable: use legacy mode, show an unavailable state, or reject in this runtime before execution. The component must still feature-detect `window.napplet.surface` because ordinary shells will ignore the private descriptor.
- The profile is fixed for a session. It cannot be escalated by a napplet message.

---

## 10. Existing napplet compatibility contract

### 10.1 Artifact compatibility

The runtime MUST support every artifact form accepted by the pinned compatibility baseline, including:

- pinned NIP-5D manifest kinds;
- signed manifest verification;
- `d`, `path`, `x`, `server`, title, description, source, and `requires` behavior as defined by the baseline;
- snapshot and addressable identities where applicable;
- aggregate-hash recomputation;
- per-blob SHA-256 verification;
- a hashed `/index.html`;
- current single-file builds;
- current external-asset builds accepted by the SDK/conformance baseline.

The runtime MUST reject:

- invalid event signatures;
- unsupported manifest kinds;
- missing or malformed index path;
- path hash mismatch;
- aggregate mismatch;
- duplicate or ambiguous critical tags where the baseline forbids them;
- a gateway response that differs from the signed manifest;
- unverified code or assets.

### 10.2 Artifact materialization

The runtime never navigates a napplet iframe to an arbitrary remote URL.

For a single-file artifact:

- verify bytes;
- inject the trusted prelude outside the signed artifact identity;
- execute through `srcdoc` according to the pinned NIP-5D behavior.

For a multi-file artifact:

- fetch and verify every referenced path;
- store immutable bytes in the local content-addressed artifact cache;
- expose them through a private, non-networked runtime scheme or equivalent verified materialization;
- ensure relative scripts, styles, workers, fonts, and media resolve only to verified artifact bytes;
- apply a deny-by-default CSP for external network access.

Redirects enter the executable artifact graph only after every hop is manually
revalidated by the Rust acquisition policy and the final bytes pass the signed
per-path and aggregate checks. Runtime fetches remain outside that graph.

### 10.3 Web projection compatibility

The trusted shell MUST:

- create the iframe;
- set `sandbox="allow-scripts"` and omit `allow-same-origin`;
- assign the verified principal before execution;
- inject selected `window.napplet.<domain>` objects before napplet scripts;
- support both domain-presence discovery and the pinned NAP-SHELL handshake behavior where the ecosystem currently contains both;
- deliver and receive JSON `domain.action` envelopes via `postMessage`;
- map inbound messages by `MessageEvent.source`, never by caller-supplied identity or `event.origin`;
- silently drop unknown source windows;
- silently ignore unrecognized message types at the compatibility boundary;
- never inject `window.nostr`;
- prevent direct access to local/session storage, IndexedDB, service workers, raw WebSocket, and arbitrary network fetch;
- clean up all session subscriptions and callbacks when the iframe is destroyed.

### 10.4 Capability compatibility

A provider is either:

- available and conformant;
- unavailable and not advertised; or
- platform-unsupported and not advertised.

There is no placeholder provider that returns plausible but incorrect behavior.

A napplet with a missing required domain MUST be rejected before code execution. A napplet with no `requires` declaration may load with the available domain set and must be allowed to perform its own graceful degradation.

### 10.5 Compatibility corpus

The repository MUST maintain three fixture classes:

1. **Reference fixtures** — minimal napplets for every supported domain and edge condition.
2. **Kehto corpus** — the pinned playground/reference napplets.
3. **Published ecosystem corpus** — real napplets selected by manifest coordinate and cached immutably for deterministic CI.

A napplet enters the accepted corpus only after its manifest and emitted messages pass the pinned conformance suite. Nonconformant artifacts may be retained in a negative corpus but are not compatibility obligations.

### 10.6 Compatibility upgrade policy

A baseline update MUST produce:

- old versus new NIP/NAP/package diff;
- changed envelope and manifest fixtures;
- corpus pass/fail report;
- provider impact report;
- explicit list of newly accepted and no-longer-accepted behavior;
- migration or dual-support plan;
- owner approval.

Once a runtime release declares a baseline supported, later patch and minor releases MUST continue to support it. Dropping a baseline requires a product major version and an explicit deprecation period.

---

## 11. Surface extension

### 11.1 Design status

The surface extension is a product-private, versioned experiment until:

- a working vertical slice exists;
- two independent components use it;
- a reference conformance harness exists;
- its boundaries have survived adversarial review.

It SHOULD later be proposed as a NAP-WORD or related napplet extension. Existing NIP-5D behavior must not depend on that proposal being accepted.

### 11.2 Descriptor

A surface-capable napplet embeds exactly one inert descriptor in its verified `/index.html` bytes:

```html
<script id="napplet-surface" type="application/napplet-surface+json">
{
  "schema": "nmp.surface/1",
  "profile": "renderer",
  "archetype": "feed",
  "inputs": [
    {
      "name": "items",
      "schema": "nostr.events.collection/1",
      "required": true
    },
    {
      "name": "viewer",
      "schema": "nostr.identity.public/1",
      "required": true
    },
    {
      "name": "evidence",
      "schema": "nmp.acquisition-evidence/1",
      "required": false
    }
  ],
  "actions": [
    { "name": "event.open", "schema": "nostr.event-ref/1" },
    { "name": "profile.open", "schema": "nostr.pubkey-ref/1" },
    { "name": "reply.compose", "schema": "nostr.reply-context/1" }
  ],
  "fallback": "legacy",
  "presentation": {
    "kind": "pane"
  }
}
</script>
```

Browsers ignore the non-executable script type. Ordinary NIP-5D shells therefore continue to load the napplet, while this runtime can parse the descriptor before execution. The descriptor is covered by the verified `/index.html` path hash and aggregate hash without requiring a new manifest kind or an extra artifact file.

The exact field spelling may change before surface v1 freezes. The semantic requirements may not:

- descriptor bytes are content-addressed with the verified index document;
- schemas are explicitly versioned;
- inputs and actions are declared before execution;
- the runtime validates compatibility before mounting;
- descriptor grants no capability by itself;
- profile escalation is impossible from inside the component.

Descriptor parsing rules:

- exactly zero or one descriptor element is permitted;
- the descriptor has a finite byte and nesting limit;
- JSON duplicate keys are rejected;
- schema identifiers are local, versioned registry values rather than arbitrary remote URLs;
- malformed descriptor presence rejects surface mounting rather than silently changing profile;
- descriptor parsing performs no network access and executes no script.

### 11.3 Surface domain

When supported and granted, the runtime injects `window.napplet.surface`.

The v1 contract includes these conceptual operations:

#### Runtime to component

- `surface.mount` — session metadata, granted ports, theme/accessibility context, initial lifecycle state;
- `surface.snapshot` — authoritative current value for one input port at one revision;
- `surface.delta` — exact transition from one revision to the next when the schema supports deltas;
- `surface.resync` — runtime instructs the component to discard local projection and await a new snapshot;
- `surface.visibility` — visible/hidden state;
- `surface.focus` — focus state;
- `surface.suspend` / `surface.resume`;
- `surface.unmount`.

#### Component to runtime

- `surface.ready` — component can receive state;
- `surface.action` — emit a declared typed action;
- `surface.action.cancel` — cancel a cancellable outstanding action;
- `surface.snapshot.request` — request resynchronization after detecting a gap;
- `surface.error` — report a component-local rendering failure without crashing the host.

### 11.4 Revision semantics

- Every input port has a monotonically increasing revision within a mount session.
- A snapshot establishes complete local state for that port at its revision.
- A delta declares both `fromRevision` and `revision`.
- A component MUST reject a delta whose `fromRevision` does not equal its current revision and request a snapshot.
- The runtime may skip intermediate states for a slow component and send the newest snapshot.
- Revision ordering is per port; cross-port atomicity is not implied unless a schema explicitly defines a transaction group.
- State frames are bounded. Oversized state must be windowed, paged, streamed through another capability, or refused explicitly.

### 11.5 Binding model

Surface v1 uses **host-defined bindings only**.

A binding provider:

- has a versioned schema;
- accepts typed host-controlled parameters;
- owns or references an NMP live query, protocol resource, or native state source;
- maps source state into surface values;
- preserves evidence and error semantics;
- has finite resource limits;
- exposes explicit cancellation;
- may be consumed by native and web surfaces simultaneously.

Surface v1 does not accept arbitrary serialized NMP demand from untrusted components. That may be considered later after quotas, source authority, access context, and schema evolution are proven.

Initial binding families:

- current public identity;
- event collection/window;
- exact event detail;
- profile;
- thread context;
- composer context;
- follow relationship resource;
- write receipt/progress;
- runtime theme/accessibility;
- scoped acquisition evidence.

### 11.6 Binding ownership

Bindings belong to the native workspace or application model, not to the WebView.

Consequences:

- replacing a renderer preserves the binding;
- WebView crashes do not automatically destroy application state;
- a native view and WebView may observe the same binding;
- the runtime reference-counts consumers while NMP may coalesce compatible underlying demand;
- closing the final owning workspace slot releases the binding and its NMP handle;
- releasing a binding never deletes canonical NMP rows.

### 11.7 Action model

A surface action is a typed request for host behavior, not an arbitrary command string.

The action router may resolve an action to:

- native navigation;
- native application state change;
- an NMP write intent;
- a protocol-module semantic operation;
- a native permission or chooser flow;
- NAP-INTENT dispatch to another napplet;
- a refusal with a typed reason.

Every action has:

- namespaced versioned type;
- validated payload;
- originating principal and session;
- correlation ID where a result is expected;
- policy class;
- cancellation semantics;
- bounded result;
- activity record.

The component never chooses a signer, bypasses approval, or supplies its own runtime identity.

### 11.8 Legacy fallback

A surface component SHOULD support legacy mode when practical:

```text
if shell.supports("surface")
    use host bindings
else
    use ordinary granted NAPs
```

During private v0, a renderer-only component MUST NOT add an unregistered `requires=surface` tag merely to force rejection; current conformance tooling may reject unknown required domains. It declares `fallback: "unavailable"` or `fallback: "reject"` in the inert descriptor and must show a safe unavailable state when loaded by an ordinary shell. After `surface` is registered in the NAP baseline, renderer-only components may use the standard `requires` tag.

---

## 12. NMP integration

### 12.1 Boundary

The runtime depends on NMP. NMP does not depend on the runtime.

All NMP calls occur through the supported facade and platform projection. Runtime packages must not import mechanism crates to bypass the public model.

### 12.2 Live queries

Binding providers and NAP providers may open NMP live queries.

They MUST:

- use explicit demand inputs and source/access policy available through the supported NMP surface;
- preserve row plus evidence semantics;
- cancel handles when their owning runtime object closes;
- avoid duplicate application caches;
- use windowing for bounded collection surfaces;
- map deletion, replacement, expiry, and negative deltas correctly;
- never reinterpret EOSE as global completion.

### 12.3 Write intents

Publishing actions and compatible NAP publish calls use NMP write intents wherever their semantics fit.

The runtime MUST preserve:

- frozen event body and identity after acceptance;
- native approval before sensitive signing when policy requires it;
- pending canonical row visibility;
- durable receipt reattachment;
- explicit cancellation rules;
- per-relay outcomes and `OutcomeUnknown` semantics;
- continuation after originating WebView destruction;
- no silent retargeting after account switch.

Where a NAP’s current result shape cannot express a durable NMP receipt, the compatibility provider returns exactly the NAP result while retaining the richer receipt internally and exposing it through runtime activity UI. An optional future NAP extension may expose receipt semantics; the compatibility response must not be changed unilaterally.

### 12.4 Accounts and access context

- The native host owns account selection and user-facing identity UX.
- Napplet identity APIs are read-only unless a separate NAP defines an action.
- A write’s selected author is frozen at approval/acceptance.
- NIP-42 or other access context must remain attributable in evidence.
- One NMP engine is one local trust domain. Separate mutually untrusted user profiles require separate stores/engine instances.
- The runtime must not infer that an account boundary is a cache privacy boundary.

### 12.5 Protocol resources

Optional NMP protocol modules may back typed bindings and actions, for example:

- following state and follow/unfollow;
- group metadata and membership;
- list reads/mutations;
- verified Blossom upload composition;
- content parsing and reference planning.

A protocol module provides semantic correctness. The WebView surface provides presentation. The native application retains product policy.

### 12.6 Diagnostics

The runtime should expose a filtered, per-component view of:

- active bindings and NMP query handles;
- source plan and evidence summaries;
- current limits and shortfalls;
- pending writes and receipts;
- relay/auth state relevant to the component’s operations;
- provider calls, refusals, and resource use.

Raw diagnostics remain available in developer mode. User-facing diagnostics must avoid leaking secrets or unrelated private activity.

---

## 13. Capability provider model

### 13.1 Provider registry

The runtime core owns a typed provider registry. Each provider declares:

- domain name;
- supported protocol/version identifiers;
- platform availability;
- dependencies;
- policy class;
- request and response schemas;
- cancellation behavior;
- resource limits;
- diagnostics hooks;
- whether it may prompt the user;
- whether it may operate while the surface is hidden or suspended.

The runtime advertises only providers that are fully registered and available for the current session.

### 13.2 Provider ownership map

| Domain family | Primary implementation owner |
|---|---|
| `shell` | compatibility adapter/runtime core |
| `identity` | native account model + NMP reads |
| `relay`, `outbox`, `count` | NMP adapter where semantics fit; explicit adapter for unsupported escape-hatch behavior |
| `storage`, `config` | runtime store, principal-scoped |
| `resource`, `upload` | native resource broker + NMP/Blossom modules where applicable |
| `intent`, `inc` | runtime action/handler routing |
| `theme`, `keys`, `notify`, `link` | native application/platform |
| `media` | native media session broker |
| `lists`, `common`, `dm` | NMP protocol resources plus runtime policy where available |
| `ble`, `serial`, `webrtc` | platform/device providers |
| `cvm` and similar external systems | separate opt-in provider, never implied by NMP |
| `surface` | native runtime binding/action subsystem |

NMP must not be falsely described as implementing domains it does not own.

### 13.3 Provider behavior rules

- Validate every known envelope before dispatch.
- Derive principal/session from the bridge; never accept them from payload.
- Check capability grant before parsing expensive payload bodies where possible.
- Apply finite per-session and global limits.
- Return typed domain errors where the NAP defines them.
- Ignore unknown message types as required by the compatibility baseline.
- Cancel in-flight work on session teardown unless the operation was explicitly transferred to a durable native owner, such as an NMP write intent.
- Record an activity fact for sensitive calls.
- Never log secret material.

### 13.4 Capability grants

Grant states:

- denied;
- ask every time;
- allowed for session;
- allowed for exact build;
- managed by host policy.

Grant decisions bind to the full internal principal. A new build receives no inherited sensitive grant by default. The update UI may offer an explicit “carry grants to this verified update” decision after showing publisher and permission differences.

---

### 13.5 Native host SDK surface

The public host integration should remain small and conceptual. Exact language spelling may differ, but every platform projects the same responsibilities:

- `Runtime.open(profileConfig)` — open one runtime/NMP trust profile;
- `Runtime.install(locator)` — resolve, verify, and stage an artifact without executing it;
- `Runtime.launch(principal, presentation)` — create a legacy or surface session;
- `Runtime.stop(session)` — terminate session-owned work;
- `Runtime.grant/revoke(principal, capability)` — manage exact-build policy;
- `Runtime.registerProvider(provider)` — add a conformant NAP provider;
- `Runtime.registerBinding(provider)` — add a host-owned surface binding family;
- `Runtime.registerActionHandler(handler)` — add a typed action route;
- `Runtime.observeActivity(scope)` — observe bounded activity/diagnostic state;
- `Runtime.close()` — stop sessions and close owned runtime state before NMP/profile shutdown.

The SDK must not expose raw bridge calls, iframe window references, NMP mechanism crates, or mutable global provider registries. Platform sugar may wrap these concepts in Swift observation or Kotlin Flow without introducing a different semantic model.

---

## 14. Workspace and composition model

### 14.1 Workspace

A workspace is native application state describing:

- slots;
- slot roles;
- selected handler/component for each slot;
- binding parameters;
- native navigation state;
- action routing;
- layout and visibility;
- persistent user preferences.

A workspace does not contain Nostr event truth. It contains references to binding definitions and selected renderers.

### 14.2 Slot

A slot is a coarse presentation location such as:

- feed;
- detail;
- profile;
- thread;
- composer;
- media player;
- tool window.

A slot may be rendered by:

- native SwiftUI/Compose code;
- a legacy napplet window;
- a surface component;
- a fallback unavailable/error view.

### 14.3 Handler selection

The runtime supports:

- built-in defaults;
- user-selected defaults by archetype/role;
- one-time “open with” choice;
- NAP-INTENT compatibility;
- surface-specific handlers where a state contract is required.

A component cannot install itself as a default handler without an explicit user or host policy decision.

### 14.4 Renderer switching

When changing a surface renderer:

1. keep the slot and binding alive;
2. mount the replacement component;
3. deliver the latest binding snapshot;
4. wait for readiness;
5. atomically switch visible presentation;
6. unmount and destroy the old component;
7. preserve native navigation, selection, scroll anchor where the slot contract supports it.

The replacement must not cause a second NMP query unless it requests a genuinely different binding.

---

## 15. Installation, updates, and artifact lifecycle

### 15.1 Install flow

1. User supplies or selects a manifest coordinate.
2. Runtime resolves the signed manifest through NMP or a policy-approved resolver.
3. Runtime verifies event signature and supported kind.
4. Runtime fetches every artifact path from hinted or configured blob sources.
5. Runtime verifies each SHA-256 and recomputes aggregate hash.
6. Runtime validates the manifest and artifact against the pinned conformance baseline.
7. Runtime inspects optional surface descriptor.
8. Runtime computes required and optional capability sets.
9. Native UI shows publisher, title, source, exact build hash, capabilities, platform support, and warnings.
10. User installs or cancels.
11. Immutable artifact bytes enter the local content-addressed cache.
12. No executable code runs until launch and session creation.

### 15.2 Update flow

- Resolve a newer signed manifest for the same publisher/dTag.
- Verify all bytes independently.
- Show old and new aggregate hashes.
- Show capability and descriptor differences.
- Keep the previous verified build available for rollback.
- Do not silently transfer sensitive grants.
- Do not migrate scoped storage automatically unless an explicit migration contract and user decision exist.
- An update failure leaves the previous installed version intact.

### 15.3 Uninstall

Uninstall removes:

- installation records;
- build-specific grants;
- component-scoped KV/config according to user choice;
- cached artifact references when no installation uses them;
- workspace handler assignments to that component.

Uninstall does not delete unrelated canonical NMP events or receipts. Outstanding durable writes initiated by the component remain visible and governed by native activity UI unless explicitly cancellable and cancelled by the user.

---

## 16. Runtime persistence

### 16.1 Separate stores

The runtime store is separate from NMP’s canonical store.

Runtime store contents:

- installed manifest principals and versions;
- verified artifact-cache indexes;
- grants and denials;
- per-component KV/config;
- workspace definitions;
- handler preferences;
- activity summaries;
- compatibility baseline metadata;
- crash/suspension recovery metadata.

NMP store contents remain governed by NMP:

- canonical Nostr events;
- provenance and evidence;
- pending rows;
- write intents and receipts;
- routing/coverage data;
- protocol resource state owned by NMP.

### 16.2 Storage scope

Default napplet storage scope is exact-build scoped:

```text
profile / manifestAuthor / dTag / aggregateHash / domain
```

The runtime MAY offer user-approved migration between versions. A component may export a declared, bounded migration payload through a dedicated migration contract; it may not read another version’s store directly.

### 16.3 Reset and profiles

- One application profile maps to one runtime store and one NMP trust domain.
- Logout/reset must close the engine and runtime before destructive reset.
- Reset UI must distinguish runtime component data from NMP canonical data and account vaults.
- Separate OS/user profiles require separate stores.

---

## 17. Security and privacy model

### 17.1 Threat model

Assume:

- napplet code may be malicious;
- publisher metadata may be deceptive;
- relays, gateways, and blob servers may return hostile bytes;
- a napplet may flood messages, allocate memory, spin CPU, or attempt side channels;
- a trusted shell bug may expose the native bridge;
- a component update may be malicious even when signed by the same publisher;
- authenticated or decrypted Nostr data may be more sensitive than public events.

Do not assume the runtime can defend against a compromised OS, compromised WebView engine, malicious native host, or all timing/side-channel attacks.

### 17.2 Required mitigations

#### Artifact integrity

- signed manifest verification;
- per-path SHA-256 verification;
- aggregate-hash verification;
- immutable local artifact cache;
- no remote navigation;
- no unverified subresource loading.

#### Web sandbox

- `sandbox="allow-scripts"`;
- no `allow-same-origin`;
- strict CSP;
- no `window.nostr`;
- no local/session storage, IndexedDB, service worker, raw socket, or direct network;
- only verified local artifact scheme and approved data/blob object URLs.

#### Bridge isolation

- native bridge exists only in the trusted top-level shell;
- napplet communicates only by `postMessage` with its parent;
- source window is mapped to native-created session state;
- session identity never comes from napplet payload;
- native validates every envelope again;
- unknown/unmapped sources are dropped;
- no generic `native.call(method, json)` is exposed to the napplet.

#### Permissions

- exact-build principal;
- least-privilege domain injection;
- native prompts for sensitive operations;
- no permission inheritance without explicit decision;
- immediate revoke path;
- provider cancellation and handle cleanup on revoke.

#### Signing and encryption

- napplet never receives key material;
- cleartext crosses the shell boundary where NAP requires host signing/encryption;
- runtime rejects pre-encrypted payload where the NAP requires plaintext input;
- native approval displays the exact draft, selected identity, and action origin;
- accepted NMP writes freeze identity and body.

#### Resource control

Finite configured ceilings for:

- envelope bytes;
- message rate and burst;
- active requests;
- subscriptions;
- filters/authors/IDs/tags;
- state frame bytes;
- surface collection window;
- storage bytes and key count;
- resource streams and total bytes;
- uploads;
- media/device sessions;
- WebView instances;
- CPU/unresponsiveness watchdog;
- runtime global and per-principal work.

Every refusal is typed and visible in diagnostics.

#### Sensitive data

- provider output is filtered by the session’s grant and access context;
- authenticated/decrypted/private data is not exposed merely because it exists in NMP’s shared local cache;
- activity logs minimize content and are redacted by default;
- clipboard, files, camera, microphone, BLE, serial, and notification contents require explicit policy.

### 17.3 WebView crash containment

- A crashed WebView invalidates only its session.
- NMP and unrelated bindings remain alive.
- Durable writes remain owned by NMP.
- The runtime presents a native crash/reload view.
- Automatic reload is bounded and stops after repeated failure.
- Crash loops are recorded against the exact build.

---

## 18. Functional requirements

### Compatibility

- **FR-C01:** Load any accepted baseline napplet without source/build changes.
- **FR-C02:** Support direct `window.napplet` and SDK-based napplets.
- **FR-C03:** Verify pinned NIP-5D manifest kinds and NIP-5A-derived hashes.
- **FR-C04:** Support all artifact modes accepted by the compatibility baseline.
- **FR-C05:** Inject domains before any napplet script runs.
- **FR-C06:** Implement source-window identity binding.
- **FR-C07:** Preserve unknown-message forward compatibility.
- **FR-C08:** Implement NAP-SHELL compatibility required by the baseline.
- **FR-C09:** Reject missing required domains before execution.
- **FR-C10:** Maintain an executable compatibility corpus and report.

### Runtime and lifecycle

- **FR-R01:** Install, launch, suspend, resume, stop, update, roll back, and uninstall a principal.
- **FR-R02:** Maintain explicit session lifecycle state.
- **FR-R03:** Revoke permissions and cancel non-durable work immediately.
- **FR-R04:** Isolate component KV/config by principal.
- **FR-R05:** Recover workspace and installed-artifact state after restart.
- **FR-R06:** Recover from a WebView crash without restarting NMP.
- **FR-R07:** Enforce finite per-session and global resource ceilings.

### NMP

- **FR-N01:** Use one NMP engine per local application trust profile.
- **FR-N02:** Back host bindings with NMP live queries where appropriate.
- **FR-N03:** Back publication with NMP write intents where appropriate.
- **FR-N04:** Surface scoped acquisition evidence and shortfalls.
- **FR-N05:** Reattach durable receipts after restart.
- **FR-N06:** Preserve pending rows through canonical query updates.
- **FR-N07:** Prevent account changes from retargeting accepted writes.
- **FR-N08:** Use public NMP facade only.

### Surface

- **FR-S01:** Detect and validate the inert surface descriptor embedded in verified `/index.html`.
- **FR-S02:** Mount renderer and hybrid profiles.
- **FR-S03:** Deliver revisioned snapshots and deltas.
- **FR-S04:** Recover from revision gaps through resynchronization.
- **FR-S05:** Route only declared, schema-valid actions.
- **FR-S06:** Keep bindings alive across renderer replacement.
- **FR-S07:** Permit native and web consumers of the same binding.
- **FR-S08:** Prevent renderer profile from receiving query/relay domains unless explicitly changed to hybrid by host policy before launch.
- **FR-S09:** Provide legacy fallback behavior.
- **FR-S10:** Support explicit unavailable/error/fallback states.

### Product UX

- **FR-U01:** Native install review with publisher, hash, capabilities, and source metadata.
- **FR-U02:** Native permission and approval UI.
- **FR-U03:** User-selectable handlers/renderers by role.
- **FR-U04:** Per-component activity and diagnostics view.
- **FR-U05:** Update comparison and rollback.
- **FR-U06:** Clear error UX for incompatible, invalid, crashed, denied, and offline states.
- **FR-U07:** Native accessibility and keyboard navigation around WebView surfaces.

### Provider system

- **FR-P01:** Typed provider registration and discovery.
- **FR-P02:** Never advertise a missing provider.
- **FR-P03:** Platform-specific provider matrix.
- **FR-P04:** Contract tests for every advertised domain.
- **FR-P05:** Explicit cancellation and cleanup for every provider.
- **FR-P06:** Activity fact for every sensitive provider call.

---

## 19. Non-functional requirements

### Correctness

- Closed, typed contracts at native boundaries.
- No silent truncation, success collapse, or missing-domain simulation.
- Deterministic fixtures for protocol, manifest, and surface schemas.
- Independent adversarial tests for security invariants.

### Performance

- Coarse WebView surfaces only in v1.
- Renderer swaps reuse host bindings.
- State delivery may skip intermediate frames but must converge to the latest correct state.
- No unbounded queue behind a slow surface.
- Repeated lifecycle churn returns handles/tasks/streams to baseline.
- Performance thresholds are measured and ratified in the hardening milestone rather than guessed before the first real workload.

### Offline behavior

- Installed verified artifacts launch from local cache.
- NMP cached rows render immediately when available.
- Resource calls fail or use approved cache according to policy.
- UI distinguishes cached/stale/scoped-evidence states without claiming global offline completeness.

### Accessibility

- Native chrome supports keyboard, screen reader, dynamic type, high contrast, reduced motion, and RTL where applicable.
- Surface descriptor includes presentation role but does not waive WebView accessibility requirements.
- Reference surface SDK provides accessibility guidance and conformance checks.
- Focus transfer between native and WebView content is deterministic.

### Privacy

- No background network access by untrusted components.
- No content logging by default.
- Grants and activity are inspectable and revocable.
- Private/authenticated data is capability-scoped.

### Portability

- Runtime semantics live in Rust where practical.
- Apple and Android adapters use native lifecycle and WebView APIs.
- Protocol fixtures and BDD scenarios are shared across platforms.
- Pixel code remains platform/web specific.

### Maintainability

- Compatibility and surface contracts are independently versioned.
- Public changes require fixtures and migration notes.
- No mechanism-level shortcuts around NMP or platform bridge policy.
- The trusted shell remains small enough for focused review.

---

## 20. Milestone plan

Milestones are ordered by proof dependency, not calendar estimate.

### M0 — Contract lock and falsifier harness

#### Objective

Turn moving ecosystem documents into one pinned, executable build target before production code spreads assumptions.

#### Deliverables

- repository skeleton and ownership boundaries;
- `compatibility.lock`;
- ADR set for all ratified decisions in this document;
- pinned NIP-5D/NAP/SDK/conformance fixtures;
- Kehto and published napplet corpus indexes;
- initial threat model;
- Gherkin runner and test tagging;
- mock relay, Blossom, signer, and artifact sources;
- baseline provider matrix;
- macOS Workbench shell with no untrusted code yet.

#### Acceptance tests

1. CI can reproduce the exact pinned package/spec corpus from a clean checkout.
2. A deliberate one-byte change to a fixture fails the compatibility hash check.
3. Every current active NAP envelope type in the pinned package is represented by a validator or an explicit unsupported record.
4. Every hard invariant has at least one failing falsifier before implementation.
5. The architecture dependency check proves NMP does not depend on runtime packages.
6. The baseline report identifies known spec/package drift rather than silently resolving it.

#### Exit gate

No WebView runtime implementation begins until the product owner signs off the compatibility lock, principal model, trust boundaries, and legacy/surface separation.

---

### M1 — Secure native WebView substrate

#### Objective

Prove that untrusted web code can execute inside a native surface without gaining native, network, storage, or sibling access.

#### Deliverables

- Rust session core;
- Swift/macOS WebView host;
- trusted local shell document;
- inner sandboxed iframe;
- source-window/session mapping;
- narrow native bridge;
- deny-by-default CSP;
- local verified artifact scheme/materializer;
- lifecycle and crash handling;
- resource accounting skeleton.

#### Acceptance tests

1. A fixture runs JavaScript in the inner iframe and exchanges one valid envelope with the native runtime.
2. The fixture cannot read host DOM, cookies, localStorage, sessionStorage, IndexedDB, service workers, or native bridge objects.
3. Direct `fetch`, WebSocket, and remote subresource loads are denied.
4. A sibling iframe cannot impersonate the fixture’s source window.
5. A caller-supplied principal/session value is ignored.
6. Destroying the WebView closes the runtime session and returns all callbacks/tasks to baseline.
7. Crashing or terminating the WebView leaves the native app and NMP test engine alive.

#### Exit gate

Independent security review passes the source-binding, bridge-isolation, CSP, and teardown invariants.

---

### M2 — Existing NIP-5D napplet compatibility

#### Objective

Run existing napplets unchanged before introducing the new surface model.

#### Deliverables

- manifest resolver/verifier;
- artifact cache and multi-file materialization;
- pinned shim/prelude behavior;
- NAP-SHELL and domain-presence compatibility;
- manifest `requires` enforcement;
- envelope validation and dispatch skeleton;
- compatibility report generator;
- Kehto/reference corpus runner.

#### Acceptance tests

1. Pinned `@napplet/conformance` passes against the native host.
2. Every accepted reference and Kehto corpus napplet boots without source or build changes.
3. Both current artifact modes accepted by the baseline work.
4. Invalid signature, blob hash, aggregate hash, or index path causes pre-execution rejection.
5. Missing required capability causes pre-execution rejection with a native error view.
6. Unknown message types are ignored and do not crash or disclose capability internals.
7. Napplet code sees only the granted `window.napplet` domains and never sees `window.nostr`.
8. Reload and development-wrapper fixtures still receive injection before their scripts.

#### Exit gate

The runtime may not market or document surface components until the unchanged legacy corpus is green.

---

### M3 — Native provider registry and NMP vertical read path

#### Objective

Make the runtime useful as a real native Nostr host while preserving capability boundaries.

#### Deliverables

- typed Rust provider registry;
- principal-scoped grants and quotas;
- NMP adapter through public facade;
- initial providers: `shell`, `identity`, `storage`, `config`, `theme`, `resource`, `link`, `outbox`, `relay`, `intent`, and `inc` as supported by the pinned baseline;
- native permission UI;
- per-component activity ledger;
- first existing feed/profile napplets using real NMP data.

#### Acceptance tests

1. Existing feed napplet receives real events through a compatible NAP provider backed by NMP.
2. Existing profile napplet reads the selected public identity without receiving keys.
3. Two compatible requests share NMP work where NMP declares them shareable, while session cancellation remains independent.
4. Storage is isolated across principals and build hashes.
5. Revoking a domain removes it for the next session and terminates current provider work according to policy.
6. NMP evidence and local shortfalls are not collapsed into a false global completion value.
7. No provider advertises itself without passing its contract suite.

#### Exit gate

At least one real existing napplet is usable end-to-end against public relays in the macOS Workbench.

---

### M4 — Surface protocol and host-owned bindings

#### Objective

Prove the new capability: replaceable WebView renderers over persistent native state.

#### Deliverables

- embedded surface-descriptor parser and validator;
- private `surface` domain v0;
- renderer and hybrid profiles;
- revisioned snapshot/delta transport;
- binding registry;
- initial event-collection, profile, identity, evidence, and theme bindings;
- typed action router;
- surface SDK/reference harness;
- two independently implemented feed renderers.

#### Acceptance tests

1. A legacy napplet without a descriptor still behaves identically.
2. A surface renderer receives host-owned feed state without an outbox/relay domain.
3. Two renderers can consume the same feed binding.
4. Swapping renderers does not close or reopen the binding’s NMP query.
5. A native SwiftUI feed counter and the WebView renderer update from the same binding.
6. An out-of-order delta causes resynchronization rather than corrupted local state.
7. An undeclared or malformed action is refused before handler execution.
8. A surface with `fallback=legacy` runs in legacy mode when `surface` is disabled.
9. A private-v0 surface with `fallback=reject` is rejected before execution by this runtime when the surface provider is disabled, while a normal shell can still load the artifact and receive its built-in unavailable behavior.

#### Exit gate

A recorded falsifier demonstrates renderer replacement while NMP query identity and canonical rows remain stable.

---

### M5 — Complete product vertical slice

#### Objective

Ship the first coherent application experience, not merely runtime demos.

#### Deliverables

- native three-slot Workbench: feed, detail, composer;
- installer and component chooser;
- existing legacy napplet windows;
- surface feed, profile/thread, and composer components;
- action routing for event/profile open and reply compose;
- native signing approval;
- NMP durable publication and receipt UI;
- renderer update/rollback;
- offline cached launch;
- user-facing activity drawer.

#### Acceptance tests

1. User installs an existing napplet from a manifest coordinate and launches it unchanged.
2. User assigns one of two surface feed renderers to the feed slot and switches between them without data restart.
3. Selecting an event opens the preferred detail handler.
4. Composer emits a draft action; native UI shows exact draft, component identity, and selected account.
5. After approval, NMP accepts the write and every active feed consumer sees the pending canonical row.
6. Destroying the composer does not stop the durable write.
7. Application restart restores the workspace and reattaches the receipt.
8. Changing active account after acceptance does not retarget the write.
9. Offline launch renders installed components and cached NMP rows with honest evidence.
10. User can inspect and revoke component permissions.

#### Exit gate

A user can operate the Workbench for the primary feed/detail/compose journey without developer tooling.

---

### M6 — NAP breadth and ecosystem compatibility

#### Objective

Turn the vertical slice into a credible general napplet runtime.

#### Deliverables

- provider matrix for every active domain in the pinned `@napplet/nap` baseline;
- conformant native/platform providers or explicit not-advertised status;
- upload/media/notify/keys/list/common/count/DM/device providers in dependency order;
- expanded real napplet corpus;
- compatibility dashboard;
- developer documentation for host providers and surface components;
- CLI/template for surface-enabled napplets;
- runtime conformance suite for advertised capabilities.

#### Acceptance tests

1. Every advertised domain passes reference and adversarial contract tests.
2. Every accepted published corpus napplet runs unchanged when its required domains are available.
3. Device-specific napplets receive native chooser/permission flows and cannot retain raw device handles after session close.
4. Upload and resource providers enforce content, size, MIME, redirect, and integrity policy.
5. NAP-INTENT and NAP-INC interoperate between legacy napplets, surface components, and native handlers.
6. Compatibility report has no unexplained failure or skipped domain.

#### Exit gate

The runtime can accurately publish a platform capability matrix and compatibility statement.

---

### M7 — Hardening and desktop beta/GA gate

#### Objective

Prove boundedness, recovery, accessibility, and security under hostile and long-running conditions.

#### Deliverables

- fuzzers for manifests, envelopes, surface frames, and provider payloads;
- malicious napplet corpus;
- long-run lifecycle and memory tests;
- slow-consumer and overload tests;
- WebView crash-loop policy;
- accessibility and keyboard/focus suite;
- update, rollback, revoke, reset, and corruption recovery tests;
- privacy review and log-redaction audit;
- performance budgets based on measured workloads;
- release signing, provenance, and reproducible package process.

#### Acceptance tests

1. Flooding one napplet cannot create unbounded runtime memory or starve all other sessions.
2. A slow surface converges to latest correct state without replaying an unbounded backlog.
3. Repeated mount/unmount, permission revoke, update, and crash cycles return handles, streams, and tasks to baseline.
4. Corrupt runtime metadata does not corrupt NMP’s canonical store and has a recoverable failure mode.
5. Unknown/invalid/deleted/replaced/expired content always has an intelligible fallback.
6. Native and WebView accessibility paths pass the platform test matrix.
7. Security review finds no route from napplet iframe to native bridge, keys, direct network, or sibling storage.
8. Desktop compatibility corpus and BDD suite are green on release artifacts, not only debug builds.

#### Exit gate

Desktop GA requires product-owner, compatibility, NMP-boundary, and independent security signoff.

---

### M8 — iOS projection

#### Objective

Project the proven runtime onto iOS without weakening sandbox, lifecycle, or NMP semantics.

#### Deliverables

- qualified NMP iOS runtime dependency;
- SwiftUI iOS host;
- mobile workspace/navigation adaptation;
- background/suspension policy;
- mobile permission/signing/upload flows;
- App Store policy assessment;
- full shared compatibility and BDD suite on device/simulator.

#### Acceptance tests

- Same artifact, bridge, principal, capability, surface revision, and NMP receipt scenarios pass.
- Backgrounding destroys or suspends WebViews according to policy without losing durable writes.
- Memory pressure can reclaim a surface and restore it from the latest binding snapshot.
- Store review requirements do not force exposing ambient native APIs to napplets.

---

### M9 — Android projection

#### Objective

Provide Kotlin/Compose and Android WebView parity after NMP’s Android package is qualified.

#### Deliverables

- Android AAR/runtime integration;
- origin-scoped WebView messaging;
- Compose host components;
- Android permission/device providers;
- lifecycle and process-death restoration;
- cross-platform conformance report.

#### Acceptance tests

- Shared compatibility, surface, and NMP scenarios pass unchanged.
- No globally exposed JavaScript interface is reachable from untrusted frames.
- Process death restores workspace metadata and NMP durable obligations correctly.
- Platform-specific unsupported domains are not advertised.

---

## 21. Core BDD specification

The companion `.feature` file should be treated as executable acceptance criteria. The essential scenarios are reproduced here.

### Feature: Existing napplet compatibility

```gherkin
@compat @legacy
Scenario: Run an existing conformant napplet without modification
  Given the runtime is pinned to a compatibility baseline
  And a published napplet passes that baseline's conformance suite
  And all of its required domains are available
  When the user installs and launches the napplet
  Then the runtime executes only the verified artifact bytes plus the runtime-owned compatibility prelude
  And the napplet receives its expected window.napplet domains before its scripts run
  And no source or build change is required

@compat
Scenario: Reject a napplet with an unavailable required domain before execution
  Given a verified napplet requires the "ble" domain
  And the current platform does not advertise "ble"
  When the user attempts to launch it
  Then the runtime does not execute the napplet
  And the native UI identifies the missing domain

@compat @forward
Scenario: Ignore an unknown message type
  Given a mapped napplet session
  When the napplet emits a well-formed envelope with an unrecognized type
  Then the runtime silently ignores the envelope at the protocol boundary
  And the session remains healthy
```

### Feature: Artifact integrity

```gherkin
@security @artifact
Scenario: Reject a blob whose bytes do not match its path hash
  Given a signed manifest references a blob hash
  And the blob source returns different bytes
  When the runtime resolves the artifact
  Then installation fails before execution
  And no returned bytes enter the executable cache

@security @artifact
Scenario: Reject an aggregate mismatch
  Given every path blob matches its individual hash
  But the manifest x tag does not match the recomputed aggregate
  When the runtime verifies the manifest
  Then installation fails before execution
```

### Feature: WebView trust boundary

```gherkin
@security @bridge
Scenario: Drop a message from an unmapped source window
  Given one mapped napplet iframe and one unmapped iframe
  When the unmapped iframe emits a valid privileged envelope
  Then the trusted shell drops it
  And the native runtime receives no provider call

@security @network
Scenario: Block direct network access from a napplet
  Given a launched napplet
  When it attempts fetch and WebSocket access to an external host
  Then the browser denies both attempts
  And no provider grant is created

@security @keys
Scenario: Never expose ambient signing capability
  Given a launched napplet
  Then window.nostr is absent
  And no key or signer object is reachable from the napplet
```

### Feature: Principal and permission isolation

```gherkin
@security @storage
Scenario: Isolate storage between builds
  Given two verified builds share a publisher and dTag but have different aggregate hashes
  And build A writes a storage key
  When build B reads the same key
  Then build B receives no value

@security @update
Scenario: Do not silently inherit sensitive grants on update
  Given build A has a persistent upload grant
  And build B is a verified update with a different aggregate hash
  When build B is installed
  Then build B does not receive the upload grant until the user explicitly approves it

@security @revoke
Scenario: Revoking a capability stops active non-durable work
  Given a napplet has an active resource stream
  When the user revokes the resource capability
  Then the stream is cancelled
  And future resource requests are denied
```

### Feature: Surface mounting and state

```gherkin
@surface
Scenario: A descriptor-less napplet remains legacy
  Given a verified napplet has no surface descriptor
  When it launches
  Then it runs in the legacy profile
  And no surface domain is injected

@surface @state
Scenario: Mount a renderer with an initial snapshot
  Given a surface renderer declares a compatible feed input
  And the host has an active feed binding at revision 7
  When the component reports surface.ready
  Then the runtime sends a feed snapshot at revision 7
  And the component can render without opening a relay capability

@surface @state
Scenario: Recover from a missing delta
  Given a component has input revision 10
  When it receives a delta from revision 11 to revision 12
  Then it does not apply the delta
  And it requests resynchronization
  And the runtime sends the latest authoritative snapshot

@surface @policy
Scenario: Renderer profile cannot escalate into hybrid
  Given a component is mounted in renderer profile without outbox
  When it emits an outbox request
  Then no outbox provider is invoked
  And the request is denied or ignored according to the compatibility contract
```

### Feature: Renderer replacement

```gherkin
@surface @composition @nmp
Scenario: Replace a feed renderer without restarting demand
  Given a workspace feed slot owns one NMP-backed binding
  And renderer A is mounted on that slot
  When the user replaces renderer A with renderer B
  Then the binding and NMP observation remain the same logical instances
  And renderer B receives the latest snapshot
  And renderer A is unmounted after renderer B is ready

@surface @composition
Scenario: Native and web renderers observe the same binding
  Given a native counter and a WebView feed consume one binding
  When the binding receives a new canonical event
  Then both consumers update from the same binding revision
```

### Feature: Typed actions

```gherkin
@surface @actions
Scenario: Route a declared profile-open action
  Given a feed surface declares the profile.open action
  And the host has a preferred profile handler
  When the surface emits a valid profile.open payload
  Then the native action router opens the preferred handler
  And the action is recorded against the originating principal

@surface @actions @security
Scenario: Refuse an undeclared action
  Given a surface did not declare system.exec
  When it emits a system.exec action
  Then the runtime refuses it before any handler runs
```

### Feature: Durable publication

```gherkin
@nmp @write @surface
Scenario: Publish from a component through native approval
  Given a composer emits a valid publish-draft action
  When the user approves the exact draft and account in native UI
  Then the runtime submits an NMP write intent
  And the pending event becomes visible through ordinary canonical queries

@nmp @write @lifecycle
Scenario: Publication survives component destruction
  Given NMP has accepted a durable write from a composer
  When the composer WebView is destroyed
  Then NMP retains the write obligation
  And another surface can observe its receipt and pending row

@nmp @write @identity
Scenario: Account switch cannot retarget an accepted write
  Given a write was accepted under account A
  When the user switches the active account to B
  Then the write remains bound to account A
```

### Feature: Restart and offline behavior

```gherkin
@offline
Scenario: Launch an installed component while offline
  Given the artifact is verified and cached
  And NMP has cached rows for the binding
  And the network is unavailable
  When the user opens the workspace
  Then the component launches from local artifact bytes
  And cached rows render
  And evidence does not claim global completeness

@restart @nmp
Scenario: Restore a workspace and reattach a receipt
  Given a workspace contains a surface slot and a pending durable receipt
  When the application is terminated and restarted
  Then the workspace is restored
  And the runtime reattaches to the NMP receipt
  And the surface receives the latest receipt state
```

### Feature: Resource limits and failure isolation

```gherkin
@security @limits
Scenario: One napplet exceeds its message-rate limit
  Given two healthy napplet sessions
  When session A floods envelopes beyond its finite quota
  Then session A is throttled or terminated with an observable reason
  And session B remains responsive

@surface @backpressure
Scenario: A slow surface converges without an unbounded queue
  Given a surface stops consuming state temporarily
  And the binding advances many times
  When the surface resumes
  Then it receives the newest correct snapshot or composed transition
  And memory does not grow with every skipped revision

@crash
Scenario: WebView crash does not stop native state
  Given a surface consumes an NMP-backed binding
  When its WebView process crashes
  Then the binding and NMP observation remain valid according to workspace ownership
  And the native host offers a bounded reload
```

---

## 22. Test strategy

### 22.1 Test layers

| Layer | Purpose |
|---|---|
| Pure unit | parsers, principals, grants, revisions, schemas, limits |
| Contract | every NAP envelope/provider and surface message |
| Compatibility | pinned `@napplet/conformance`, SDK/global fixtures, corpus |
| Native bridge integration | source mapping, injection timing, CSP, teardown |
| NMP integration | real facade, mock relays, strict AUTH, signers, restart |
| Product E2E | install, launch, switch, compose, publish, update, revoke |
| Adversarial | malicious manifests, messages, resource abuse, spoofing |
| Performance | lifecycle churn, WebView count, slow consumer, large feed |
| Platform | macOS/iOS/Android lifecycle and permission differences |

### 22.2 Required test infrastructure

- deterministic Nostr relay with programmable EOSE, errors, AUTH, reconnect, replacement, deletion, and expiry;
- deterministic Blossom/resource server with redirects, MIME errors, size violations, corrupt hashes, and slow streams;
- deterministic local and remote signer fixtures;
- frozen real napplet corpus;
- WebView test harness capable of inspecting globals, network attempts, source windows, and lifecycle;
- runtime resource counters for sessions, tasks, provider calls, NMP handles, streams, cached artifacts, and WebViews;
- cross-platform surface fixture corpus with identical expected revisions/actions.

### 22.3 Release test matrix

Every release candidate runs:

- full compatibility corpus;
- all advertised provider contracts;
- all core BDD scenarios;
- NMP restart and durable write scenarios;
- malicious artifact and message corpus;
- lifecycle/memory soak;
- platform accessibility suite;
- release-build bridge and CSP verification;
- compatibility report generation.

A skipped test requires an explicit issue and blocks GA unless the product owner marks the affected capability unsupported and removes its advertisement.

---

## 23. Observability and activity

### 23.1 User activity view

Per principal/build, show:

- installed publisher, dTag, hash, version metadata;
- active and historical sessions;
- granted capabilities;
- active surface bindings;
- resource/storage usage;
- sensitive actions and approvals;
- uploads and external links;
- NMP writes and receipt summaries;
- scoped relay/evidence summary;
- throttles, refusals, crashes, and revocations.

### 23.2 Developer inspector

Developer mode adds:

- raw compatibility envelope log with redaction;
- source-window/session mapping;
- provider dispatch and timing;
- surface revisions and resyncs;
- NMP query/binding identities and diagnostics;
- resource counters and leak baselines;
- compatibility baseline and package versions.

Developer mode must not change authorization or runtime semantics.

---

## 24. Versioning and governance

### 24.1 Independent version axes

The product versions separately:

1. native runtime API;
2. compatibility baseline;
3. provider/domain matrix;
4. surface protocol;
5. surface schema registry;
6. platform packages.

A runtime release notes all six.

### 24.2 Change gates

A public or compatibility change requires:

- issue explaining the unexpressed requirement or unsafe behavior;
- ADR when ownership or trust changes;
- updated BDD scenario;
- updated fixtures and corpus report;
- Rust/Swift/Kotlin/JS impact assessment;
- migration and fallback behavior;
- explicit compatibility reviewer signoff;
- removal of superseded paths rather than indefinite parallel APIs.

### 24.3 Upstream strategy

- Do not fork NIP-5D or redefine existing NAP domains.
- Report compatibility defects upstream with minimal reproducible fixtures.
- Use pinned upstream packages where possible.
- Keep the surface extension namespaced/private until implementation proof.
- After proof, propose the narrowest reusable contract upstream.
- Preserve legacy mode regardless of surface-extension outcome.

### 24.4 Release channels

- **Falsifier:** engineering-only app; no compatibility promise beyond the pinned fixtures.
- **Developer preview:** legacy conformance is green; provider and surface APIs may still change.
- **Alpha:** complete Workbench vertical slice; exact supported baseline and provider matrix published.
- **Beta:** compatibility corpus, adversarial suite, recovery, accessibility, and measured resource budgets are green.
- **Desktop GA:** all M7 exit gates and independent signoffs pass.
- **Mobile preview/beta/GA:** separate platform gates; desktop GA does not imply mobile readiness.

Every distributed build includes its compatibility lock and machine-readable provider matrix.

---

## 25. Risk register

| Risk | Impact | Mitigation / gate |
|---|---|---|
| NIP-5D and NAP drift | Compatibility breaks during build | pinned lock, executable corpus, explicit baseline upgrades |
| SDK/spec disagreement | Ambiguous behavior | record drift; support baseline behavior; never silently “correct” artifacts |
| WebView bridge exposure | Native compromise | trusted top-level shell only, source mapping, narrow channel, native validation |
| Direct network escape | Data exfiltration | deny CSP, private artifact scheme, no remote navigation, adversarial tests |
| WebView memory/process overhead | Poor UX | coarse surfaces, measured budgets, bounded pooling, crash recovery |
| NMP public API changes pre-v2 | Adapter churn | pin NMP, public facade only, one adapter crate, surface tests |
| NMP trust-domain misunderstanding | Private data leak | one engine per local profile, provider filtering, explicit access-context tests |
| Provider breadth becomes a second client library | Scope explosion | provider registry, dependency order, no false advertisement, platform matrix |
| Surface schema explosion | Unmaintainable ecosystem | small host-defined v1 registry, versioned schemas, no arbitrary remote schemas |
| Surface becomes another monolithic framework | Loss of composability | coarse role contracts, state-down/actions-up, replaceable handlers |
| New surface mode breaks ordinary shells | Ecosystem split | standard NIP-5D artifact, optional domain, legacy fallback |
| App Store restrictions | Distribution risk | macOS first, explicit review, no ambient native API exposure |
| Malicious update inherits trust | Supply-chain compromise | grants bound to aggregate hash, explicit carry-forward decision, rollback |
| Slow or malicious component causes pressure | Whole-app failure | finite quotas, latest-state delivery, per-session termination |
| Duplicate application state | Divergence | NMP canonicality invariant, runtime stores no event truth |
| Overpromising “native” | Product confusion | explicit definition: native host/runtime, web-rendered module |

---

## 26. Ratified architecture decisions

These decisions should be copied into individual ADRs during M0.

1. Existing napplets are a hard compatibility contract.
2. Legacy compatibility is implemented before the surface extension.
3. The runtime core is native Rust; WebViews are presentation sandboxes.
4. NMP is consumed through its public facade only.
5. NMP remains unaware of napplet sessions and WebViews.
6. macOS is the first reference platform.
7. One coarse surface instance uses one platform WebView by default.
8. The WebView contains a trusted local shell and an inner sandboxed iframe.
9. The native bridge is never exposed directly to the napplet iframe.
10. Current accepted single-file and external-asset napplets are supported.
11. Surface capability is additive and optional.
12. Surface v1 uses host-defined bindings, not arbitrary component-defined NMP demand.
13. Surface renderer profile has no relay/outbox access by default.
14. Binding lifetime belongs to the workspace slot, not the WebView.
15. Renderer replacement preserves binding and NMP state.
16. Surface descriptor is inert metadata embedded in the verified, hashed index document.
17. State uses revisioned snapshots and schema-specific deltas.
18. Actions are declared, typed, policy-checked, and routed by the native host.
19. Runtime persistence is separate from NMP canonical persistence.
20. Grants bind to publisher, dTag, and aggregate hash.
21. Sensitive grants do not silently transfer to an update.
22. Unsupported capabilities are not advertised.
23. One NMP engine is one local application trust profile.
24. No global synced/complete state is invented.
25. Dynamic native code and dynamic NMP protocol cartridges are outside v1.

---

## 27. Agent execution model

### 27.1 Workstreams

Run parallel agents only across stable boundaries:

- **A — Compatibility:** manifests, artifacts, pinned shim, conformance, corpus.
- **B — WebView security:** trusted shell, CSP, source binding, native bridge, lifecycle.
- **C — Runtime core:** principals, sessions, grants, quotas, provider registry, persistence.
- **D — NMP adapter:** live queries, evidence, writes, receipts, identity, diagnostics.
- **E — Surface:** descriptor, revisions, bindings, action router, SDK.
- **F — Native product:** Workbench, install/update, permission UI, workspace, activity.
- **G — Adversarial QA:** BDD, fuzzing, malicious fixtures, resource counters, platform tests.

Do not let two agents independently define the same public envelope, principal, persistence schema, or lifecycle state machine.

### 27.2 Issue template

Every implementation issue must contain:

- product requirement IDs;
- invariant being advanced;
- exact compatibility baseline;
- in-scope and out-of-scope behavior;
- API/schema changes;
- BDD scenario IDs;
- falsifier that must fail before implementation;
- resource limits and teardown obligations;
- security/privacy impact;
- NMP/public-facade impact;
- platform impact;
- acceptance commands and artifacts;
- owner/reviewer roles.

### 27.3 Review roles

Every vertical slice receives at least:

- implementation review;
- compatibility review;
- adversarial/security review;
- NMP ownership-boundary review when it touches NMP;
- platform review when it touches WebView/native lifecycle.

The adversarial reviewer should not be the implementing agent and should preferably use a different model or independent context.

### 27.4 Definition of done for every issue

- implementation complete with no stubs or TODO behavior on the supported path;
- BDD and lower-level tests green;
- failure and teardown path tested;
- compatibility fixtures updated when applicable;
- no capability advertised without behavior;
- diagnostics/activity added for sensitive or bounded behavior;
- docs and ADR updated;
- no new unbounded queue, map, stream, or retry owner;
- no duplicate Nostr truth outside NMP;
- release-build test where bridge/CSP/platform behavior matters;
- independent review findings resolved or explicitly rejected with rationale.

---

## 28. Recommended initial issue backlog

### M0 issues

1. Create repository and enforce dependency direction.
2. Produce `compatibility.lock` and source snapshot report.
3. Import pinned conformance fixtures and generate envelope inventory.
4. Build immutable napplet corpus index.
5. Ratify principal and grant model ADR.
6. Ratify bridge and WebView trust-boundary ADR.
7. Ratify surface/legacy separation ADR.
8. Build deterministic relay/blob/signer test services.
9. Install BDD runner and core feature file.
10. Create Workbench native shell skeleton.

### M1 issues

11. Implement Rust session lifecycle core.
12. Implement trusted local WebView shell.
13. Implement source-window to native-session mapping.
14. Implement narrow Apple bridge.
15. Implement deny-by-default CSP and verified artifact scheme.
16. Implement teardown/resource counters.
17. Implement malicious bridge/network fixture suite.

### M2 issues

18. Implement manifest resolution and signature verification.
19. Implement path and aggregate verification.
20. Implement artifact CAS and materialization.
21. Integrate pinned shim/prelude behavior.
22. Implement requires/domain negotiation.
23. Implement compatibility envelope validation.
24. Run reference and Kehto corpus.
25. Generate first native-runtime compatibility report.

### M3 issues

26. Implement provider registry and provider lifecycle.
27. Implement grants, quotas, and activity facts.
28. Implement NMP engine adapter and query observation.
29. Implement identity provider.
30. Implement storage/config/theme/link providers.
31. Implement resource broker baseline.
32. Implement outbox/relay compatibility provider subset.
33. Implement NAP-INTENT/NAP-INC routing.
34. Run first real existing feed/profile napplets.

### M4 issues

35. Freeze surface descriptor v0.
36. Implement surface session and revision state machine.
37. Implement binding provider registry.
38. Implement NMP event-collection binding.
39. Implement profile/identity/evidence bindings.
40. Implement typed action router.
41. Build surface SDK/reference harness.
42. Build two independent feed renderers.
43. Prove renderer swap without NMP demand restart.

### M5 issues

44. Implement Workbench workspace persistence.
45. Implement component installer and chooser.
46. Implement feed/detail/composer slots.
47. Implement native signing approval.
48. Implement NMP durable write/receipt integration.
49. Implement update/rollback and grant review.
50. Implement offline launch and recovery.
51. Implement user activity/diagnostics drawer.
52. Run complete product BDD journey.

---

## 29. Questions intentionally deferred until evidence exists

These do not block M0-M4 unless noted:

- exact final surface domain name and upstream NAP form;
- exact snapshot/delta encoding beyond semantic revision rules;
- dynamic-height inline WebView surfaces;
- WebView pooling strategy;
- portable workspace distribution over Nostr;
- user-editable binding definitions;
- arbitrary serialized NMP demand from components;
- WASM service components;
- dynamic protocol cartridges;
- centralized versus purely Nostr-discovered component catalog;
- final mobile store-distribution model.

Any deferred question that becomes load-bearing for a milestone must be promoted into an ADR before implementation proceeds.

---

## 30. Final product acceptance statement

The product is ready for desktop GA only when this statement is true:

> A user can install and run existing conformant napplets unchanged in a secure native host; install optional surface components that render native-owned NMP state; switch those renderers without restarting the underlying application demand; approve durable Nostr actions through native policy; recover after component and application failure; and inspect or revoke every sensitive capability—without giving untrusted web code keys, direct network access, native bridge access, or authority over canonical Nostr state.

That is the product. Anything materially weaker is merely another NIP-5D host implementation. Anything materially broader should be proposed as a later product after this boundary is proven.
