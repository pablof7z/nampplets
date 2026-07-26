---
name: nip-5d-napplets
description: Source-grounded advisory workflow for NIP-5D and the full Napplets ecosystem, including protocol and NAP design, napplet authoring, web and native runtimes, security, compatibility, tooling, conformance, and product composition.
---

# NIP-5D and Napplets advisory

Use this skill whenever the work concerns NIP-5D, napplets, NAP capabilities,
napplet manifests or artifacts, shell/runtime behavior, compatible SDKs and
tooling, interoperability, or a web/native projection.

## Start with authority and time

NIP-5D and the NAP ecosystem move quickly. Before giving concrete advice:

1. Identify whether the task follows a repository's pinned compatibility
   contract or live upstream.
2. Record the relevant commit, package version, PR state, and observation date.
3. Open the current NIP-5D PR for core web-projection rules and the exact NAP
   document for domain operations and semantics.
4. Inspect the implementation or package source for claims about what ships.
5. Treat the bundled references as a model and source index. Refresh facts that
   could have drifted.

Use this precedence when sources conflict:

1. Explicit consumer compatibility lock and accepted fixtures for that product.
2. Living NIP-5D proposal for the current proposed web projection.
3. The relevant NAP document for a capability domain.
4. Merged adjacent NIPs such as NIP-5A for the substrate they own.
5. Exact package or runtime source and tests for implementation behavior.
6. Documentation, tutorials, demos, and conformance tools.
7. Interviews, issue discussion, author intent, and design inference.

A higher item does not erase a lower item's evidence about its own layer. A
package can accurately implement an older pin while differing from live
upstream.

## Load the relevant references

- Read `references/source-ledger.md` for authoritative and implementation
  sources, status vocabulary, and freshness rules.
- Read `references/protocol-model.md` for the layered model, identity, manifests,
  web projection, NAPs, conventions, and archetypes.
- Read `references/ecosystem-map.md` before comparing Kehto, @napplet packages,
  native runtimes, prototypes, or tools.
- Read `references/security-and-design.md` for threat modeling, permission
  binding, artifact resolution, capability providers, limits, and teardown.
- Read `references/advisory-playbooks.md` for role-specific workflows,
  debugging, protocol review, migration, and validation.
- Read `references/cohesive-composition.md` for intent routing, product
  composition, developer-generated shells, user-selected replacements, and the
  dated stlstr implementation study.
- Read `references/visual-identity-and-themes.md` for the current NAP-THEME
  minimum, a composition-owned design system, native/web projection, branded
  defaults, third-party replacements, and visual conformance.
- Read `references/sandwich-interview.md` when discussing the system's product
  thesis, design history, native direction, or the intent behind the model.

Do not read only the Kehto section when the question is about Napplets generally.

## Diagnose the layer before solving

Place every concern in one primary owning layer:

| Layer | Owns |
| --- | --- |
| Nostr / adjacent NIPs | Events, signatures, relay protocol, nsite substrate |
| NIP-5D | Proposed web projection and napplet-specific manifest profile |
| NAP | One runtime-provided capability contract |
| Projection | Binding a NAP seam to web, native IPC/FFI, WASM, or another host |
| Convention | Napplet-agreed message meaning, not a runtime API |
| Archetype | Canonical role such as note, profile, feed, or composer |
| Package/tool | One implementation of authoring, shim, SDK, deploy, or testing |
| Runtime/product | Policy, providers, composition, UX, persistence, and limits |

If a design spans layers, keep dependency direction explicit. Do not move
product policy into a NIP or transport mechanics into a transport-neutral NAP.

For a composed app, identify three distinct artifacts:

```text
portable archetype/action contract
product composition manifest and shell policy
verified exact napplet build selected for each role
```

A coherent interface may hide those seams in normal use, but permissions,
diagnostics, replacement, and rollback must retain them.

## Use an evidence table for consequential advice

For design, security, compatibility, or migration decisions, write a small
ledger:

| Claim | Class | Source/revision | Confidence | Consequence |
| --- | --- | --- | --- | --- |

Use classes such as `pinned`, `proposal`, `NAP`, `implementation`, `observed`,
`intent`, and `inference`. Mark disagreements rather than averaging them.

## Preserve the authority boundary

The untrusted napplet renders and expresses intent. The trusted runtime mediates
capabilities and owns policy. The platform owns actual OS integration. Nostr
engines or facades retain canonical protocol truth.

Audit the full path:

```text
verified artifact -> exact principal -> session -> exposed domain
-> validated request -> policy/grant -> provider -> bounded result/refusal
-> diagnostics -> cancellation/teardown
```

Fail closed on missing identity, unknown source, invalid artifact, unsupported
required domain, stale or mismatched grant, unbounded input, provider failure,
and teardown races.

## Treat unknowns honestly

Use:

- "The current proposal says..." for open NIP-5D text.
- "The NAP draft/active document says..." for domain contracts.
- "Version X implements..." for packages.
- "Runtime Y chooses..." for product policy.
- "The interview describes the intent as..." for oral history.
- "I infer..." for synthesis.

Do not use "NIP-5D requires" unless the cited current or pinned NIP-5D text owns
that rule.

## Finish with proof

Match validation to the claim:

- Napplet authoring: build, static authority scan, manifest inspection, real
  sandboxed conformance, optional-domain fallback, live runtime smoke.
- Runtime: artifact falsifiers, source-window binding, injection-before-script,
  required-domain preflight, provider contract tests, exact-build grant tests,
  resource caps, cancellation, crash/restart, and corpus compatibility.
- Protocol: two independent implementations or executable fixtures, unknown
  message behavior, version skew, negative cases, transport-neutral review, and
  migration impact.
- Product: user-visible consent/refusal, install/update/rollback, activity and
  diagnostics, accessibility, composition, and state preservation.

Report what was proven, what remains proposed, and what was not tested.
