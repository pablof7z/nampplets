import NMPNativeRuntimeApple
import SwiftUI

extension ContentView {
    @ViewBuilder
    var platformBody: some View {
        #if os(iOS)
        if layout.mode == .fullWindow {
            WorkbenchFullWindowView(
                layout: $layout,
                rootID: fullWindowRootID,
                path: $fullWindowPath,
                onExit: exitFullWindow,
                windowContent: windowContent,
                topBars: { topStatusBars }
            )
        } else {
            NavigationStack {
                canvasBody
                    .navigationTitle("Napplets")
                    .navigationBarTitleDisplayMode(.inline)
                    .toolbar {
                        ToolbarItem(placement: .topBarLeading) {
                            accountMenu
                        }
                        ToolbarItemGroup(placement: .topBarTrailing) {
                            Button {
                                isCatalogSheetPresented = true
                            } label: {
                                Label("Add Napplet", systemImage: "plus")
                            }
                            .accessibilityIdentifier("add-napplet")
                            .accessibilityHint(
                                "Opens the network napplet catalog"
                            )

                            settingsToolbarButton

                            Button {
                                withAnimation(.easeInOut(duration: 0.18)) {
                                    isInspectorPresented.toggle()
                                }
                            } label: {
                                Label(
                                    isInspectorPresented ? "Hide Inspector" : "Show Inspector",
                                    systemImage: "sidebar.right"
                                )
                            }
                            .accessibilityIdentifier("toggle-napplet-inspector")

                            layoutMenu
                        }
                    }
            }
        }
        #else
        canvasBody
            .toolbar {
                macOSToolbar
            }
        #endif
    }

    @ViewBuilder
    var topStatusBars: some View {
        // An observation setup failure must be visible on its own: an empty
        // `writes`/`receipts` list alone is indistinguishable from "nothing
        // is pending" and would otherwise hide a napplet stuck waiting on an
        // approval or receipt update that can never arrive.
        if let reason = pendingWrites.observationFailureReason {
            ObservationFailureBar(title: "Pending writes unavailable", detail: reason)
        }
        if let reason = receipts.observationFailureReason {
            ObservationFailureBar(title: "Receipts unavailable", detail: reason)
        }
        if let pendingWrite = pendingWrites.writes.first {
            PendingWriteApprovalBar(write: pendingWrite) { approve in
                pendingWrites.decide(
                    pendingWrite,
                    approve: approve,
                    profile: profile
                )
            }
        }
        if let receipt = receipts.receipts.last {
            ReceiptStatusBar(receipt: receipt)
        }
    }
}

/// Compact top-of-canvas notice for an observation that could not be
/// established, so its silence never reads as "nothing pending".
struct ObservationFailureBar: View {
    let title: String
    let detail: String

    var body: some View {
        HStack(alignment: .top, spacing: NappletMetrics.snug) {
            Image(systemName: "exclamationmark.triangle")
                .foregroundStyle(NappletInk.caution)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                Text(title)
                    .font(NappletType.heading)
                Text(detail)
                    .font(NappletType.caption)
                    .foregroundStyle(NappletInk.inkSecondary)
            }
            Spacer(minLength: 0)
        }
        .padding(NappletMetrics.comfortable)
        .background(NappletInk.ground(for: .caution("")))
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityIdentifier("observation-failure-bar")
    }
}
