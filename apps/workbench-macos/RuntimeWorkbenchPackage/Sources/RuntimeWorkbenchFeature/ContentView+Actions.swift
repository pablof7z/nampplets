import NMPNativeRuntimeApple
import SwiftUI

extension ContentView {
    @MainActor
    func openActivityDrawer() {
        activitySheetError = nil
        guard let profile else {
            activitySheetError =
                bootstrapError ?? "The application runtime profile is unavailable."
            isActivitySheetPresented = true
            return
        }
        guard let scope = selectedActivityScope else {
            activitySheetError =
                "Select an exact-build napplet window to view its activity."
            isActivitySheetPresented = true
            return
        }
        activitySource = nil
        do {
            activitySource = try RuntimeWorkbenchActivitySource(
                profile: profile,
                scope: scope
            )
        } catch {
            activitySheetError = error.localizedDescription
        }
        isActivitySheetPresented = true
    }

    @MainActor
    func handleCatalogInstallation(
        _ build: CatalogInstalledBuild
    ) {
        guard let profile else {
            activity = "Refused: the application runtime profile is unavailable"
            return
        }
        let identity = WorkbenchExactBuildIdentity(
            manifestAuthor: build.manifestAuthor,
            dTag: build.dTag,
            aggregateHash: build.exactAggregateHash
        )
        guard
            let installed = profile.installedCatalogArtifact(for: identity)
        else {
            activity =
                "Refused: the verified artifact handle is unavailable for this profile"
            return
        }
        do {
            try prepareInstalledArtifact(
                installed,
                identity: identity,
                presentation: .afterCatalogDismiss
            )
        } catch {
            activity = "Refused: \(error.localizedDescription)"
        }
    }

    @MainActor
    func openPermissionReview() {
        permissionSheetError = nil
        permissionManager = nil
        if let injectedPermissionManager {
            permissionManager = injectedPermissionManager
            isPermissionSheetPresented = true
            return
        }
        guard let profile else {
            permissionSheetError =
                bootstrapError ?? "The application runtime profile is unavailable."
            isPermissionSheetPresented = true
            return
        }
        guard
            let identity =
                permissionTargetIdentity ?? layout.selectedWindow?.exactBuild
        else {
            permissionSheetError =
                "Select an exact-build napplet window to review permissions."
            isPermissionSheetPresented = true
            return
        }
        do {
            guard
                installedArtifacts[identity] != nil
                    || profile.installedCatalogArtifact(for: identity) != nil
            else {
                throw RuntimeWorkbenchPermissionError.refused(
                    code: "artifact-handle-unavailable",
                    detail: "Reinstall this exact build to reopen its verified artifact."
                )
            }
            let principal = try permissionPrincipal(identity)
            permissionManager = try RuntimeWorkbenchPermissionManager(
                profile: profile,
                principal: principal
            )
            permissionTargetIdentity = identity
        } catch {
            permissionSheetError = error.localizedDescription
        }
        isPermissionSheetPresented = true
    }

    @MainActor
    func openSettings() {
        let unavailableReason =
            bootstrapError ?? "The application runtime profile is still opening."
        settingsSnapshot = WorkbenchSettingsSnapshot(
            profileAvailable: profile != nil,
            unavailableReason: profile == nil ? unavailableReason : nil
        )
        settingsRoute = WorkbenchSettingsRouteState()
        isSettingsSheetPresented = true
    }

    @MainActor
    func scheduleSettingsDestination(
        _ destination: WorkbenchSettingsDestination
    ) {
        settingsRoute.schedule(destination)
    }

    @MainActor
    func openSettingsDestination(
        _ destination: WorkbenchSettingsDestination
    ) {
        switch destination {
        case .account:
            isAccountSheetPresented = true
        case .installedLibrary:
            isLibrarySheetPresented = true
        case .activity:
            openActivityDrawer()
        }
    }

    @MainActor
    func handleNativeAction(_ action: NativeWorkbenchAction) {
        let identity = WorkbenchExactBuildIdentity(
            manifestAuthor: action.manifestAuthor,
            dTag: action.dTag,
            aggregateHash: action.aggregateHash
        )
        guard let window = layout.windows.first(where: {
            $0.exactBuild == identity
        }) else {
            activity = "Refused: INC action came from an unopened exact build"
            return
        }
        guard let notice = NativeActionNotice.decode(action) else {
            activity = "Refused: INC action payload was not recognized"
            return
        }
        let previouslyDisplayed = fullWindowPath.last ?? fullWindowRootID
        nativeActionNotice = notice
        mutateLayout { $0.bringToFront(window.id) }
        pushFullWindowIfNeeded(
            target: window.id,
            previouslyDisplayed: previouslyDisplayed
        )
        isInspectorPresented = true
        activity = "\(notice.title) from \(window.title)"
    }
}
