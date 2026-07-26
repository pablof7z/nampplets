# Cohesive multi-napplet products

The product goal is stronger than putting several iframes beside each other:

- a developer can define a collection of napplets and generate a polished
  shell with safe, useful defaults;
- a normal user experiences one coherent product, not a protocol workbench;
- an advanced user can replace any feature role with another compatible
  napplet without replacing the whole app or silently widening authority.

This reference separates the portable contract, product composition, exact
component selection, and visual system needed to make that credible.

## The three-layer model

```text
portable contract
  archetype + action + typed payload + capability expectations
product composition
  routes + presentation + chrome + defaults + theme + fallback policy
exact selection
  verified author + dTag + aggregateHash assigned to each role
```

The first layer allows independent napplets and runtimes to interoperate. The
second makes a specific product. The third determines what code and authority
are active now. Do not collapse them into one manifest or treat one layer as
proof of another.

## stlstr implementation study

Evidence was inspected at
`hzrd149/stlstr@69c220d27ae0f5d5a9a3a80928a4e284af338c4f` on
2026-07-26. stlstr is implementation evidence, not a normative source.

### What the shell owns

The React/Vite host stays intentionally small but decisive. It owns persistent
chrome, identity, relay and Blossom policy, routes, browser history, overlays,
settings, the iframe boundary, and NAP providers. Product features live in
eleven built-in Svelte napplets.

`services/intent-map.ts` is the product's composition vocabulary:

- each archetype maps to a default dTag and route identity;
- each action documents its payload fields;
- `intentToHref` and `intentFromLocation` make URLs and typed intents inverse
  representations;
- the STL preview is a shell-owned overlay over a base route;
- convention identifiers use `napplet:<archetype>/<action>`.

Feature napplets therefore request a role:

```text
intent.open("printable-detail", { address })
```

They do not choose a URL, frame, or concrete handler. The shell validates the
archetype, action, handler preference, protocol, and payload before resolving a
route. It sends success before navigation because navigation unmounts the
calling frame.

### Destination delivery

NAP-INTENT is used for the outbound request. The destination payload travels
over a targeted INC event:

1. the shell seeds one bounded pending payload before assigning `srcdoc`;
2. the destination subscribes, then emits `<archetype>:ready`;
3. the shell verifies the source window and sends the payload only to that
   frame;
4. an already-mounted frame can receive a redelivery;
5. disposal clears delivery state.

This ready/delivery exchange is a stlstr projection choice. Do not claim that
the current NAP-INTENT standardizes inbound route payload delivery.

### Packaging and replacement

The production build first builds every bundled napplet, emits its artifact at
`napplets/<dTag>/`, and generates `napplets.json`. This is a concrete example of
a developer-curated product shipping sane offline defaults in one deployable
shell.

Settings store a selected manifest override for each archetype. The user can
discover candidates, paste an `naddr`, select a replacement, or restore the
default. A Napplets reference page exposes the product's archetypes, actions,
payloads, and published component addresses.

Shared DaisyUI styling and an app-owned `napplet-kit` make the curated napplets
feel related. The shell also requires napplets to omit duplicate title bars,
card wrappers, and borders so it can present them as seamless surfaces.

### Evidence and limitations

Observed locally:

- dependency installation passed;
- three static source-policy tests passed;
- all twenty-four napplet/library type-check and build tasks passed;
- the production app build passed and bundled all eleven napplets;
- committed browser tests cover deep links, targeted delivery, handler
  selection, overlay history, and cross-frame isolation.

The browser tests were inspected but not run: no local Chromium or required
relay was available. The deployed site returned HTTP 200, but its visual
coherence was not independently inspected in a browser.

Important limitations for a native NMP runtime:

- candidate compatibility indicators are explicitly advisory;
- the picker can select a loadable manifest that does not advertise the target
  archetype;
- the inspected artifact fetch path is not evidence of signature, per-path
  hash, aggregate hash, exact-principal, and grant enforcement;
- localStorage is suitable for UI preference in that app, not authoritative
  native composition or grant state;
- a shared component library helps curated defaults but cannot guarantee that
  an arbitrary third-party replacement matches the product visually.

Copy the routing and product patterns, not these trust shortcuts.

