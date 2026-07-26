# The napplet browser: a visual system

- Status: Proposed
- Date: 2026-07-26
- Governed by: `docs/adr/0008-verdicts-on-the-path.md`
- Dresses: `docs/design/napplet-browser-experience.md` (structure settled there;
  not reopened here except in §10, Tensions)
- Scope: the visual system — type, space, colour, surface, icon, motion — and
  the concrete layout of Discover, the napplet page, reviews, install/consent,
  and the library, on macOS and iOS.

---

## 0. What I verified, and what I am guessing

Everything in §1–§9 that describes the current tree was read, not assumed.
Read in full: `NappletStyle.swift`, `NappletDisclosure.swift`,
`CatalogView.swift`, `CatalogEntryRow.swift`, `CatalogInstallReviewSheet.swift`,
`PermissionReviewSheet.swift`, `CatalogBrowseEvidenceView.swift`. Read in part:
`NappletVocabulary.swift`, `NappletIdentityPresentation.swift`,
`CatalogModels.swift`, `WorkbenchLibraryBuildRow.swift`. Counted across the
whole `RuntimeWorkbenchFeature` package by grep.

**One factual correction to the brief I was given.** The green
`checkmark.seal`-on-every-surface described in ADR 0008 and in the brief is
**not in the tree any more**. Grep across the package returns zero
`checkmark.seal`. What actually remains is:

| Finding | Count | Where |
| --- | --- | --- |
| `checkmark.seal` in any colour | **0** | — |
| Distinct SF Symbols in the package | **~50** | everywhere |
| `foregroundStyle(.orange / .red / .green / .yellow)` on the path | **10** | rows, capability rows, evidence footer |
| `cornerRadius:` call sites | 7 | — |
| …of those using `style: .continuous` | **0** | — |
| `ContentUnavailableView` (centred, icon-led) | **17** | every empty state in the app |

So the first pass removed the seal. The genre problem it left behind is
different and, I think, more interesting: **~50 icons and 17 centred
icon-and-caption empty states**. The app no longer looks like a traffic light.
It looks like a *utility* — a well-behaved settings pane. That is what this
document is for.

`.continuous` at zero across all seven radii is a real, visible defect: every
rounded rectangle in the app is drawn with circular arcs while every rounded
rectangle the OS draws around it is a squircle. It reads as slightly wrong and
nobody can say why.

I have **not** read `ContentView*.swift`, `WorkbenchLibrarySheet*.swift`,
`ActivityDrawer.swift`, or `WorkbenchWorkspaceView.swift` in full. Where this
document specifies the library, it specifies the target, and an implementer
should check the current shape before assuming a delta. Contrast ratios in §4
are computed from the sRGB values given, not measured on a display.

---

## 1. The position

### 1.1 The thesis

This product's content is *words and names*. There is no artwork, no rating, no
count, no chart, no editorial, no photograph of anything. Every fact it can show
is a name someone signed, a sentence someone wrote, or a verdict the runtime is
willing to stand behind. A visual language that spends its budget on containers
for pictures is spending it on a payload we do not have.

So: **this is a page, not a shelf.** The genre is the well-set printed page —
the contents page of a serious book, the front of a broadsheet, a specimen
sheet. Strict left-aligned single measure, generous margin, hierarchy built from
size and space rather than from boxes and fills, warm off-white ground rather
than device white, and a serif for the things that have names. That genre is
correct here for a reason that is not taste: **a printed page's authority comes
from the care of its setting, not from a badge printed on it** — which is
exactly the claim ADR 0008 §4 makes about verification. "Verified is not a green
seal; it is the fact that the application did not ask." The typographic analogue
is: *quality is not a badge; it is the fact that everything is set correctly.*

The mechanism that makes the system cohere is one idea, and it is the strongest
thing in this document:

> **ADR 0008's disclosure tier becomes a typographic boundary.**
>
> The plain tier speaks in **prose** — SF, proportional, sentences.
> The technical tier speaks in **record** — SF Mono, tabular, verbatim.
> Names are set in **display** — New York, serif, large.
>
> You can tell which tier you are in from across the room, without reading a
> word, without a single pixel of colour.

Three voices, each with a job: *serif names the thing, sans is the app talking,
mono is the machine's record.* Nothing else in the system needs to signal the
tier boundary, because the typeface already did.

### 1.2 Two directions I am rejecting, and why

**Rejected: the App Store pastiche.** Big rounded artwork tiles, a hero shelf, a
featured banner, gradient placeholder squares, a pill-shaped GET button. Every
single element of that language is a *container for artwork*, and we have none.
Filling those containers with generated decoration does not read as confidence;
it reads as compensation, which is worse than absence. It is also, per the
experience doc §7.1, a set of claims about supply we cannot back. The App Store
comparison in the brief is about *care and restraint*, and we should take that
and leave the furniture.

**Rejected: the security aesthetic.** Near-black ground, monospace everywhere,
a neon accent, a lock glyph, a fingerprint motif. This is the tempting one,
because the product genuinely is cryptographic and the visual language is
sitting right there. It is wrong for two reasons. First, it is *precisely* the
"insane amount of mega technical stuff" the product owner is complaining about,
rendered as styling instead of as content — a normal person runs from a terminal
whether or not the words are friendly. Second, and structurally: if monospace is
the ambient texture of the whole app, then monospace stops marking the technical
tier, and §1.1's boundary collapses. **Mono has a job here. It cannot also be
the wallpaper.**

**Also rejected: hash-derived identity art.** Argued at length in §7.1, because
it is the obvious answer to the missing-artwork problem and it is a violation of
ADR 0008 dressed as a feature.

### 1.3 Where I build on the brief, and where I push

The lead's instinct — near-monochrome plus one accent, trust carried by
typography and restraint — is right and this document adopts it wholesale. Two
places I go further:

1. **One accent is not enough restraint on its own.** The accent needs a *rule*
   about where it may appear, or it will end up on every heading. §4.3: the
   accent appears on **exactly one element per screen**, the primary action.
2. **Near-monochrome is a floor, not a position.** Grey type on white is what
   default SwiftUI already is; the difference between "restrained" and
   "unstyled" is invisible to the person we are designing for. The two moves
   that make monochrome read as *deliberate* are a **warm paper ground** (not
   `#FFFFFF`) and a **serif display face** (New York, ships with the OS, full
   Dynamic Type). Both cost nothing and are the entire difference between
   "clean" and "designed."

---

## 2. Type

### 2.1 Three faces, and the rule for each

| Voice | Face | SwiftUI | Used for | Never used for |
| --- | --- | --- | --- | --- |
| **Display** | New York | `.fontDesign(.serif)` | Napplet names at page scale; place titles (Napplets / Saved / Yours) | Anything below `.title2`. Anything the user did not name. |
| **Prose** | SF | default | Everything the app says | Hashes, keys, addresses |
| **Record** | SF Mono | `.fontDesign(.monospaced)` | The `.technical` tier, exclusively | Any `.plain` surface, ever |

**New York is banned below `.title2`.** Optical sizing makes it beautiful at 26pt
and mushy at 13pt, and a serif in body copy on screen buys nothing.

**`.fontDesign(.rounded)` is banned outright.** It is the "friendly consumer app"
tell, and friendliness applied as a typeface to a product about signed
provenance reads as a costume.

No licensed third-party face. Not a budget decision — SF/New York/SF Mono are a
genuinely excellent three-face system with variable weights, optical sizes, full
Dynamic Type, and correct rendering on every device we ship to. Shipping a
webfont would trade all of that for novelty.

### 2.2 The scale

Every token maps to a **semantic text style**. Nothing in this app is ever
`Font.system(size:)` without `relativeTo:`. That is the whole Dynamic Type
answer, and it is enforceable by grep.

Resolved point sizes are shown at the default content size for orientation only;
they are outputs, not inputs.

| Token | SwiftUI | Weight | iOS pt | macOS pt | Line spacing | Tracking | Role |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `display` | `.largeTitle` + `.serif` | semibold | 34 | 26 | 0 | −0.5 | Napplet name on its page |
| `place` | `.title` + `.serif` | semibold | 28 | 22 | 0 | −0.4 | Discover / Saved / Yours title |
| `title` | `.title2` | semibold | 22 | 17 | 0 | −0.2 | Card titles; sheet titles |
| `lede` | `.title3` | regular | 20 | 15 | 3 | 0 | The one-line description under a name |
| `heading` | `.headline` | semibold | 17 | 13 | 0 | 0 | Section headings; a person's name on a review |
| `body` | `.body` | regular | 17 | 13 | 4 (iOS) / 3 (macOS) | 0 | Prose, review text |
| `secondary` | `.callout` | regular | 16 | 12 | 3 | 0 | Supporting sentences; capability lines |
| `caption` | `.caption` | regular | 12 | 10 | 2 | 0 | Publisher line, footers, counts |
| `record` | `.footnote` + `.monospaced` | regular | 13 | 11 | 2 | 0 | Technical tier only |

