# Source ledger and freshness rules

This is a dated orientation, not a lock file. The snapshot below was checked on
2026-07-26. Refresh it before making current-status claims.

## Primary sources

### NIP-5D living proposal

- PR: <https://github.com/nostr-protocol/nips/pull/2303>
- Current proposed text: the `5D.md` file on the PR head
- Observed state: open, ready for review, not merged, head
  `eb45dfd7335b7f88cb53781984c553581d2b4c34`
- Scope: proposed web projection, napplet-specific manifest profile, artifact
  verification/loading, injected namespace, envelope, identity binding, and
  security considerations

Read the file diff, not only the PR description. The description and older
comments preserve earlier designs. For example, the PR body still describes an
older extension-track split that the living NAP registry has since changed.

Useful PR discussion:

- The May 2026 `nostr-station` exchange compares opaque origins with dedicated
  origins, CSP, diagnostics, storage mediation, fingerprinting, and DNS/cert
  trust.
- The June 2026 author note rejects gateway-asserted identity in favor of
  runtime resolution, byte verification, and `srcdoc`.
- The July 2026 update points to Kehto, napplet.run, and the NAP registry.
- The 2026-07-24 commit added the current CSP injection advisory.

Classify all of this as proposal or design discussion, never merged NIP status.

### NIP-5A

- Current document: <https://github.com/nostr-protocol/nips/blob/master/5A.md>
- File revision observed: `5d6b432267d4046464490b1923b96844ac4559d0`
- Scope: static-site manifest kinds `5128`, `15128`, `35128`; `path` tags;
  aggregate `x` calculation; server hints; snapshots; nsite host behavior

The current NIP-5D head adopts the NIP-5A tag/hash schema but proposes distinct
napplet kinds `5129`, `15129`, and `35129`. Do not conflate nsite and napplet
kinds.

### NAP registry

- Repository: <https://github.com/napplet/naps>
- Registry revision observed:
  `5ac0490461ca6fec2f0d2e45b4835cf9bc08de24`
- Web projection: `projections/web.md`
- Active files on that revision: `NAP-SHELL`, `NAP-INTENT`, `NAP-INC`,
  `NAP-THEME`, and `NAP-IDENTITY`
- Many other domains are represented by open PRs and package implementations.

Open the exact NAP file or PR. A row in a registry, a package subpath, or a demo
does not prove a proposal is merged, active, complete, or interoperable.

The registry currently distinguishes:

- NAP: a runtime-provided capability API surface
- projection: a host binding for the same capability seam
- convention: napplet-agreed message meaning identified by
  `napplet:<archetype>/<intent>`
- NAAT: a canonical napplet role or archetype

The numbered message-protocol track was removed in July 2026. Older NIP-5D
summaries can still mention it.

At the observed revision, the registry table marks NAP-THEME `Active` while the
NAP-THEME document itself is headed `draft`. Report both labels rather than
silently choosing one. Its portable payload is intentionally narrow: required
`background`, `text`, and `primary` colors; optional body/title fonts,
background media, and title; read-only `get`; and automatic `theme.changed`
pushes. It is a transport for the shell's theme, not a complete product design
system. See `visual-identity-and-themes.md`.

## Implementations and tools

### @napplet packages and napplet.run

- Repository: <https://github.com/napplet/napplet>
- Documentation: <https://napplet.run/docs>
- Revision observed: `dbd2cc2e53a9e311cf263fa020d43105a7d75192`
- Package snapshot: core `0.28.0`, shim `0.26.8`, SDK `0.24.4`, NAP package
  `0.28.0`, Vite plugin `0.11.3`, conformance `0.13.0`, conformance CLI
  `0.2.15`, CLI `0.2.1`

These are implementation surfaces, not protocol authority. Package docs
explicitly warn of alpha drift. Inspect actual exports before recommending an
API. The package set can ship a domain whose NAP is still an open proposal, and
some docs can describe an older handshake or manifest revision.

### Kehto

- Repository: <https://github.com/kehto/web>
- Documentation: <https://kehto.github.io/web/docs/>
- Runtime visualization: <https://kehto.github.io/web/>
- Revision observed: `738c3ce5aa398a413e50155ea505bd96bb6792e3`

