import SwiftUI

/// One napplet on the Discover page.
///
/// A card marks content the app did not write, and a napplet's name and words
/// are the napplet's. The title is set at artwork scale and given artwork's
/// position, because the name is the only honest recognition handle this
/// product has: there is no icon, no screenshot, no rating and no count.
///
/// Generated identity marks -- identicons, gradient-from-hash, a monogram
/// square -- are deliberately absent. A hash-derived mark is a visual
/// fingerprint a person cannot verify but will learn to trust, and humans
/// compare shapes approximately, so near-collisions look alike. That is a
/// verdict the app would assert without being able to stand behind it: the
/// picture-shaped twin of a five-star average.
///
/// See `docs/design/napplet-browser-visual.md` §7.1.
struct CatalogEntryRow: View {
    let entry: CatalogEntry
    var isPressed = false

    var body: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.tight) {
            HStack(alignment: .firstTextBaseline, spacing: NappletMetrics.snug) {
                Text(entry.title)
                    .font(NappletType.title)
                    .foregroundStyle(NappletInk.ink)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)

                Spacer(minLength: NappletMetrics.snug)

                // Stated in words, in ordinary ink. A row that flagged every
                // napplet as "Compatible" told the reader nothing; a row that
                // speaks only when something is off is worth reading, and it
                // does not need a colour to be believed.
                if let problem {
                    Text(problem)
                        .font(NappletType.caption)
                        .foregroundStyle(NappletInk.inkSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .multilineTextAlignment(.trailing)
                }
            }

            if !entry.summary.isEmpty {
                Text(entry.summary)
                    .font(NappletType.secondary)
                    .foregroundStyle(NappletInk.inkSecondary)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
            }

            if let publisherLine {
                Text(publisherLine)
                    .font(NappletType.caption)
                    .foregroundStyle(NappletInk.inkSecondary)
                    .lineLimit(1)
                    .padding(.top, NappletMetrics.hairline)
            }
        }
        // Fills its grid row so cards in a row share a height. Without this a
        // napplet with no description sits in a visibly shorter box beside one
        // that has one, and the grid reads as broken rather than as sparse.
        .frame(
            maxWidth: .infinity,
            maxHeight: .infinity,
            alignment: .topLeading
        )
        .padding(NappletMetrics.comfortable)
        .background(
            isPressed ? NappletInk.fillSelected : NappletInk.fillQuiet,
            in: RoundedRectangle(
                cornerRadius: NappletMetrics.cardCorner,
                style: .continuous
            )
        )
        .contentShape(Rectangle())
    }

    /// Absence renders as absence.
    ///
    /// Naming every unnamed publisher put the identical non-fact on every row,
    /// which is the same mistake as a seal on every screen. The install review
    /// still states it, because there it bears on a decision.
    private var publisherLine: String? {
        guard
            !NappletIdentityPresentation.publisherIsUnnamed(
                displayName: entry.publisher.displayName,
                publicKey: entry.publisher.publicKey
            )
        else {
            return nil
        }
        return "by " + NappletIdentityPresentation.publisherName(
            displayName: entry.publisher.displayName,
            publicKey: entry.publisher.publicKey
        )
    }

    private var problem: String? {
        switch entry.compatibility {
        case .compatible, .unreviewed:
            nil
        case .incompatible:
            "Won't run here"
        case .unknown:
            "Might not run here"
        }
    }
}