**Three weights exist: `.regular`, `.medium`, `.semibold`. `.bold` and heavier
are banned.** On a page whose hierarchy is carried by size and space, bold is
the tool you reach for when the hierarchy has already failed.

`record` is a **change from the current floor**: `NappletFieldGrid` sets
`.font(.caption)`, and 64 hex characters at caption size is unreadable and
untrustworthy-looking. ADR 0008 says the technical tier exists so the plain tier
can be confident; illegible evidence does not achieve that. Move it to
`.footnote` monospaced.

### 2.3 Measure, alignment, and clamping

- **Measure.** Prose is capped at **680pt** (`NappletMetrics.measure`), roughly
  62–68 characters at the default size. macOS detail column: content is
  `.frame(maxWidth: 680, alignment: .leading)` inside the available width, not
  stretched. iPhone: full width minus margins is already inside the measure.
  iPad regular width: apply the cap.
- **Everything is leading-aligned.** No centred text anywhere, including empty
  states, including the three-item Discover page. Centred short text in a large
  frame is the universal signal for "nothing here / something failed", and §7.2
  depends on refusing it.
- **Clamping.** Card title 2 lines, card description 2 lines, review body
  unclamped (a review is the content; truncating it and adding "more" is a
  tap between a person and a sentence). Publisher line 1 line, tail-truncated.
- `.fixedSize(horizontal: false, vertical: true)` on every multi-line label, as
  the current code already does correctly.

### 2.4 Dynamic Type behaviour

- All tokens scale. No opt-outs.
- At `dynamicTypeSize >= .accessibility1`, three layout switches fire:
  1. `PermissionCapabilityRow` and `NappletCapabilityLine` go from
     `Label` (icon leading) to a `VStack` with the icon on its own line, or drop
     the icon entirely (preferred — see §6.2 rule 8).
  2. Review rows drop the avatar and stack name above body.
  3. The Discover grid collapses to a single column regardless of width.
- Implement with `@Environment(\.dynamicTypeSize)` and `.isAccessibilitySize`,
  or `ViewThatFits` where the switch is purely about fit.
- **Buttons never clamp their label.** `.lineLimit(nil)` on the primary action;
  it grows taller rather than truncating. A truncated action is an unusable one.

---

## 3. Space

### 3.1 Reconciling with `NappletMetrics`

**Keep it.** The existing 4/8/12/16/24/32 ladder is a correct 4pt grid and is
referenced across ~20 files; renaming it would be churn in exchange for taste.
Three things are missing and one is a smell.

```swift
public enum NappletMetrics {
    public static let step = 4.0        // NEW: documents the grid

    public static let hairline = 4.0    // keep (name is unfortunate; not worth the churn)
    public static let micro = 6.0       // NEW: kills `hairline + 2` in NappletFieldGrid
    public static let tight = 8.0       // keep
    public static let snug = 12.0       // keep
    public static let comfortable = 16.0// keep
    public static let roomy = 24.0      // keep
    public static let generous = 32.0   // keep
    public static let spacious = 48.0   // NEW: between page regions
    public static let page = 64.0       // NEW: top of a page, above a place title

    public static let measure = 680.0   // NEW: prose measure cap
    public static let pageMarginCompact = 20.0  // NEW: iPhone
    public static let pageMarginRegular = 32.0  // NEW: macOS / iPad

    public static let radiusSmall = 6.0     // NEW: fields, chips, inline tints
    public static let cardCorner = 12.0     // CHANGED from 10
    public static let radiusSheet = 16.0    // NEW: iOS sheet content inset
    public static let hitTarget = 44.0      // NEW: iOS minimum
}
```

`hairline + 2` appearing inline in `NappletFieldGrid` is the scale telling you
it has a gap at 6. Add `micro` and use it.

`cardCorner` 10 → 12: at 16pt padding, a 10pt radius reads slightly tight and
sits awkwardly against the 12–16pt continuous radii the OS draws around it.

**Every rounded rectangle uses `style: .continuous`.** All seven current call
sites omit it. This is not a preference; iOS and macOS draw squircles and our
cards currently do not.

### 3.2 The rhythm rule

> **The space between two things is proportional to how different they are.**

| Between | Space |
| --- | --- |
| Lines inside one thought (name and its publisher) | `hairline` 4 |
| A label and the thing it labels | `tight` 8 |
| Sibling rows of the same kind (capability lines, reviews) | `snug` 12 |
| Subsections inside one section | `comfortable` 16 |
| Inside a card, edge to content | `comfortable` 16 (`roomy` 24 for a page-scale card) |
| Sections of a page | `generous` 32 |
| Page regions (header → body; body → technical details) | `spacious` 48 |
| Above a place title | `page` 64 (macOS), `generous` 32 (iOS, under nav) |

Page margins: **macOS/iPad 32, iPhone 20.**

**The single biggest visual delta from today** is `spacious` 48 above the
technical-details line. Today evidence sits `roomy` 24 below the last content,
which puts it inside the reading flow. 48 puts it below the fold of intent
without hiding it. See §7.4.

---

## 4. Colour

### 4.1 The rule

Colour is permitted in exactly three places. Everywhere else, everything is a
value on the ink ramp.

1. **The accent**, on **one** element per screen: the primary action. Nothing
   else is ever accent-coloured. Not headings, not links inside prose (those are
   underlined ink), not selected sidebar rows (those are `fillSelected`), not
   icons.
2. **Caution and refusal inks**, which appear only when a projected
   `NappletTrustVerdict` is `.caution` or `.blocked` — that is, only when the
   runtime told us something is wrong. Native never derives them.
3. **User-supplied content**: a person's profile picture on a Tier 1 review.
   That is their colour, not ours.

Everything else — every state, every category, every status, every tier, every
count — is carried by words, size, position, and structure.

### 4.2 The palette

Ship as a `Color` asset catalog or as a `NappletInk` enum with
`Color(light:dark:)`. Values are sRGB hex.

**Ink ramp**

| Token | Light | Dark | Contrast on paper | Use |
| --- | --- | --- | --- | --- |
| `ink` | `#111213` | `#F2F2F0` | 19.0 : 1 / 16.0 : 1 | All primary text |
| `inkSecondary` | `#5B6067` | `#A0A5AC` | 6.1 : 1 / 7.6 : 1 | Supporting sentences, labels |
| `inkTertiary` | `#8A9099` | `#6E747C` | 3.2 : 1 / 3.8 : 1 | **Non-essential only** (see below) |

**Ground**

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `paper` | `#FCFCFB` | `#17181A` | Window / page ground |
| `paperRaised` | `#FFFFFF` | `#1F2124` | Sheets, popovers, iOS grouped ground |
| `fillQuiet` | `#F2F1EE` | `#212326` | Cards, search field, open evidence block |
| `fillSelected` | `#E8E7E3` | `#2A2D31` | Sidebar selection, pressed states |
| `rule` | `#E4E3E0` | `#2E3134` | Hairline dividers |

**Accent — one hue, chosen by elimination**

| Token | Light | Dark | Contrast |
| --- | --- | --- | --- |
| `accent` | `#3A3F8F` | `#A0A6F0` | 9.0 : 1 / 8.1 : 1 on paper |
| `onAccent` | `#FFFFFF` | `#101114` | 9.2 : 1 / — |

Why indigo, argued rather than asserted: green is the hue we are deliberately
retiring (the seal), amber is `caution`, red is `blocked`, and system blue on
macOS is indistinguishable from having made no choice at all. Indigo is the only
hue in the space that is not already carrying a semantic, and desaturating it to
`#3A3F8F` moves it decisively off systemBlue while staying sober. It reads as
ink from a good pen, which is the correct association for a product about
signatures.

**Semantic — rare by construction**

| Token | Light | Dark | Contrast | Use |
| --- | --- | --- | --- | --- |
| `caution` | `#8A5A00` | `#E0B25C` | 5.8 : 1 / 8.9 : 1 | `.caution` glyph + text |
| `cautionGround` | `caution` @ 8% | `caution` @ 12% | — | `NappletNotice` ground |
| `refusal` | `#9A2B22` | `#F0938A` | 7.4 : 1 / 7.7 : 1 | `.blocked` glyph + text |
| `refusalGround` | `refusal` @ 8% | `refusal` @ 12% | — | `NappletNotice` ground |

