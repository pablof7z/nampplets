import NMPNativeRuntimeApple
import SwiftUI

extension ContentView {
    @MainActor
    func bootstrapProfile() async {
        if let bootstrapError {
            activity = .failed(detail: bootstrapError)
            return
        }
        guard let profile else {
            activity = .preparing
            return
        }
        profile.native.setIncActionHandler { action in
            Task { @MainActor in
                handleNativeAction(action)
            }
        }
        profile.native.setIntentActivationHandler { request in
            Task { @MainActor in
                handleIntentActivation(request)
            }
        }
        subscribeToRunningLibrarySessions()
        if await restorePersistedCanvasWindows() {
            return
        }
        do {
            guard let seed = try UITestNappletSeed.fromLaunchEnvironment()
            else {
                // The canvas starts empty. The Workbench bundles no napplet
                // and auto-opens none; everything reaches the canvas through
                // the catalog or the installed library.
                activity = .readyToAdd
                return
            }
            let installed = try await Task.detached {
                try seed.install(profile: profile)
            }.value
            try prepareInstalledArtifact(installed, identity: seed.identity)
        } catch {
            activity = .failed(detail: error.localizedDescription)
        }
    }

    /// Keeps `runningLibrarySessionBuilds` in step with Rust's own view of
    /// which exact builds have a live `.running` session, independent of
    /// whether this process's window bookkeeping (`runningArtifacts`) still
    /// thinks a napplet is open. Without this, the Inspector can only ever
    /// say "Running" or "Not open" — it has no way to notice that Rust
    /// already ended a session out from under an still-open window.
    @MainActor
    private func subscribeToRunningLibrarySessions() {
        librarySessionSubscription?.cancel()
        librarySessionSubscription = libraryManager.subscribe { [self] update in
            let snapshot: WorkbenchLibrarySnapshot =
                switch update {
                case let .authoritative(snapshot), let .next(snapshot, _, _):
                    snapshot
                }
            runningLibrarySessionBuilds = Self.buildsWithLiveSessions(in: snapshot)
        }
    }

    /// The exact builds Rust currently reports at least one *live* session
    /// for. Live, not whole: a session missing a required domain is degraded
    /// but still running, and the Inspector must say "Running" for it rather
    /// than "Session ended".
    ///
    /// This reads `isLive` instead of comparing against `.running`. The
    /// comparison was written when `running` was the only live state, and
    /// would have silently started excluding degraded sessions the moment
    /// `.runningDegraded` existed — turning "the Inspector wrongly says
    /// Running for a dead session" into "the Inspector wrongly says Session
    /// ended for a live one", which is the same defect from the other side.
    ///
    /// Kept as a pure function so the distinction is testable without a live
    /// runtime, matching `hasObservedRunningSession` below.
    nonisolated static func buildsWithLiveSessions(
        in snapshot: WorkbenchLibrarySnapshot
    ) -> Set<WorkbenchLibraryExactBuild> {
        Set(
            snapshot.builds
                .filter { build in
                    build.sessions.contains(where: \.state.isLive)
                }
                .map(\.exactBuild)
        )
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
                reason: "Select a napplet window first — activity is shown one "
                    + "napplet at a time."
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
                scope: scope,
                title: layout.selectedWindow?.title
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
            activity = .failed(
                detail: "The application runtime profile is unavailable."
            )
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
            activity = .failed(
                detail: "The verified artifact handle is unavailable for this profile."
            )
            return
        }
        do {
            try prepareInstalledArtifact(
                installed,
                identity: identity,
                presentation: .afterCatalogDismiss
            )
        } catch {
            activity = .failed(detail: error.localizedDescription)
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
            activity = .refused(
                detail: "INC action came from an unopened exact build."
            )
            return
        }
        let notice = NativeActionNotice.presentation(action)
        let previouslyDisplayed = fullWindowPath.last ?? fullWindowRootID
        nativeActionNotice = notice
        mutateLayout { $0.bringToFront(window.id) }
        pushFullWindowIfNeeded(
            target: window.id,
            previouslyDisplayed: previouslyDisplayed
        )
        isInspectorPresented = true
        activity = .nativeAction(
            title: notice.title,
            nappletTitle: window.title
        )
    }

    /// Handles a NAP-INTENT "create (if needed) and bring to front" signal.
    /// Unlike `handleNativeAction`, this may fire before any window for the
    /// handler exists yet -- `prepareInstalledArtifact` already contains the
    /// exact "focus if open, otherwise install and launch" branch this
    /// needs, so it is reused as-is rather than duplicated here.
    @MainActor
    func handleIntentActivation(
        _ request: NativeIntentActivationHandlerRequest
    ) {
        guard let profile else {
            activity = .failed(
                detail: "The application runtime profile is unavailable."
            )
            return
        }
        let identity = WorkbenchExactBuildIdentity(
            manifestAuthor: request.manifestAuthor,
            dTag: request.dTag,
            aggregateHash: request.aggregateHash
        )
        guard let installed = profile.installedCatalogArtifact(for: identity) else {
            activity = .refused(
                detail: "The requested napplet is not installed."
            )
            return
        }
        do {
            try prepareInstalledArtifact(
                installed,
                identity: identity,
                presentation: .afterCatalogDismiss
            )
        } catch {
            activity = .failed(detail: error.localizedDescription)
        }
    }
}
