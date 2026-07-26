# Ecosystem map

No single repository is the Napplets ecosystem. Compare each surface by the
contract it owns and the evidence it provides.

## NIP and NAP sources

| Surface | Role | Do not infer |
| --- | --- | --- |
| NIP-5D PR #2303 | Living proposed web projection and napplet manifest profile | Merged or stable standard |
| NIP-5A | Static-site manifest/hash substrate | Napplet runtime policy |
| `napplet/naps` | Capability, convention, archetype, and projection registry | Package availability or complete implementation |
| NAP PR discussion | Design evidence and unresolved tradeoffs | Active contract |

Always check the current file and status. The PR description, registry README,
package docs, and live head can describe different generations.

## @napplet authoring and protocol packages

Observed package surface on 2026-07-26:

| Package | Role |
| --- | --- |
| `@napplet/core` | Envelope types, domain types, dispatch, clone-safe boundary helpers |
| `@napplet/shim` | Runtime-side injection and web transport helpers |
| `@napplet/sdk` | Napplet-author wrappers around injected domains |
| `@napplet/nap` | Per-domain types, shims, and helpers |
| `@napplet/vite-plugin` | Build output, hashes, manifest generation, `requires`, archetypes |
| `@napplet/cli` | Create, configure, publish, signer, relay, and Blossom workflows |
| `@napplet/conformance` | Programmatic reference checks and shell behavior |
| `@napplet/conformance-cli` | Real-browser conformance runner |
| `@napplet/skills` | Authoring, porting, building, and testing guidance |
| `@napplet/boilerplate` | Maintained Vite/TypeScript starting point |

The current package repository exposes implementations for domains including:

```text
relay identity storage inc theme keys media notify config resource
cvm outbox upload intent ble webrtc link count lists serial common dm
```

`ifc` appears as a compatibility alias in some package revisions; new designs
use `inc`. Package presence is not proof that the corresponding NAP is merged or
semantically final.

### Authoring advice

- Prefer the highest-level domain that owns the user intent.
- Use `outbox` for normal author-aware reads/publishes; keep `relay` for a named
  relay-local reason.
- Use social/action domains such as `common`, `lists`, `count`, and `dm` rather
  than rebuilding signing, routing, replacement, or encryption in the iframe.
- Use SDK helpers that actually ship in the chosen version.
- Declare only hard dependencies in `requires`; gate optional domains by the
  chosen projection's availability contract.
- Treat package tutorials as implementation help, not normative text.

### Tooling proof

The strongest authoring handoff normally includes:

1. self-contained build inspection;
2. manifest and hash inspection;
3. forbidden-authority scan;
4. browser conformance against a reference shell;
5. optional-domain fallback tests;
6. a live compatible-runtime smoke.

A conformance tool validates what it tests. It does not make a stale package
contract current or prove all runtime policy and UX.

## Kehto

Kehto is an early web runtime implementation and a rich evidence source. It is
not the definition of Napplets.

Observed package structure:

| Package | Kehto responsibility |
| --- | --- |
| `@kehto/acl` | Capability state and enforcement primitives |
| `@kehto/runtime` | Browser-neutral dispatch, sessions, service routing, policy |
| `@kehto/shell` | Iframe lifecycle, source mapping, transport, namespace injection |
| `@kehto/services` | Reference capability providers |
| `@kehto/firewall` | Pressure and rate controls |
| `@kehto/nip` | NIP utilities, resolution, verification, artifact caching |
| `@kehto/wm` | Host-owned window-management contracts |
| `@kehto/paja` / CLI | Real-runtime local authoring workshop |
| playground | Runtime visualization, napplet corpus, browser test target |

Kehto's docs are valuable for:

- runtime/shell/service separation;
- Paja development and diagnostics;
- grant and provider examples;
- opaque-origin debugging;
- resource-fetch policy;
- artifact cache design;
- test corpus and migration history.

Kehto-specific choices include package boundaries, ACL names, provider options,
cache defaults, UI, and release cadence. Label them as Kehto choices.

### Kehto freshness hazards

Some current docs preserve production-equivalent gateway-navigation guidance
from an older design. Current source has a resolver that queries relays,
verifies the event, fetches and re-hashes Blossom blobs, computes identity, and
loads verified HTML through `srcdoc`. Compare the current code path, NIP head,
and docs before advising.

The docs also distinguish current guidance from a large migration archive.
Historical migration files explain how earlier AUTH, capability, signer, and
envelope models changed; they are not current integration rules.

