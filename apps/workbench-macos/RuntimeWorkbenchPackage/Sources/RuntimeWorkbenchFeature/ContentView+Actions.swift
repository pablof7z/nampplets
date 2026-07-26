import NMPNativeRuntimeApple
import SwiftUI

extension ContentView {
    @MainActor
    func bootstrapProfile() async {
        if let bootstrapError {
            activity = "Refused: \(bootstrapError)"
            return
        }
        guard let profile else {
            activity = "Getting things ready"
            return
        }
        profile.native.setIncActionHandler { action in
            Task { @MainActor in
                handleNativeAction(action)
            }
        }
        if await restorePersistedCanvasWindows() {
            return
        }
        guard [
            "good-morning-permission-launch",
            "full-window-layout-transition",
        ].contains(
            ProcessInfo.processInfo.environment[
                "NMP_WORKBENCH_UI_TEST_SCENARIO"
            ]
        ) else {
            activity = "Ready · add a napplet"
            return
        }
        do {
            let fixture = try GoodMorningFixture.load()
            let installed = try await Task.detached {
                try fixture.install(profile: profile)
            }.value
            try prepareInstalledArtifact(
                installed,
                identity: WorkbenchExactBuildIdentity(
                    manifestAuthor: GoodMorningFixture.author,
                    dTag: GoodMorningFixture.dTag,
                    aggregateHash: GoodMorningFixture.aggregateHash
                )
            )
        } catch {
            activity = "Refused: \(error.localizedDescription)"
        }
    }

    @MainActor
    func openActivityDrawer() {
        guard let profile else {
            activitySheetPresentation = .unavailable(
                reason: bootstrapError
                    ?? "The application runtime profile is unavailable."
            )
            return
        }
        guard let scope = selectedActivityScope else {
            activitySheetPresentation = .unavailable(
                reason: "Select an exact-build napplet window to view its activity."
            )
            return
        }
        do {
            let source = try RuntimeWorkbenchActivitySource(
                profile: profile,
                scope: scope
            )
            activitySheetPresentation = .admitted(
                source: source,
                scope: scope
            )
        } catch {
            activitySheetPresentation = .unavailable(
                reason: error.localizedDescription
            )
        }
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
        if let profile {
            settingsSnapshot = profile.settingsSnapshot()
        } else {
            settingsSnapshot = WorkbenchSettingsSnapshot(
                unavailableReason: bootstrapError
                    ?? "Preferences are unavailable while the app is opening."
            )
        }
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
