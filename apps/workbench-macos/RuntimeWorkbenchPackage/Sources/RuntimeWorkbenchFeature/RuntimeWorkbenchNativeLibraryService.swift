import Foundation
import NMPNativeRuntimeApple

protocol RuntimeWorkbenchNativeLibraryObservation:
    AnyObject,
    Sendable
{
    func cancel()
}

extension NativeRuntimeLibraryObservation:
    RuntimeWorkbenchNativeLibraryObservation
{}

protocol RuntimeWorkbenchNativeLibraryService:
    AnyObject,
    Sendable
{
    func projection() -> NativeRuntimeLibraryProjection

    func observe(
        _ receive: @escaping @Sendable (NativeRuntimeLibraryUpdate) -> Void
    ) throws -> any RuntimeWorkbenchNativeLibraryObservation

    func setFilter(_ query: String)
    func suspend(sessionID: UInt64)
    func resume(sessionID: UInt64)
    func assign(
        _ exactBuild: NativeRuntimeLibraryExactBuild,
        toWorkspaceID workspaceID: String
    )
    func clearAssignment(
        _ exactBuild: NativeRuntimeLibraryExactBuild,
        fromWorkspaceID workspaceID: String
    )
    func uninstall(_ exactBuild: NativeRuntimeLibraryExactBuild)
}

final class ProfileNativeLibraryService:
    RuntimeWorkbenchNativeLibraryService,
    @unchecked Sendable
{
    private let profile: WorkbenchRuntimeProfile

    init(profile: WorkbenchRuntimeProfile) {
        self.profile = profile
    }

    func projection() -> NativeRuntimeLibraryProjection {
        profile.native.installedLibraryProjection()
    }

    func observe(
        _ receive: @escaping @Sendable (NativeRuntimeLibraryUpdate) -> Void
    ) throws -> any RuntimeWorkbenchNativeLibraryObservation {
        try profile.native.observeInstalledLibrary(receive)
    }

    func setFilter(_ query: String) {
        profile.native.setInstalledLibraryFilter(query)
    }

    func suspend(sessionID: UInt64) {
        profile.native.suspendInstalledSession(sessionID)
    }

    func resume(sessionID: UInt64) {
        profile.native.resumeInstalledSession(sessionID)
    }

    func assign(
        _ exactBuild: NativeRuntimeLibraryExactBuild,
        toWorkspaceID workspaceID: String
    ) {
        profile.native.assignInstalledBuild(
            exactBuild,
            toWorkspaceID: workspaceID
        )
    }

    func clearAssignment(
        _ exactBuild: NativeRuntimeLibraryExactBuild,
        fromWorkspaceID workspaceID: String
    ) {
        profile.native.clearInstalledBuildAssignment(
            exactBuild,
            fromWorkspaceID: workspaceID
        )
    }

    func uninstall(_ exactBuild: NativeRuntimeLibraryExactBuild) {
        profile.native.uninstallInstalledBuild(exactBuild)
    }
}
