import Foundation
import NMPNativeRuntimeApple

@MainActor
extension WorkbenchRuntimeProfile: RuntimeWorkbenchCatalogProfileBacking {
    public func observeCatalogChanges(
        _ receive: @escaping @MainActor @Sendable () -> Void
    ) -> CatalogFeedObservation {
        let mailbox = WorkbenchCatalogChangeMailbox(receive: receive)
        do {
            let observation = try native.observeCatalog { _ in
                mailbox.offer()
            }
            return CatalogFeedObservation {
                mailbox.close()
                observation.cancel()
            }
        } catch {
            mailbox.close()
            return CatalogFeedObservation()
        }
    }

    public func browseCatalog(
        _ request: CatalogSearchRequest
    ) async -> CatalogSearchResponse {
        let native = native
        let result = await Task.detached {
            native.browseCatalog(query: request.query)
        }.value
        if let failure = result.failure {
            return .unavailable(Self.catalogIssue(failure))
        }
        guard let page = result.page else {
            return .unavailable(
                CatalogIssue(
                    title: "Catalog unavailable",
                    message: "Rust returned no bounded catalog page."
                )
            )
        }
        return Self.projectCatalogPage(page)
    }

    public func resolveCatalogReview(
        _ target: CatalogReviewTarget
    ) async -> CatalogReviewResponse {
        let native = native
        let result = await Task.detached {
            switch target {
            case let .entryID(eventID):
                native.reviewCatalogEntry(eventID: eventID)
            case let .manualCoordinate(request):
                native.reviewCatalogCoordinate(request.coordinate)
            }
        }.value
        if let failure = result.failure {
            return .unavailable(Self.catalogIssue(failure))
        }
        guard
            let review = result.review,
            let projected = Self.projectCatalogReview(review)
        else {
            return .unavailable(
                CatalogIssue(
                    title: "Review unavailable",
                    message: "The exact Rust review exceeded native screen limits."
                )
            )
        }
        storeCatalogReview(review)
        return .ready(projected)
    }

    public func cancelCatalogWork() {
        native.cancelPendingCatalogWork()
    }

    public func cancelCatalogReview(_ reviewID: String) {
        if takeCatalogReview(id: reviewID) != nil {
            native.cancelCatalogReview(token: reviewID)
        }
    }

    public func installCatalogReview(
        _ confirmation: CatalogInstallConfirmation
    ) async -> CatalogInstallResponse {
        let review = takeCatalogReview(id: confirmation.reviewID)
        guard
            let review,
            let dTag = review.dTag,
            review.manifestAuthor == confirmation.publisherPublicKey,
            review.coordinate == confirmation.coordinate,
            review.aggregateHash == confirmation.exactAggregateHash
        else {
            return .refused(
                CatalogIssue(
                    title: "Exact review changed",
                    message: "The install confirmation no longer matches the frozen Rust review."
                )
            )
        }
        let native = native
        let result = await Task.detached {
            native.confirmCatalogInstall(
                token: review.token,
                expectedAuthor: review.manifestAuthor,
                expectedDTag: dTag,
                expectedAggregateHash: review.aggregateHash
            )
        }.value
        switch result {
        case let .refused(failure):
            return .refused(Self.catalogIssue(failure))
        case let .installed(installation):
            guard
                let build = CatalogInstalledBuild(
                    title: installation.title,
                    manifestAuthor: installation.manifestAuthor,
                    dTag: installation.dTag,
                    exactAggregateHash: installation.aggregateHash
                )
            else {
                return .refused(
                    CatalogIssue(
                        title: "Installed build unavailable",
                        message: "The Rust installation exceeded native screen limits."
                    )
                )
            }
            let identity = WorkbenchExactBuildIdentity(
                manifestAuthor: installation.manifestAuthor,
                dTag: installation.dTag,
                aggregateHash: installation.aggregateHash
            )
            storeCatalogArtifact(
                installation.installedArtifact,
                identity: identity
            )
            return .installed(build)
        }
    }