## Native runtimes

A native runtime can keep the same portable napplet surface while moving trusted
work into Rust/Swift/Kotlin and OS services.

Useful architecture:

```text
native app and UI
  -> native runtime state machines and NAP provider registry
    -> supported Nostr facade/engine
  -> trusted local web shell
    -> untrusted WebView napplet
```

The native application owns UX, platform lifecycle, permissions, and bounded OS
capability execution. A shared core owns policy, grants, sessions, quotas,
artifact verification, compatibility, and error semantics. A Nostr engine owns
events, routing, replacement, deletion, signers, pending writes, and receipts.

Downloaded components remain web-rendered. Calling a product "native" does not
authorize downloaded Swift/Kotlin/Rust modules or expose the WebView to a raw
native bridge.

A host-owned state/actions surface is a plausible additive profile: the host
streams revisioned state down and receives typed actions up. It must not replace
or weaken unchanged legacy napplet support, and it must not let renderers own
canonical Nostr state.

### Observed Nampplets baseline

The Nampplets evidence checked on 2026-07-26 is deliberately narrower than its
architecture:

- its compatibility baseline is unratified and pins older NIP-5D, NAP,
  package, and Kehto revisions;
- the live NIP-5D CSP advisory is newer than that pin and is upstream drift,
  not silently accepted product behavior;
- it verifies and caches kinds `5129` and `15129` but refuses to execute them
  until it has a collision-free principal mapping for their missing signed
  `dTag`; only kind `35129` currently supplies its executable exact-build
  identity;
- M0 advertises no executable providers, including `resource` and `link`;
- its imported Kehto corpus is reported as `built-not-run`, not native runtime
  compatibility proof.

Refresh the lock, provider matrix, compatibility report, and executable
conformance evidence before making a current Nampplets claim. Keep these
implementation gaps separate from the portable NIP/NAP model.

## stlstr: a composed product case study

At revision `69c220d27ae0f5d5a9a3a80928a4e284af338c4f`, stlstr presents a
Thingiverse-like product through a small host and eleven built-in feature
napplets. Its source is useful evidence that a product can own stable chrome,
routes, history, overlays, identity, provider policy, and replacement controls
while feature napplets call roles such as `printable-detail` instead of knowing
which component or URL handles them.

Its build packages all default napplet artifacts and a registry into one
self-contained deployment. Its settings UI lets a user replace the napplet
assigned to an archetype. Shared UI helpers and design-system use help the
curated defaults read as one product.

This does not prove visual interchangeability for arbitrary third-party
napplets, protocol-level compatibility, or the stronger artifact/grant boundary
required by a native NMP runtime. Keep its portable ideas, product choices, and
security gaps distinct. The detailed review and native design translation live
in `cohesive-composition.md`.

## Other approaches and prototypes

The Sandwich interview mentions:

- early `napp.run` Tauri work;
- a Chromium/Thorium fork experiment;
- Fiatjaf/Balazs Nostr apps/naps;
- a simple Yakihonne surface;
- Soapbox Tiles, described as Lua-based and native-oriented;
- Hypnote as a spiritually similar composition idea;
- integrations or experiments in Amethyst and Nostrudel;
- game/mod, media, collaboration, desktop, and microkernel runtimes.

These references prove a broader design space, not current interoperability.
Locate the current repository, build, protocol revision, and maintainer claim
before comparing them.

## Product-shape comparison

| Question | Napplet author | Runtime/product |
| --- | --- | --- |
| What renders? | One focused experience | Composition and host chrome |
| Who owns keys? | Never the napplet | Runtime/signer system |
| Who chooses relays? | Expresses intent or explicit escape hatch | Routing/provider policy |
| Who persists? | Scoped requests/transient UI | Exact-principal KV and canonical engine |
| Who applies limits? | Handles refusals and degradation | Enforces quotas and pressure |
| Who performs OS work? | Requests capability | Native/browser provider |
| Who proves compatibility? | Build and conformance | Corpus, providers, lifecycle, product UX |

## Comparison rule

When comparing runtimes, use the same matrix:

```text
compatibility pin
artifact modes and verification
identity and principal
injection/handshake revision
implemented domains and versions
grants and user consent
network/resource policy
storage scope and quotas
composition and lifecycle
diagnostics and activity
conformance corpus
platforms and accessibility
known deviations
```

Do not rank runtimes by domain count alone. A smaller truthful capability
surface is safer and more interoperable than a large surface of placeholders.