Kehto says explicitly that it is an early implementation, one runtime rather
than the runtime. Its packages separate ACL, protocol runtime, browser shell,
services, NIP helpers, a window-management contract, Paja authoring tooling,
and the playground.

Kehto is especially useful for implementation evidence, negative cases, Paja
workflows, capability policy, and a substantial demo corpus. Its documentation
also demonstrates why freshness matters: some pages retain gateway-navigation
guidance while current source and the current NIP-5D head use runtime-resolved,
verified `srcdoc`. Verify the live path before treating a page as current.

### Native runtime evidence

- Repository: <https://github.com/pablof7z/nampplets>
- Scope: native runtime above NMP, legacy compatibility, exact-build grants,
  provider boundaries, bounded lifecycle, native web projection, and an
  additive host-owned surface model

Repository compatibility locks and fixtures outrank live upstream for that
product's accepted behavior. Compare pins to upstream as a separate drift
report. Never mutate or bypass a referenced Nostr engine to solve runtime work.

At the observed revision, the baseline is unratified and predates the live
NIP-5D CSP advisory. Kinds `5129` and `15129` are verified and cached but not
executable because the product has not ratified a collision-free exact-build
principal for their absent signed `dTag`. M0 advertises no providers; the
imported Kehto corpus is `built-not-run`; and `resource` plus `link` remain
unavailable until bounded native executors are production-wired. These are
dated Nampplets implementation facts, not NIP-5D requirements.

Nampplets now has a bounded NAP-THEME provider. Apple code reports raw
light/dark, contrast, transparency, and accent facts; Rust maps them into the
pinned three-color payload and pushes changes to declaring ready sessions. That
proves the native projection and accessibility flow, not a branded
cross-napplet visual system.

### stlstr composition case study

- Repository: <https://github.com/hzrd149/stlstr>
- Deployed product: <https://stlstr.xyz>
- Revision observed:
  `69c220d27ae0f5d5a9a3a80928a4e284af338c4f`
- Scope: small persistent host, bundled default napplets, archetype/action
  routing, deep links, targeted destination delivery, overlay presentation,
  napplet catalog, and per-archetype user overrides

Source and committed tests were inspected on 2026-07-26. Dependency install,
static source-policy tests, napplet type-check/build verification, and the full
production host build all passed locally. The deployed site returned HTTP 200,
but no compatible local browser was available for visual inspection and its
browser suite was not run because its Chromium and local-relay prerequisites
were absent.

Use stlstr as implementation evidence for cohesive composition, never as NIP or
NAP authority. In particular, its picker compatibility indicators are described
as advisory, and its artifact fetch path is not evidence for Nampplets'
signature, per-path hash, aggregate hash, exact-principal, or grant requirements.
See `cohesive-composition.md`.

## Oral history and design intent

- Episode: <https://sovereignengineering.io/podcast/30-napplets-w-sandwich>
- Title: `30: Napplets w/ Sandwich`
- Published: 2026-07-22

Use the timestamped synthesis in `sandwich-interview.md`. The interview explains
motivation, experiments, and intended product shape. It is not normative and
contains conversational shorthand, early names, unverified implementation
claims, and ideas the specifications later changed.

## Freshness procedure

For current advice:

1. Check PR #2303 state and head SHA.
2. Read current `5D.md`, not just the PR body.
3. Check NIP-5A if manifest/hash semantics matter.
4. Check the relevant NAP file or PR and its status.
5. Inspect exact package versions and exports.
6. Inspect any cited product case study at an exact revision.
7. Inspect the target runtime's lock, source, fixtures, and report.
8. State the observation date and identify unresolved drift.

For pinned advice, do steps 4-6 against the pinned revisions first. Never
silently update a compatibility contract while answering a design question.

## Source labels

Use these exact labels in reports:

- `merged NIP` — text present on the NIPs repository's main branch
- `living proposal` — open PR text
- `NAP active` / `NAP draft` / `NAP open PR` — status at the cited revision
- `package behavior` — exact released or source version
- `runtime policy` — product choice, not ecosystem law
- `observed` — demonstrated by a test, report, or live run
- `author intent` — interview or discussion
- `inference` — advisor synthesis
