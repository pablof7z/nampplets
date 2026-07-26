import SwiftUI

/// Shared spacing used by the consumer-facing surfaces, so that restraint is a
/// default rather than a thing each view remembers.
///
/// **These are native fallback tokens, not theme authority.** They are the
/// Workbench's built-in defaults for host chrome, and nothing here is the
/// product-wide source of truth for spacing or appearance. When a
/// Rust-resolved semantic theme revision exists, precedence, validation,
/// revision and active composition remain Rust's; this layer's job is to map
/// semantic tokens onto concrete SwiftUI values and to supply these defaults
/// when no revision is resolved. Treating the fixed numbers below as canonical
/// product state is the mistake to avoid.
public enum NappletMetrics {
    public static let hairline = 4.0
    public static let tight = 8.0
    public static let snug = 12.0
    public static let comfortable = 16.0
    public static let roomy = 24.0
    public static let generous = 32.0

    public static let cardCorner = 10.0
}

/// A heading whose weight comes from type and space rather than from colour.
struct NappletHeading: View {
    let title: String
    var subtitle: String?

    var body: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.tight) {
            Text(title)
                .font(.title2.weight(.semibold))
                .fixedSize(horizontal: false, vertical: true)
            if let subtitle {
                Text(subtitle)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
    }
}

/// The one way this app interrupts a person.
///
/// It renders nothing at all for `.settled`, which is what makes its presence
/// meaningful: a notice on screen always means something needs reading.
struct NappletNotice: View {
    let verdict: NappletTrustVerdict

    var body: some View {
        if let message = verdict.message {
            HStack(alignment: .firstTextBaseline, spacing: NappletMetrics.snug) {
                Image(systemName: verdict.symbol)
                    .foregroundStyle(tint)
                    .accessibilityHidden(true)
                Text(message)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .font(NappletType.secondary)
            .padding(NappletMetrics.snug)
            .background(
                NappletInk.ground(for: verdict),
                in: RoundedRectangle(
                    cornerRadius: NappletMetrics.cardCorner,
                    style: .continuous
                )
            )
            .accessibilityElement(children: .combine)
        }
    }

    /// Colour reinforces the words; it never carries them. Every notice reads
    /// correctly with colour removed, which is why `caution` and `blocked`
    /// differ in glyph and wording and not only in hue.
    private var tint: Color {
        NappletInk.tint(for: verdict)
    }
}

/// The deliberate move from a verdict to the evidence behind it.
///
/// Collapsed by default, and raises the disclosure tier for everything inside
/// it -- so a view places its evidence here and does not otherwise think about
/// tiers. Nothing inside is truncated or prettified: this is the tier that
/// lets the plain one be confident.
struct NappletEvidence<Content: View>: View {
    var label: String = "Technical details"
    @ViewBuilder let content: Content

    @State private var isExpanded = false

    var body: some View {
        DisclosureGroup(isExpanded: $isExpanded) {
            content
                .nappletDisclosure(.technical)
                .padding(.top, NappletMetrics.snug)
                .textSelection(.enabled)
        } label: {
            Text(label)
                .font(.callout)
        }
        .accessibilityIdentifier("napplet-evidence")
        .accessibilityHint(
            "Shows the exact values the runtime verified, for people who want them"
        )
    }
}

/// One row of evidence.
///
/// A named type rather than a tuple: `[(String, String)]` and
/// `[(label: String, value: String)]` are different array types in Swift, and
/// building these lists conditionally makes that difference show up as a
/// confusing error at every call site.
struct NappletField: Identifiable, Equatable, Hashable, Sendable {
    let label: String
    let value: String

    var id: String { label }

    init(_ label: String, _ value: String) {
        self.label = label
        self.value = value
    }
}

/// Label/value rows for the technical tier. Monospaced and selectable, because
/// the only reason to show these is so that someone can compare or copy them.
///
/// This renders **nothing** outside `.technical`, which makes the disclosure
/// tier load-bearing rather than decorative. Every value that reaches this
/// view is by definition the kind of thing ADR 0008 keeps off the plain path
/// -- a hash, a key, a coordinate, a revision -- so a grid that finds itself
/// in a `.plain` subtree has been misplaced, and failing closed is the only
/// safe reading. Placing it inside `NappletEvidence` raises the tier
/// automatically; that is the intended way to use it.
struct NappletFieldGrid: View {
    let fields: [NappletField]

    @Environment(\.nappletDisclosure) private var disclosure

    init(fields: [NappletField]) {
        self.fields = fields
    }

    var body: some View {
        if disclosure.isTechnical {
            // Two columns where they fit, stacked where they don't. A
            // sixty-four character hash in the value column of a 320pt
            // inspector either overflows or wraps into an unreadable ladder,
            // and evidence that cannot be read does not let the plain tier be
            // confident. `ViewThatFits` decides from the real available width
            // rather than from a platform guess, so the same grid is correct
            // in a 680pt sheet, a narrow inspector section, and on iPhone.
            ViewThatFits(in: .horizontal) {
                grid
                stacked
            }
        }
    }

    private var grid: some View {
        Grid(
            alignment: .leadingFirstTextBaseline,
            horizontalSpacing: NappletMetrics.comfortable,
            verticalSpacing: NappletMetrics.hairline + 2
        ) {
            ForEach(fields) { field in
                GridRow {
                    Text(field.label)
                        .foregroundStyle(NappletInk.inkSecondary)
                        .gridColumnAlignment(.leading)
                    value(field)
                }
            }
        }
        .font(NappletType.record)
    }

    private var stacked: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.tight) {
            ForEach(fields) { field in
                VStack(alignment: .leading, spacing: 1) {
                    Text(field.label)
                        .foregroundStyle(NappletInk.inkSecondary)
                    value(field)
                }
            }
        }
        .font(NappletType.record)
    }

    private func value(_ field: NappletField) -> some View {
        Text(field.value)
            .fontDesign(.monospaced)
            .textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// A quiet container used where several related facts belong together.
struct NappletCard<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        content
            .padding(NappletMetrics.comfortable)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                .quaternary.opacity(0.4),
                in: RoundedRectangle(cornerRadius: NappletMetrics.cardCorner)
            )
    }
}
