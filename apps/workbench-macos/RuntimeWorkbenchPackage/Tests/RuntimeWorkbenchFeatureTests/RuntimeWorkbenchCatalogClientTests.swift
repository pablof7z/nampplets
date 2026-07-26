import CryptoKit
import Testing
@testable import RuntimeWorkbenchFeature

private let goodMorningEntryID =
    "published:b330bfaefd2ddf268ebe4196403e6163533c54f41dabc3518bdc1a896c68f40e"
private let goodMorningAuthor =
    "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5"
private let goodMorningAggregate =
    "828a6df02afd56782ea20f805084acce65c53f7c37554948c1e0a64aa5a2b0a8"

@MainActor
private func searchPage(
    _ client: RuntimeWorkbenchCatalogClient,
    query: String = ""
) async throws -> CatalogSearchPage {
    let response = await client.search(CatalogSearchRequest(query: query)!)
    guard case let .page(page) = response else {
        Issue.record("Expected a bundled catalog page, got \(response)")
        throw CatalogClientTestError.expectedPage
    }
    return page
}

@MainActor
@Test func bundledIndexesRemainExactPinnedCorpusCopies() throws {
    let expected = [
        "published":
            "c3743dba3b8d719ba9f5041bae01db22a5c221449ffefded2cf0fe08c8f9cb74",
        "reference":
            "5cc6f85dca4a3db7ef63ce7d44f7bc18d2a233744e82bd3c2c4ac4b2e883685f",
        "kehto":
            "8070424a466a98c729ddf1885a3a76b8ae8f50d16b89e636e4f23b10de79c603",
    ]

    for (name, expectedDigest) in expected {
        let data = try RuntimeWorkbenchCatalogClient.bundledIndexData(
            named: name
        )
        let digest = SHA256.hash(data: data)
            .map { String(format: "%02x", $0) }
            .joined()
        #expect(digest == expectedDigest)
    }
}

@MainActor
@Test func defaultClientDoesNotMasqueradeBundledFixturesAsLiveNetwork() async {
    let response = await RuntimeWorkbenchCatalogClient().search(
        CatalogSearchRequest(query: "")!
    )

    guard case let .unavailable(issue) = response else {
        Issue.record("A profile-less catalog unexpectedly returned fixtures")
        return
    }
    #expect(issue.title == "Live catalog unavailable")
}

@MainActor
@Test func emptySearchListsEveryPinnedCorpusEntryWithoutPagination() async throws {
    let page = try await searchPage(
        RuntimeWorkbenchCatalogClient.offlineFixture()
    )

    #expect(page.entries.count == 20)
    #expect(!page.hasMore)
    #expect(page.evidence.scope == .offlineFixture)
    #expect(page.evidence.projectedRows == 20)
    #expect(page.entries.first?.id == goodMorningEntryID)
    #expect(
        page.entries.contains {
            $0.id
                == "reference:external-assets:"
                + "0136a6481a347a856d877c8729650222cc6ca8110095f35a9f2bd016b3534d81"
        }
    )
    #expect(
        page.entries.contains {
            $0.id
                == "kehto:feed:"
                + "8a146f7511a4fc887cecc0f29ccbf4871234bc56"
        }
    )
}

@MainActor
@Test func searchIsLocalLiteralAndSurfacesBuiltNotRunStatus() async throws {
    let page = try await searchPage(
        RuntimeWorkbenchCatalogClient.offlineFixture(),
        query: "resource"
    )

    // #223 removed the hardcoded "Good Morning Protocol" from
    // `publishedRecord` and derives every catalog title from the fixture's own
    // name via `displayCatalogTitle`, exactly as the neighbouring
    // "Resource Demo" entry already was. So "good-morning" reads as
    // "Good Morning"; the product is right and this assertion was stale.
    #expect(page.entries.map(\.title) == ["Good Morning", "Resource Demo"])
    #expect(page.evidence.scope == .offlineFixture)
    #expect(page.evidence.queryWasLocalFilter)
    #expect(page.evidence.locallyFilteredRows == 18)
    let demo = try #require(
        page.entries.first(where: { $0.title == "Resource Demo" })
    )
    guard case let .incompatible(reason) = demo.compatibility else {
        Issue.record("Expected Kehto source entry to be incompatible")
        return
    }
    #expect(reason.contains("Built, not run"))
    #expect(reason.contains("resource, theme"))
}

