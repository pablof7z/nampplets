import SwiftUI

struct ActivityFactRow: View {
    let fact: ActivityFact
    let detailFields: [ActivityDetailField]

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: symbol)
                .foregroundStyle(color)
                .frame(width: 22)
                .accessibilityHidden(true)

            VStack(alignment: .leading, spacing: 5) {
                HStack {
                    Text(fact.title)
                        .font(.headline)
                    Spacer()
                    Text(fact.kind.title)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Text(fact.summary)
                    .foregroundStyle(.secondary)

                if let evidence = fact.evidenceSummary {
                    LabeledContent("Evidence", value: evidence)
                        .font(.caption)
                        .textSelection(.enabled)
                }

                if !detailFields.isEmpty {
                    DisclosureGroup("Developer detail") {
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(detailFields) { field in
                                LabeledContent(
                                    field.key,
                                    value: field.displayValue
                                )
                                    .font(.caption.monospaced())
                                    .textSelection(.enabled)
                            }
                        }
                        .padding(.top, 4)
                    }
                    .font(.caption)
                }
            }
        }
        .padding(.vertical, 5)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(
            "\(fact.severity.title), \(fact.kind.title), "
                + "\(fact.title). \(fact.summary)"
        )
    }

    private var symbol: String {
        switch fact.kind {
        case .providerCall: "arrow.left.arrow.right"
        case .providerRefusal: "hand.raised"
        case .activeSession: "rectangle.connected.to.line.below"
        case .activeBinding: "link"
        case .activeResource: "shippingbox"
        case .pendingReceipt: "clock.badge.exclamationmark"
        case .crash: "bolt.trianglebadge.exclamationmark"
        case .recovery: "cross.case"
        }
    }

    private var color: Color {
        switch fact.severity {
        case .debug: .secondary
        case .information: .blue
        case .warning: .orange
        case .error: .red
        }
    }
}
