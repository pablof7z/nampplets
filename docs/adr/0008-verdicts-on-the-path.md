# ADR 0008: Verdicts on the path, evidence off the path

- Status: Accepted architecture
- Date: 2026-07-26
- Invariants: I-01, I-02
- Supersedes no ADR. Constrains every native presentation surface.

## Context

The runtime is correct and the shell that renders it is not usable by anyone
who does not already know the protocol.

Every native surface in `apps/workbench-macos` and `apps/workbench-ios` was
built as an instrument for verifying that the runtime behaves. The install
sheet is a provenance affidavit. The activity drawer is an oscilloscope. The
account form is a key-management console. The library is a build inventory.
The browse screen explains that "the pinned NMP facade does not expose NIP-50
full-text search" before it shows a single napplet.

Each of those surfaces is defensible as an instrument. Collectively they are
the wrong genre. A person who wants to run a napplet is required to read
`aggregateHash`, `dTag`, `coordinate`, `manifest author`, `principal`, and raw
capability domain strings in order to make decisions the runtime has already
made correctly on their behalf.

Three specific failures motivated this ADR.

**The state machine leaked into the interface.** `runtime-core` models "a
registered signer" and "the active account" as distinct states because they
genuinely are distinct: a profile may hold several registered accounts and
activate one. That is correct. Projecting it 1:1 into the UI means a person
holding exactly one key is asked twice whether they would like to sign in --
once to register the secret, once to activate it. The runtime's plurality
became the user's ceremony.

**The heaviest consent wall guards the lightest action.** The catalog install
review presents publisher, public key, coordinate, exact hash, provenance
sources, raw capability domains, a platform matrix, install relationship, and
warnings -- and then closes with the sentence "Installing does not launch this
napplet or grant any capability." A security affidavit gates an action that the
sheet itself certifies as inert, while the moment that actually grants
capability sits behind a second, separate wall.

**Semantic colour became the primary carrier of state.** `checkmark.seal` in
green appears on the catalog row, the install sheet, the permission sheet, the
activity header, and the library row. Red/orange/green status glyphs carry
compatibility, platform availability, severity, sensitivity, source status, and
availability. Traffic-light colour is the visual grammar of a diagnostic
dashboard; it reads as instrumentation regardless of how good the words are,
and a signal that appears on every surface has stopped being a signal.

## Decision

Native presentation is governed by one rule:

> **Verdicts on the path. Evidence off the path.**

A surface a person passes **through** in order to accomplish something shows
only a conclusion the application is willing to stand behind, stated in the
user's language. The evidence that produced that conclusion remains complete,
unsummarised, and reachable -- but always behind a deliberate, explicit move,
and never in the way of a decision.

This is a relocation, not a deletion. The ability to prove exactly what was
verified, from which source, against which hash, is this product's reason to
exist. Nothing is removed from the application; things are moved out of the
path of people who did not ask for them.

Four consequences bind implementations.

### 1. Disclosure is a typed, two-tier property, not a per-view judgement

`NappletDisclosure` has two cases, `.plain` and `.technical`, and is carried in
the SwiftUI environment. `.plain` is the default everywhere, including at the
root of every sheet.

In `.plain`, a surface must not render: an aggregate hash, an event id, a
public key in any encoding, a `dTag`, a manifest coordinate, a raw capability
domain, a relay URL, a revision number, a NIP number, a session identifier, or
the words *principal*, *manifest*, *aggregate*, *projection*, *coordinate*,
*facade*, or *exact build*.

In `.technical`, all of it is rendered verbatim, monospaced, and selectable.
Truncating, prettifying, or summarising evidence in `.technical` is a defect:
the tier exists precisely so that the plain tier may be confident.

A view never decides its own tier from context. It reads the environment. This
makes "what may an ordinary person see" one reviewable decision instead of one
per view.

### 2. The interface maps onto intent, never onto runtime states