### 4.3 Three defects in the current colour handling

1. **`NappletNotice` uses system `.orange` and `.red`.** System orange on white
   measures about **2.2 : 1**. It is used there for a glyph that carries
   meaning, and it fails WCAG for non-text contrast (3 : 1) as well as text
   (4.5 : 1). Replace with `caution` / `refusal` above. The 9% ground becomes 8%
   (and 12% in dark, where a light tint at 9% is nearly invisible).
2. **Ten `foregroundStyle(.orange)` / `.red` call sites are on the path**, in
   `CatalogEntryRow` ("Won't run here"), `PermissionCapabilityRow`
   (unavailability), `WorkbenchLibraryBuildRow`, and
   `CatalogBrowseEvidenceView`'s partial-window summary. All of these are
   already fully legible as sentences. Per ADR 0008 §4, colour there is
   reinforcing nothing that the words have not said, and it is what makes the
   list look like a status board. **Ruling: none of them get colour.** They are
   `inkSecondary`, and where the fact is genuinely blocking, it takes the place
   of the action (§8.3), which is a far stronger signal than a hue.
3. **`.tertiary` carries the publisher line in `CatalogEntryRow`.** At 3.2 : 1
   that is below the text threshold, and the publisher is not decoration — in a
   store with no artwork it is one of only two recognition handles a person has
   (§7.1). **Rule: `inkTertiary` is banned for any text that is the only place a
   fact appears.** Publisher moves to `inkSecondary`. `inkTertiary` survives
   only for chevrons, the divider-adjacent count, and text duplicated elsewhere
   on the same screen.

### 4.4 Greyscale proof (ADR 0008 consequence 4)

Every state, with hue removed:

| State | What carries it | Reads in greyscale because |
| --- | --- | --- |
| Verified / settled | **Nothing renders.** | There is nothing to lose. This is the ADR's own point. |
| Caution | `exclamationmark.triangle` + a sentence + an 8% ground | The glyph and the sentence are unchanged; the ground survives as a ~7% luminance step against paper, which is still a visible band. |
| Blocked | `hand.raised` + a sentence, **and the primary action is gone**, replaced by the reason | Structural. The strongest carrier is the absence of a button; zero colour dependence. |
| Primary action | Filled rectangle + `onAccent` label | It is the only *filled* element on the screen. Fill vs. no-fill, not hue. |
| Secondary action | Ink label, no fill, no border | Same. |
| Review tier | Heading words + attribution density + card vs. flat (§7.3) | Entirely structural. |
| "Written about an earlier version" | A sentence at `caption`, `inkSecondary` | It is a sentence. |
| Won't run here | A sentence in the reason position | It is a sentence. |
| Sidebar selection | `fillSelected` ground + `ink` (vs `inkSecondary`) label | Ground step plus weight step. |
| Technical tier | **Monospace** | §1.1. The typeface is the tier. |

Not one state in this system requires hue to be read. That is checkable by
screenshotting any screen through a greyscale filter and confirming nothing has
become ambiguous, which is a review step I would add to the PR template.

### 4.5 Increased contrast and reduced transparency

Read `@Environment(\.colorSchemeContrast)`. When `.increased`:

- `inkSecondary` → `ink`; `inkTertiary` → `inkSecondary`.
- Notice grounds 8% → 16%, and gain a 1pt border in `caution` / `refusal`.
- `rule` → `#B4B2AD` (light) / `#4A4E53` (dark).
- Cards gain a 1pt `rule` border in addition to `fillQuiet`.
- The primary action gains a 1pt `ink` border outside its accent fill.

`.accessibilityReduceTransparency`: we author no blur, so the only effect is
that iOS system nav chrome goes opaque, which the system handles.

---

## 5. Surface: elevation, borders, radii, dividers

### 5.1 No shadows. Anywhere.

The app authors **zero** `.shadow()`. Elevation is expressed by *ground value
plus a hairline rule*, never by a cast shadow. A page has no shadows; the
moment a card floats, we are back in the shelf metaphor. The system may draw
shadows around windows, sheets, menus, and popovers — those are the OS's and we
leave them alone.

Corollary: on iOS 26, **Liquid Glass is accepted only where the system draws
it** — navigation bars, tab bars, toolbars, sheet chrome. We never author a
glass material for content. A translucent card is elevation by another name and
it makes text contrast a function of what happens to be behind it, which we
cannot guarantee.

### 5.2 When is a card a card?

> **A card marks content the app did not write.**

That is the whole rule, and it is enforceable in review.

| Is a card | Because |
| --- | --- |
| A napplet in Discover | It is the napplet's own name and words |
| The capability block | It is the napplet's claim about itself |
| A review (Tiers 1 and 2) | It is a person's words |
| A pasted-address confirmation's summary | It is the acquired artifact's own claims |

| Is **not** a card | Because |
| --- | --- |
| A section of the page you are already on | The page is the container |
| The app's own explanatory prose | We wrote it; it is not quoted |
| The technical-details region when closed | It is a line of text |
| An empty state | There is nothing to quote |
| Tier 3 reviews | Deliberate — see §7.3 |

**Nested cards are banned.** Today `PermissionReviewSheet` renders
heading → card → heading → card → heading → card, which is exactly the grouped
`Form` look the brief is trying to escape. Under this rule it becomes one card
per capability *group*, with the group headings living on the page outside them,
and no card around the whole thing.

**A card that fills the screen is not a card**, it is a background. If a card is
the only thing in a region, delete the card.

### 5.3 Card anatomy

- Ground `fillQuiet`, radius `cardCorner` 12 `.continuous`, no border in default
  contrast, no shadow.
- Padding `comfortable` 16, or `roomy` 24 for page-scale cards (the capability
  block on the napplet page).
- Cards in a list are separated by `snug` 12. **Cards are never separated by
  dividers** — the gap is the divider.

### 5.4 Dividers

Hairline `rule`, 1 physical pixel (`1 / displayScale`), full-bleed only when it
separates *regions* (the search field from results; the evidence footer from the
list). Inset to the content margin when it separates *items* (Tier 3 reviews).

**Dividers between cards, between form rows, or under headings are banned.** A
divider under a heading is decoration; the space already did that job.

---

## 6. Iconography

The package currently uses **~50 distinct SF Symbols**. That is a vocabulary,
and nobody learns a 50-word vocabulary for an app they open twice a week. The
target is **under 20**, and the rules that get us there are these.

### 6.1 When an icon earns its place

An icon earns its place when **one** of these is true:

1. It is the **only** representation of a control (toolbar buttons, the
   overflow menu, the search-field clear button).
2. It is a **repeating noun** that helps a person re-find a row across screens —
   the capability domains, specifically. `camera` next to "take photographs"
   appears on the napplet page, the consent sheet, and the permissions list; the
   glyph is what lets you recognise it as the same thing in three places.
3. It is one of the **two verdict glyphs** (`exclamationmark.triangle`,
   `hand.raised`), where ADR 0008 §4 explicitly wants a non-colour carrier.

### 6.2 When it does not

4. **Never an icon for a state that is also stated in words.** This is the rule
   that killed the seal and it applies to everything downstream of it.
5. **Never an icon beside a section heading.** Decoration.
6. **Never an icon in an empty state.** §7.2. The 17 `ContentUnavailableView`
   call sites are all in violation.
7. **Never a `.fill` variant** unless the control it represents is *on*.
8. **Never an icon at accessibility text sizes** where it competes for width
   with the sentence it is decorating (§2.4).
9. **Never an icon carrying `accent` or a semantic colour**, with the two
   verdict glyphs as the only exception. Icons are `inkSecondary`.

### 6.3 Rendering

- Always via `Label`, so size tracks the adjacent text automatically. No fixed
  `.frame` on a symbol, ever — a symbol in a fixed frame at accessibility sizes
  is a broken layout.
- `.imageScale(.medium)`, `.symbolRenderingMode(.monochrome)`, weight `.regular`.
- Every icon accompanied by text is `.accessibilityHidden(true)`. The current
  code does this correctly in most places and should do it everywhere.
- **No symbol effects.** No `.symbolEffect(.bounce)`, no `.pulse`, no
  `.variableColor`. See §9.

### 6.4 Specific removals

| Remove | Why |
| --- | --- |
| `square.grid.2x2` on the empty catalog | A grid icon on an empty grid is a joke about itself |
| `antenna.radiowaves.left.and.right` on "Looking for napplets" | Protocol imagery on the plain tier; a person does not have an antenna |
| `chevron.right` on macOS catalog rows | A list of buttons on macOS does not need per-row chevrons. **iOS keeps it** — it is the platform's disclosure convention. A genuine platform difference. |
| `checkmark` on "doesn't ask for access to anything" (6 call sites) | The sentence is the verdict. A checkmark beside it is a green seal in greyscale clothing. |
| `slider.horizontal.3` / `chevron.up` on the customise toggle | It is a text control; the label carries it |

