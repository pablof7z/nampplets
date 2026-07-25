import NMPNativeRuntimeApple
import SwiftUI

extension ContentView {
    @MainActor
    func prepareInstalledArtifact(
        _ installed: NativeRuntimeInstalledArtifact,
        identity: WorkbenchExactBuildIdentity,
        presentation: InstalledArtifactPresentation = .immediate
    ) throws {
        guard let profile else {
            throw RuntimeWorkbenchPermissionError.malformed(
                "the application runtime profile is unavailable"
            )
        }
        let previouslyDisplayed = fullWindowPath.last ?? fullWindowRootID
        let targetWindowID: WorkbenchWindowID
        if let existing = layout.windows.first(where: {
            $0.exactBuild == identity
        }) {
            if presentation.focusesExistingWindow {
                mutateLayout {
                    $0.bringToFront(existing.id)
                }
            }
            targetWindowID = existing.id
        } else {
            var next = layout
            let window = WorkbenchCanvasWindow.installed(
                title: installed.title,
                identity: identity,
                offset: Double(next.windows.count % 8) * 24
            )
            guard next.addWindow(window) else {
                throw RuntimeWorkbenchPermissionError.refused(
                    code: "workspace-capacity",
                    detail: "Close a canvas window before adding another napplet."
                )
            }
            layout = next
            scheduleLayoutSave()
            targetWindowID = window.id
        }
        pushFullWindowIfNeeded(
            target: targetWindowID,
            previouslyDisplayed: previouslyDisplayed
        )
        installedArtifacts[identity] = installed
        permissionTargetIdentity = identity
        let principal = try permissionPrincipal(identity)
        permissionManager = try RuntimeWorkbenchPermissionManager(
            profile: profile,
            principal: principal
        )
        let nativeReview = profile.native.permissionReview(
            for: installed.permissionCoordinate
        )
        guard nativeReview.refusal == nil,
              let review = nativeReview.review
        else {
            throw RuntimeWorkbenchPermissionError.refused(
                code: nativeReview.refusal?.code ?? "missing-review",
                detail: nativeReview.refusal?.detail
                    ?? "Rust returned no permission review"
            )
        }
        if review.launchPermitted {
            launchInstalledIfPermitted(identity)
        } else {
            activity = "Permission review required before launch"
            switch presentation {
            case .immediate:
                isPermissionSheetPresented = true
            case .afterCatalogDismiss:
                deferredPermissionIdentity = identity
            case .restoration:
                break
            }
        }
    }

    @MainActor
    @discardableResult
    func reacquireInstalledArtifact(
        _ identity: WorkbenchExactBuildIdentity,
        presentation: InstalledArtifactPresentation
    ) async -> Bool {
        guard let profile else {
            activity = "Refused: the application runtime profile is unavailable"
            return false
        }
        guard
            !reacquiringIdentities.contains(identity),
            !launchingIdentities.contains(identity)
        else {
            return false
        }
        reacquiringIdentities.insert(identity)
        activity = "Reopening installed exact build"
        let result = await Task.detached {
            profile.reacquirePersistedCanvasArtifact(for: identity)
        }.value
        reacquiringIdentities.remove(identity)
        switch result {
        case let .refused(failure):
            activity = "Refused: \(failure.code): \(failure.detail)"
            return false
        case let .installed(installation):
            guard
                presentation != .restoration
                    || layout.windows.contains(where: {
                        $0.exactBuild == identity
                    })
            else {
                return false
            }
            do {
                try prepareInstalledArtifact(
                    installation.installedArtifact,
                    identity: identity,
                    presentation: presentation
                )
                return true
            } catch {
                activity = "Refused: \(error.localizedDescription)"
                return false
            }
        }
    }

    @MainActor
    func restorePersistedCanvasWindows() async -> Bool {
        let plan = WorkbenchRestoredCanvasLaunchPlan(layout: layout)
        guard !plan.identities.isEmpty else {
            return false
        }
        activity =
            "Reopening \(plan.identities.count) persisted napplet"
            + (plan.identities.count == 1 ? "" : "s")
        for identity in plan.identities {
            _ = await reacquireInstalledArtifact(
                identity,
                presentation: .restoration
            )
        }
        return true
    }

    @MainActor
    func launchInstalledIfPermitted(
        _ identity: WorkbenchExactBuildIdentity
    ) {
        guard
            let profile,
            let installed =
                installedArtifacts[identity]
                ?? profile.installedCatalogArtifact(for: identity),
            !launchingIdentities.contains(identity),
            runningArtifacts[identity] == nil
        else {
            return
        }
        let review = profile.native.permissionReview(
            for: installed.permissionCoordinate
        )
        guard review.refusal == nil,
              review.review?.launchPermitted == true
        else {
            activity = "Permission review still requires a decision"
            return
        }
        launchingIdentities.insert(identity)
        activity = "Launching signed exact build"
        Task {
            defer { launchingIdentities.remove(identity) }
            do {
                let launched = try await Task.detached {
                    try profile.native.launchInstalled(installed)
                }.value
                guard layout.windows.contains(where: {
                    $0.exactBuild == identity
                }) else {
                    return
                }
                runningArtifacts[identity] = launched
                if permissionTargetIdentity == identity {
                    permissionTargetIdentity = nil
                }
                activity = "Signed exact-build session ready"
            } catch {
                activity = "Refused: \(error.localizedDescription)"
            }
        }
    }

    func permissionPrincipal(
        _ identity: WorkbenchExactBuildIdentity
    )
        throws -> PermissionExactBuildPrincipal
    {
        guard
            let principal = PermissionExactBuildPrincipal(
                manifestAuthorPublicKey: identity.manifestAuthor,
                dTag: identity.dTag,
                aggregateHash: identity.aggregateHash
            )
        else {
            throw RuntimeWorkbenchPermissionError.malformed(
                "the selected exact-build identity is invalid"
            )
        }
        return principal
    }

    /// Pushes onto the full-window navigation stack when a different napplet
    /// becomes active while `.fullWindow` is engaged, so opening one napplet
    /// from another reads as a normal iOS push rather than an in-place swap.
    /// The very first napplet displayed becomes the stack root instead.
    @MainActor
    func pushFullWindowIfNeeded(
        target: WorkbenchWindowID,
        previouslyDisplayed: WorkbenchWindowID?
    ) {
        guard layout.mode == .fullWindow else {
            return
        }
        guard let previouslyDisplayed else {
            fullWindowRootID = target
            return
        }
        guard previouslyDisplayed != target else {
            return
        }
        fullWindowPath.append(target)
    }
}
