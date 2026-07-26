import SwiftUI

extension WorkbenchLibrarySheet {
    func unavailableBanner(_ reason: String) -> some View {
        LibraryStatusBanner(
            title: "Can't reach your napplets right now",
            message: WorkbenchLibraryPlainPresentation.unavailableMessage,
            symbol: "externaldrive.badge.xmark",
            color: NappletInk.caution,
            accessibilityIdentifier: "workbench-library-unavailable",
            evidenceFields: [NappletField("Reason", reason)]
        )
    }

    /// The refusal code is the runtime's own identifier and means nothing to
    /// the person reading it. The message is the part written for them, so it
    /// is the part shown; the code stays available in activity.
    func refusalBanner(_ refusal: WorkbenchLibraryRefusal) -> some View {
        LibraryStatusBanner(
            title: "That didn't work",
            message: WorkbenchLibraryPlainPresentation.refusalMessage,
            symbol: "hand.raised",
            color: NappletInk.refusal,
            accessibilityIdentifier: "workbench-library-refusal",
            evidenceFields: [
                NappletField("Code", refusal.code),
                NappletField("Detail", refusal.message),
                NappletField("Occurred at milliseconds", "\(refusal.occurredAtMillis)"),
            ]
        )
    }

    func updateGapBanner(
        _ gap: WorkbenchLibraryUpdateGap
    ) -> some View {
        HStack(alignment: .top, spacing: NappletMetrics.snug) {
            Image(systemName: "exclamationmark.arrow.triangle.2.circlepath")
                .foregroundStyle(NappletInk.inkSecondary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                Text("This list might be out of date")
                    .font(.headline)
                    .accessibilityIdentifier("workbench-library-update-gap")
                Text("Refresh to get the latest available list.")
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
                    ]
                    // Only when the runtime reported a loss. Printing "0" for
                    // a plain revision discontinuity would assert something it
                    // never said.
                    + (gap.lostBeforeBatch > 0
                        ? [
                            NappletField(
                                "Events lost before this batch",
                                "\(gap.lostBeforeBatch)"
                            ),
                        ]
                        : []))
                }
                .font(.caption)
            }
            Spacer()
            Button("Refresh") {
                model.refresh()
            }
        }
        .padding(NappletMetrics.comfortable)
        .background(NappletInk.fillQuiet)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(
            "This list might be out of date. Refresh to get the latest "
                + "available list."
        )
    }
}
