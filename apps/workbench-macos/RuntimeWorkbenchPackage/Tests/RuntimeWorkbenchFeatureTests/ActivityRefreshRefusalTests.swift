@testable import RuntimeWorkbenchFeature
import Testing

@MainActor
private final class RefusingActivitySource: ActivitySource {
    let initial: ActivitySnapshot
    let refusal: RuntimeWorkbenchActivitySourceRefusal

    init(
        initial: ActivitySnapshot,
        refusal: RuntimeWorkbenchActivitySourceRefusal
    ) {
        self.initial = initial
        self.refusal = refusal
    }

    func subscribe(
        to _: ActivityExactBuildScope,
        receive: @escaping @MainActor (ActivityUpdate) -> Void
    ) -> any ActivitySubscription {
        receive(.authoritative(initial))
        return RefusingActivitySubscription()
    }

    func refresh(scope _: ActivityExactBuildScope) throws -> ActivitySnapshot {
        throw refusal
    }
}

@MainActor
private final class RefusingActivitySubscription: ActivitySubscription {
    func cancel() {}
}

@MainActor
@Test func refusedRefreshNeverRelabelsTheCachedSnapshotAsAuthoritative() throws {
    let scope = try #require(
        ActivityExactBuildScope(
            manifestAuthor: "publisher",
            dTag: "component",
            aggregateHash: "aggregate"
        )
    )
    let initial = try #require(
        ActivitySnapshot(
            scope: scope,
            revision: 7,
            inventory: .empty,
            facts: []
        )
    )
    let refusal = RuntimeWorkbenchActivitySourceRefusal.snapshotRefused(
        code: "snapshot-integrity-missing-build-session",
        detail: "build references session 42, but it is absent"
    )
    let source = RefusingActivitySource(initial: initial, refusal: refusal)
    let model = ActivityViewModel(
        source: source,
        scope: scope,
        developerModeAvailable: false
    )

    model.start()
    model.refresh()

    #expect(model.snapshot == initial)
    #expect(model.refreshRefusal == refusal)
}