---

## 7. The hard visual problems

### 7.1 No icons, no screenshots

This is the central problem of the brief and I want to be precise about the
answer, because the obvious one is wrong.

#### The obvious answer, and why it is forbidden here

**Generated identity marks — identicons, Blockies, gradient hashes derived from
the pubkey or the coordinate — must not ship.** Not "would be nice to avoid."
Must not.

The argument is not aesthetic. A hash-derived mark is a **visual fingerprint the
user cannot verify but will inevitably learn to trust**. People do use identicons
as recognition: "that's the blue-triangle one." The moment they do, the mark is
carrying identity — and it is carrying it badly, because humans compare shapes
approximately and near-collisions look alike. That is a verdict the application
would be asserting without being able to stand behind it, which is the exact
failure mode ADR 0008 exists to prevent, and it is the visual twin of the
five-star average the experience doc §5.1 rejects. We would be replacing a
number-shaped decoration on an unknown with a *picture*-shaped one.

There is a sharper version: this product's entire pitch is that it does not
manufacture confidence. Manufacturing a confident-looking picture out of a hash
is the single most on-brand-looking, most off-brand-actually thing we could
build.

So: **no generated art. No gradient-from-hash. No letter-in-a-circle avatar for
napplets** (a monogram avatar implies a person or an org, and it collides with
the real avatars we do show on Tier 1 reviews).

#### What I am doing instead: the title *is* the artwork

The honest content we have is the name. So set the name at artwork scale and
give it artwork's position in the layout.

The Discover card leads with the title at `title` (22/17 semibold), occupying
the top of the card the way a thumbnail would, with the description and
publisher below it. At three cards on a 680pt measure, that is a contents page
and it looks expensive. This is not a fallback; a well-set name at scale is a
better recognition handle than a meaningless generated square, because it is
*readable*.

Two additional recognition handles, both real and both free:

- **Publisher, in a fixed position on every card.** When five of eight napplets
  come from one publisher, that is genuine visual grouping — and it is a fact,
  not a decoration. It moves to `inkSecondary` (§4.3) so it is actually legible.
- **"You've opened this before"** (experience doc §7, item 2). Rendered as a
  `caption` line in the card's footer, not a badge. Local, true, and the single
  best recall aid a store without pictures can have.

#### Being honest about what this looks like at scale

It looks excellent at 3–15 items and it degrades at roughly **40**.

Recognition memory is visual. A person who saw something yesterday and wants it
today re-finds it by shape and colour before they re-find it by name. A purely
typographic store denies them that, and the failure is not gradual — below ~40
items you can scan the whole page, above it you cannot, and the store becomes a
search box with a list attached. Beyond about 100, the wall of grey text is
genuinely worse than an ugly icon grid.

Two consequences I want on the record:

1. **Experience doc §3.2 is right that manifest artwork is the highest-value
   missing thing, and this document raises the priority rather than solving
   around it.** The visual system is designed to be correct *now* and to be
   correct *later* with artwork; it is not designed to make artwork unnecessary.
2. **The component reserves the affordance, not the pixels.**
   `NappletListingCard` takes `artwork: (() -> some View)?`, currently always
   `nil`, and lays out a 64pt (macOS) / 60pt (iOS) leading square when it is
   non-nil. Adding hash-pinned publisher artwork later is then a data change,
   not a redesign. **This is not a placeholder well** — when `artwork` is nil,
   *no space is reserved and no box is drawn*, in compliance with experience doc
   §9. An implementer must not "temporarily" fill it with grey.

### 7.2 Three napplets that look intentional

The diagnosis first, because it determines everything: **emptiness reads as
breakage when the container is larger than the content.** A 3-column grid
holding 3 items is not sparse, it is *nine slots with six missing*. Fix the
container, not the copy.

Five rules:

1. **Layout responds to count, not just to width.**
   ```
   columns = (containerWidth >= 900 && count >= 7) ? 2 : 1
   ```
   Below 7 items, a single column of full-measure cards. Never more than 2
   columns at any count on macOS at typical window sizes; never more than 1 on
   iPhone. Three full-measure cards fill their column completely and read as a
   deliberate page. Three cards in a 3-up grid read as a bug.
2. **Content is top-aligned and the page ends.** Never vertically centred, never
   stretched to fill, no `Spacer()` pushing content apart. A page that ends is
   honest (experience doc §7.1); a page that distributes three items across a
   1200pt window is a loading screen.
3. **The place has a header that does work.** `place` title ("Napplets") plus
   one true `lede` line — *"Everything that's reached this Mac."* A page with a
   title, a sentence, and three well-set cards is a page. Three cards floating
   with no header is a result set that failed.
4. **The count line stays at the bottom**, in the existing
   `CatalogBrowseEvidenceView` footer, which already has exactly the right copy
   model. Never a count at the top. "3 results" at the top of a page is the
   difference between a shelf and a query.
5. **`ContentUnavailableView` is refused, all 17 of them.** It is centred,
   icon-led, and it is macOS/iOS's standard *failure* chrome — it is what you
   see when a search returns nothing or a folder is gone. Using it for the
   primary state of a young catalog tells every new user the app is broken.

   Replaced by `NappletEmptyPlace` (§8.9): leading-aligned, no icon, a `title`
   line, one `body` sentence, and the real actions as plain buttons. For the
   truly-empty catalog those actions are exactly the two from experience doc
   §7.1 — paste an address, and look at what shipped with the app.

### 7.3 Three review tiers without a scoreboard

The trap: any vertical stack under headings implies rank, and emphasis differences
(bigger, bolder, tinted) turn implication into assertion.

**The move: differentiate on attribution density, not on emphasis.** The tiers
already differ in *how much we honestly know about the author*. Let that — and
only that — be the visual difference. The gradient then reads as informational,
because it is.

| | Tier 1 — People you follow | Tier 2 — Your wider network | Tier 3 — From people you don't follow |
| --- | --- | --- | --- |
| Container | Card, `fillQuiet` | Card, `fillQuiet` | **No card.** Flat on paper, hairline `rule` between items |
| Avatar | 28pt circle, their real picture | none | none |
| Name | `heading` (semibold), `ink` | `body` medium, `ink` | `secondary`, `inkSecondary` |
| Body | `body`, `ink` | `body`, `ink` | `body`, `ink` |
| Agreement line | *"Ana and 2 others you follow agree."* `caption` | not shown | not shown |
| Same type size for the review text | ✓ | ✓ | ✓ |

The critical detail: **the review text itself is identical in all three tiers**
— same size, same weight, same ink. What changes is the frame around it. Nobody
is being told a Tier 3 opinion is worth less; they are being told we know less
about who wrote it, which is literally what the heading says.

Headings are the experience doc's, unchanged, at `heading` size with `generous`
32 above and `snug` 12 below. The Tier 3 caveat — *"Anyone can write these, and
anyone can write many of them."* — sits directly under its heading at
`secondary` / `inkSecondary`.

**It is explicitly not in a tinted notice box.** A `caution` ground there would
be alarm colour on a non-alarm fact, would be the only tinted thing on a page
about other people's opinions, and would break §4.1's rule that semantic colour
means the runtime found something wrong. It is a description of the terrain, and
descriptions are prose.

**No badges on any review, in any tier.** Not "follows you", not "installed",
not "new". The experience doc §4.2 already rules out the install badge; the
visual system extends that to all of them, because a badge is a verdict in
sticker form.

**On the ordering objection.** Presenting 1, 2, 3 top to bottom does imply
*something*. My defence: a scoreboard's ordering is a claim the application
makes and cannot substantiate; this ordering is a restatement of the reader's
own follow list, and each heading says so in the reader's own words. The
ordering is *explained on the page* rather than asserted, which is the whole
difference. I have no better answer and I do not think one exists — every
possible order is an order.

**Greyscale:** card vs. flat, avatar vs. none, three different heading
sentences. Zero hue involved.

### 7.4 "Technical details": findable and invisible

Four properties, in order of importance:

1. **Position is always last, and separated by `spacious` 48.** After the final
   content of the region, below everything a person came for. This single number
   is most of the effect. Today's `roomy` 24 keeps it inside the reading flow.
