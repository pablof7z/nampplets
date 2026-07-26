import Testing
@testable import RuntimeWorkbenchFeature

@MainActor
private final class FakeActivitySubscription: ActivitySubscription {
    private(set) var isCancelled = false

    func cancel() {
        isCancelled = true
    }
}

@MainActor
private final class FakeActivitySource: ActivitySource {
    let initial: ActivitySnapshot
    var refreshed: ActivitySnapshot

    private(set) var subscribedScopes: [ActivityExactBuildScope] = []
    private(set) var refreshedScopes: [ActivityExactBuildScope] = []
    private(set) var subscription = FakeActivitySubscription()
    private var receiver: (@MainActor (ActivityUpdate) -> Void)?

    init(initial: ActivitySnapshot, refreshed: ActivitySnapshot? = nil) {
        self.initial = initial
        self.refreshed = refreshed ?? initial
    }

    func subscribe(
        to scope: ActivityExactBuildScope,
        receive: @escaping @MainActor (ActivityUpdate) -> Void
    ) -> any ActivitySubscription {
        subscribedScopes.append(scope)
        receiver = receive
        receive(.authoritative(initial))
        return subscription
    }

    func refresh(scope: ActivityExactBuildScope) throws -> ActivitySnapshot {
        refreshedScopes.append(scope)
        return refreshed
    }

    func push(_ update: ActivityUpdate) {
        receiver?(update)
    }
}

private let activityScope = ActivityExactBuildScope(
    manifestAuthor: "publisher-public-key",
    dTag: "good-morning",
    aggregateHash: "exact-aggregate-hash"
)!

private func activityFact(
    id: String,
    severity: ActivitySeverity,
    category: ActivityCategory,
    kind: ActivityFactKind,
    scope: ActivityExactBuildScope = activityScope,
    detailFields: [ActivityDetailField] = []
) -> ActivityFact {
    ActivityFact(
        id: id,
        scope: scope,
        ordinal: UInt64(id.utf8.count),
        severity: severity,
        category: category,
        kind: kind,
        title: "\(kind.title) title",
        summary: "\(category.title) summary",
        evidenceSummary: kind == .pendingReceipt
            ? "Write accepted; relay acknowledgement pending"
            : nil,
        detailFields: detailFields
    )!
}

private func activitySnapshot(
    revision: UInt64,
    facts: [ActivityFact],
    scope: ActivityExactBuildScope = activityScope
) -> ActivitySnapshot {
    ActivitySnapshot(
        scope: scope,
        revision: revision,
        inventory: ActivityInventorySummary(
            activeSessions: 1,
            activeBindings: 2,
            activeResources: 3,
            pendingReceipts: 1
        )!,
        facts: facts,
        omittedFactCount: 4
    )!
}

@MainActor
@Test func activityFilteringUsesRuntimeSeverityAndCategoryFacts() {
    let facts = [
        activityFact(
            id: "provider",
            severity: .information,
            category: .provider,
            kind: .providerCall
        ),
        activityFact(
            id: "refusal",
            severity: .warning,
            category: .provider,
            kind: .providerRefusal
        ),
        activityFact(
            id: "receipt",
            severity: .warning,
            category: .receipt,
            kind: .pendingReceipt
        ),
        activityFact(
            id: "crash",
            severity: .error,
            category: .recovery,
            kind: .crash
        ),
    ]
    let source = FakeActivitySource(
        initial: activitySnapshot(revision: 1, facts: facts)
    )
    let model = ActivityViewModel(
        source: source,
        scope: activityScope,
        developerModeAvailable: false
    )

    model.start()
    for severity in ActivitySeverity.allCases where severity != .warning {
        model.setSeverity(severity, isIncluded: false)
    }
    for category in ActivityCategory.allCases where category != .receipt {
        model.setCategory(category, isIncluded: false)
    }

    #expect(source.subscribedScopes == [activityScope])
    #expect(model.visibleFacts.map(\.id) == ["receipt"])

    model.stop()
    #expect(source.subscription.isCancelled)
}

