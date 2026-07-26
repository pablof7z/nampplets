# The napplet browser: an experience design

- Status: Proposed
- Date: 2026-07-26
- Governed by: `docs/adr/0008-verdicts-on-the-path.md`
- Scope: browse, search, the napplet page, the library, and the social layer
  (saving, reviews, reactions). Visual design is deliberately out of scope;
  this document decides structure, sequence, and what is true.

---

## 1. The premise, and where the brief's metaphor breaks

The brief asks for "the App Store experience." Most of what makes that
experience good is available to us: a page per app, a clear thing to press, a
publisher with a name, restraint. One thing is not, and it is the load-bearing
one.

The App Store works because Apple is a trusted aggregator. It controls
identity, gates review-writing behind a purchase, removes what it decides to
remove, and computes a number nobody can forge. Every visible affordance —
the 4.6 stars, the "1,203 Ratings", Top Charts, Featured, the flag button —
is downstream of that authority. Take the authority away and those affordances
do not degrade gracefully. They **invert**: they become the most
trusted-looking and least trustworthy things on the page, and the cheapest
things in the system to attack.

We have no aggregator, no gate, no moderator, and no global index. What we
have instead is unusually good: every artifact is signed, hash-pinned, and
verified before a byte runs, and every opinion about it is signed by a
nameable person. That is a different kind of confidence, and it deserves a
different set of affordances.

So the governing rule of this document is the social analogue of ADR 0008:

> **Attributed opinion, never aggregate authority.**
>
> Every social signal is shown as a named person's act, or it is not shown at
> all. The application never displays a number that summarises strangers.

This is not a taste preference. It follows from ADR 0008 directly. That ADR
says a surface a person passes through shows "only a conclusion the
application is willing to stand behind." A five-star average over an open,
costless identity set is not a conclusion we can stand behind. It is
therefore already forbidden by our own architecture, and the rest of this
document is largely working out what to build in its place.

The second premise is scarcity. This catalog will hold single digits of
napplets for a long time, and it will never be complete — every browse is one
bounded window over a relay plan, not a global index (`broad_manifest_query()`
in `crates/nmp-adapter/src/catalog.rs`). An interface that implies abundance
in front of a nearly-empty shelf does not look aspirational. It looks broken.
Sparsity is therefore a design input from the first screen, not a caveat at
the end.

---

## 2. Information architecture

### 2.1 The browser becomes a place, not an errand

Today every non-canvas surface is a sheet over the workspace: catalog,
library, account, activity, settings, permissions (`ContentView.swift:143-174`).
A sheet is modal, transient, and single-purpose. It says *finish this errand
and get out*. That is exactly wrong for the surface we want people to spend
time in, and it is the single biggest structural reason the current browse
screen cannot feel like a store no matter how it is styled.

**Decision.** The napplet browser becomes a persistent top-level place.

- **macOS**: a separate window scene, titled **Napplets**, with a
  `NavigationSplitView`. Sidebar sections: **Discover**, **Saved**, **Yours**.
  The detail column holds the napplet page. It stays open beside running
  napplets; you can look something up without dismissing your work. The
  install-review sheet's role largely disappears (§6.3).
- **iOS**: a `TabView` with the same three destinations, detail pages pushed
  onto a `NavigationStack`. No sheet for browse.

Search is not a fourth destination. It is a field at the top of Discover that
filters what Discover is already showing, because that is literally what it
does (§2.3). Making it a separate place would imply it reaches somewhere
Discover does not.

*Rejected:* keeping browse as a sheet and restyling it. Cheaper, and it
preserves the errand framing that is the actual problem. *Rejected:* a
sidebar item per category — see §5.2.

### 2.2 Three destinations, and what each one is for

**Discover** — the whole inventory, arranged. Not ranked, not paginated, not
partitioned. Ordered newest-first (§7). Its job is to show you everything
there is and be honest that "everything there is" means "everything that has
reached this device so far."

**Saved** — napplets you bookmarked. A list of intentions, not possessions.

**Yours** — what you have installed, running, and recently opened. This is
today's library, promoted out of its sheet. Bookmarks are *not* a section of
this; wanting and having are different states and collapsing them makes
"Saved" feel like a staging area for installs, which pressures people toward
installing.

The napplet page is not a destination; it is what fills the detail column
from any of the three.

