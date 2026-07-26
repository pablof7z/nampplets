import NMPNativeRuntimeApple
import SwiftUI

extension ContentView {
    var canvasBody: some View {
        VStack(spacing: 0) {
            topStatusBars
            HStack(spacing: 0) {
                WorkbenchWorkspaceView(
                    layout: $layout,
                    onLayoutChange: scheduleLayoutSave,
                    onClose: closeWindow,
                    onAddNapplet: { isCatalogSheetPresented = true },
                    windowContent: windowContent
                )
                if isInspectorPresented {
                    Divider()
                    nappletInspector
                        .transition(.move(edge: .trailing).combined(with: .opacity))
                }
            }
            #if os(macOS)
            activityBar
            #endif
        }
        .background(.background)
    }

    @ViewBuilder
    func windowContent(_ window: WorkbenchCanvasWindow) -> some View {
        if
            let identity = window.exactBuild,
            let artifact = runningArtifacts[identity]
        {
            nappletSurface(artifact, title: window.title)
        } else if
            let identity = window.exactBuild,
            launchingIdentities.contains(identity)
                || reacquiringIdentities.contains(identity)
        {
            VStack(spacing: NappletMetrics.snug) {
                ProgressView()
                Text("Opening \(window.title)…")
                    .font(.headline)
            }
            .accessibilityIdentifier("napplet-launching")
        } else if
            let identity = window.exactBuild,
            installedArtifacts[identity] != nil
        {
            ContentUnavailableView {
                Label("Ready when you are", systemImage: "hand.wave")
            } description: {
                Text(
                    "\(window.title) needs your permission before it can run."
                )
            } actions: {
                Button("Continue") {
                    permissionTargetIdentity = identity
                    openPermissionReview()
                }
                .accessibilityIdentifier("review-installed-permissions")
            }
            // No identifier on the container. SwiftUI propagates an
            // `accessibilityIdentifier` down its subtree, so one here
            // overwrote the button's and made `review-installed-permissions`
            // unfindable -- which is why this used to be queried by its
            // visible label. Nothing queries the container, so the button
            // keeps the identifier and the label stays free to change.
        } else if let identity = window.exactBuild {
            ContentUnavailableView {
                Label("Not open", systemImage: "app.dashed")
            } description: {
                Text("Opening it again won't change what it's allowed to do.")
            } actions: {
                Button("Open") {
                    Task {
                        await reacquireInstalledArtifact(
                            identity,
                            presentation: .immediate
                        )
                    }
                }
                .accessibilityIdentifier("reopen-installed-napplet")
            }
        } else {
            ContentUnavailableView(
                "Nothing here",
                systemImage: "app.dashed",
                description: Text("This window has no napplet in it.")
            )
        }
    }

    @ViewBuilder
    private func nappletSurface(
        _ artifact: NappletArtifact,
        title: String
    ) -> some View {
        TrustedNappletView(artifact: artifact) { event in
            switch event {
            case .loading:
                activity = "Opening \(title)…"
            case .mounted:
                activity = "\(title) is running"
            case .request:
                // A napplet asking the runtime for something is the system
                // working, not news. Narrating every request turned this bar
                // into a log; it now speaks only when something changed for
                // the person watching.
                break
            case .refused(let reason):
                activity = "Refused: \(reason)"
            case .crashed:
                activity = "\(title) stopped unexpectedly"
            }
        }
        .accessibilityIdentifier("bundled-napplet")
    }

    func mutateLayout(
        _ mutation: (inout WorkbenchLayoutModel) -> Void
    ) {
        var next = layout
        mutation(&next)
        guard next != layout else {
            return
        }
        layout = next
        scheduleLayoutSave()
    }

    @MainActor
    private func closeWindow(_ window: WorkbenchCanvasWindow) {
        mutateLayout {
            $0.removeWindow(id: window.id)
        }
        if let identity = window.exactBuild {
            runningArtifacts.removeValue(forKey: identity)
            installedArtifacts.removeValue(forKey: identity)
            reacquiringIdentities.remove(identity)
            launchingIdentities.remove(identity)
            if permissionTargetIdentity == identity {
                permissionTargetIdentity = nil
            }
            if deferredPermissionIdentity == identity {
                deferredPermissionIdentity = nil
            }
        }
        activity = "Closed \(window.title). It's still installed."
    }

    func scheduleLayoutSave() {
        guard pendingLayoutSave == nil else {
            return
        }
        let pending = DispatchWorkItem {
            guard !(pendingLayoutSave?.isCancelled ?? true) else {
                pendingLayoutSave = nil
                return
            }
            pendingLayoutSave = nil
            do {
                try layoutStore.saveLayout(
                    layout.snapshot,
                    workspaceID: Self.workspaceID,
                    retainedReceiptIDs: receipts.receiptIDs
                )
                layoutPersistenceError = nil
            } catch {
                layoutPersistenceError =
                    "Layout was not saved: \(error.localizedDescription)"
            }
        }
        pendingLayoutSave = pending
        DispatchQueue.main.async(execute: pending)
    }

    func persistLayoutImmediately() {
        do {
            try layoutStore.saveLayout(
                layout.snapshot,
                workspaceID: Self.workspaceID,
                retainedReceiptIDs: receipts.receiptIDs
            )
            layoutPersistenceError = nil
        } catch {
            layoutPersistenceError =
                "Layout was not saved: \(error.localizedDescription)"
        }
    }
}
