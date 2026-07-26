import SwiftUI

/// The three voices.
///
/// Display names the thing (New York, serif). Prose is the app talking (SF).
/// Record is the machine's own account (SF Mono), and it belongs to the
/// technical tier exclusively. You can tell which disclosure tier you are in
/// from the typeface, without reading a word and without a pixel of colour --
/// which is what makes the tier boundary hold without a badge.
///
/// Every token maps to a semantic text style, so Dynamic Type works
/// everywhere. Nothing here uses `Font.system(size:)` without `relativeTo:`.
///
/// New York is deliberately unavailable below `.title2`: optical sizing makes
/// it lovely at display sizes and mushy at 13pt. `.rounded` is absent on
/// purpose -- it is the friendly-consumer-app tell, and friendliness applied
/// as a typeface to a product about signed provenance reads as a costume.
/// Three weights exist; `.bold` and heavier are absent, because on a page
/// whose hierarchy comes from size and space, bold is what you reach for once
/// the hierarchy has already failed.
public enum NappletType {
    /// A napplet's name on its own page.
    public static let display = Font.largeTitle
        .weight(.semibold)
        .width(.standard)

    /// The name of a place: Napplets, Saved, Yours.
    public static let place = Font.title.weight(.semibold)

    /// Card and sheet titles.
    public static let title = Font.title2.weight(.semibold)

    /// The one-line description under a name.
    public static let lede = Font.title3

    /// Section headings, and a person's name on a review.
    public static let heading = Font.headline

    public static let body = Font.body

    /// Supporting sentences and capability lines.
    public static let secondary = Font.callout

    /// Publisher lines, footers, counts.
    public static let caption = Font.caption

    /// The technical tier, and nowhere else.
    ///
    /// `.footnote` rather than `.caption`: sixty-four hexadecimal characters at
    /// caption size are unreadable, and illegible evidence does not let the
    /// plain tier be confident, which is the whole reason the tier exists.
    public static let record = Font.footnote.monospaced()
}

public extension View {
    /// Serif, for things a person named. Applied only at `title2` and above.
    func nappletDisplayFace() -> some View {
        fontDesign(.serif)
    }
}

public extension NappletMetrics {
    /// Prose measure. Roughly 62-68 characters at the default size; beyond
    /// that a line is hard to track back to the start of the next one.
    static let measure = 680.0
    static let spacious = 48.0
    static let page = 64.0
}
