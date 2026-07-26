# Advisory playbooks

## Universal intake

Answer these before choosing a solution:

```text
consumer role
desired user outcome
web, native, WASM, or abstract seam
live upstream or pinned compatibility
manifest/artifact revision
NAP/package/runtime versions
hard and optional capabilities
authority and data involved
platform constraints
proof required
```

Ask the user only when a missing answer changes product intent, authority, or a
materially risky choice. Otherwise inspect the repository and proceed.

## Napplet product design

1. State the single job and user-visible outcome.
2. Identify what belongs in host chrome or another napplet.
3. Choose a known archetype only when the role actually matches.
4. Inventory every required capability and make optional ones degrade.
5. Define handoffs through intent, INC, a convention, clipboard, or host UI.
6. Keep durable protocol truth, keys, routing, and OS work in the runtime.
7. Define signed-out, offline, denied, unsupported, and update experiences.
8. Verify accessibility and responsive behavior inside the host surface.

Falsifier: if the design needs tabs for unrelated application areas, its own
signer, relay pool, database, notification center, and window manager, it is
probably rebuilding a monolith.

## Compose a cohesive application

1. Describe the product in user language, then identify stable host chrome and
   focused feature roles.
2. Give each role an archetype plus supported actions and typed payload fields.
   Do not begin with routes, filenames, or implementation dTags.
3. Define one composition manifest containing routes, presentation, defaults,
   required capabilities, a branded theme-profile reference, fallbacks, and
   migration policy.
4. Resolve every default or replacement to a verified exact build. Preflight
   domains and action compatibility before activation.
5. Route napplet intent through the trusted runtime. Parse deep links into the
   same typed intent so refresh, history, and in-product navigation agree.
6. Deliver destination payloads through a bounded, source-bound session channel
   after the destination is ready. Treat the chosen channel as a projection or
   product convention unless a current NAP standardizes it.
7. Let developers generate a branded shell with sane defaults from the
   composition manifest. Let users edit a separate overlay that replaces one
   role without mutating the product's base definition.
8. On replacement, show compatibility and permission consequences, bind new
   grants to the new exact build, preserve the prior selection for rollback,
   and define component-state migration explicitly.
9. Project one revisioned design system into native chrome and every napplet.
   Test theme compliance independently from protocol compatibility.
10. Test the curated product as one accessible interface and each replacement
    as an isolated trust transition.

Falsifiers: direct top-level navigation from a napplet, URL knowledge embedded
in feature code, grants inherited by a replacement, archetype tags accepted
without verified bytes, unbounded cross-frame broadcast, or a theme that can
expand authority.

## Port an existing Nostr app

Create an authority inventory:

| Feature | Existing authority | Target boundary | Hard/optional |
| --- | --- | --- | --- |

Typical mapping:

| Existing code | Preferred boundary |
| --- | --- |
| NIP-65 routing, fanout, dedup, event validation | `outbox` |
| Explicit group/tool relay | `relay` escape hatch |
| Signing, reactions, lists, DM encryption | intent-oriented NAP domain |
| Browser storage | `storage` |
| Fetch/images/media | `resource`, `media`, `link`, or `upload` |
| Global shortcuts | `keys` |
| Cross-feature navigation | `intent`, INC, or convention |

Remove app-owned authority; do not wrap it behind a local compatibility shim.
Split unrelated screens into focused napplets. Record package/spec gaps instead
of inventing private APIs.

## Napplet implementation

1. Inspect exact current package exports and canonical NAP.
2. Build from a maintained starter when appropriate.
3. Declare bare hard domains in `requires`.
4. Use shipped SDK helpers; use direct injected-domain access only where the
   chosen package contract requires it.
5. Keep the artifact self-contained for the current proposed profile, unless a
   pinned legacy contract explicitly accepts external assets.
6. Implement teardown for subscriptions, object URLs, media/device sessions,
   timers, and callbacks.
7. Expose precise denial and unsupported states in the UI.

Do not recommend `shell.ready`, `shell.supports`, or domain-presence-only
availability from memory. Check the chosen NIP/NAP/package pin because this
surface has drifted.

## Web runtime implementation

Work in this order:

