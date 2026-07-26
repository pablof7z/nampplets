# Visual identity across disjoint napplets

A cohesive product does not require every napplet to look identical. It
requires them to feel governed by one intentional visual language: common
hierarchy, color roles, type rhythm, density, controls, motion, accessibility,
and shell behavior. The host owns that visual system; napplets render through
it without gaining authority over one another.

## Start with the current portable minimum

At `napplet/naps@5ac0490461ca6fec2f0d2e45b4835cf9bc08de24`,
NAP-THEME is listed as `Active` in the registry while its document still says
`draft`. Report that source discrepancy.

The contract is read-only and shell-owned:

```text
theme.get -> theme.get.result
theme.changed -> automatic shell push
```

Every theme contains:

```text
colors.background
colors.text
colors.primary
```

It may also contain body/title font name and URL, background media, and a
human-readable title. The shell may derive it from a kind `16767` event, user
preference, hardcoded default, operating-system appearance, or another source.
The source remains invisible to napplets.

This is sufficient for a portable baseline. It is not a complete visual
identity. It has no semantic surfaces, secondary text, borders, destructive or
success colors, typography scale, spacing, radii, control metrics, iconography,
motion, layout modes, or accessibility policy.

Do not invent those fields and call them NAP-THEME. Label richer data as product
composition policy, a private additive projection, or a proposed NAP revision.

## One canonical product theme

The composition should select one revisioned `ProductThemeProfile`:

```text
identity
  id, revision, title, provenance
brand
  palette, typography, icon family, imagery, voice
semantics
  canvas, surface, elevated, text, muted, accent, onAccent
  border, focus, selection, success, warning, destructive
metrics
  spacing scale, radii, border widths, control heights, density
type
  families, sizes, weights, line heights, tracking
motion
  durations, easing, transition roles
presentation
  supported slot/layout modes, safe areas, backdrop rules
accessibility
  contrast floor, text scaling, reduced motion/transparency behavior
portable projection
  NAP-THEME colors, optional fonts/background/title
```

This shape is a recommended product model, not a current NAP schema.

The profile belongs to the composition rather than to a particular napplet.
Individual components may derive local emphasis from it, but they should not
silently establish unrelated palettes, type systems, control shapes, or global
chrome. An immersive/game/media role may opt into a declared presentation mode;
that exception remains shell-visible product policy.

## Resolution and ownership

Use one deterministic pipeline:

```text
brand profile
+ explicit user selection
+ light/dark platform facts
+ accessibility facts
-> Rust-owned resolved theme revision
-> native shell projection
-> portable NAP-THEME projection
-> optional richer product-token projection
```

Ownership:

- The product/composition owns brand, supported variants, fallback, and which
  user customizations are allowed.
- Native code reports raw appearance and accessibility facts. It does not
  choose palette semantics independently.
- Rust owns validation, precedence, revision, limits, lifecycle, and the
  canonical resolved theme state.
- Native chrome renders from that resolved revision.
- The trusted shell projects the same revision to every eligible napplet.
- Napplet code maps received values into its own bounded CSS variables and
  component styles.
- NMP owns any source Nostr event and its canonical replacement/deletion truth.
  Runtime preference state may retain the selected reference, not duplicate the
  event as a second truth.

A useful precedence rule is:

```text
mandatory accessibility transformation
explicit user choice allowed by product
product brand variant for platform light/dark
safe built-in fallback
```

Operating-system accent should normally modify the brand within declared
bounds, not erase it. Otherwise every generated product becomes the same
generic system theme.

## Apply one revision everywhere

Avoid a split frame where native chrome changes before napplets or different
napplets observe different theme generations.

1. Resolve and validate the complete next theme in Rust.
2. Assign a monotonic composition-owned revision.
3. Update native chrome and stage napplet projections from that snapshot.
4. Send the portable `theme.changed` payload only to mapped, declaring, ready
   sessions through finite conflating lanes.
5. If a richer private token projection exists, bind it to the same revision.
6. Napplet adapters apply all CSS variables in one render transaction.
7. Stale sessions and stale revisions remain inert.

Current NAP-THEME does not carry a revision field. The runtime may retain the
revision internally and in an additive product projection, but must not add it
to the portable wire shape while claiming unchanged compatibility.