A native flow is designed from what a person is trying to do and then bound to
whatever sequence of runtime transitions accomplishes it. Where the runtime
distinguishes states that a person in the common case does not, the interface
collapses them into one gesture and reveals the distinction only in the
situation that gives it meaning.

Concretely: adding an account registers *and* activates. The distinction
survives for a profile that holds more than one account, where switching is a
real user intent.

This does not weaken the boundary in AGENTS.md. Rust still owns the state
machine, the policy, and the validation; native still owns rendering. This ADR
says only that rendering is not transcription.

### 3. Consent is weighted by what is actually at stake

Install acquires verified bytes and grants nothing. It is presented as a light,
reversible action, and its evidence lives in `.technical`.

First run is where capability is granted. It carries the single consent moment,
phrased as what the napplet will be able to do, with the runtime's own
`recommendedDecision` preselected and per-capability scope available to anyone
who asks for it. The application never asks a person to operate a control per
capability in order to reach the common outcome.

The runtime keeps every decision it already owns. Native must not invent,
reorder, or rank decision options; `recommendedDecision` and `requestedDecision`
remain Rust's to project.

### 4. Colour reinforces state; it never carries it

Every state must be legible with colour removed. Language and typography carry
meaning; colour may only strengthen what the words already say. Semantic colour
is reserved for states that genuinely warrant alarm -- a refusal, a block, a
failure -- and is therefore rare enough to mean something when it appears.

"Verified" is not a green seal. It is the fact that the application did not ask.

### These are native tokens, not theme authority

`NappletMetrics` and the components in `NappletStyle.swift` are the Workbench's
built-in defaults for host chrome. They are not the product-wide source of
truth for appearance. If a Rust-resolved semantic theme revision lands, Rust
keeps precedence, validation, revision and active composition, and this layer
maps semantic tokens onto SwiftUI values while supplying these defaults when no
revision is resolved.

Host chrome and napplet presentation slots are distinct: the consent, browse
and library surfaces are chrome and should consume the same resolved theme as
any curated component adapter, but they are not themselves slots.

### Capability strata are Rust's to classify, not Swift's to infer

`NappletVocabulary` describes what a capability *does* in plain language. It
deliberately does **not** classify how much authority a capability carries,
and native code must not start inferring that from the domain string.

The distinction is real and the vocabulary cannot currently express it. A
capability may be a mediated outcome (this runtime's `keys` is a mediated
signing request where the secret never leaves the host), a privileged broker, a
raw escape hatch, or host-managed plumbing. Calling a mediated service "raw
access" would be as misleading as smoothing genuine ambient authority into
another calm row, and the same domain may mix levels across operations.

Rust does not project an authority class today. `sensitivity`
(ordinary/sensitive/unknown) is not sufficient to express it. Until an
authority class and reason are projected — ideally per operation, or else at
the highest authority a grant enables — native renders every capability with
the same weight and says only what it does. **An escape hatch must never be
smoothed, and a mediated service must never be exaggerated into ambient
access**; neither is achievable in native code alone, so this is recorded as a
known gap rather than papered over with string matching.

### Honest degradation is preserved

A capability domain the shell does not have plain language for is never given
an invented one and never hidden. It is named as unrecognised, and its raw
domain is shown. Unknown is a verdict too, and it is the one an ordinary person
most needs stated plainly.

## Consequences

Native presentation gains a shared vocabulary layer
(`NappletVocabulary`, `NappletIdentityPresentation`) that owns the plain-language
mapping for the domains in `docs/provider-matrix.md` and the rules for rendering
identity. Surfaces consume it instead of formatting protocol values inline.

Accessibility labels follow the same tier as the visible text. A `.plain`
surface that reads an aggregate hash to VoiceOver has not complied with this
ADR; it has only hidden the failure from sighted users.

UI tests that assert on protocol strings in `.plain` surfaces are asserting the
defect. They are updated to assert the verdict, and evidence assertions move to
`.technical`.

Because this ADR forbids specific strings on specific surfaces, it is
enforceable. A surface may be reviewed against it without reference to taste.