@Test func activityModelsRejectUnboundedOrCrossBuildProjections() {
    let facts = (0...ActivityLimits.maximumFacts).map {
        activityFact(
            id: "fact-\($0)",
            severity: .information,
            category: .session,
            kind: .activeSession
        )
    }
    let otherScope = ActivityExactBuildScope(
        manifestAuthor: "other-publisher",
        dTag: "good-morning",
        aggregateHash: "other-build"
    )!
    let crossBuildFact = activityFact(
        id: "cross-build",
        severity: .error,
        category: .recovery,
        kind: .crash,
        scope: otherScope
    )
    let oversizedDetailValue = String(
        repeating: "x",
        count: ActivityLimits.maximumDetailValueUTF8Bytes + 1
    )

    #expect(
        ActivitySnapshot(
            scope: activityScope,
            revision: 1,
            inventory: .empty,
            facts: facts
        ) == nil
    )
    #expect(
        ActivitySnapshot(
            scope: activityScope,
            revision: 1,
            inventory: .empty,
            facts: [crossBuildFact]
        ) == nil
    )
    #expect(
        ActivityDetailField(
            key: "detail",
            value: .visible(oversizedDetailValue)
        ) == nil
    )
    #expect(
        ActivityInventorySummary(
            activeSessions: ActivityInventorySummary.maximumActiveSessions + 1,
            activeBindings: 0,
            activeResources: 0,
            pendingReceipts: 0
        ) == nil
    )
}

@MainActor
@Test func developerDetailIsGatedAndHonorsTheRuntimeClassification() {
    // The runtime classified `authorization` as secret, so its bytes never
    // reached this layer. It classified `token-relay` as public even though
    // the old substring heuristic would have redacted it.
    let fields = [
        ActivityDetailField(key: "authorization", value: .redacted)!,
        ActivityDetailField(
            key: "token-relay",
            value: .visible("wss://relay.example")
        )!,
    ]
    let fact = ActivityFact(
        id: "classified-fact",
        scope: activityScope,
        ordinal: 1,
        severity: .debug,
        category: .provider,
        kind: .providerCall,
        title: "Provider detail",
        summary: "Provider call completed",
        evidenceSummary: "Bearer token withheld by the runtime",
        detailFields: fields
    )!
    let source = FakeActivitySource(
        initial: activitySnapshot(revision: 1, facts: [fact])
    )
    let unavailableModel = ActivityViewModel(
        source: source,
        scope: activityScope,
        developerModeAvailable: false
    )
    let developerModel = ActivityViewModel(
        source: source,
        scope: activityScope,
        developerModeAvailable: true
    )

    unavailableModel.start()
    unavailableModel.setDeveloperModeEnabled(true)
    #expect(unavailableModel.detailFields(for: fact).isEmpty)

    developerModel.start()
    #expect(developerModel.detailFields(for: fact).isEmpty)
    developerModel.setDeveloperModeEnabled(true)
    #expect(
        developerModel.detailFields(for: fact).map(\.displayValue)
            == ["[REDACTED]", "wss://relay.example"]
    )
    #expect(developerModel.detailFields(for: fact).map(\.isRedacted)
        == [true, false])

    // A withheld value has no bytes to retain, and the runtime-owned display
    // strings are rendered exactly as the runtime produced them.
    #expect(!String(describing: fact).contains("nsec1"))
    #expect(fact.summary == "Provider call completed")
    #expect(fact.evidenceSummary == "Bearer token withheld by the runtime")
}

