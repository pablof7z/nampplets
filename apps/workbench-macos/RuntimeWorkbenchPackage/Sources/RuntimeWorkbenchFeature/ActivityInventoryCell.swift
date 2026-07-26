import SwiftUI

struct ActivityInventoryPresentation: Equatable, Sendable {
    let openNow: Int
    let unavailableCountsMessage: String

    init(inventory: ActivityInventorySummary) {
        openNow = inventory.activeSessions
        unavailableCountsMessage =
            "Other activity counts aren't available in this version."
    }
}

enum ActivityPlainPresentation {
    static let header =
        "Recent activity available to this version of the app."
    static let updateGap =
        "Refresh to load the latest available entries."
    /// Says "across all napplets" on purpose: the runtime's rings are not
    /// partitioned by napplet, so the count cannot be attributed to this one.
    static let runtimeDiscarded =
        "The runtime keeps only its most recent entries across all napplets. "
            + "These are gone and refreshing will not bring them back."
}

struct ActivityInventoryCell: View {
    let title: String
    let value: Int
    let symbol: String

    var body: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.hairline + 2) {
            Label(title, systemImage: symbol)
                .font(NappletType.caption)
                .foregroundStyle(NappletInk.inkSecondary)
            Text(value, format: .number)
                .font(NappletType.title.monospacedDigit())
                .foregroundStyle(NappletInk.ink)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(NappletMetrics.snug)
        .background(
            NappletInk.fillQuiet,
            in: RoundedRectangle(
                cornerRadius: NappletMetrics.tight,
                style: .continuous
            )
        )
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(title): \(value)")
    }
}