2. **Closed, it is the quietest text on the screen.** One line: `footnote`,
   `inkSecondary`, the words "Technical details", a trailing `chevron.right`.
   **No card, no ground, no divider above it, no `DisclosureGroup` triangle.**
   A `DisclosureGroup` label renders at body prominence with a system triangle
   and reads as a control you are expected to use; this must read as a footnote
   you may follow.
3. **Open, it visibly becomes a different document.** The chevron rotates 90°,
   and the content appears on `fillQuiet` with `cardCorner` radius, a hairline
   `rule` at its top, and *everything inside set in `record` — monospace*. This
   is §1.1's payoff: the tier boundary is a typeface change you can see from
   across the room. Nothing inside is truncated, prettified, or summarised
   (ADR 0008 §1).
4. **The label never changes.** "Technical details", always, on every surface,
   in every context. Not "Show details", not "Provenance", not "Where these came
   from". (`CatalogBrowseEvidenceView` currently uses "Where these came from" —
   it is nicer copy and it is the wrong call, because a variable label cannot be
   learned. One exception I would accept: the footer's label may stay if the
   footer is understood as a different affordance; I would rather it did not.)

**Findability for the curious, on macOS:** a menu item **View → Technical
Details ⌥⌘T** that toggles *every* `NappletEvidence` on the frontmost surface at
once. Implemented as an environment value the component observes:

```swift
@Environment(\.nappletEvidenceExpansion) // .collapsed | .expanded | .perView
```

This is genuinely excellent for the person who wants evidence — one keystroke
turns any screen into the affidavit — and completely invisible to everyone else.
It is also the correct home for the "power user" energy the current app spreads
across every surface.

**iOS** has no menu bar, so the equivalent is a Settings toggle: *"Always show
technical details."* Off by default. Set once by the person who wants it. This
is a real platform difference, not a port.

---

## 8. The screens

Wireframes are proportional, not to scale. `·····` marks a hairline rule.
`▓` marks the accent-filled primary action.

### 8.1 Discover — macOS, populated

```
┌────────────────┬──────────────────────────────────────────────────────────┐
│                │                                                          │
│  NAPPLETS      │   [🔍 Search napplets                              ]     │  ← fillQuiet, radiusSmall
│                │                                                          │
│  ▸ Discover    │                                                          │  ← page 64
│    Saved       │   Napplets                                               │  ← place, New York 26 semibold
│    Yours       │   Everything that's reached this Mac.                    │  ← lede, inkSecondary
│                │                                                          │  ← spacious 48
│                │   ┌──────────────────────────────────────────────────┐   │
│                │   │  Good Morning                                    │   │  ← title 17 semibold
│                │   │  A calm start to the day: weather, your first    │   │  ← secondary, inkSecondary
│                │   │  meeting, and nothing else.                      │   │
│                │   │                                                  │   │  ← comfortable 16
│                │   │  by Sanity Island            You've opened this   │   │  ← caption, inkSecondary
│                │   └──────────────────────────────────────────────────┘   │
│                │                                                          │  ← snug 12
│                │   ┌──────────────────────────────────────────────────┐   │
│                │   │  Ledger                                          │   │
│                │   │  What you spent, without an account anywhere.    │   │
│                │   │                                                  │   │
│                │   │  by Unnamed publisher                            │   │
│                │   └──────────────────────────────────────────────────┘   │
│                │                                                          │
│                │   ┌──────────────────────────────────────────────────┐   │
│                │   │  Field Notes                       Won't run here│   │  ← reason, inkSecondary (NOT orange)
│                │   │  Quick notes that stay on this device.           │   │
│                │   │                                                  │   │
│                │   │  by Ana Ruiz                                     │   │
│                │   └──────────────────────────────────────────────────┘   │
│                │                                                          │
│                │   (page ends here — no filler, no centring)              │
│                ├··········································································│
│                │   7 napplets                          Technical details >│  ← footer, caption
└────────────────┴──────────────────────────────────────────────────────────┘
```

Cards: 680pt max width, leading-aligned, `comfortable` 16 padding, `snug` 12
apart. At ≥7 items and ≥900pt container, two columns of the same card at
`(measure − 12) / 2` each. **No chevrons on macOS rows.**

### 8.2 Discover — macOS, sparse (three napplets)

Identical to 8.1 with three cards. **That is the point.** There is no separate
sparse layout, no "getting started" module, no reduced-inventory apology. A
single column of full-measure cards is already the right shape for three, so
three looks like the design working. The only difference is the footer says
"3 napplets".

### 8.3 Discover — macOS, genuinely empty

```
│                │                                                          │
│  ▸ Discover    │   Napplets                                               │  ← place, New York
│    Saved       │                                                          │  ← generous 32
│    Yours       │   Nothing has arrived here yet.                          │  ← title 17 semibold, ink
│                │                                                          │  ← tight 8
│                │   Napplets show up here as they reach this Mac. In the   │  ← body, inkSecondary
│                │   meantime, you can open one someone sent you, or look   │     measure-capped
│                │   at what came with the app.                             │
│                │                                                          │  ← roomy 24
│                │   Paste an address                                       │  ← plain buttons, ink, underlined on hover
│                │   See what's included                                    │
│                │                                                          │
│                │   (nothing below. no icon. no spinner. no retry.)        │
```

Leading-aligned. No icon. No `ContentUnavailableView`. The two actions are the
two that genuinely work (experience doc §7.1). "Still arriving" is *not* this
state — while the window is filling, the footer says "Still looking…" and cards
appear as they arrive (§9.2).

### 8.4 Discover — iOS

```
┌─────────────────────────────┐
│  ⌄                          │  ← system nav / Liquid Glass
│  Napplets                    │  ← place, New York 28, large title
│                              │
│  [🔍 Search napplets     ]   │
│                              │
│  Everything that's reached   │  ← lede
│  this iPhone.                │
│                              │
│  ┌────────────────────────┐  │
│  │ Good Morning        ›  │  │  ← chevron KEPT on iOS
│  │ A calm start to the    │  │
│  │ day: weather, your     │  │
│  │ first meeting.         │  │
│  │                        │  │
│  │ by Sanity Island       │  │
│  └────────────────────────┘  │
│  ┌────────────────────────┐  │
│  │ Ledger              ›  │  │
│  │ …                      │  │
│  └────────────────────────┘  │
│                              │
│  7 napplets  Technical det.› │
├──────────────────────────────┤
│  Discover   Saved    Yours   │  ← TabView
└──────────────────────────────┘
```

Single column always on iPhone. Margins 20. Card tap target is the whole card.
**No pull-to-refresh** (§9.3).

### 8.5 The napplet page — macOS

```
│                │                                                          │
│                │   Good Morning                                           │  ← display, New York 26 semibold
│                │   Sanity Island                                          │  ← lede, inkSecondary
│                │                                                          │  ← roomy 24
│                │   A calm start to the day: weather, your first meeting,  │  ← body, ink, measure-capped
│                │   and nothing else.                                      │
│                │                                                          │  ← spacious 48
│                │   What it will be able to do                             │  ← heading
│                │   ┌──────────────────────────────────────────────────┐   │
│                │   │  ◷  Know roughly where you are                   │   │  ← secondary; icon = repeating noun
│                │   │  ✉  Read your calendar for today                 │   │
│                │   │                                                  │   │  ← comfortable 16
│                │   │  Only if you say yes                             │   │  ← caption semibold, inkSecondary
│                │   │  ⊙  Send notifications                           │   │
│                │   └──────────────────────────────────────────────────┘   │
│                │   You choose what it can do the first time you open it.  │  ← secondary, inkSecondary
│                │                                                          │  ← generous 32
│                │   ▓▓▓▓▓▓▓▓▓▓▓▓                                           │
│                │   ▓    Add    ▓        Save                              │  ← the ONE accent element
│                │   ▓▓▓▓▓▓▓▓▓▓▓▓                                           │
│                │                                                          │  ← spacious 48
│                │   People you follow                                      │  ← heading
│                │   ┌──────────────────────────────────────────────────┐   │
│                │   │ (●) Ana Ruiz                                     │   │  ← 28pt avatar + heading
│                │   │     I've used this every morning for a month.    │   │  ← body, ink
│                │   │     It does one thing.                           │   │
│                │   │     Ben and 2 others you follow agree.           │   │  ← caption, inkSecondary
│                │   └──────────────────────────────────────────────────┘   │
│                │                                                          │  ← generous 32
│                │   From people you don't follow                           │  ← heading
│                │   Anyone can write these, and anyone can write many      │  ← secondary, inkSecondary
│                │   of them.                                               │     NOT a tinted box
│                │                                                          │  ← comfortable 16
│                │   npub1x…kq                                              │  ← secondary, inkSecondary
│                │   Works. Wish it had a week view.                        │  ← body, ink
│                │   Written about an earlier version.                      │  ← caption, inkSecondary
│                │ ·········································                │  ← hairline, inset
│                │   Marta                                                  │
│                │   Fine for what it is.                                   │
│                │                                                          │  ← spacious 48
│                │   Technical details  ›                                   │  ← footnote, inkSecondary. LAST.
│                │                                                          │
```