### 2.3 Search must be described as what it is

The pinned NMP facade exposes no NIP-50 full-text search. Browse search is a
local filter over the rows already delivered into the current bounded window
(`CatalogBrowseFrame::filtered`, and the module doc in
`crates/nmp-adapter/src/catalog.rs`). ADR 0008 names the current screen's
attempt to explain this — "the pinned NMP facade does not expose NIP-50
full-text search" — as a motivating failure.

The verdict, in the user's language, is one sentence and it is placed with
the results, not with the field:

> *Searching the napplets that have arrived so far.*

That is complete and true. It tells a person exactly why something they know
exists might not appear, without a protocol noun. It appears whenever a
search is active and there is anything at all to say about the window's
boundedness; it does not appear as a permanent apology above the field.

The evidence — sources, statuses, window bound, refused rows — stays exactly
where `CatalogBrowseEvidenceView` already puts it: one deliberate move away,
complete and unsummarised. That component is a good model for everything in
this document and should be reused rather than reinvented.

---

## 3. The napplet page

This is the one screen the whole design is for. Everything else routes here.

### 3.1 Order of the page, and why

1. **What it is.** Name, one-line description, and — when it exists — the
   visual. See §3.2; this is our biggest content gap.
2. **Who made it.** The publisher, by name, through
   `NappletIdentityPresentation.publisherName`. When they have not given one,
   "Unnamed publisher" is the honest and more useful answer, and it stays.
3. **What it will be able to do.** The plain-language capability sentences
   from `NappletVocabulary`, required and optional separated.
4. **The action.** *Add*, or *Open* if you already have it, or the reason it
   cannot be added, in place of the button.
5. **What people say.** The social layer (§5). Often empty.
6. **Evidence.** Off the path, behind one deliberate move: publisher key,
   coordinate, aggregate hash, provenance sources, platform matrix, warnings —
   verbatim, monospaced, selectable, exactly as `CatalogInstallEvidence`
   already renders them.

**Capabilities sit above the social layer, not below it.** This is the one
ordering choice worth arguing. The conventional store puts social proof high
because it is what converts. Two reasons to invert it here. First, capability
is the actual stake: what this napplet will be able to do as you is a decision
only you can make, and it should not sit under a fold of other people's
opinions. Second, and structurally: in a sparse catalog the social block is
usually empty, and an empty block placed high leaves a hole at the top of
every page. Putting the scarce thing below the decision means its absence
costs nothing.

### 3.2 The gap: napplets have no pictures

Manifests carry `d`, `title`, `description`, and the aggregate hash. That is
all the candidate projection reads (`project_candidate` in
`crates/nmp-adapter/src/catalog.rs`), and I found nothing else in the manifest
schema to read.

A store page with no screenshot, no icon, and a one-line description is not a
store page. **This is the highest-value missing thing in the entire brief**,
and it is worth more to the "premier, gorgeous" goal than every social feature
combined. It is also the most expensive: it needs a manifest schema field, a
content-addressed image host (Blossom is already in the NMP workspace), hash
pinning so images are verified like everything else, and a decision about
whether unverified images may render at all (they must not).

I am flagging it rather than designing around it. Designing a beautiful page
for content that does not exist would be designing a lie. In the interim the
page leans on typography and generous space rather than reserving an empty
image well — a placeholder rectangle on every page is worse than no image at
all, because it advertises the absence on every screen.

### 3.3 The states this page must handle honestly

- **Not installed, installable.** *Add*. One press.
- **Not installed, blocked.** `canInstall == false`, or a blocking warning, or
  incompatible on this platform. The reason replaces the button. Not a
  disabled button with a tooltip — a disabled control asks a person to
  discover why by hovering.
- **Installed, never opened.** *Open*, and one line: opening it is when you
  choose what it can do.
- **Installed, capability decided.** *Open*, plus a quiet route to what it was
  allowed to do.
- **Installed, a newer build offered.** *Update*. Per §4.3, the reviews below
  gain their "written about an earlier version" labelling from the same fact.
- **Arrived by pasted address.** The one case that keeps a confirmation step
  (§6.3).

---

## 4. Reviews

### 4.1 Reviews are about the napplet, not the build

An artifact here has two identities: the addressable coordinate
(`kind:pubkey:d`), which survives updates, and the aggregate hash, which
names exact bytes. Grants bind to the hash — correctly, per ADR 0002. The
question is whether opinions do too.

