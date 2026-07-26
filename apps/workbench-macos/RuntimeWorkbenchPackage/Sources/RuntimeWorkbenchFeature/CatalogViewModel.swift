import Observation

@MainActor
@Observable
public final class CatalogViewModel {
    public var query = ""
    public var manualCoordinate = ""

    public private(set) var entries: [CatalogEntry] = []
    public private(set) var hasMore = false
    public private(set) var evidence: CatalogBrowseEvidence?
    public private(set) var review: CatalogInstallReview?
    public private(set) var installedBuild: CatalogInstalledBuild?
    public private(set) var issue: CatalogIssue?
    public private(set) var isResolvingReview = false
    public private(set) var isInstalling = false

    /// Live profiles expose a connecting replacement before the first relay
    /// frame arrives, so the UI never turns a permanent subscription into an
    /// empty or missing catalog surface.
    public var connectingEvidence: CatalogBrowseEvidence? {
        guard client.feedScope == .liveNMPWindow, evidence == nil else {
            return nil
        }
        return CatalogBrowseEvidence(
            scope: .liveNMPWindow,
            queryWasLocalFilter: !query.isEmpty,
            locallyFilteredRows: 0,
            projectedRows: 0,
            projectionLimitedRows: 0,
            refusedRows: 0,
            window: .requesting,
            sourceEvidence: [],
            shortfalls: []
        )
    }

    private let client: any CatalogClient
    private let onInstalled: @MainActor (CatalogInstalledBuild) -> Void
    private var operationGeneration: UInt = 0
    private var feedGeneration: UInt = 0
    private var feedObservation: CatalogFeedObservation?
    private var started = false

    public init(
        client: any CatalogClient,
        onInstalled: @escaping @MainActor (CatalogInstalledBuild) -> Void = {
            _ in
        }
    ) {
        self.client = client
        self.onInstalled = onInstalled
    }

    /// Attaches to the profile-owned permanent feed and renders its latest
    /// bounded replacement immediately.
    public func start() async {
        guard !started else {
            return
        }
        started = true
        if client.feedScope == .liveNMPWindow, evidence == nil {
            evidence = CatalogBrowseEvidence(
                scope: .liveNMPWindow,
                queryWasLocalFilter: !query.isEmpty,
                locallyFilteredRows: 0,
                projectedRows: 0,
                projectionLimitedRows: 0,
                refusedRows: 0,
                window: .requesting,
                sourceEvidence: [],
                shortfalls: []
            )
        }
        feedObservation = client.observeChanges { [weak self] in
            guard let self else {
                return
            }
            Task { @MainActor in
                await self.refreshFeed()
            }
        }
        await refreshFeed()
    }

    /// Stops only this view's bounded native fanout. The profile-owned NMP
    /// subscription remains open until the profile closes.
    public func stop() {
        feedGeneration &+= 1
        feedObservation?.cancel()
        feedObservation = nil
        cancelTransientWork()
    }

    public func search() async {
        await refreshFeed()
    }

    private func refreshFeed() async {
        guard let request = CatalogSearchRequest(query: query) else {
            // These two are the shell's own words, not a projected refusal,
            // so they say what to do rather than quoting a byte ceiling.
            issue = CatalogIssue(
                title: "Search is too long",
                message: "Try a shorter search."
            )
            return
        }

        feedGeneration &+= 1
        let generation = feedGeneration
        let response = await client.search(request)
        guard generation == feedGeneration else {
            return
        }

        switch response {
        case let .page(page):
            entries = page.entries
            hasMore = page.hasMore
            evidence = page.evidence
            if review == nil {
                issue = nil
            }
        case let .unavailable(problem):
            entries = []
            hasMore = false
            evidence = nil
            issue = problem
        }
    }

    public func review(entry: CatalogEntry) async {
        await resolveReview(.entryID(entry.id))
    }

    public func reviewManualCoordinate() async {
        cancelTransientWork()
        guard let request = CatalogManualCoordinateRequest(
            coordinate: manualCoordinate
        ) else {
            issue = CatalogIssue(
                title: "That address doesn't look right",
                message: "Check you copied the whole thing."
            )
            return
        }
        await resolveReview(
            .manualCoordinate(request),
            cancelCurrentWork: false
        )
    }

    public func cancelReview() {
        cancelTransientWork()
        review = nil
        issue = nil
    }

    @discardableResult
    public func confirmInstall() async -> CatalogInstalledBuild? {
        guard let review, review.canInstall, !isInstalling else {
            return nil
        }

        let generation = operationGeneration
        isInstalling = true
        issue = nil
        let response = await client.confirmExactVerifiedInstall(
            CatalogInstallConfirmation(review: review)
        )
        guard generation == operationGeneration else {
            return nil
        }
        isInstalling = false

        switch response {
        case let .installed(build):
            installedBuild = build
            self.review = nil
            onInstalled(build)
            return build
        case let .refused(problem):
            issue = problem
            return nil
        }
    }

    private func resolveReview(
        _ target: CatalogReviewTarget,
        cancelCurrentWork: Bool = true
    ) async {
        if cancelCurrentWork {
            cancelTransientWork()
        }
        let generation = operationGeneration
        isResolvingReview = true
        issue = nil
        let response = await client.resolveReview(target)
        guard generation == operationGeneration else {
            return
        }
        isResolvingReview = false

        switch response {
        case let .ready(review):
            self.review = review
        case let .unavailable(problem):
            review = nil
            issue = problem
        }
    }

    private func cancelTransientWork() {
        operationGeneration &+= 1
        isResolvingReview = false
        isInstalling = false
        client.cancelPendingCatalogWork()
    }
}