Order is the experience doc's §3.1, unchanged: what it is → who made it → what
it will do → the action → what people say → evidence. **Capabilities above
social**, as decided there.

**Blocked variant.** The action row is replaced, in place, by:

```
│                │   This napplet doesn't run on Mac.                       │  ← body, ink
│                │   ⚠ It needs a camera this device doesn't have.          │  ← NappletNotice, caution ground
```

No disabled button. The reason occupies the button's position, which is the
strongest possible non-colour signal (§4.4).

### 8.6 The napplet page — iOS

Same vertical order, with one genuine platform difference:

```
┌─────────────────────────────┐
│  ‹ Napplets                  │
│                              │
│  Good Morning                │  ← display, New York 34
│  Sanity Island               │
│                              │
│  A calm start to the day:    │
│  weather, your first meeting,│
│  and nothing else.           │
│                              │
│  What it will be able to do  │
│  ┌────────────────────────┐  │
│  │ ◷ Know roughly where…  │  │
│  │ ✉ Read your calendar…  │  │
│  └────────────────────────┘  │
│                              │
│  People you follow           │
│  …                           │
│                              │
│  Technical details  ›        │
│                              │
├──────────────────────────────┤
│  ▓▓▓▓▓▓▓ Add ▓▓▓▓▓▓▓  ♡ Save │  ← pinned, safeAreaInset(.bottom)
└──────────────────────────────┘
```

**The action pins to the bottom on iOS and scrolls with the page on macOS.**
Genuine difference, not a stretch: a phone page is taller than its viewport and
the reachable zone is the bottom third; a Mac window shows the whole page and a
floating bar there would be chrome for its own sake. The pinned bar is
`paperRaised` with a top hairline `rule`, 50pt action height, 16pt padding, plus
the safe-area inset.

### 8.7 Install / consent

**Install from the napplet page: there is no sheet.** Experience doc §6.3. `Add`
acts directly; the button crossfades to "Adding…" at a reserved width (§9.1) and
then to `Open`.

**Pasted address — the surviving confirmation.** macOS sheet 520×420; iOS
`.presentationDetents([.medium])`.

```
┌────────────────────────────────────────────┐
│  Cancel                              Add   │  ← Add is accent-tinted text (toolbar), not filled
├────────────────────────────────────────────┤
│                                            │  ← roomy 24
│  Good Morning                              │  ← title 17 semibold (NOT display; this is a sheet)
│  Sanity Island                             │  ← lede, inkSecondary
│                                            │  ← roomy 24
│  ┌──────────────────────────────────────┐  │
│  │  It will ask to:                     │  │  ← caption semibold, inkSecondary
│  │  ◷ Know roughly where you are        │  │
│  │  ✉ Read your calendar for today      │  │
│  └──────────────────────────────────────┘  │
│                                            │
│  Adding it now gives it access to nothing. │  ← secondary, inkSecondary
│                                            │  ← spacious 48
│  Technical details  ›                      │
└────────────────────────────────────────────┘
```

Note `display` is not used in sheets. New York at large-title scale inside a
520pt sheet is shouting.

**First run — the actual consent moment.** macOS 560×520; iOS full-height sheet.
This is the heaviest surface in the app and it should *feel* heavier: `roomy` 24
padding throughout instead of `comfortable` 16, one card per capability group,
group headings on the page outside the cards (§5.2).

```
┌────────────────────────────────────────────┐
│  Not Now                    Allow and Open │  ← confirmation action, accent
├────────────────────────────────────────────┤
│                                            │
│  Open Good Morning?                        │  ← title 17 semibold
│  From Sanity Island. Here's what it's      │  ← lede, inkSecondary
│  asking for.                               │
│                                            │  ← generous 32
│  It needs to                               │  ← heading, on the page
│  ┌──────────────────────────────────────┐  │
│  │ ◷  Know roughly where you are        │  │  ← secondary medium, ink
│  │    So it can show your weather.      │  │  ← caption, inkSecondary
│  │                                      │  │  ← comfortable 16
│  │ ✉  Read your calendar for today      │  │
│  │    To show your first meeting.       │  │
│  └──────────────────────────────────────┘  │
│                                            │  ← comfortable 16
│  It would also like to                     │  ← heading
│  ┌──────────────────────────────────────┐  │
│  │ ⊙  Send notifications                │  │
│  │    Not available on this device, so  │  │  ← caption, inkSecondary (NOT orange)
│  │    it won't work.                    │  │
│  └──────────────────────────────────────┘  │
│                                            │  ← roomy 24
│  Choose each one myself                    │  ← secondary, ink, underlined. No icon.
│                                            │  ← spacious 48
│  Technical details  ›                      │
└────────────────────────────────────────────┘
```

The single gesture is the toolbar's confirmation action, preselected with
Rust's `recommendedDecision`, exactly as today. "Choose each one myself" loses
its icon (§6.2 rule 5) and its `.buttonStyle(.link)` accent (§4.1); it becomes
ink with an underline, which is this system's only link treatment.

### 8.8 Yours (the library)

```
│    Saved       │   Yours                                                  │  ← place, New York
│  ▸ Yours       │   What you've added.                                     │  ← lede
│                │                                                          │  ← spacious 48
│                │   Good Morning                                  Open  ⋯  │  ← title 17 semibold + actions
│                │   Open since this morning                                │  ← caption, inkSecondary
│                │ ·········································                │
│                │   Ledger                                       Open  ⋯  │
│                │   Update available                                       │  ← caption, inkSecondary
│                │ ·········································                │
│                │   Field Notes                                  Open  ⋯  │
│                │   Won't run here                                         │  ← caption, inkSecondary
│                │                                                          │  ← spacious 48
│                │   Technical details  ›                                   │  ← ONE per page, not per row
```

**Yours is a list, not a grid of cards.** Deliberate contrast with Discover:
these are things you already chose, so the job is *acting on them*, not
evaluating them, and a list is denser and faster to act on. Rows are separated
by inset hairlines (§5.4), not cards (§5.2 — this is your inventory, not quoted
content).

**One `Technical details` per page, not one per row.** Today
`WorkbenchLibraryBuildRow` puts a `NappletEvidence` on every row, which puts a
disclosure control on every line of the library and is the single densest patch
of instrumentation left in the app. Collapse to one at the page foot listing
every build, or move per-build evidence into the row's `⋯` menu.

iOS: same list, 44pt minimum row height, `Open` as the row tap, `⋯` as a
trailing menu, destructive actions in swipe actions rather than visible.

---

## 9. Motion

Restrained to three durations and two curves. Everything not listed here does
not animate.

| What | Duration | Curve | Notes |
| --- | --- | --- | --- |
| Micro (press, hover, focus) | 0.15s | `.easeOut` | System-provided where possible |
| Disclosure open/close, chevron rotation | 0.22s | `.easeInOut` | Reversible, so symmetric curve |
| Card arrival in Discover | 0.30s | `.easeOut` | Opacity 0→1 + 8pt rise, staggered 30ms, **stagger capped at 6 items** |

`.bouncy` and any spring with `bounce > 0` are banned. `.snappy` is acceptable
as a stand-in for the micro tier.

### 9.1 The one deliberate width reservation

The primary action's label changes (`Add` → `Adding…` → `Open`). It **crossfades
only** — no width animation, no spring. Reserve the width to the longest label in
the set via `.frame(minWidth:)` measured from the label strings, so the button
never resizes. A button that grows and shrinks while you are deciding whether to
press it is a small hostility.

### 9.2 The one piece of expressive motion, and why it earns its place

Card arrival. Experience doc §7.1 says *"'Still arriving' is a state, not a
spinner"* — the catalog is a live window that fills in. Something has to make
that legible, and the two conventional options are both wrong: a spinner blocks
and implies a request/response the runtime does not have, and a skeleton loader
is a *claim that content is coming*, which in a bounded relay window is a claim
we cannot make.

A card fading and rising into place makes "it is filling in" visible with no
claim about what else is coming. It is the only expressive motion in the app and
it is doing real informational work.

### 9.3 What must not animate, and what I am refusing

- **Notices and verdicts never animate in.** A caution that slides in reads as a
  toast; toasts are dismissible and transient; this is neither. It is simply
  present.
