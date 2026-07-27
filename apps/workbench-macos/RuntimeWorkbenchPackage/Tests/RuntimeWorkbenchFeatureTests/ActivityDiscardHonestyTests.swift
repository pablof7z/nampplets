@testable import RuntimeWorkbenchFeature
import Testing

/// `omittedFactCount` and `runtimeDiscardedCount` answer different questions:
/// "this app cannot render these yet" versus "the runtime destroyed these".
/// Summing them, or letting one stand in for the other, would turn a
/// recoverable presentation gap into a claim about data that no longer exists.
@Test func omittedAndDiscardedCountsStaySeparateFacts() throws {
    let snapshot = try #require(
        ActivitySnapshot(
            scope: discardScope,
            revision: 4,
            inventory: .empty,
            facts: [],
            omittedFactCount: 9,
            runtimeDiscardedCount: 12
        )
    )

    #expect(snapshot.omittedFactCount == 9)
    #expect(snapshot.runtimeDiscardedCount == 12)
}

/// A snapshot built without the count must claim nothing was discarded rather
/// than inheriting a number from elsewhere.
@Test func discardedCountDefaultsToZeroRatherThanGuessing() throws {
    let snapshot = try #require(
        ActivitySnapshot(
            scope: discardScope,
            revision: 1,
            inventory: .empty,
            facts: [],
            omittedFactCount: 3
        )
    )

    #expect(snapshot.runtimeDiscardedCount == 0)
}

/// The runtime's rings are not partitioned by exact build, so the count cannot
/// be attributed to the napplet whose drawer is open. The copy has to say so.
/// This guards the wording specifically, because "12 entries were discarded"
/// shown inside one napplet's drawer reads as a claim about that napplet, and
/// shortening the sentence is the obvious future tidy-up.
@Test func discardCopyRefusesToAttributeTheLossToOneNapplet() {
    let copy = ActivityPlainPresentation.runtimeDiscarded

    #expect(copy.contains("across all napplets"))
    // Refreshing cannot recover evicted entries, unlike the observer-gap
    // banner, which does offer a Refresh button.
    #expect(copy.contains("will not bring them back"))
}

private let discardScope = ActivityExactBuildScope(
    manifestAuthor: String(repeating: "a", count: 64),
    dTag: "good-morning",
    aggregateHash: String(repeating: "b", count: 64)
)!
