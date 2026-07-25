import Foundation
import NMPNativeRuntime

// MARK: - Catalog browse, review, install, and reacquisition

extension NativeRuntimeProfile {
    /// Opens one finite, source-scoped NMP catalog projection. This call is
    /// blocking and must be invoked away from the main actor.
    public func browseCatalog(
        query: String
    ) -> NativeRuntimeCatalogPageResult {
        controller.catalogBrowse(query: query)
    }

    /// Freezes one exact signed review from the current bounded catalog page.
    /// This call is blocking and must be invoked away from the main actor.
    public func reviewCatalogEntry(
        eventID: String
    ) -> NativeRuntimeCatalogReviewResult {
        controller.catalogReviewEntry(eventId: eventID)
    }

    /// Resolves a manually entered manifest coordinate exclusively in Rust.
    /// This call is blocking and must be invoked away from the main actor.
    public func reviewCatalogCoordinate(
        _ coordinate: String
    ) -> NativeRuntimeCatalogReviewResult {
        controller.catalogReviewManual(coordinate: coordinate)
    }

    /// Wakes every blocking catalog observation or acquisition.
    @discardableResult
    public func cancelPendingCatalogWork()
        -> NativeRuntimeCatalogCancellationResult
    {
        controller.catalogCancelPending()
    }

    /// Cancels and discards one frozen review without installing it.
    @discardableResult
    public func cancelCatalogReview(
        token: String
    ) -> NativeRuntimeCatalogCancellationResult {
        controller.catalogCancelReview(token: token)
    }

    /// Confirms and installs one frozen exact review. Permission grants and
    /// launch remain separate operations.
    ///
    /// This call is blocking and must be invoked away from the main actor.
    public func confirmCatalogInstall(
        token: String,
        expectedAuthor: String,
        expectedDTag: String,
        expectedAggregateHash: String
    ) -> NativeRuntimeCatalogInstallResult {
        let result = controller.catalogConfirmInstall(
            token: token,
            expectedAuthor: expectedAuthor,
            expectedDTag: expectedDTag,
            expectedAggregateHash: expectedAggregateHash
        )
        if let failure = result.failure {
            return .refused(failure)
        }
        guard
            let confirmation = result.confirmation,
            let artifact = result.artifact,
            let dTag = confirmation.dTag,
            confirmation.manifestAuthor == expectedAuthor,
            dTag == expectedDTag,
            confirmation.aggregateHash == expectedAggregateHash
        else {
            return .refused(
                NativeRuntimeCatalogFailure(
                    code: "incomplete-confirmation",
                    detail: "Rust returned no complete exact catalog installation",
                    provenance: []
                )
            )
        }
        let title = confirmation.title ?? "Untitled napplet"
        let coordinate = NativeRuntimePermissionCoordinate(
            manifestAuthor: confirmation.manifestAuthor,
            dTag: dTag,
            aggregateHash: confirmation.aggregateHash
        )
        let installedArtifact = NativeRuntimeInstalledArtifact(
            title: title,
            ownerID: profileID,
            artifact: artifact,
            permissionCoordinate: coordinate
        )
        return .installed(
            NativeRuntimeCatalogInstallation(
                title: title,
                manifestAuthor: confirmation.manifestAuthor,
                dTag: dTag,
                aggregateHash: confirmation.aggregateHash,
                installedArtifact: installedArtifact
            )
        )
    }

    /// Reacquires one installed exact build from the current profile's retained
    /// verifier handle without granting or launching it.
    ///
    /// Rust owns the unfiltered installation lookup and exact-build drift
    /// checks. Native supplies only the complete installed coordinate and
    /// receives the same sealed handle shape as a fresh catalog installation.
    /// A restarted profile fails closed until artifact-owned persistent exact
    /// bytes can be reopened through a supported Rust seam.
    public func reacquireInstalledArtifact(
        _ coordinate: NativeRuntimePermissionCoordinate
    ) -> NativeRuntimeCatalogInstallResult {
        lock.lock()
        let closed = isClosed
        lock.unlock()
        guard !closed else {
            return .refused(
                NativeRuntimeCatalogFailure(
                    code: "closed",
                    detail: "The application runtime profile is closed",
                    provenance: []
                )
            )
        }
        let result = controller.reacquireInstalledArtifact(
            coordinate: RuntimeExactBuildCoordinate(
                manifestAuthor: coordinate.manifestAuthor,
                dTag: coordinate.dTag,
                aggregateHash: coordinate.aggregateHash
            )
        )
        if let failure = result.failure {
            return .refused(failure)
        }
        guard
            let confirmation = result.confirmation,
            let artifact = result.artifact,
            confirmation.manifestAuthor == coordinate.manifestAuthor,
            confirmation.dTag == coordinate.dTag,
            confirmation.aggregateHash == coordinate.aggregateHash
        else {
            return .refused(
                NativeRuntimeCatalogFailure(
                    code: "incomplete-reacquisition",
                    detail: "Rust returned no complete exact installed artifact",
                    provenance: []
                )
            )
        }
        let title = confirmation.title ?? "Untitled napplet"
        return .installed(
            NativeRuntimeCatalogInstallation(
                title: title,
                manifestAuthor: coordinate.manifestAuthor,
                dTag: coordinate.dTag,
                aggregateHash: coordinate.aggregateHash,
                installedArtifact: NativeRuntimeInstalledArtifact(
                    title: title,
                    ownerID: profileID,
                    artifact: artifact,
                    permissionCoordinate: coordinate
                )
            )
        )
    }
}