@MainActor
@Test func goodMorningReviewPinsIdentityBuildCapabilitiesAndPlatforms() async throws {
    let client = RuntimeWorkbenchCatalogClient.offlineFixture()
    let page = try await searchPage(client, query: "good morning")
    let entry = try #require(page.entries.first)

    #expect(entry.id == goodMorningEntryID)
    #expect(entry.publisher.displayName == nil)
    #expect(entry.publisher.publicKey == goodMorningAuthor)
    #expect(entry.coordinate == "35129:\(goodMorningAuthor):good-morning")
    guard case let .incompatible(reason) = entry.compatibility else {
        Issue.record("Expected current macOS compatibility to remain incompatible")
        return
    }
    #expect(reason.contains("advertises no macOS NAP domains"))

    let response = await client.resolveReview(.entryID(entry.id))
    guard case let .ready(review) = response else {
        Issue.record("Expected exact bundled review, got \(response)")
        return
    }

    #expect(review.publisher.publicKey == goodMorningAuthor)
    #expect(review.coordinate == "35129:\(goodMorningAuthor):good-morning")
    #expect(review.exactAggregateHash == goodMorningAggregate)
    #expect(review.requiredDomains == ["identity", "inc", "outbox"])
    #expect(review.optionalDomains == ["resource", "theme", "link"])
    #expect(review.sources.map(\.kind) == [
        .approvedCatalog,
        .manifestEvent,
        .verifiedArtifactIndex,
        .artifact,
    ])
    #expect(
        review.sources.contains {
            $0.evidence.contains(
                "ffd35eea5c84d03cdda74c23e1bbb2c40500f503833503aa688036faa52f3808"
            )
        }
    )
    #expect(
        review.platformCompatibility.map(\.status)
            == [.incompatible, .unavailable, .unavailable]
    )
    #expect(review.warnings.map(\.severity) == [.caution, .caution, .blocking])
    #expect(!review.canInstall)
}

@MainActor
@Test func referenceAndKehtoEntriesCannotMasqueradeAsInstallableBuilds() async throws {
    let client = RuntimeWorkbenchCatalogClient.offlineFixture()
    let page = try await searchPage(client)
    let externalAssets = try #require(
        page.entries.first { $0.id.hasPrefix("reference:external-assets:") }
    )
    let feed = try #require(
        page.entries.first { $0.id.hasPrefix("kehto:feed:") }
    )

    let referenceResponse = await client.resolveReview(
        .entryID(externalAssets.id)
    )
    let kehtoResponse = await client.resolveReview(.entryID(feed.id))

    guard case let .unavailable(referenceIssue) = referenceResponse,
          case let .unavailable(kehtoIssue) = kehtoResponse
    else {
        Issue.record("Non-published corpus entries returned an install review")
        return
    }
    #expect(referenceIssue.message.contains("native artifact URL scheme"))
    #expect(kehtoIssue.message.contains("Built, not run"))
}

@MainActor
@Test func manualResolutionAndInstallConfirmationStayTruthfullyUnavailable() async throws {
    let client = RuntimeWorkbenchCatalogClient.offlineFixture()
    let manual = await client.resolveReview(
        .manualCoordinate(
            CatalogManualCoordinateRequest(
                coordinate: "35129:\(goodMorningAuthor):good-morning"
            )!
        )
    )
    guard case let .unavailable(manualIssue) = manual else {
        Issue.record("Manual coordinate unexpectedly resolved")
        return
    }
    #expect(manualIssue.title == "Offline coordinate resolution unavailable")

    let page = try await searchPage(client, query: "good morning")
    let entry = try #require(page.entries.first)
    let reviewResponse = await client.resolveReview(.entryID(entry.id))
    guard case let .ready(review) = reviewResponse else {
        Issue.record("Expected bundled Good Morning review")
        return
    }
    let install = await client.confirmExactVerifiedInstall(
        CatalogInstallConfirmation(review: review)
    )
    guard case let .refused(installIssue) = install else {
        Issue.record("Read-only catalog unexpectedly installed a build")
        return
    }
    #expect(installIssue.title == "Installation unavailable")
    #expect(installIssue.message.contains(goodMorningAggregate))
    #expect(installIssue.message.contains("offline UI-test corpus"))
}

