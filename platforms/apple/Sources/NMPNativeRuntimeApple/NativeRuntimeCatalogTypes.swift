import Foundation
import NMPNativeRuntime

// MARK: - Installed artifacts and catalog installation types

/// One immutable verified artifact installed into exactly one runtime profile.
///
/// The Rust handle remains opaque. Native callers can use the exact coordinate
/// for review presentation, but cannot replace its bytes, requirements, or
/// launch authority.
public final class NativeRuntimeInstalledArtifact: @unchecked Sendable {
    public let title: String
    public let permissionCoordinate: NativeRuntimePermissionCoordinate

    let ownerID: UUID
    let artifact: VerifiedArtifact

    init(
        title: String,
        ownerID: UUID,
        artifact: VerifiedArtifact,
        permissionCoordinate: NativeRuntimePermissionCoordinate
    ) {
        self.title = title
        self.ownerID = ownerID
        self.artifact = artifact
        self.permissionCoordinate = permissionCoordinate
    }
}

/// One catalog-confirmed exact build installed into this profile.
///
/// The opaque artifact remains profile-bound and is retained only so the app
/// may perform the separate permission and launch steps later.
public struct NativeRuntimeCatalogInstallation: @unchecked Sendable {
    public let title: String
    public let manifestAuthor: String
    public let dTag: String
    public let aggregateHash: String
    public let installedArtifact: NativeRuntimeInstalledArtifact
}

public enum NativeRuntimeCatalogInstallResult: @unchecked Sendable {
    case installed(NativeRuntimeCatalogInstallation)
    case refused(NativeRuntimeCatalogFailure)
}

/// Replacement semantics for the profile-owned permanent NMP catalog feed.
public enum NativeRuntimeCatalogUpdate: Sendable {
    case authoritative(NativeRuntimeCatalogFeedSnapshot)
    case next(
        NativeRuntimeCatalogFeedSnapshot,
        predecessorRevision: UInt64
    )
}

public enum NativeRuntimeCatalogObservationError: Error, Equatable, Sendable {
    case profileClosed
    case observerCapacity(maximum: Int)
}

/// Idempotent application-observer cancellation. Cancelling this fanout does
/// not stop the profile-owned NMP subscription.
public final class NativeRuntimeCatalogObservation: @unchecked Sendable {
    private let lock = NSLock()
    private var cancellation: (@Sendable () -> Void)?

    init(cancellation: @escaping @Sendable () -> Void) {
        self.cancellation = cancellation
    }

    public func cancel() {
        lock.lock()
        let cancellation = cancellation
        self.cancellation = nil
        lock.unlock()
        cancellation?()
    }

    deinit {
        cancel()
    }
}
