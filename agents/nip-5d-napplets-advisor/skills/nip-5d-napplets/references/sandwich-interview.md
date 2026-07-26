# Sandwich interview: philosophy, vision, and tension

Source: `30: Napplets w/ Sandwich`, published 2026-07-22:
<https://sovereignengineering.io/podcast/30-napplets-w-sandwich>

This is a critical synthesis of the complete diarized interview, not a
transcript substitute. Timestamp ranges are an audit trail for themes that recur
across the conversation. They preserve author intent and experience, not
normative protocol or current implementation fact.

## The essence

The central claim is that the ordinary client is the wrong unit of software
sovereignty. A client bundles identity, relay behavior, storage, signing,
rendering, and every product feature under one team's roadmap. Users can switch
clients, but they cannot normally keep the whole environment and replace only
the feed, editor, uploader, profile, or media experience that disappoints them.

Napplets move the boundary inward. A feature becomes a small replaceable
experience, while a trusted runtime supplies identity, policy, infrastructure,
and composition. That changes competition from "which monolithic client wins?"
to "which component best serves this role here?" The user can preserve the
product they understand while replacing one decision at a time.

Evidence: `01:04-05:59`, `25:12-25:38`, `45:52-51:04`.

## The philosophy

### Sovereignty through replaceability

Sovereignty is not merely holding a key or choosing a relay. It is having
credible exit at the level where dissatisfaction occurs. Replaceability makes
software choice granular; portability makes that choice durable across
runtimes; host composition lets those freedoms add up to a usable product.

This philosophy rejects both the closed monolith and the fantasy that every
small component should become its own infrastructure stack. The napplet is
valuable because it owns less.

Evidence: `01:04-02:17`, `48:22-51:04`.

### Power is removed before it is reintroduced

The design emerged by subtracting iframe authority until communication was
essentially a typed `postMessage` seam. Capabilities are then reintroduced as
inspectable, revocable runtime services. A napplet says what it wants; the
runtime decides whether, how, and through which provider it happens.

The upload example makes the philosophy concrete. The component asks to upload.
The runtime may choose Blossom, HashTree, IPFS, Google Drive, a prompt, or a
future provider without changing the napplet's product intent. Signing,
encryption, relay selection, and resource access follow the same division.

Evidence: `05:59-09:32`, `12:53-21:04`.

### The runtime is an operating system, not a permissive iframe host

The operating-system analogy is functional. The runtime mediates scarce
resources, user authority, provider choice, pressure, lifecycle, and
observation. Cleartext requests at the trusted boundary are intentional: a
runtime cannot protect the user from behavior it is forbidden to inspect.
Sandboxing and CSP are therefore paired with capability design, consent,
limits, and diagnostics rather than treated as the complete security model.

Evidence: `14:15-15:06`, `18:59-21:04`, `29:26-30:40`.

### High-level help with explicit escape hatches

Recreating all of Nostr inside a sub-protocol would preserve the complexity the
model is trying to remove. Most components should ask for a user-level outcome,
such as outbox-aware retrieval, publishing, or upload. Lower-level relay access
still matters for truly relay-local products, but it is an escape hatch rather
than the default abstraction.

This is a philosophy of progressive disclosure: make the safe, optimized path
easy without making unusual but legitimate software impossible.

Evidence: `07:40-09:32`, `1:03:14-1:14:29`.

## The grand vision

The long horizon is a user-owned software environment whose shape is not fixed
by one vendor:

- a coherent social client can be assembled from independently replaceable
  feeds, profiles, composers, media tools, and detail views;
- the same component can run in a minimal browser host, a polished native app,
  a desktop/window system, a game or mod environment, a collaboration tool, or
  an experimental microkernel;
- the runtime becomes a translation and optimization layer over mature Nostr
  engines and platform services;
- developers compete on focused experiences instead of repeatedly rebuilding
  keys, caching, routing, permissions, and deployment;
