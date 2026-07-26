import SwiftUI

extension WorkbenchLibrarySheet {
    func unavailableBanner(_ reason: String) -> some View {
        LibraryStatusBanner(
            title: "Can't reach your napplets right now",
            message: reason,
            symbol: "externaldrive.badge.xmark",
            color: .orange,
            accessibilityIdentifier: "workbench-library-unavailable"
        )
    }

    /// The refusal code is the runtime's own identifier and means nothing to
    /// the person reading it. The message is the part written for them, so it
    /// is the part shown; the code stays available in activity.
    func refusalBanner(_ refusal: WorkbenchLibraryRefusal) -> some View {
        LibraryStatusBanner(
            title: "That didn't work",
            message: refusal.message,
            symbol: "hand.raised",
            color: .red,
            accessibilityIdentifier: "workbench-library-refusal"
        )
    }

    func updateGapBanner(
        _ gap: WorkbenchLibraryUpdateGap
    ) -> some View {
        HStack(alignment: .top, spacing: NappletMetrics.snug) {
            Image(systemName: "exclamationmark.arrow.triangle.2.circlepath")
                .foregroundStyle(.orange)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                Text("This list might be out of date")
                    .font(.headline)
                Text("Refresh to make sure you're seeing everything.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                NappletEvidence(label: "Why") {
                    NappletFieldGrid(fields: [
                        NappletField(
                            "Expected predecessor revision",
                            "\(gap.expectedPredecessorRevision)"
                        ),
                        NappletField(
                            "Received predecessor revision",
                            "\(gap.receivedPredecessorRevision)"
                        ),
                        NappletField(
                            "Received revision",
                            "\(gap.receivedRevision)"
                        ),
                    ])
                }
                .font(.caption)
            }
            Spacer()
            Button("Refresh") {
                model.refresh()
            }
        }
        .padding(NappletMetrics.comfortable)
        .background(.orange.opacity(0.08))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(
            "This list might be out of date. Refresh to make sure you're "
                + "seeing everything."
        )
        .accessibilityIdentifier("workbench-library-update-gap")
    }
}