## NMP-native composition model

Rust should own a durable `AppComposition` state machine:

```text
product id and composition revision
roles: archetype, actions, payload schema, route, presentation
default exact build per role
required and optional capability policy
theme/design tokens and accessibility contract
user selection overlay
preflight, activation, fallback, rollback, migration
bounded navigation and delivery state
```

The state machine resolves manifests and verified bytes through the runtime's
artifact boundary, then binds the active selection to
`(manifest author, dTag, aggregateHash)`. NMP remains the only owner of Nostr
events, routing, signer, pending writes, replacement/deletion semantics, and
receipts.

Native UI executes Rust decisions: present a route, preserve or replace a
WebView, focus an overlay, update accessibility, and report raw lifecycle
results. It must not independently choose handlers, grants, compatibility,
fallbacks, or state migrations.

The intent router should:

1. authenticate the source session and exact principal;
2. validate archetype, action, payload size and schema;
3. resolve the active exact build for the target role;
4. check action/version compatibility and required capabilities;
5. return a bounded success or typed refusal to the caller;
6. commit the navigation/presentation transition;
7. deliver the payload through a bounded source-bound destination session;
8. cancel delivery on replacement, crash, close, or generation mismatch.

Deep links must parse into the same typed intent path. A URL is an external
serialization, not a second routing authority.

## Developer shell generator

The developer tool should accept a declarative composition manifest and produce:

- a native shell project with branded chrome, navigation, routes, settings,
  accessibility, and design tokens;
- pinned default exact builds plus reproducible artifact inventory;
- generated intent/archetype types and route adapters;
- provider requirements and an unsupported-domain report;
- consent, activity, diagnostics, update, rollback, and replacement surfaces;
- fixtures for deep links, intent routing, destination readiness, lifecycle,
  denied capabilities, and component substitution;
- a lock/report that makes upstream and exact-build movement explicit.

The generator should have strong defaults but produce ordinary editable source.
It must not bake one napplet's dTag into feature-to-feature calls or turn a
starter's private convention into ecosystem law.

## End-user composer

The user-facing composer edits an overlay above the product definition:

```text
role -> selected verified exact build
```

For each candidate it should explain:

- which archetype, actions, and payload versions are advertised and verified;
- which required capabilities are supported by this runtime;
- what new permissions or data visibility the replacement requests;
- whether scoped state is new, migratable, intentionally shared, or abandoned;
- which previous selection can be restored.

Activation is a transaction: resolve and verify, preflight, obtain exact-build
grants, stage, health-check, commit, or roll back. Never silently reuse the old
component's grants. Keep the prior build available until the new selection has
crossed its declared health boundary.

## Cohesion without hidden authority

A product can feel monolithic in normal use while remaining componentized:

- the shell owns global navigation, history, focus, accessibility, account
  state, notifications, permission UI, loading/error language, and transitions;
- a design contract supplies semantic tokens, density, typography, spacing,
  motion, and surface rules without granting ambient authority;
- archetypes use product language and stable actions rather than implementation
  names;
- replacements render inside host-owned presentation slots and may declare
  supported layout modes;
- diagnostics and advanced settings reveal exact components and boundaries
  without forcing protocol vocabulary into everyday UI.

Visual consistency is not compatibility. A technically compatible replacement
may be visually poor; a visually matching component may request unacceptable
authority. Score and disclose those axes separately. Use
`visual-identity-and-themes.md` for the complete theme pipeline and conformance
model.

## Proof matrix

| Claim | Minimum proof |
| --- | --- |
| Intent portability | Two independently built handlers and callers |
| Deep-link equivalence | URL and in-app request produce identical typed intent |
| Isolation | Cross-frame spoof, broadcast, stale-session, and oversize negatives |
| Safe replacement | Exact-build grant separation, rollback, crash, and state tests |
| Product cohesion | Accessibility, focus, history, responsive, and visual review |
| Generated shell | Reproducible build from manifest plus editable output |
| NMP boundary | No second Nostr truth; supported facade only |

Do not call the product composable merely because it can mount multiple
napplets. Prove typed handoff, independent replacement, authority isolation,
coherent lifecycle, and rollback.
