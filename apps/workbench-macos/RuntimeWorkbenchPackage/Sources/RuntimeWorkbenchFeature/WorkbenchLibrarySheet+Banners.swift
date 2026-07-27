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
    /// Only the most recent refusal is shown. When it is not the only one, the
    /// evidence has to say so — otherwise a single banner reads as a single
    /// refusal, and the ones the runtime evicted leave no trace at all.
    func refusalBanner(
        _ refusal: WorkbenchLibraryRefusal,
        retainedCount: Int,
        droppedCount: UInt64
    ) -> some View {
        LibraryStatusBanner(
            title: "That didn't work",
            message: WorkbenchLibraryPlainPresentation.refusalMessage,
            symbol: "hand.raised",
            color: NappletInk.refusal,
            accessibilityIdentifier: "workbench-library-refusal",
            evidenceFields: Self.refusalEvidenceFields(
                refusal,
                retainedCount: retainedCount,
                droppedCount: droppedCount
            )
        )
    }

    /// Extracted so the decision is testable. Rendering it inline would leave
    /// the one rule that matters — when the counts are stated at all — as view
    /// logic no test can reach.
    static func refusalEvidenceFields(
        _ refusal: WorkbenchLibraryRefusal,
        retainedCount: Int,
        droppedCount: UInt64
    ) -> [NappletField] {
        let recorded = UInt64(retainedCount) &+ droppedCount
        return [
            NappletField("Code", refusal.code),
            NappletField("Detail", refusal.message),
            NappletField("Occurred at milliseconds", "\(refusal.occurredAtMillis)"),
            // Silent when this is the only refusal ever recorded: saying
            // "1 recorded, showing the most recent" would be noise.
            recorded > 1
                ? NappletField(
                    "Refusals recorded",
                    "\(recorded), showing the most recent"
                )
                : nil,
            droppedCount > 0
                ? NappletField("Older refusals discarded", "\(droppedCount)")
                : nil,
        ].compactMap { $0 }
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