    private static func projectCatalogPage(
        _ page: NativeRuntimeCatalogPage
    ) -> CatalogSearchResponse {
        let entries = page.entries.compactMap { entry -> CatalogEntry? in
            guard let coordinate = entry.coordinate else {
                return nil
            }
            return CatalogEntry(
                id: entry.eventId,
                title: entry.title ?? entry.dTag ?? "Untitled napplet",
                summary: entry.description
                    ?? "Signed public napplet manifest from the current NMP window.",
                publisher: CatalogPublisher(
                    displayName: nil,
                    publicKey: entry.manifestAuthor
                ),
                coordinate: coordinate,
                compatibility: .unreviewed
            )
        }
        guard entries.count == page.entries.count else {
            return .unavailable(
                CatalogIssue(
                    title: "Catalog page refused",
                    message: "A Rust catalog row had no supported manifest coordinate."
                )
            )
        }
        let sources = page.sources.enumerated().compactMap {
            index,
            source -> CatalogBrowseSourceEvidence? in
            let access: CatalogBrowseAccessContext
            switch source.access {
            case .public:
                access = .public
            case let .nip42(publicKey):
                access = .nip42(publicKey: publicKey)
            }
            let status: CatalogBrowseSourceStatus
            switch source.state {
            case .requesting:
                status = .requesting
            case .connecting:
                status = .connecting
            case .disconnected:
                status = .disconnected
            case .awaitingAuth:
                status = .awaitingAuthentication
            case .authDenied:
                status = .authenticationDenied
            case .error:
                status = .error
            }
            return CatalogBrowseSourceEvidence(
                id: "\(index):\(source.relay)",
                source: source.relay,
                access: access,
                status: status,
                reconciledThrough: source.reconciledThrough
            )
        }
        let shortfalls = page.shortfalls.map {
            switch $0 {
            case .noPlannedSource:
                CatalogBrowseShortfall.noPlannedSource
            case .noResolvedDemand:
                CatalogBrowseShortfall.noResolvedDemand
            case .localLimit:
                CatalogBrowseShortfall.localLimit
            }
        }
        let window: CatalogBrowseWindowState
        switch page.window {
        case .idle:
            window = .idle
        case .requesting:
            window = .requesting
        case let .returned(added):
            window = .returned(addedRows: added)
        case let .atBound(maximum):
            window = .atBound(maximumRows: maximum)
        case .unknown:
            window = .unknown
        }
        guard
            sources.count == page.sources.count,
            let locallyFilteredRows = UInt(exactly: page.locallyFilteredRows),
            let projectionLimitedRows = UInt(
                exactly: page.projectionLimitedRows
            ),
            let refusedRows = UInt(exactly: page.refusedRows),
            let evidence = CatalogBrowseEvidence(
                scope: .liveNMPWindow,
                queryWasLocalFilter: page.queryWasLocalFilter,
                locallyFilteredRows: locallyFilteredRows,
                projectedRows: UInt(entries.count),
                projectionLimitedRows: projectionLimitedRows,
                refusedRows: refusedRows,
                window: window,
                sourceEvidence: sources,
                shortfalls: shortfalls
            ),
            let projected = CatalogSearchPage(
                entries: entries,
                hasMore: page.hasMore,
                evidence: evidence
            )
        else {
            return .unavailable(
                CatalogIssue(
                    title: "Catalog page refused",
                    message: "The bounded Rust projection exceeded native screen limits."
                )
            )
        }
        return .page(projected)
    }

    private static func projectCatalogReview(
        _ review: NativeRuntimeCatalogReview
    ) -> CatalogInstallReview? {
        let lookupSources = review.provenance.enumerated().map {
            index,
            fact in
            let evidence: String
            switch fact.state {
            case let .observed(rows):
                evidence = "Observed \(rows) matching canonical row(s)."
            case let .shortfall(reason):
                evidence = "Source shortfall: \(reason)"
            case let .selected(eventID):
                evidence = "Selected exact signed event \(eventID)."
            }
            return CatalogSourceProvenance(
                id: "lookup-\(index)",
                kind: .manifestEvent,
                source: fact.source,
                evidence: evidence
            )
        }
        let blobSources = review.blobSources.enumerated().map {
            index,
            source in
            CatalogSourceProvenance(
                id: "artifact-\(index)",
                kind: .artifact,
                source: source,
                evidence: "HTTPS source declared by the exact signed manifest."
            )
        }
        let requiredDomains = review.capabilities.compactMap {
            $0.requirement == .required ? $0.domain : nil
        }
        let optionalDomains = review.capabilities.compactMap {
            $0.requirement == .optional ? $0.domain : nil
        }
        let supportsExactInstall = review.dTag != nil
        let warnings = supportsExactInstall
            ? []
            : [
                CatalogWarning(
                    id: "named-build-required",
                    severity: .blocking,
                    message: "Only named manifests can mint an exact-build runtime principal."
                ),
            ]
        return CatalogInstallReview(
            id: review.token,
            title: review.title ?? review.dTag ?? "Untitled napplet",
            publisher: CatalogPublisher(
                displayName: nil,
                publicKey: review.manifestAuthor
            ),
            coordinate: review.coordinate,
            exactAggregateHash: review.aggregateHash,
            sources: lookupSources + blobSources,
            requiredDomains: requiredDomains,
            optionalDomains: optionalDomains,
            platformCompatibility: [
                CatalogPlatformCompatibility(
                    id: "native-runtime",
                    platform: "Native macOS runtime",
                    status: .compatible,
                    detail: "Rust verified the exact signed manifest and immutable artifact bytes."
                ),
            ],
            warnings: warnings,
            updateRelationship: .unknown(
                reason: "The exact installed-library relationship is resolved during installation."
            ),
            canInstall: supportsExactInstall
        )
    }

    private static func catalogIssue(
        _ failure: NativeRuntimeCatalogFailure
    ) -> CatalogIssue {
        CatalogIssue(
            title: failure.code == "cancelled"
                ? "Catalog operation cancelled"
                : "Catalog operation refused",
            message: failure.detail
        )
    }
}
