import SwiftUI

struct CatalogEntryRow: View {
    let entry: CatalogEntry

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: compatibilitySymbol)
                .foregroundStyle(compatibilityColor)
                .frame(width: 24)

            VStack(alignment: .leading, spacing: 4) {
                Text(entry.title)
                    .font(.headline)
                Text(entry.summary)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                LabeledContent("Publisher", value: entry.publisher.visibleName)
                    .font(.caption)
                Text(entry.coordinate)
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }

            Spacer()

            Text(entry.compatibility.title)
                .font(.caption)
                .foregroundStyle(compatibilityColor)
        }
        .padding(.vertical, 6)
    }

    private var compatibilitySymbol: String {
        switch entry.compatibility {
        case .unreviewed:
            "doc.text.magnifyingglass"
        case .compatible:
            "checkmark.seal"
        case .incompatible:
            "xmark.octagon"
        case .unknown:
            "questionmark.diamond"
        }
    }

    private var compatibilityColor: Color {
        switch entry.compatibility {
        case .unreviewed:
            .secondary
        case .compatible:
            .green
        case .incompatible:
            .red
        case .unknown:
            .orange
        }
    }
}

struct CatalogIssueView: View {
    let issue: CatalogIssue

    var body: some View {
        Label {
            VStack(alignment: .leading, spacing: 2) {
                Text(issue.title)
                    .font(.headline)
                Text(issue.message)
            }
        } icon: {
            Image(systemName: "exclamationmark.triangle")
        }
        .foregroundStyle(.orange)
        .accessibilityElement(children: .combine)
    }
}
