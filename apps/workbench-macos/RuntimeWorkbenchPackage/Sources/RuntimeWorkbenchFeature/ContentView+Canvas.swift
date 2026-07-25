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
            VStack(spacing: 12) {
                ProgressView()
                Text("Launching verified napplet")
                    .font(.headline)
                Text("The exact build is opening inside its isolated runtime.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .accessibilityIdentifier("napplet-launching")
        } else if
            let identity = window.exactBuild,
            installedArtifacts[identity] != nil
        {
            ContentUnavailableView {
                Label("Permission review required", systemImage: "lock.shield")
            } description: {
                Text(
                    "This exact verified build is installed. Review its required "
                        + "capabilities before it runs."
                )
            } actions: {
                Button("Review Permissions") {
                    permissionTargetIdentity = identity
                    openPermissionReview()
                }
                .accessibilityIdentifier("review-installed-permissions")
            }
            .accessibilityIdentifier("installed-napplet-awaiting-permission")
        } else if let identity = window.exactBuild {
            ContentUnavailableView {
                Label("Napplet is not running", systemImage: "app.badge")
            } description: {
                Text(
                    "Reopen this installed exact build without changing its grants."
                )
            } actions: {
                Button("Open Installed Napplet") {
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
                "Napplet is not running",
                systemImage: "app.badge",
                description: Text("This canvas window has no exact installed build.")
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
                activity = "Loading trusted shell"
            case .mounted:
                activity = "Signed \(title) napplet mounted"
            case .request(let type):
                activity = "Mapped \(type) from napplet window"
            case .refused(let reason):
                activity = "Refused: \(reason)"
            case .crashed:
                activity = "\(title) WebView crashed"
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
        activity = "Closed \(window.title) without uninstalling it"
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