- **No skeleton / shimmer loaders.** They are a lie about content that may never
  arrive, and in a sparse catalog they would be the dominant visual on the
  screen.
- **No hero / matched-geometry transition** from card to napplet page. It would
  be the most "designed" moment in the app and it would be animating a card that
  has no artwork into a page that has no artwork — motion in place of content.
- **No pull-to-refresh.** The catalog is an observed live window, not a fetch.
  A refresh gesture implies a request whose completion means something, and
  nothing here completes. The footer already says "Still looking…".
- **No symbol effects, no `contentTransition(.numericText())`** (there are no
  numbers), no parallax, no scroll-linked anything.
- **Sidebar / tab switching uses the system transition.** No custom.

### 9.4 Reduce Motion

`@Environment(\.accessibilityReduceMotion)`:

- Card arrival → opacity only, 0.15s, no rise, no stagger.
- Disclosure → no height animation; content appears. Chevron does not rotate;
  it swaps `chevron.right` → `chevron.down`.
- Micro tier → unchanged (already below the threshold that matters).

---

## 10. Component specifications

Common to all: `.accessibilityHidden(true)` on every icon accompanied by text;
`accessibilityIdentifier` preserved wherever one exists today (the UI suite
drives them); no component reads `\.nappletDisclosure` to decide its *own* tier.

### 10.1 `NappletActionButton`

The only accent surface in the app.

**Anatomy.** Label (`body`, `.medium`) centred in a rounded rectangle,
`radiusSmall` 6 `.continuous`, no border, no shadow.

**Roles.**

| Role | Fill | Label | Occurrence |
| --- | --- | --- | --- |
| `.primary` | `accent` | `onAccent` | **Max one per screen** |
| `.secondary` | none | `ink` | Any number |
| `.destructive` | none | `refusal` | In menus only, never on the path |

**Sizes.** macOS: height 28 (`.regular`) / 32 (`.large`, page action), padding-x
16. iOS: height 44 minimum, 50 for the pinned page action, padding-x 20, full
width when pinned.

**States.**

| State | Primary | Secondary |
| --- | --- | --- |
| default | `accent` fill | ink label |
| hover (macOS) | fill lightens 6% | label gains underline |
| pressed | fill darkens 10%, no scale transform | `fillSelected` ground |
| disabled | fill → `fillSelected`, label → `inkTertiary` | label → `inkTertiary` |
| focused (keyboard) | system focus ring, 2pt, outside the shape | same |

**Disabled is a last resort.** Per experience doc §3.3, a blocked action is
*replaced by its reason*, not disabled. Disabled is reserved for genuinely
transient states — mid-install, mid-submit.

