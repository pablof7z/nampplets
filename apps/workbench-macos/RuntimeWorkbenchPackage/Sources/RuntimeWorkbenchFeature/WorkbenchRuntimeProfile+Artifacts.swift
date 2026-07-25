import Foundation
import NMPNativeRuntimeApple

extension WorkbenchRuntimeProfile {
    func installedCatalogArtifact(
        for identity: WorkbenchExactBuildIdentity
    ) -> NativeRuntimeInstalledArtifact? {
        catalogStateLock.lock()
        let artifact = catalogArtifacts[identity]
        catalogStateLock.unlock()
        return artifact
    }

    func reacquireInstalledArtifact(
        for identity: WorkbenchExactBuildIdentity
    ) -> NativeRuntimeCatalogInstallResult {
        let coordinate = NativeRuntimePermissionCoordinate(
            manifestAuthor: identity.manifestAuthor,
            dTag: identity.dTag,
            aggregateHash: identity.aggregateHash
        )
        let result = native.reacquireInstalledArtifact(coordinate)
        if case let .installed(installation) = result {
            storeCatalogArtifact(
                installation.installedArtifact,
                identity: identity
            )
        }
        return result
    }

    /// Reopens an exact build represented by a persisted canvas window.
    ///
    /// The fast path borrows the current process's retained verified handle.
    /// After a process restart that handle is intentionally absent, so the
    /// fallback asks the existing Rust/NMP catalog boundary to resolve the
    /// named coordinate again. It confirms only when the signed replacement
    /// still has the exact persisted aggregate; a changed build fails closed.
    func reacquirePersistedCanvasArtifact(
        for identity: WorkbenchExactBuildIdentity
    ) -> NativeRuntimeCatalogInstallResult {
        let retained = reacquireInstalledArtifact(for: identity)
        guard
            case let .refused(retainedFailure) = retained,
            retainedFailure.code == "artifact-handle-unavailable"
        else {
            return retained
        }

        let result = persistedArtifactResolver(native, identity)
        guard case let .installed(installation) = result else {
            return result
        }
        guard
            WorkbenchRestoredCanvasLaunchPlan
                .reviewMatchesPersistedBuild(
                    manifestAuthor: installation.manifestAuthor,
                    dTag: installation.dTag,
                    aggregateHash: installation.aggregateHash,
                    identity: identity
                )
        else {
            return .refused(Self.restoredBuildChanged())
        }
        storeCatalogArtifact(
            installation.installedArtifact,
            identity: identity
        )
        return result
    }

    static func resolvePersistedArtifact(
        native: NativeRuntimeProfile,
        identity: WorkbenchExactBuildIdentity
    ) -> NativeRuntimeCatalogInstallResult {
        let coordinate = "35129:\(identity.manifestAuthor):\(identity.dTag)"
        let reviewResult = native.reviewCatalogCoordinate(coordinate)
        if let failure = reviewResult.failure {
            return .refused(failure)
        }
        guard let review = reviewResult.review else {
            return .refused(
                NativeRuntimeCatalogFailure(
                    code: "restored-review-unavailable",
                    detail: "NMP returned no exact signed review for the persisted canvas build.",
                    provenance: []
                )
            )
        }
        guard
            WorkbenchRestoredCanvasLaunchPlan
                .reviewMatchesPersistedBuild(
                    manifestAuthor: review.manifestAuthor,
                    dTag: review.dTag,
                    aggregateHash: review.aggregateHash,
                    identity: identity
                )
        else {
            native.cancelCatalogReview(token: review.token)
            return .refused(restoredBuildChanged(provenance: review.provenance))
        }

        return native.confirmCatalogInstall(
            token: review.token,
            expectedAuthor: identity.manifestAuthor,
            expectedDTag: identity.dTag,
            expectedAggregateHash: identity.aggregateHash
        )
    }

    private static func restoredBuildChanged(
        provenance: [NativeRuntimeCatalogProvenance] = []
    ) -> NativeRuntimeCatalogFailure {
        NativeRuntimeCatalogFailure(
            code: "restored-build-changed",
            detail: "The current signed manifest does not match the persisted exact build.",
            provenance: provenance
        )
    }
}