**Decision: a review is attached to the coordinate. The build it was written
against is recorded on the review as provenance.**

Arguments:

- **The user's noun is the app, not the bytes.** Nobody has an opinion about
  an aggregate hash. Asking them to is the exact leak ADR 0008 exists to stop.
- **Hash-scoping punishes maintenance.** A well-reviewed napplet would become
  unreviewed the instant its author fixed a typo. In a runtime whose security
  posture depends on publishers shipping fixes promptly, an interface that
  zeroes your reputation when you ship a fix is actively harmful.
- **Hash-scoped reviews are unfindable.** No one queries by aggregate hash.
  The coordinate is the thing with an address people can reach.
- **The protocol already models exactly this.** NIP-22's
  `CommentRoot::Address { author, kind, identifier, event_id }`
  (`nmp-nip22/src/root.rs`) carries the coordinate as the thread root *and* an
  optional pin to the specific revision the author was looking at. Both
  readings, no invention, one event.

*Rejected:* review the build and aggregate across builds for display. That is
the worst of both — it re-introduces an aggregation we cannot defend, and it
makes the app decide which builds' opinions carry forward, which is a curation
authority we do not have.

### 4.2 What a review contains

Free text. **No rating field.**

Removing the star field is not a compromise forced by the sybil problem; it
is a better artifact. A review with no number forces a sentence, and a
sentence is legible, attributable, and arguable in a way a number is not. It
also means there is nothing to average, which removes the temptation to build
the thing we already decided we cannot defend.

Length is bounded, and the bound is Rust's, like every other limit in this
system.

The composer is available to anyone. The app additionally requires that *you*
have the napplet installed before it will let you write about it. This is a
quality nudge and nothing more: anyone can publish a kind:1111 event without
our app, so it filters nothing on the network, and we must never present it
as if it did. No "verified install" badges on anyone's review, including our
own users' — we cannot verify that claim for others, so we do not make it for
anyone.

**Editing.** I did not find a deletion or replacement verb in the pinned
facade's re-export list; kind 1111 is a regular event, not replaceable. Until
that is resolved, **there is no editing**, and the composer says so plainly
before you post — once, in the composer, not as a scary confirmation dialog.
Permanence stated up front is fine. Permanence discovered afterwards is not.

### 4.3 What happens when the napplet updates

Nothing disappears and nothing is down-ranked. The app has no basis to decide
an update invalidated an opinion.

What changes is labelling, and it is derived from tags rather than judgement:

- A review whose pinned revision predates the build now on offer carries one
  plain line: **"Written about an earlier version."**
- When *most* of a napplet's reviews predate the current build, the section
  gains one line at the top saying so.

Both are verdicts we can stand behind, because both are facts about which
event id a tag names. Neither is presented as a criticism of the review or of
the publisher.

A review with no pinned revision — legal under NIP-22, where the `E` tag is a
SHOULD — gets no label. Absence of a pin is not evidence of age, and
inventing a hedge for it would be dishonest.

---

## 5. The hard part: what to show when anyone can say anything

This is the centre of the brief and the section I want argued with rather than
skimmed.

### 5.1 The position

**An average over an open, costless identity set is not a measurement.** It is
a number-shaped decoration on an unknown. Sybils cost one keypair each.
Brigading costs one event each. There is no purchase gate, no rate limit we
control, and no one to appeal to.

The failure mode matters more than the inaccuracy. A wrong number that *looks*
authoritative is worse than no number, because it displaces the judgement it
replaced. A person shown "4.8 ★ (312)" stops looking. A person shown three
named opinions and a note that anyone can write these keeps looking, which is
the correct behaviour in a system that cannot vouch for strangers.

So: **the application never displays a count or an average of strangers.**

### 5.2 Three tiers, and only the ones with content appear

Reviews are grouped by the reader's own social graph. The tier is computed in
Rust and projected; native renders what it is given and never derives a trust
level itself (AGENTS.md ownership boundary).

**Tier 1 — People you follow.** Named, with whatever profile name and picture
they have. *"Ana and Ben wrote about this."* This is the only genuinely
trustworthy tier, and its trustworthiness comes from a source the app did not
manufacture: **the reader built the trust set themselves.** There is no
scoring, no weighting, and no maths the reader cannot reconstruct in their
head. They trust it because they know Ana.