**Accessibility.** Label is the verb; `accessibilityHint` states the
consequence, which the current code already does well ("Adds this napplet. It
cannot do anything until you open it."). Never announce a colour or a shape. On
disabled, `.accessibilityHint` must state *why* — a disabled control with no
announced reason is inaccessible.

### 10.2 `NappletCard` (revised)

**Changes from the current implementation:**
- Ground `.quaternary.opacity(0.4)` → `fillQuiet` token. A material at 40%
  opacity composites unpredictably against different parents and is nearly
  invisible in dark mode.
- `cornerRadius: 10` → `cornerRadius: 12, style: .continuous`.
- Padding parameterised: `.standard` 16 / `.page` 24.
- **Assert non-nesting.** In `DEBUG`, an environment flag set by `NappletCard`
  traps if a second `NappletCard` appears inside one. Cheap, and it enforces
  §5.2 without review vigilance.
- Increased contrast → 1pt `rule` border.

### 10.3 `NappletListingCard` (new)

The Discover card.

**Anatomy.**
```
[artwork?] Title            [reason?]
           Description (2 lines max)
           <comfortable 16>
           by Publisher     [You've opened this?]
```

- `artwork: (() -> some View)?` — **always `nil` today**; lays out a 64pt
  (macOS) / 60pt (iOS) leading square with `radiusSmall` when non-nil. When
  `nil`, **no space is reserved and no box is drawn** (§7.1, experience doc §9).
- Title `title`, 2 lines, `ink`.
- Description `secondary`, 2 lines, `inkSecondary`.
- Publisher `caption`, `inkSecondary` (**changed from `.tertiary`**, §4.3).
- Reason (`Won't run here` / `Might not run here`) `caption`, `inkSecondary`,
  trailing-aligned, max width 140 — **no orange** (§4.3).
- Chevron: iOS only.

**States.** default / hover (macOS: ground → `fillSelected`) / pressed (ground →
`fillSelected`, no scale) / focused (2pt system ring outside the card).

**Accessibility.** One element, `.accessibilityElement(children: .combine)`,
label = the existing `"\(title), from \(publisher). \(summary)"`, hint = "Opens
this napplet's page". Trait `.isButton`. Reason is appended to the label, not
left as a separate element.

### 10.4 `NappletPageHeader` (new)

`display` (New York) name, `lede` publisher, `roomy` 24, then `body` description
capped at `measure`. Used only on the napplet page. `.accessibilityAddTraits(.isHeader)`
on the name.

### 10.5 `NappletSection` (new)

`heading` + content, `tight` 8 between them, `generous` 32 above. Replaces the
ad-hoc `VStack(spacing: .roomy) { Text().font(.headline); … }` repeated across
the current sheets. **No icon, no divider, no card around the heading** (§5.2,
§6.2). `.accessibilityAddTraits(.isHeader)` on the heading so VoiceOver
rotor navigation by heading works — this is currently missing everywhere and it
is the cheapest accessibility win in the codebase.

### 10.6 `NappletEvidence` (revised)

**Changes:**
- `DisclosureGroup` → a custom `Button` + conditional content. The system
  disclosure triangle and label prominence are both wrong (§7.4).
- Label `footnote` / `inkSecondary`, trailing `chevron.right` rotating 90° over
  0.22s. Reduce Motion → glyph swap.
- `spacious` 48 above, always. Always last in its region.
- Open content gets `fillQuiet`, `cardCorner`, top hairline `rule`, and
  `.fontDesign(.monospaced)` applied at the region root.
- Reads `@Environment(\.nappletEvidenceExpansion)` for the ⌥⌘T / Settings
  override (§7.4).
- Keeps `.nappletDisclosure(.technical)`, `.textSelection(.enabled)`, and the
  `napplet-evidence` identifier exactly as they are.

**Accessibility.** `.isButton` + `.accessibilityValue("expanded"/"collapsed")`.
The existing hint is good and stays. Content inside must announce hashes as
character-by-character where VoiceOver would otherwise read a hex string as a
word — set `.speechSpellsOutCharacters(true)` on `NappletFieldGrid` values.
That is a real defect today: VoiceOver currently reads aggregate hashes as
gibberish syllables, which makes the technical tier useless to the exact person
most likely to need it read aloud.

### 10.7 `NappletFieldGrid` (revised)

- `.font(.caption)` → `.font(.footnote)` (§2.2).
- `hairline + 2` → `micro`.
- `.speechSpellsOutCharacters(true)` on values (§10.6).
- Value column gets `.lineLimit(nil)` and wraps; a truncated hash is a defect
  under ADR 0008 §1.
- At accessibility text sizes, the `Grid` collapses to stacked label-above-value
  pairs — a two-column grid at `.accessibility5` is unreadable.

### 10.8 `NappletReviewBlock` + `NappletTierHeading` (new)

Per §7.3. Three configurations from one component, selected by a projected tier
value (Rust's — native never derives it).

**Anatomy (Tier 1):** 28pt circular avatar, `heading` name, `body` text,
optional `caption` agreement line, optional `caption` "Written about an earlier
version." Card, `comfortable` 16.

**Tier 2:** no avatar, `body` medium name. Card.
**Tier 3:** no avatar, `secondary`/`inkSecondary` name, **no card**, inset
hairline separators between items.

**Avatar failure is silent.** No initials fallback, no placeholder person glyph
— an unloaded avatar renders as nothing and the layout reflows. A grey person
silhouette is a picture of an absence.

**Accessibility.** One element per review. Label:
`"\(name) wrote: \(text)"`, followed by the agreement line and the version note
if present. The tier heading is `.isHeader`. **The tier itself is never
announced as a rank** — VoiceOver hears the heading text, which is a sentence
about the reader's follow list, and nothing else.

### 10.9 `NappletEmptyPlace` (new)

Replaces all 17 `ContentUnavailableView` uses.

**Anatomy.** Leading-aligned, no icon. `title` line (`title` 17 semibold, `ink`),
`tight` 8, `body` sentence at `inkSecondary` capped at `measure`, `roomy` 24,
then zero or more `.secondary`-role actions stacked with `snug` 12.

**Never centred. Never vertically distributed. Top-aligned in its container.**

**Accessibility.** The title carries `.isHeader`. The whole block is not
combined into one element — the actions must be separately reachable.

### 10.10 `NappletSearchField` (revised)

`fillQuiet` ground, `radiusSmall` 6 `.continuous`, `magnifyingglass` leading at
`inkSecondary`, `xmark.circle.fill` trailing only when non-empty. Height 28
(macOS) / 36 (iOS). Focused: 2pt accent ring — **the one place accent appears
that is not the primary action**, and it is a system focus convention rather
than a decision, so §4.1 tolerates it.

The "Searching the napplets that have arrived so far." line (experience doc
§2.3) renders at `caption` / `inkSecondary` **directly above the results, not
under the field**, and only while a search is active.

### 10.11 `NappletCapabilityLine` (revised)

`Label`: domain symbol at `inkSecondary`, sentence at `secondary`/`.medium`,
optional explanation at `caption`/`inkSecondary` below. At accessibility sizes
the icon is dropped entirely (§2.4, §6.2 rule 8). Combined into one
accessibility element with the sentence and explanation, as today.

Unrecognised domains keep `NappletVocabulary`'s honest degradation verbatim —
the raw domain token appears in the `record` voice inline (a monospace run
inside a prose line), which is the one place the two voices meet and it is
correct: it marks exactly the fragment we could not translate.

### 10.12 `NappletNotice` (revised)

- `.orange` / `.red` → `caution` / `refusal` tokens (§4.3, contrast defect).
- Ground 9% → 8% light / 12% dark.
- `cornerRadius` gains `.continuous`.
- **Does not animate in** (§9.3).
- Increased contrast → 16% ground + 1pt border.
- Keeps: renders nothing for `.settled`; glyph differs by case as well as hue;
  `.accessibilityElement(children: .combine)`. All three are correct today.

### 10.13 Keyboard focus order

**macOS, napplet page window:** search field → sidebar (arrow keys within) →
card grid (arrow keys within, Return opens) → detail column: primary action →
secondary action → review section (VoiceOver rotor by heading) → Technical
details. Escape returns focus to the card grid; ⌘F focuses search; ⌥⌘T toggles
evidence.

**iOS:** the system order (top to bottom, then the pinned action bar last).
Full Keyboard Access must reach the pinned action — verify, because
`safeAreaInset` content has historically been ordered inconsistently.

---

## 11. What I am refusing to do, and why

| Refused | Why |
| --- | --- |
| **Hash-derived identicons / generated artwork** | §7.1. An unverifiable visual fingerprint that people will use as identity. It is the five-star average in picture form, and it is forbidden by ADR 0008, not merely disfavoured. The most important refusal in this document. |
| **Shadows and elevation** | §5.1. Elevation is the shelf metaphor. Ground + hairline does the same job and cannot drift. |
| **Skeleton / shimmer loaders** | §9.3. A claim that content is coming, made about a bounded relay window that may return nothing. |
| **`ContentUnavailableView`, all 17** | §7.2. Centred, icon-led failure chrome used for the primary state of a young catalog. |
| **All-caps, letter-spaced section labels** | The single most reliable tell of a dashboard. `heading` in sentence case does the same job. |
| **`.bold` and heavier** | §2.2. The tool you reach for when the hierarchy failed. |
| **`.fontDesign(.rounded)`** | §2.1. Friendliness as a costume on a provenance product. |
| **A licensed display face** | §2.1. New York is free, variable, optically sized, Dynamic Type-native, and already installed. |
| **Colour-coded rows, status dots, tinted list items** | §4.1. Precisely the diagnostics grammar ADR 0008 §4 names. |
| **Semantic colour on the ten current call sites** | §4.3. All are already legible sentences; the colour reinforces nothing and costs the whole aesthetic. |
| **A tinted warning box around the Tier 3 caveat** | §7.3. Alarm colour on a non-alarm fact; it would be the only tinted thing on a page about opinions. |
| **Badges of any kind on reviews** | §7.3. A badge is a verdict in sticker form, and we do not have the verdicts. |
| **A hero transition from card to page** | §9.3. Animating artwork we do not have into artwork we do not have. |
| **Pull-to-refresh** | §9.3. Implies a fetch that completes; nothing here completes. |
| **Symbol effects, numeric transitions, parallax** | §9.3. There are no numbers, and nothing here is a delight moment. |
| **Authored Liquid Glass / blur on content** | §5.1. Translucent content makes contrast a function of whatever is behind it. System chrome only. |
| **~30 of the ~50 current SF Symbols** | §6. Nobody learns a 50-word icon vocabulary for an app they open twice a week. |
| **A dark "security" theme, a lock motif, a fingerprint motif** | §1.2. It is the complaint, restyled. |
| **Centring anything** | §2.3. Centred short text in a large frame is the universal signal for "nothing here". |

Two of these are worth naming as genuine costs rather than easy wins. Refusing
generated artwork means Discover is grey text for as long as manifests carry no
images, and §7.1 is honest that this stops working around 40 items. Refusing
elevation and semantic colour means the app will look *quieter* than its
competitors in a screenshot, and quiet does not photograph well. I am taking
both trades, because the alternative in each case is a visual claim the product
cannot back — which is the one thing this product is not allowed to do.

---

## 12. Tensions with the experience design

Per instruction, disagreements are declared rather than silently resolved. There
are three, and none of them changes a decision in that document.

**12.1 The Tier 1/2/3 vertical order is a ranking gesture, and no visual system
can fully neutralise it.** §7.3 gets as close as I know how — identical review
typography across tiers, difference expressed only as attribution density, order
explained by headings in the reader's own words. But three stacked groups top to
bottom will always read as strongest-first to some people. I am not proposing a
change; every alternative order is also an order, and randomising would be
worse. Flagging it so nobody believes the visual layer solved it.

**12.2 §7.1's "Discover shows its entire inventory, no See All" and my
count-driven column rule interact.** At 40+ items, a single scroll of
full-measure cards is very long. I accept the length as honest, and the
two-column switch at ≥7 is the only pressure valve I am building. Past ~60 items
the tension becomes real and the answer is artwork (§7.1), not pagination.

**12.3 The macOS three-item sidebar is heavy chrome for a sparse app.** A
`NavigationSplitView` sidebar with Discover / Saved / Yours, where two are
usually empty, is a lot of furniture around three cards. I considered proposing a
segmented control in the toolbar instead. **I am not proposing it**, and the
experience doc's reasoning wins: the sidebar is what makes the browser a *place*
rather than an errand (§2.1), and that framing is worth more than the pixels it
costs. Noted only so the trade is visible.

One clarification rather than a tension: §3.2 forbids placeholder image wells,
and §10.3 gives `NappletListingCard` an optional `artwork` slot. These do not
conflict — the slot reserves an *API affordance*, and when it is `nil` no space
is reserved and nothing is drawn. An implementer must not fill it with a grey
box "temporarily."

---

## 13. Implementation order

Sequenced so that each step is independently shippable and visible.

1. **Tokens.** Extend `NappletMetrics` (§3.1); add the `NappletInk` palette
   (§4.2); add `.continuous` to all seven radii. No layout changes. Immediately
   visible: warm paper, correct squircles.
2. **Type.** Add the token layer (§2.2); New York on the two display roles;
   `record` → `.footnote`. Immediately visible: the app stops looking default.
3. **Colour rules.** Remove the ten semantic-colour call sites (§4.3); fix
   `NappletNotice`'s contrast defect; promote the publisher line off `.tertiary`.
4. **`NappletEvidence`** (§10.6) plus ⌥⌘T. This is the largest single perceived
   change: every surface's bottom third goes quiet.
5. **`NappletEmptyPlace`** replacing all 17 `ContentUnavailableView` (§10.9).
6. **Icon cull** (§6.4), and `.isHeader` traits everywhere (§10.5).
7. **`NappletListingCard`** and the count-driven column rule (§7.2).
8. **The napplet page** (§8.5, §8.6) — depends on the IA change the experience
   doc specifies, and is the first step that is not purely a restyle.
9. **Reviews** (§10.8) — gated on the Rust work in experience doc §10.2–10.3.

Steps 1–6 are pure native restyling against surfaces that already exist and
would, on their own, get most of the way to the brief.

---

## 14. The one-line summary

Set the words as though they matter, in three voices that mark exactly which
tier you are in; spend colour once per screen and never on a state; let the
container be the size of the content so three napplets look like three napplets;
and refuse every picture we cannot verify, including the pretty ones we could
generate ourselves.