@MainActor
@Test func pushedRevisionGapStaysVisibleUntilExplicitRefresh() {
    let initial = activitySnapshot(
        revision: 10,
        facts: [
            activityFact(
                id: "initial",
                severity: .information,
                category: .session,
                kind: .activeSession
            )
        ]
    )
    let refreshed = activitySnapshot(
        revision: 13,
        facts: [
            activityFact(
                id: "refreshed",
                severity: .information,
                category: .recovery,
                kind: .recovery
            )
        ]
    )
    let source = FakeActivitySource(initial: initial, refreshed: refreshed)
    let model = ActivityViewModel(
        source: source,
        scope: activityScope,
        developerModeAvailable: false
    )

    model.start()
    source.push(
        .next(
            activitySnapshot(
                revision: 12,
                facts: [
                    activityFact(
                        id: "after-gap",
                        severity: .warning,
                        category: .provider,
                        kind: .providerRefusal
                    )
                ]
            ),
            predecessorRevision: 11,
            // A pure revision discontinuity: this observer missed a frame, but
            // the runtime evicted nothing.
            lostBeforeBatch: 0
        )
    )

    #expect(
        model.updateGap
            == ActivityUpdateGap(
                expectedPredecessorRevision: 10,
                receivedPredecessorRevision: 11,
                receivedRevision: 12,
                lostBeforeBatch: 0
            )
    )
    #expect(model.snapshot?.revision == 12)
    #expect(source.refreshedScopes.isEmpty)

    model.refresh()

    #expect(source.refreshedScopes == [activityScope])
    #expect(model.snapshot == refreshed)
    #expect(model.updateGap == nil)
}

@MainActor
@Test func activityDrawerBuildsWithFakeDataOnly() {
    let source = FakeActivitySource(
        initial: activitySnapshot(revision: 1, facts: [])
    )
    let view = ActivityDrawer(
        source: source,
        scope: activityScope,
        developerModeAvailable: true
    )

    #expect(String(describing: type(of: view)) == "ActivityDrawer")
}

/// The runtime can evict events the observer never saw while the revisions
/// line up perfectly — `cursor_was_stale` and `lost_before_batch > 0` are the
/// same fact in Rust, and neither implies a revision discontinuity.
///
/// The old adapter had no field to say that in, so it XORed the predecessor
/// revision with 1 to force the mismatch check to trip. The warning appeared,
/// but the banner renders the received predecessor as evidence, so the
/// evidence panel showed a revision the runtime never produced.
@MainActor
@Test func lossWithMatchingRevisionsWarnsWithoutFabricatingARevision() {
    let initial = activitySnapshot(revision: 10, facts: [])
    let source = FakeActivitySource(initial: initial, refreshed: initial)
    let model = ActivityViewModel(
        source: source,
        scope: activityScope,
        developerModeAvailable: false
    )

    model.start()
    source.push(
        .next(
            activitySnapshot(revision: 11, facts: []),
            // Agrees with the current revision: nothing about the revisions is
            // wrong. Only the runtime knows events were lost.
            predecessorRevision: 10,
            lostBeforeBatch: 7
        )
    )

    let gap = model.updateGap
    #expect(gap != nil)
    #expect(gap?.lostBeforeBatch == 7)
    // The true predecessor, not `10 ^ 1`.
    #expect(gap?.receivedPredecessorRevision == 10)
    #expect(gap?.expectedPredecessorRevision == 10)
    #expect(gap?.receivedRevision == 11)
}

/// The mirror case: revisions disagree, the runtime reports no loss. The
/// warning still belongs, but claiming events were lost would assert something
/// the runtime never said.
@MainActor
@Test func revisionDiscontinuityAloneReportsNoLostEvents() {
    let initial = activitySnapshot(revision: 10, facts: [])
    let source = FakeActivitySource(initial: initial, refreshed: initial)
    let model = ActivityViewModel(
        source: source,
        scope: activityScope,
        developerModeAvailable: false
    )

    model.start()
    source.push(
        .next(
            activitySnapshot(revision: 12, facts: []),
            predecessorRevision: 11,
            lostBeforeBatch: 0
        )
    )

    #expect(model.updateGap?.lostBeforeBatch == 0)
    #expect(model.updateGap?.receivedPredecessorRevision == 11)
}