- agents can generate and combine smaller legible artifacts more reliably than
  entire clients;
- users can open a link immediately, later install or package a curated product,
  and still retain the right to replace one role.

The endpoint is not a desktop full of visibly unrelated mini-apps. It is
software that feels intentionally designed while remaining internally
substitutable. Composition is successful when ordinary users can ignore the
seams and powerful users can exercise them.

Evidence: `10:21-11:22`, `31:30-36:13`, `39:35-50:33`.

## The load-bearing tensions

### Cohesion versus replaceability

Curated components can share a design language and feel like one product.
Arbitrary replacements may not. If the host standardizes too little, the result
feels fragmented; if it standardizes layout and behavior too aggressively, it
strangles the independent expression that makes replacement valuable.

The resolution is layered: host-owned chrome, navigation, focus, accessibility,
tokens, and presentation slots; component-owned feature experience; explicit
compatibility and visual-quality signals rather than pretending they are the
same.

### Portability versus optimization

A transport-neutral intent can survive web, native, and unusual runtimes.
Platform-specific projections can be safer, faster, and more capable. The seam
must remain stable while trusted execution changes beneath it. Otherwise "native
Napplets" becomes either Kehto copied mechanically or a private plugin system
that no independent napplet can enter.

Evidence: `31:30-36:13`, `45:52-48:22`.

### Legibility versus completeness

Archetypes and intent vocabulary let independently built components understand
one another. Exhaustively standardizing every payload too early would freeze
weak ideas; leaving semantics implicit produces fragile interoperability.
Sandwich favors learning from working examples and agent-legible conventions,
then hardening what survives.

The advisor should therefore separate experiment, product convention, active
NAP, and proposed NIP—and demand executable interoperability before promotion.

Evidence: `09:34-10:21`, `39:35-43:27`.

### Convenience versus authority

Fast generation, publication, and permissive helpers make the model exciting.
The same convenience can hide signer access, direct networking, weak artifact
identity, unbounded work, or stale grants. A polished demo is not security
evidence. Every convenient abstraction needs an authority ledger and a
falsifier.

Evidence: `10:21-11:22`, `18:59-21:04`, `29:26-30:40`.

### Permissionless discovery versus trustworthy execution

Manifest and blob resolution can be open, gateway-independent, and offline
capable. Execution still requires signatures, hashes, exact principals,
capability preflight, and policy. Permissionless availability should eliminate
gatekeepers, not verification.

Evidence: `31:30-33:34`, `59:12-1:03:14`.

### Ecosystem motion versus compatibility

The interview celebrates rapid exploration while acknowledging that clients
and integrations had not followed every moving revision. Reference stacks can
create momentum, but one author's complete stack can also become an accidental
monoculture. Independent implementations and unchanged legacy artifacts are the
test of an ecosystem, not repository count.

Evidence: `21:38-24:59`, `37:03-37:22`, `59:12-1:03:14`.

## Engineering posture

The interview's process mirrors its architecture: begin with a high-level
model, repeatedly compress it, split it into small contracts, and subject each
contract to independent review and negative tests. Novel areas require
source-grounded research, not confident model priors. TDD/BDD, strict PR policy,
anti-slop checks, and model diversity are proposed as defenses against code and
tests that merely reinforce the first implementation.

For this advisor, that means philosophy must end in proof: exact revisions,
executable fixtures, two-sided interoperability, denial cases, pressure limits,
teardown, and honest compatibility reports.

Evidence: `1:19:32-1:32:30`.

## Advisor stance

Use the interview to recover the purpose behind the machinery:

```text
replaceable user experiences
inside a coherent user-owned product
over a trusted, inspectable, portable capability boundary
```

Then verify the machinery live. The interview does not determine current wire
formats, package exports, NAP status, manifest kinds, exact intent spelling,
interoperability, or the security of any named runtime. Kehto is evidence, not
the definition. The vision is the compass; current specs, locks, source, and
tests are the map.