On initial mount, obtain or inject the theme before the first meaningful paint
when the selected projection permits it. Otherwise render a shell-consistent
neutral skeleton until `theme.get` resolves; do not flash a napplet-specific
default brand.

## What a napplet should do

A theme-aware napplet should:

- declare/support `theme` according to the selected compatibility contract;
- map semantic usage through local CSS custom properties rather than scatter
  literal colors and font names through components;
- use `background`, `text`, and `primary` as fallbacks, not force three colors
  to serve every semantic role;
- listen for `theme.changed` and replace the theme atomically;
- respect host-owned surface edges, title bars, padding, focus, and safe areas;
- support text scaling, keyboard focus, contrast, reduced motion, and reduced
  transparency;
- keep a visually safe fallback when the theme domain is absent or refused;
- avoid persisting the active theme as component truth.

The shell generator should emit a maintained adapter for each supported
framework. Curated napplets should share semantic components—buttons, fields,
empty states, menus, cards, dialogs, icon wrappers—built on those variables.
This is what stlstr gains from DaisyUI and its shared `napplet-kit`: coordinated
authoring above the minimal theme transport.

Do not inject a shared global stylesheet into an untrusted iframe. It couples
DOM internals, breaks isolation, and gives the host a brittle private API.
Share tokens, framework adapters, authoring components, and conformance rules.

## Fonts, images, and network authority

Font and background URLs are data, not permission to fetch.

- Validate scheme, length, MIME, digest/provenance when applicable, and public
  destination policy.
- Fetch through a bounded trusted resource/media path or package verified
  assets with the shell.
- Never relax iframe CSP or grant direct network merely so a font loads.
- Prefer shell-rendered product backdrops when a background need not enter the
  napplet.
- Give every napplet a metric-compatible system-font fallback.

Current NAP-THEME describes URLs but does not standardize a secure native asset
delivery projection. Name that gap instead of smuggling a native path, blob
handle, or ambient URL loader into the frame.

Theme data is presentation-only. It cannot select relays, sign, navigate,
change grants, choose another napplet, inject executable CSS/HTML, or address a
native bridge.

## Developer-generated products

The shell generator should compile the selected product theme into:

- native color, typography, spacing, motion, and accessibility resources;
- the minimal NAP-THEME projection;
- optional versioned product-token adapters;
- framework packages for curated napplets;
- host presentation slots and layout-mode definitions;
- neutral loading, refusal, offline, permission, and crash states;
- theme documentation and a component gallery;
- visual-conformance fixtures and reference captures.

Generated source should be ordinary and editable. The composition lock records
the theme-profile revision independently from napplet exact builds, so a brand
update does not masquerade as component replacement.

## User-selected replacements

For each candidate, evaluate four independent dimensions:

| Dimension | Question |
| --- | --- |
| Protocol | Does it implement the role's archetype/actions/payloads? |
| Authority | Are its capabilities acceptable for this exact build? |
| Visual | Does it consume the product theme and fit the slot? |
| Accessibility | Does it remain usable across required modes? |

An archetype tag proves none of the last three. Preview a replacement inside
the actual shell with real theme variants before activation. A visually
nonconforming but safe component may be allowed with an explicit warning; it
must not silently degrade the curated product.

Possible compatibility labels:

```text
curated
theme-conformant
theme-minimum-only
visually unverified
incompatible with this slot
```

These are product evidence labels, not NIP-5D or NAP statuses.

## Visual conformance matrix

Test the complete product, not isolated screenshots:

```text
default and third-party components
x light and dark variants
x normal and increased contrast
x normal and large text
x motion and reduced motion
x transparency and reduced transparency
x compact and expanded slots
x loading, empty, denied, error, and populated states
```

Verify:

- contrast and focus visibility;
- type scale and baseline rhythm;
- spacing, control height, radius, and border consistency;
- no nested global chrome or duplicated titles;
- atomic theme changes without flashes or mixed revisions;
- keyboard, screen-reader, and focus-order behavior;
- bounded font/background failure and safe fallbacks;
- replacement preview, activation, rollback, and state preservation.

Pixel identity is not the goal. The falsifier is whether a normal user feels
they crossed into a different application when a product role changes.
