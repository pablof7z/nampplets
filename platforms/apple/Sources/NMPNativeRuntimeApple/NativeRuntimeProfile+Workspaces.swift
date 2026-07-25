import Foundation
import NMPNativeRuntime

// MARK: - Workspaces, permission decisions, and installed-build commands

extension NativeRuntimeProfile {
    public func saveWorkspace(
        _ workspace: NativeRuntimeWorkspaceDefinition
    ) -> NativeRuntimeWorkspaceUpdate {
        controller.saveWorkspace(workspace: workspace)
    }

    public func restoreWorkspaces() -> NativeRuntimeWorkspaceRestore {
        controller.restoreWorkspaces()
    }

    /// Returns one bounded Rust-owned review for an installed exact build.
    /// This operation never grants or launches the napplet.
    public func permissionReview(
        for coordinate: NativeRuntimePermissionCoordinate
    ) -> NativeRuntimePermissionReviewResult {
        controller.permissionReview(coordinate: coordinate)
    }

    /// Applies one complete exact-build decision set atomically in Rust.
    /// Success never launches the napplet; launch remains a separate operation.
    public func applyPermissionDecisions(
        _ batch: NativeRuntimePermissionDecisionBatch
    ) -> NativeRuntimePermissionBatchUpdate {
        controller.applyPermissionDecisions(batch: batch)
    }

    /// Resolves one Rust-retained provider write proposal. The native shell
    /// supplies only the opaque operation id and decision; the frozen write
    /// remains owned by RuntimeApp.
    public func decideProviderWrite(
        operationID: UInt64,
        approve: Bool
    ) {
        controller.decideProviderWrite(
            operationId: operationID,
            approve: approve
        )
    }

    /// Applies the Rust-owned finite installed-library filter.
    public func setInstalledLibraryFilter(_ query: String) {
        controller.setLibraryFilter(query: query)
    }

    public func suspendInstalledSession(_ sessionID: UInt64) {
        controller.suspend(sessionId: sessionID)
    }

    public func resumeInstalledSession(_ sessionID: UInt64) {
        controller.resume(sessionId: sessionID)
    }

    public func assignInstalledBuild(
        _ exactBuild: NativeRuntimeLibraryExactBuild,
        toWorkspaceID workspaceID: String
    ) {
        controller.assignBuildToWorkspace(
            workspaceId: workspaceID,
            coordinate: runtimeCoordinate(exactBuild)
        )
    }

    public func clearInstalledBuildAssignment(
        _ exactBuild: NativeRuntimeLibraryExactBuild,
        fromWorkspaceID workspaceID: String
    ) {
        controller.clearBuildFromWorkspace(
            workspaceId: workspaceID,
            coordinate: runtimeCoordinate(exactBuild)
        )
    }

    public func uninstallInstalledBuild(
        _ exactBuild: NativeRuntimeLibraryExactBuild
    ) {
        controller.uninstallBuild(coordinate: runtimeCoordinate(exactBuild))
    }

    private func runtimeCoordinate(
        _ exactBuild: NativeRuntimeLibraryExactBuild
    ) -> RuntimeExactBuildCoordinate {
        RuntimeExactBuildCoordinate(
            manifestAuthor: exactBuild.manifestAuthor,
            dTag: exactBuild.dTag,
            aggregateHash: exactBuild.aggregateHash
        )
    }
}
