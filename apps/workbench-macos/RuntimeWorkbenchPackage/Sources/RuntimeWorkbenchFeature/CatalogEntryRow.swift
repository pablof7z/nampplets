import SwiftUI

/// One napplet in the browse list.
///
/// A person scanning this list is deciding what to look at, not auditing a
/// build. The address, the compatibility vocabulary and the publisher's key
/// belong to the review sheet's evidence, not here.
/// See `docs/adr/0008-verdicts-on-the-path.md`.
struct CatalogEntryRow: View {
    let entry: CatalogEntry

    var body: some View {
        HStack(alignment: .top, spacing: NappletMetrics.snug) {
            VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                Text(entry.title)
                    .font(.headline)

                Text(entry.summary)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)

                Text(publisherLine)
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }

            Spacer(minLength: NappletMetrics.snug)

            if let problem {
                Text(problem)
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: 140, alignment: .trailing)
                    .multilineTextAlignment(.trailing)
            }

            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
                .accessibilityHidden(true)
        }
        .padding(.vertical, NappletMetrics.tight)
        .contentShape(Rectangle())
    }

    private var publisherLine: String {
        let name = NappletIdentityPresentation.publisherName(
            displayName: entry.publisher.displayName,
            publicKey: entry.publisher.publicKey
        )
        return NappletIdentityPresentation.publisherIsUnnamed(
            displayName: entry.publisher.displayName,
            publicKey: entry.publisher.publicKey
        )
            ? name
            : "by \(name)"
    }

    /// Silence when there is nothing wrong. A row that flags every napplet as
    /// "Compatible" or "Review required" has told the reader nothing; a row
    /// that speaks only when something is off is worth reading.
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
