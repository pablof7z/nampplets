import Testing
@testable import RuntimeWorkbenchFeature

/// What exhaustion means at the boundary a person sees: no further request is
/// launched, the refusal is visible, teardown stays idempotent, and the exact
/// generation survives next to the bounded copy.
///
/// The view model takes injectable starting generations precisely so this is
/// reachable without `UInt.max` iterations.
@MainActor
private final class CountingCatalogClient: CatalogClient {
    private(set) var searches = 0
    private(set) var reviews = 0
    private(set) var cancellations = 0

    func search(_ request: CatalogSearchRequest) async -> CatalogSearchResponse {
        _ = request
        searches += 1
        return .unavailable(
            CatalogIssue(title: "Unused", message: "Unused by this test.")
        )
    }

    func resolveReview(
        _ target: CatalogReviewTarget
    ) async -> CatalogReviewResponse {
        _ = target
        reviews += 1
        return .unavailable(
            CatalogIssue(title: "Unused", message: "Unused by this test.")
        )
    }

    func cancelPendingCatalogWork() {
        cancellations += 1
    }

    func confirmExactVerifiedInstall(
        _ confirmation: CatalogInstallConfirmation
    ) async -> CatalogInstallResponse {
        _ = confirmation
        return .refused(
            CatalogIssue(title: "Unused", message: "Unused by this test.")
        )
    }
}

@MainActor
private func model(
    client: CountingCatalogClient,
    feedStart: UInt = 0,
    operationStart: UInt = 0
) -> CatalogViewModel {
    CatalogViewModel(
        client: client,
        feedGenerationStart: feedStart,
        operationGenerationStart: operationStart
    )
}

@MainActor
@Test
func theLastValidFeedGenerationStillReachesTheClient() async {
    let client = CountingCatalogClient()
    let catalog = model(client: client, feedStart: UInt.max - 1)

    await catalog.search()

    #expect(client.searches == 1)
    #expect(catalog.feedGenerationExhaustion == nil)
}

@MainActor
@Test
func aFeedRequestPastTheLastGenerationNeverReachesTheClient() async {
    let client = CountingCatalogClient()
    let catalog = model(client: client, feedStart: UInt.max - 1)

    await catalog.search()
    await catalog.search()

    // The second search consumed the lane rather than being sent.
    #expect(client.searches == 1)
    #expect(catalog.feedGenerationExhaustion?.lane == .feed)
}

@MainActor
@Test
func feedExhaustionIsVisibleAndKeepsItsExactGeneration() async {
    let client = CountingCatalogClient()
    let catalog = model(client: client, feedStart: UInt.max)

    await catalog.search()

    let exhaustion = catalog.feedGenerationExhaustion
    #expect(exhaustion?.exhaustedGeneration == UInt.max)
    // Bounded copy for a person, exact evidence still available beside it.
    #expect(catalog.browseIssue != nil)
    #expect(exhaustion?.technicalDetail.contains("\(UInt.max)") == true)
}

@MainActor
@Test
func aTransientOperationPastItsLastGenerationIsRefusedAndCancelsPendingWork() async {
    let client = CountingCatalogClient()
    let catalog = model(client: client, operationStart: UInt.max)

    #expect(catalog.beginTransientOperation() == nil)
    #expect(catalog.operationGenerationExhaustion?.lane == .transientOperation)
    #expect(client.cancellations == 1)
    #expect(catalog.operationIssue != nil)
}

/// Exhaustion in one lane must not be reported as exhaustion in the other.
@MainActor
@Test
func feedAndTransientExhaustionStayIndependentlyAttributable() async {
    let client = CountingCatalogClient()
    let catalog = model(client: client, feedStart: UInt.max)

    await catalog.search()

    #expect(catalog.feedGenerationExhaustion != nil)
    #expect(catalog.operationGenerationExhaustion == nil)
}

@MainActor
@Test
func exhaustionIsRecordedOnceHoweverManyTimesItIsReached() async {
    let client = CountingCatalogClient()
    let catalog = model(client: client, operationStart: UInt.max)

    _ = catalog.beginTransientOperation()
    _ = catalog.beginTransientOperation()
    _ = catalog.beginTransientOperation()

    // One cancellation, not one per attempt: the terminal state is entered
    // once and re-entering it is a no-op.
    #expect(client.cancellations == 1)
    #expect(catalog.operationGenerationExhaustion?.exhaustedGeneration == UInt.max)
}

@MainActor
@Test
func stoppingAfterExhaustionIsIdempotent() async {
    let client = CountingCatalogClient()
    let catalog = model(client: client, feedStart: UInt.max, operationStart: UInt.max)

    await catalog.search()
    catalog.stop()
    catalog.stop()

    #expect(catalog.feedGenerationExhaustion?.exhaustedGeneration == UInt.max)
    #expect(client.searches == 0)
}

@MainActor
@Test
func noFurtherFeedRequestLaunchesAfterTerminalExhaustion() async {
    let client = CountingCatalogClient()
    let catalog = model(client: client, feedStart: UInt.max)

    await catalog.search()
    await catalog.search()
    await catalog.search()

    #expect(client.searches == 0)
}