**Tier 2 — Your wider network** (follows-of-follows). Weaker, and labelled as
weaker: *"3 people in your wider network."* Same query one hop further out.

**Tier 3 — Everyone else.** Readable, unattributed by trust, **no count, no
ordering that implies rank, no aggregate**, under a heading that states
exactly what it is:

> **From people you don't follow**
> Anyone can write these, and anyone can write many of them.

That sentence is the whole sybil defence, and it is the correct one: we cannot
stop the attack, so we describe the terrain accurately and let the reader
discount appropriately. Hiding Tier 3 would make us a censor of a network we
do not own. Numbering it would make us a liar.

Within every tier, order is **newest first**. Boring, unattackable, and
explainable in four words. Any other order is a ranking authority we do not
have, and the ranking function immediately becomes the thing worth attacking.

### 5.3 What this costs, stated plainly

This design is materially worse than stars at signalling genuine quality. A
napplet loved by five hundred strangers looks identical to one loved by three.
That is a real loss and I am accepting it deliberately.

The trade is that it is also *immune to being made to look good*. Under this
model, brigading a napplet with a thousand hostile reviews produces a Tier 3
list that looks exactly like a list of ten: unnumbered, newest first. There is
no counter to move and no score to sink. **We removed the scoreboard, so there
is nothing to win.**

For a product whose entire reason to exist is that it does not lie about
provenance, a popularity number we cannot defend is a category error. I would
rather under-sell a good napplet than over-sell a malicious one.

### 5.4 The common case: a reader with no follows

This is not an edge case here; it is most people at launch. Tiers 1 and 2 are
empty and the page shows Tier 3 only.

That is the right outcome. The app says, truthfully, *this is unvetted*.
Compare the alternative: 4.8 stars from six sybils, shown to the person least
equipped to discount it. The design is at its most honest precisely where a
conventional store is at its most dangerous.

It also creates the right incentive: the way to make this browser better for
yourself is to follow some people. That is a real, non-nagging reason to build
a social graph, and it should be offered as an explanation where the empty
tier would be — once, quietly, as a fact rather than a prompt.

### 5.5 Explicitly rejected

- **Star ratings and numeric averages.** §5.1. Also already forbidden by
  ADR 0008 as a verdict we cannot stand behind.