1. Pin NIP, NAP, package, and corpus revisions.
2. Implement manifest resolution and executable-byte verification.
3. Build the trusted local shell and opaque iframe boundary.
4. Prove injection occurs before authored scripts.
5. Bind source window to exact session/principal.
6. Implement required-domain preflight.
7. Add one truthful provider at a time with limits and contract tests.
8. Add grants, consent, activity, diagnostics, and teardown.
9. Run a legacy corpus and adversarial tests.
10. Add composition and product UX.

Do not start with dozens of domain stubs or a polished desktop.

## Native runtime implementation

Keep dependency direction:

```text
platform/app -> runtime core -> supported Nostr facade
platform/app -> trusted WebView adapter -> runtime core
```

Rust/shared core is a good owner for product state machines, policy, validation,
persistence, routing decisions, quotas, compatibility, lifecycle, and error
semantics. Native UI owns rendering, accessibility, platform lifecycle, and
bounded OS execution, reporting raw results to the core.

Prove:

- untrusted content cannot address the native bridge;
- the trusted shell forwards only validated envelopes plus an opaque native
  session handle;
- a WebView crash releases resources;
- durable Nostr writes outlive the renderer;
- runtime storage never becomes a second Nostr truth;
- unsupported platform domains remain absent;
- legacy napplets run unchanged before additive extensions are claimed.

## NAP proposal review

First apply the boundary rule:

- Runtime-provided API surface -> NAP.
- Host binding mechanics -> projection.
- Napplet-agreed message semantics -> convention.
- Canonical napplet role -> archetype.
- Product-specific UX/policy -> implementation.

Then review:

```text
domain ownership and non-overlap
operations and direction
request/result/event correlation
typed refusal and error semantics
limits, cancellation, and teardown
dependencies on other domains
permission and user-visible effects
privacy and retained data
transport neutrality
forward/version compatibility
at least two implementation perspectives
executable fixtures and negative cases
```

A helpful proposal names what it refuses to standardize.

## Compatibility movement

Upstream movement is a dedicated change, not a dependency bump hidden in feature
work.

1. Record old and new commits/versions.
2. Diff NIP text, NAP contracts, package exports, and generated fixtures.
3. Regenerate inventories and conformance artifacts.
4. Run accepted legacy napplets without source/build changes.
5. Publish a drift report with accepted, adapted, rejected, and unknown rows.
6. Obtain product, compatibility, security, and underlying-engine signoff.
7. Update the lock only after proof.

Keep live-upstream advice separate from the behavior a shipping product has
promised.

## Security review output

Use this compact structure:

```text
asset / authority
attacker
entry point
trust boundary
existing mitigation
falsifier
impact
recommendation
verification
residual risk
```

Prioritize executable-byte substitution, source spoofing, bridge exposure,
grant confusion, signer abuse, direct-network exfiltration, SSRF, unbounded
resources, cross-session storage, stale callbacks, and misleading diagnostics.

## Debugging matrix

| Symptom | Fast falsifier |
| --- | --- |
| Blank iframe | CSP console, module `Origin: null`, injection order, `srcdoc` |
| No message | raw clone error, plain-object/type guard, parent receiver |
| Message silently disappears | unknown type vs unmapped source |
| Domain missing | manifest `requires`, runtime inventory, injection result |
| Domain present but call fails | operation version, grant, provider, input cap |
| Wrong identity/grant | verified bytes, author/dTag/hash, window-session map |
| Stale result after close | request registry and generation/session token |
| Memory grows | subscriptions, buffers, object URLs, WebViews, provider tasks |
| Works in one runtime | pin, domain versions, optional fallback, private API use |
| Gateway/cache mismatch | per-path hash and aggregate recomputation |

Trace the state chain. Do not jump directly to napplet code when the real failure
is manifest resolution, preflight, a provider, or lifecycle.

## Implementation comparison

Compare evidence, not marketing:

| Dimension | Runtime A | Runtime B | Source |
| --- | --- | --- | --- |

Include pin, artifact modes, identity, handshake/injection, domains, grants,
network policy, storage, limits, diagnostics, corpus, platforms, deviations,
and observed tests. "Supports NIP-5D" is too broad without a revision.

## Answer template

Lead with the recommendation. Then, as needed:

1. `Status and source mode`
2. `What is true`
3. `Implementation-specific behavior`
4. `Recommendation`
5. `Security/compatibility consequences`
6. `How to prove it`
7. `Open proposal or uncertainty`

Use citations close to claims. Name commits and versions for drift-prone facts.
When a source conflict matters, show a two-row table rather than silently
choosing one.
