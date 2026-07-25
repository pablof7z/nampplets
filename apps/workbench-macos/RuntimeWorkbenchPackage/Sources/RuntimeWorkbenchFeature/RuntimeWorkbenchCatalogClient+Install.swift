import Foundation

extension RuntimeWorkbenchCatalogClient {
    public func resolveReview(
        _ target: CatalogReviewTarget
    ) async -> CatalogReviewResponse {
        if let profileBacking {
            if let activeLiveReviewID {
                profileBacking.cancelCatalogReview(activeLiveReviewID)
                self.activeLiveReviewID = nil
            }
            let response = await profileBacking.resolveCatalogReview(target)
            if case let .ready(review) = response {
                activeLiveReviewID = review.id
            }
            return response
        }
        if let loadIssue {
            return .unavailable(loadIssue)
        }

        switch target {
        case let .entryID(id):
            guard let record = records.first(where: { $0.entry.id == id }) else {
                return .unavailable(
                    CatalogIssue(
                        title: "Catalog entry unavailable",
                        message: "That entry is not in this build's exact "
                            + "pinned compatibility corpus."
                    )
                )
            }
            if let review = record.review {
                return .ready(review)
            }
            return .unavailable(
                record.reviewIssue
                    ?? CatalogIssue(
                        title: "Build unavailable",
                        message: "This corpus entry has no installable signed build."
                    )
            )

        case .manualCoordinate:
            return .unavailable(Self.remoteResolutionUnavailable)
        }
    }

    public func cancelPendingCatalogWork() {
        profileBacking?.cancelCatalogWork()
        if let activeLiveReviewID {
            profileBacking?.cancelCatalogReview(activeLiveReviewID)
            self.activeLiveReviewID = nil
        }
    }

    public func confirmExactVerifiedInstall(
        _ confirmation: CatalogInstallConfirmation
    ) async -> CatalogInstallResponse {
        if let profileBacking {
            activeLiveReviewID = nil
            return await profileBacking.installCatalogReview(confirmation)
        }
        return .refused(
            CatalogIssue(
                title: "Installation unavailable",
                message: (isOfflineFixture
                    ? "The offline UI-test corpus is read-only, so build "
                    : "No runtime profile is connected, so build ")
                    + "\(confirmation.exactAggregateHash) was not installed, "
                    + "launched, or granted capabilities."
            )
        )
    }

    private static var remoteResolutionUnavailable: CatalogIssue {
        CatalogIssue(
            title: "Offline coordinate resolution unavailable",
            message: "Manual manifest coordinates are not resolved by this "
                + "offline UI-test corpus. Connect the Rust resolver and "
                + "install-only boundary before reviewing remote coordinates."
        )
    }
}