- **Web-of-trust or PageRank scoring over the follow graph.** Two objections.
  It replaces a legible reason ("you follow Ana") with an illegible one ("0.73
  trust"), destroying the exact property that makes Tier 1 work. And it is an
  arms race we lose without a server.
- **Proof-of-work or payment gating on reviews.** Raises the sybil price and
  prices out ordinary users, whose reviews we want most. A paid review market
  is worse than no reviews.
- **Report / flag buttons.** Flagging with no moderator is theatre. It
  collects a signal nobody acts on and implies someone is watching. Replaced
  by **mute this person**, which is local, immediate, real, and reuses a
  concept already in the vocabulary (`lists`). If we ever have a curator, add
  flagging then.
- **"Helpful" voting, or sorting by helpfulness.** Same aggregation problem;
  the sort function becomes the attack surface.
- **Showing "0 reviews" or "No reviews yet."** Zero is a number, and it
  invites comparison. Absence renders as absence.

---

## 6. Reactions, saving, and consent

### 6.1 Reactions: cut them from napplets, keep them on reviews

The brief asks for reactions. I think reactions *on napplets* should not be
built, and I want to be clear that this is a disagreement rather than an
oversight.

A heart on a napplet is a like-count. A like-count is an aggregate over
strangers, which is the one thing §5 rules out — and it is the cheapest
possible target, since a kind:7 event has no content to write and no cost to
produce. A reaction is a review with the content removed, and the content was
the part that made it trustworthy.

**What I would build instead, which I think is better and still satisfies the
intent:** reactions **on reviews**. A kind:7 referencing a kind:1111 comment,
rendered only as named agreement from people within your graph:

> *Ana and 2 others you follow agree.*

This is genuinely useful and it is not the thing we rejected. It is how a thin
review set gains weight without a scoring model: instead of one opinion you
get one opinion plus a visible, named set of people who endorse it. It is
attributable, it is bounded by your own follow list, and it degrades to
nothing when there is nothing to say.

If reactions on napplets ship anyway, they must follow the same rule: named
people from your graph, never a number.

### 6.2 Saving: local first, publishable later, never by default

**Decision: bookmarks are local, and the target is the coordinate.**

You save "this napplet", never "these bytes" — the same argument as §4.1, and
it means a save survives updates.

Local, because a bookmark is the one social act with a genuinely useful
*private* reading: *I want to come back to this.* Publishing it by default
would turn a private intention into a public disclosure of your interest
graph, which is a privacy defect dressed as a feature. Local also means saving
works with no account, instantly, offline, and with no signature per toggle.

The cost is real and should be disclosed once, in Saved, not as a warning:
local saves do not follow you to another device. A later opt-in publishes them
as a list.

### 6.3 Install and consent hang off the page — and one wall comes down

ADR 0008 §3 already decided the weighting: adding acquires verified bytes and
grants nothing, so it is light; first run grants capability, so it carries the
consent moment. Nothing here changes that.

One thing should change. The install-review sheet shows title, publisher,
capabilities, warnings, and evidence — which is exactly what the napplet page
already shows. Presenting the same information twice, the second time in a
modal, to confirm an action the sheet's own copy certifies as inert, is
precisely the misweighted consent ADR 0008 names as a motivating failure. It
is a wall in front of a door that is already open.

**Decision: from the napplet page, *Add* acts directly. No confirmation
sheet.** The page *is* the review.

**The sheet survives for the pasted-address path only**, where the person has
not seen a page and genuinely does need to be told what they are about to
acquire before it happens. That is a real difference in what the user knows,
which is the correct basis for a difference in ceremony.

First-run capability consent is untouched: the single consent moment, phrased
as what the napplet will be able to do, with the runtime's own
`recommendedDecision` preselected.

---

## 7. Curation without a curator

There is none, so we must not fake it. What we have instead, in the order I
would build it:

1. **Recency.** Discover's default order is newest manifest first. It is not a
   quality signal and never claims to be. It is fair, it makes a live network
   feel alive, and gaming it buys you visibility rather than endorsement —
   a much smaller prize.
2. **Your own history.** *You've opened this before.* Local, honest,
   individually useful, and needs nothing from anyone else.
3. **Publishers you already have.** If two things you installed share a
   publisher, a third from them is more relevant to you. A local join on
   `manifestAuthor`, which installs already carry. No protocol work.
4. **What people you follow use.** The only genuine discovery mechanism a
   decentralised system has, and the one worth real investment. It requires
   publishing your install list, which is a serious privacy decision:
   **opt-in, per napplet, never retroactive.** Build the shape now; publish
   later.
5. **A starter set shipped with the app.** Honest curation by us, labelled as
   what it is: **"Included with Napplets."** Not "Featured" — featured implies
   selection from a field, and there is no field.

*Rejected:* Trending and Top Charts (need counts we cannot trust). Editors'
Picks (no editors). "You might also like" (no corpus to compute similarity
over, and it would be a ranking authority besides).

### 7.1 Sparsity shapes the surface, not just the copy

- **Discover shows its entire inventory.** No "See All", no horizontal
  carousels. A grid that ends is honest; a carousel that scrolls off-screen is
  a claim about supply.
- **No categories or genres.** Categories partition a large set into browsable
  chunks. Partitioning eleven napplets into eight categories produces eight
  nearly-empty rooms and makes a working catalog feel broken. Reintroduce them
  when a category would hold a real number of things, and let that threshold
  be Rust's, not a native guess.
- **The empty state is the primary state and must be excellent.** Not a
  sad-face and a Retry. It says what is true — nothing has arrived here yet —
  and offers the two actions that actually work: paste an address someone sent
  you (this path exists today), and look at the included set.
- **"Still arriving" is a state, not a spinner.** The catalog is a live window
  that fills in; it should visibly grow rather than block behind a loading
  screen. The current `observeChanges` model already does this and should be
  preserved.
- **Never imply completeness.** Today's footer — *"12 napplets so far — there
  are more than fit here"* — is close to the right model for everything: what
  is here, whether there is more, nothing else.

---

## 8. Signed out, and the account ask

Everything except writing works with no account: browsing, searching, reading
reviews, adding, running, and saving. This falls out of the architecture
rather than being engineered — catalog queries are public
(`SourceAuthority::Public` / `AccessContext::Public`) and saves are local.

Two things need a key: writing a review, and Tiers 1–2, which need a follow
list and therefore a public key. A read-only account
(`WorkbenchAccountConnectionKind.readOnly`) is enough for the tiers; only
writing needs a signer.

**How the ask is handled:**

- **No sign-in banner. Anywhere. Ever.** No "Sign in to see more."
- The empty Tier 1 heading simply does not render. Nothing announces its
  absence except, once and quietly, the fact from §5.4: following people is
  what makes this section appear.
- **The "Write a review" control is present and enabled.** Pressing it opens
  the composer and you type.
- The account requirement appears **at submit**, in context, phrased as the
  next step in what you are already doing — *to post this as you, you need a
  name people can see* — with the account flow inline and **the draft
  preserved**.

That last detail is the whole design. An account ask that arrives before you
have a reason is a toll. One that arrives at the moment it becomes necessary,
without destroying your work, is just the next step. Per ADR 0008 §2, adding
an account registers and activates in one gesture.

---

## 9. What we should not build

Consolidated, with reasons already given above:

| Not building | Because |
| --- | --- |
| Star ratings, numeric averages | §5.1 — a verdict we cannot stand behind |
| Review counts of any kind | Zero and 312 are both claims about strangers |
| Reactions on napplets | §6.1 — a like-count by another name; kept on reviews |
| "Helpful" voting / sort by helpful | Aggregation, and the sort becomes the attack surface |
| Report / flag | Theatre without a moderator; replaced by mute |
| Categories, genres | §7.1 — eight empty rooms |
| Top Charts, Trending, Featured | No trustworthy counts, no editors |
| Carousels, "See All" | Claims about supply we cannot back |
| "Verified publisher" badges | We have no basis to verify anyone |
| Editors' Notes / editorial copy | There are no editors |
| A sign-in wall or banner | §8 |
| Placeholder image wells | §3.2 — advertises the absence on every page |
| Update push notifications | Interrupting for a thing nobody asked to be interrupted about |

Cutting is the design here. Nearly every one of these is a mechanism for
compressing many strangers into one number or one rank, and we have decided
we cannot do that honestly. What remains — a page, a publisher, a plain
statement of what it will be able to do, and named people's sentences — is
less, and all of it is true.

---

## 10. Feasibility: what is real, what is not

Separated as requested. Everything below marked *verified* was read in the
pinned tree; everything marked *unverified* is flagged as a guess.

### 10.1 Buildable today (native only)

- The entire IA change: window/tab structure, three destinations, the napplet
  page, promoting library out of its sheet.
- Dissolving the install-review sheet on the page path; keeping it for pasted
  addresses (`CatalogManualCoordinateRequest` already exists).
- All sparse-catalog behaviour: whole-inventory Discover, honest empty states,
  the search wording, reusing `CatalogBrowseEvidenceView` for off-path
  evidence.
- Discover ordered by recency: manifest `created_at` is already projected
  (`CatalogManifestCandidate.created_at`). *Verified.*
- "Publishers you already have": installs carry `manifestAuthor`
  (`CatalogInstalledBuild`). Local join. *Verified.*
- The bundled starter set: the offline fixture path already exists.

### 10.2 Needs Rust work in this repo (NMP facade is already sufficient)

- **A host-originated write path.** This is the gating item for reviews and
  for any published list. *Verified gap*: `PlatformCommand`
  (`crates/runtime-app/src/commands.rs`) has `ApproveWrite` and
  `DecideProviderWrite` for *napplet*-originated writes only; there is no
  command for the shell to publish its own event. The mechanism underneath is
  present and used — `Engine::publish(WriteIntent)` with a `WriteStatus`
  receipt stream, already driven by `crates/nmp-adapter/src/nap.rs`.
- **A bounded reviews observation.** Kind 1111 filtered by the napplet's
  coordinate on the uppercase `A` tag, projected into screen-shaped rows with
  finite limits and observable refusals — a direct structural copy of
  `NmpManifestCatalog`. `Filter.tags` accepts any ASCII-letter indexed tag
  name, so `#A` is expressible. *Verified.*
- **Social-graph tiering, owned by Rust.** The follow-derived author set is
  natively expressible in the pinned grammar: `Binding::Derived { inner: a
  kind:3 demand bound to `Binding::Reactive(IdentityField::ActivePubkey)`,
  project: Selector::Tag("p") }` used as the `authors` binding, with
  `Binding::SetOp`/`SetAlgebra::Diff` available to subtract mutes. *Verified*
  in `nmp-grammar/src/binding.rs` and `selector.rs`. Which tier a review lands
  in is policy and therefore Rust's; native must only render it.
- **A local bookmarks table** in `crates/runtime-store`. *Verified* that
  `schema.rs` currently has installations, grants, component_kv, workspaces,
  activity, and workspace_assignments — no bookmarks.
- **A local mute list.**

### 10.3 Needs protocol work, or an ownership ruling this repo has not made

- **NIP-22 comment composition — the single biggest unblock.** Upstream
  `nmp-nip22` has precisely the API this design needs:
  `compose_top_level_comment`, `comment_intent`, `comment_thread_demand`,
  `decode_comment`, and `CommentRoot::Address` with its optional revision pin.
  It never signs and never touches the engine. **But it is not a dependency of
  the pinned `nmp` facade** — *verified*: `crates/nmp/Cargo.toml` lists
  `nostr`, `nmp-grammar`, `nmp-engine`, `nmp-signer`, `nmp-store`,
  `nmp-router`, `nmp-resolver`, `nmp-executor`, and no NIP modules. Consuming
  it requires this repo to rule that opt-in protocol modules are part of its
  supported surface, plus a `compatibility.lock` movement. That is a
  governance decision, not a code change, and it should be taken early because
  everything in §4 and §5 waits on it.
- **NIP-51 lists, for publishing bookmarks or install lists.** Upstream
  `nmp-nip51` covers **only kind:10009 simple groups, decode-only** — its own
  module doc states that replacement-write encoding is explicitly out of
  scope. *Verified.* Publishing a bookmark set needs a new upstream module.
- **Reactions (kind 7).** No NIP-25 module exists at the pinned rev.
  *Verified* by search across the upstream workspace.
- **A note on hand-composition.** The facade re-exports `UnsignedEvent`,
  `Tag`, `Kind`, and `WritePayload::Unsigned`, and kind ownership
  (`nmp-ownership`) appears to be a workspace *audit* convention rather than
  something the engine enforces at publish — *verified* only in the negative
  (no `nmp_ownership` references in `nmp-engine`). So this repo *could*
  hand-compose kind 7 or kind 30003 events and publish them. **It should not.**
  Doing so makes this repo the de-facto schema owner of kinds it does not own,
  which is the exact modularity boundary NMP's protocol-module design exists
  to protect.
- **Screenshots and icons.** §3.2. Manifest schema field, content-addressed
  hosting, hash pinning, and a rule that unverified images never render.
- **NIP-50 search.** Explicitly unavailable through the pinned facade. Search
  stays a local filter and must be worded as one, permanently, not as a
  temporary apology.

### 10.4 Things I am guessing about

Flagged so nobody builds on them without checking:

- **Two-hop derived bindings (Tier 2).** The resolver mentions bounded graphs
  of "depth ≤ 3", but I did not find the enforced derived-depth limit or its
  validation. Tier 2 is contingent on a two-hop `Derived` being admitted.
- **Outbox-model coverage for strangers' reviews.** Whether the pinned demand
  routing will fetch a non-followed author's kind:1111 events from *their*
  write relays. If it does not, Tier 3 coverage will be materially worse than
  manifest coverage, which changes how §5.2's Tier 3 should be worded.
- **Deletion.** I found no deletion or replacement verb in the facade's
  re-export list, which is why §4.2 says reviews are permanent. If NMP does
  expose deletion, editing becomes possible and §4.2 should be revisited.
- **Retention of "recently opened".** The `activity` table exists; I did not
  confirm its retention window, so §7 item 2 may need a separate local record.
- **Whether all of this is moot for capabilities.** `docs/provider-matrix.md`
  currently advertises **no provider on any platform**. Every capability
  sentence on the napplet page therefore describes what a napplet *declares it
  will want*, not what it can currently do. That is still the right thing to
  show, but nobody should read §3.1 item 3 as describing live capability.

---

## 11. The one-line summary

Build a store page, a publisher's name, a plain sentence about what a napplet
will be able to do, and other people's sentences signed by people you chose to
trust — and build nothing that turns strangers into a number.