@MainActor
@Test func contentViewAcceptsCatalogClientForToolbarSheetIntegration() async {
    let view = ContentView(
        catalogClient: RuntimeWorkbenchCatalogClient.offlineFixture()
    )
    #expect(String(describing: type(of: view)) == "ContentView")
}

@MainActor
@Test func profileBackedClientForwardsLiveOperationsAndCancellation() async {
    let backing = FakeCatalogProfileBacking()
    let client = RuntimeWorkbenchCatalogClient(profileBacking: backing)
    let request = CatalogSearchRequest(query: "clock")!
    let coordinate = CatalogManualCoordinateRequest(
        coordinate: "35129:publisher:clock"
    )!

    let search = await client.search(request)
    let review = await client.resolveReview(.manualCoordinate(coordinate))
    client.cancelPendingCatalogWork()
    let install = await client.confirmExactVerifiedInstall(
        CatalogInstallConfirmation(review: backing.review)
    )

    #expect(search == .page(backing.page))
    #expect(review == .ready(backing.review))
    #expect(install == .installed(backing.installed))
    #expect(backing.searches == [request])
    #expect(backing.reviewTargets == [.manualCoordinate(coordinate)])
    #expect(backing.confirmations == [
        CatalogInstallConfirmation(review: backing.review)
    ])
    #expect(backing.cancellations == 1)
}

@MainActor
private final class FakeCatalogProfileBacking:
    RuntimeWorkbenchCatalogProfileBacking
{
    let review: CatalogInstallReview
    let installed: CatalogInstalledBuild
    let page: CatalogSearchPage

    private(set) var searches: [CatalogSearchRequest] = []
    private(set) var reviewTargets: [CatalogReviewTarget] = []
    private(set) var confirmations: [CatalogInstallConfirmation] = []
    private(set) var cancellations = 0

    init() {
        let author = String(repeating: "c", count: 64)
        let aggregate = String(repeating: "d", count: 64)
        let publisher = CatalogPublisher(
            displayName: "Network publisher",
            publicKey: author
        )
        let entry = CatalogEntry(
            id: "event",
            title: "Clock",
            summary: "A live candidate",
            publisher: publisher,
            coordinate: "35129:\(author):clock",
            compatibility: .unreviewed
        )!
        let source = CatalogBrowseSourceEvidence(
            id: "relay",
            source: "wss://relay.example",
            access: .public,
            status: .connecting,
            reconciledThrough: 42
        )!
        let evidence = CatalogBrowseEvidence(
            scope: .liveNMPWindow,
            queryWasLocalFilter: true,
            locallyFilteredRows: 3,
            projectedRows: 1,
            projectionLimitedRows: 4,
            refusedRows: 2,
            window: .atBound(maximumRows: 512),
            sourceEvidence: [source],
            shortfalls: [.localLimit]
        )!
        page = CatalogSearchPage(
            entries: [entry],
            hasMore: true,
            evidence: evidence
        )!
        review = CatalogInstallReview(
            id: "frozen-review",
            title: "Clock",
            publisher: publisher,
            coordinate: entry.coordinate,
            exactAggregateHash: aggregate,
            sources: [],
            requiredDomains: [],
            optionalDomains: [],
            platformCompatibility: [],
            warnings: [],
            updateRelationship: .firstInstall,
            canInstall: true
        )!
        installed = CatalogInstalledBuild(
            title: review.title,
            manifestAuthor: publisher.publicKey,
            dTag: "clock",
            exactAggregateHash: review.exactAggregateHash
        )!
    }

    func browseCatalog(
        _ request: CatalogSearchRequest
    ) async -> CatalogSearchResponse {
        searches.append(request)
        return .page(page)
    }

    func resolveCatalogReview(
        _ target: CatalogReviewTarget
    ) async -> CatalogReviewResponse {
        reviewTargets.append(target)
        return .ready(review)
    }

    func cancelCatalogWork() {
        cancellations += 1
    }

    func installCatalogReview(
        _ confirmation: CatalogInstallConfirmation
    ) async -> CatalogInstallResponse {
        confirmations.append(confirmation)
        return .installed(installed)
    }
}

private enum CatalogClientTestError: Error {
    case expectedPage
}
