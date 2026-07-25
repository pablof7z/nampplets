import SwiftUI

extension WorkbenchLibrarySheet {
    func unavailableBanner(_ reason: String) -> some View {
        LibraryStatusBanner(
            title: "Installed library unavailable",
            message: reason,
            symbol: "externaldrive.badge.xmark",
            color: .orange,
            accessibilityIdentifier: "workbench-library-unavailable"
        )
    }

    func refusalBanner(_ refusal: WorkbenchLibraryRefusal) -> some View {
        LibraryStatusBanner(
            title: "Runtime refused an action",
            message: "\(refusal.code): \(refusal.message)",
            symbol: "hand.raised",
            color: .red,
            accessibilityIdentifier: "workbench-library-refusal"
        )
    }

    func updateGapBanner(
        _ gap: WorkbenchLibraryUpdateGap
    ) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "exclamationmark.arrow.triangle.2.circlepath")
                .foregroundStyle(.orange)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 3) {
                Text("Library update may be incomplete")
                    .font(.headline)
                Text(
                    "Expected revision \(gap.expectedPredecessorRevision), "
                        + "received \(gap.receivedPredecessorRevision)."
                )
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            Spacer()
            Button("Refresh") {
                model.refresh()
            }
        }
        .padding()
        .background(.orange.opacity(0.08))
        .accessibilityElement(children: .combine)
        .accessibilityLabel(
            "Library update may be incomplete. Expected predecessor revision "
                + "\(gap.expectedPredecessorRevision), received "
                + "\(gap.receivedPredecessorRevision)."
        )
        .accessibilityHint(
            "Activate Refresh to request an authoritative snapshot"
        )
        .accessibilityIdentifier("workbench-library-update-gap")
    }
}
