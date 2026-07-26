import Testing
@testable import RuntimeWorkbenchFeature

private let libraryExactBuild = WorkbenchLibraryExactBuild(
    manifestAuthor: String(repeating: "a", count: 64),
    dTag: "good-morning",
    aggregateHash: String(repeating: "b", count: 64)
)!

private let libraryWorkspace = WorkbenchLibraryWorkspace(
    id: "main",
    displayName: "Main"
)!

private func libraryBuild(
    availability: WorkbenchLibraryBuildAvailability
        = .sealedExactBytesReady,
    sessions: [WorkbenchLibrarySession] = [
        WorkbenchLibrarySession(id: 11, state: .running),
        WorkbenchLibrarySession(id: 12, state: .suspended),
    ],
    assignedWorkspaceIDs: [String] = ["main"]
) -> WorkbenchLibraryBuild {
    WorkbenchLibraryBuild(
        exactBuild: libraryExactBuild,
        title: "Good Morning",
        availability: availability,
        sessions: sessions,
        assignedWorkspaceIDs: assignedWorkspaceIDs
    )!
}

private func librarySnapshot(
    revision: UInt64 = 1,
    availability: WorkbenchLibraryServiceAvailability = .available,
    query: String = "",
    builds: [WorkbenchLibraryBuild]? = nil,
    refusals: [WorkbenchLibraryRefusal] = []
) -> WorkbenchLibrarySnapshot {
    WorkbenchLibrarySnapshot(
        revision: revision,
        availability: availability,
        filterQuery: query,
        totalInstalled: 1,
        builds: builds ?? [libraryBuild()],
        workspaces: [libraryWorkspace],
        refusals: refusals
    )!
}

@MainActor
private final class RecordingLibrarySubscription:
    WorkbenchLibrarySubscription
{
    private(set) var isCancelled = false

    func cancel() {
        isCancelled = true
    }
}

@MainActor
private final class RecordingLibraryManager: WorkbenchLibraryManaging {
    enum Action: Equatable {
        case filter(String)
        case suspend(UInt64)
        case resume(UInt64)
        case assign(WorkbenchLibraryExactBuild, String)
        case clear(WorkbenchLibraryExactBuild, String)
        case uninstall(WorkbenchLibraryExactBuild)
    }

    var currentSnapshot: WorkbenchLibrarySnapshot
    private(set) var actions: [Action] = []
    private(set) var subscription = RecordingLibrarySubscription()
    private var receiver: (@MainActor (WorkbenchLibraryUpdate) -> Void)?

    init(snapshot: WorkbenchLibrarySnapshot) {
        currentSnapshot = snapshot
    }

    func subscribe(
        receive: @escaping @MainActor (WorkbenchLibraryUpdate) -> Void
    ) -> any WorkbenchLibrarySubscription {
        receiver = receive
        receive(.authoritative(currentSnapshot))
        return subscription
    }

    func refresh() -> WorkbenchLibrarySnapshot {
        currentSnapshot
    }

    func setFilter(_ query: String) {
        actions.append(.filter(query))
    }

    func suspend(sessionID: UInt64) {
        actions.append(.suspend(sessionID))
    }

    func resume(sessionID: UInt64) {
        actions.append(.resume(sessionID))
    }

    func assign(
        _ exactBuild: WorkbenchLibraryExactBuild,
        toWorkspaceID workspaceID: String
    ) {
        actions.append(.assign(exactBuild, workspaceID))
    }

    func clearAssignment(
        _ exactBuild: WorkbenchLibraryExactBuild,
        fromWorkspaceID workspaceID: String
    ) {
        actions.append(.clear(exactBuild, workspaceID))
    }

    func uninstall(_ exactBuild: WorkbenchLibraryExactBuild) {
        actions.append(.uninstall(exactBuild))
    }

    func push(_ update: WorkbenchLibraryUpdate) {
        receiver?(update)
    }
}

@MainActor
@Test func unavailableLibraryManagerPublishesOneTruthfulSnapshot() {
    let manager = UnavailableWorkbenchLibraryManager(
        reason: "Typed installed-library projection is unavailable."
    )
    var updates: [WorkbenchLibraryUpdate] = []

    let subscription = manager.subscribe { update in
        updates.append(update)
    }
    defer { subscription.cancel() }

    #expect(
        updates == [
            .authoritative(manager.refresh()),
        ]
    )
    #expect(manager.refresh().revision == 0)
    #expect(manager.refresh().totalInstalled == 0)
    #expect(manager.refresh().builds.isEmpty)
    #expect(
        manager.refresh().availability.unavailableReason
            == "Typed installed-library projection is unavailable."
    )
}

@MainActor
@Test func unavailableLibraryManagerRejectsInvalidReasonWithoutLeakingIt() {
    let manager = UnavailableWorkbenchLibraryManager(
        reason: "invalid\nreason"
    )

    #expect(
        manager.refresh().availability.unavailableReason
            == UnavailableWorkbenchLibraryManager.defaultReason
    )
}

@Test func installedLibraryModelsRejectUnboundedAndInexactProjections() {
    let uppercaseAuthor = String(repeating: "A", count: 64)
    #expect(
        WorkbenchLibraryExactBuild(
            manifestAuthor: uppercaseAuthor,
            dTag: "good-morning",
            aggregateHash: String(repeating: "b", count: 64)
        ) == nil
    )

    let duplicateSession = WorkbenchLibrarySession(
        id: 11,
        state: .suspended
    )
    #expect(
        WorkbenchLibraryBuild(
            exactBuild: libraryExactBuild,
            title: "Good Morning",
            availability: .sealedExactBytesReady,
            sessions: [
                WorkbenchLibrarySession(id: 11, state: .running),
                duplicateSession,
            ],
            assignedWorkspaceIDs: []
        ) == nil
    )

    let tooManyBuilds = Array(
        repeating: libraryBuild(),
        count: WorkbenchLibraryLimits.maximumBuilds + 1
    )
    #expect(
        WorkbenchLibrarySnapshot(
            revision: 1,
            availability: .available,
            filterQuery: "",
            totalInstalled: UInt64(tooManyBuilds.count),
            builds: tooManyBuilds,
            workspaces: [libraryWorkspace]
        ) == nil
    )

    #expect(
        WorkbenchLibrarySnapshot(
            revision: 1,
            availability: .available,
            filterQuery: "",
            totalInstalled: 0,
            builds: [libraryBuild()],
            workspaces: [libraryWorkspace]
        ) == nil
    )

    #expect(
        WorkbenchLibrarySnapshot(
            revision: 1,
            availability: .available,
            filterQuery: "",
            totalInstalled: 1,
            builds: [
                libraryBuild(assignedWorkspaceIDs: ["missing-workspace"])
            ],
            workspaces: [libraryWorkspace]
        ) == nil
    )
}

@Test func installedRowsPreserveRuntimeAvailabilityAndLifecycleStates() {
    let metadataOnly = libraryBuild(
        availability: .metadataOnly,
        sessions: [],
        assignedWorkspaceIDs: []
    )
    let sealedReady = libraryBuild()

    #expect(metadataOnly.availability == .metadataOnly)
    #expect(metadataOnly.sessions.isEmpty)
    #expect(sealedReady.availability == .sealedExactBytesReady)
    #expect(sealedReady.sessions.map(\.state) == [.running, .suspended])
    #expect(sealedReady.assignedWorkspaceIDs == ["main"])
}

@MainActor
@Test func modelForwardsExactCommandsWithoutOptimisticMutation() {
    let initial = librarySnapshot()
    let manager = RecordingLibraryManager(snapshot: initial)
    let model = WorkbenchLibrarySheetModel(manager: manager)

    model.start()
    #expect(model.snapshot == initial)

    model.filterDraft = "morning"
    model.applyFilter()
    model.suspend(initial.builds[0].sessions[0])
    model.resume(initial.builds[0].sessions[1])
    model.assign(libraryExactBuild, to: libraryWorkspace)
    model.clearAssignment(libraryExactBuild, from: libraryWorkspace)
    model.uninstall(libraryExactBuild)

    #expect(
        manager.actions == [
            .filter("morning"),
            .suspend(11),
            .resume(12),
            .assign(libraryExactBuild, "main"),
            .clear(libraryExactBuild, "main"),
            .uninstall(libraryExactBuild),
        ]
    )
    #expect(model.snapshot == initial)

    model.stop()
    #expect(manager.subscription.isCancelled)
}

@MainActor
@Test func invalidLifecycleCommandsAndUnavailableServiceAreInert() {
    let unavailable = librarySnapshot(
        availability: .unavailable(
            reason: "The native runtime library boundary is not connected."
        )
    )
    let manager = RecordingLibraryManager(snapshot: unavailable)
    let model = WorkbenchLibrarySheetModel(manager: manager)

    model.start()
    model.applyFilter()
    model.suspend(
        WorkbenchLibrarySession(id: 12, state: .suspended)
    )
    model.resume(
        WorkbenchLibrarySession(id: 11, state: .running)
    )
    model.uninstall(libraryExactBuild)

    #expect(manager.actions.isEmpty)
    #expect(
        model.snapshot?.availability.unavailableReason
            == "The native runtime library boundary is not connected."
    )
}

@MainActor
@Test func pushedRefusalAndRevisionGapRemainVisibleUntilRefresh() {
    let initial = librarySnapshot(revision: 4)
    let refusal = WorkbenchLibraryRefusal(
        code: "not-installed",
        message: "The exact build is no longer installed.",
        occurredAtMillis: 100
    )!
    let refused = librarySnapshot(
        revision: 7,
        query: "morning",
        refusals: [refusal]
    )
    let refreshed = librarySnapshot(
        revision: 8,
        query: "morning",
        refusals: [refusal]
    )
    let manager = RecordingLibraryManager(snapshot: initial)
    let model = WorkbenchLibrarySheetModel(manager: manager)

    model.start()
    manager.push(.next(refused, predecessorRevision: 5, lostBeforeBatch: 0))

    #expect(model.snapshot == refused)
    #expect(
        model.updateGap == WorkbenchLibraryUpdateGap(
            expectedPredecessorRevision: 4,
            receivedPredecessorRevision: 5,
            receivedRevision: 7
        )
    )
    #expect(model.snapshot?.refusals.last == refusal)
    #expect(model.filterDraft == "morning")

    manager.currentSnapshot = refreshed
    model.refresh()
    #expect(model.snapshot == refreshed)
    #expect(model.updateGap == nil)
}

@MainActor
@Test func librarySheetBuildsWithInjectedManagerAndCanvasOpenContract() {
    let manager = RecordingLibraryManager(snapshot: librarySnapshot())
    let sheet = WorkbenchLibrarySheet(manager: manager, onOpen: { _ in })

    #expect(String(describing: type(of: sheet)) == "WorkbenchLibrarySheet")
    #expect(manager.actions.isEmpty)
}

/// The runtime evicted events the observer never saw. That is true whether or
/// not the replacement carries a newer revision, and the frame layer
/// deliberately re-delivers at the same one. Warning only about newer
/// snapshots would drop precisely the case this exists to catch.
@MainActor
@Test func aReportedLossWarnsEvenWhenTheRevisionDoesNotAdvance() {
    let initial = librarySnapshot(revision: 4, query: "")
    let manager = RecordingLibraryManager(snapshot: initial)
    let model = WorkbenchLibrarySheetModel(manager: manager)

    model.start()
    manager.push(
        .next(
            librarySnapshot(revision: 4, query: ""),
            predecessorRevision: 4,
            lostBeforeBatch: 6
        )
    )

    #expect(model.updateGap?.lostBeforeBatch == 6)
    // The revisions agreed; nothing about them is wrong and neither is
    // reported as suspect.
    #expect(model.updateGap?.expectedPredecessorRevision == 4)
    #expect(model.updateGap?.receivedPredecessorRevision == 4)
}

/// A revision discontinuity with no reported loss still warns, but must not
/// claim events were lost — the runtime never said so.
@MainActor
@Test func aRevisionDiscontinuityAloneClaimsNoLostEvents() {
    let initial = librarySnapshot(revision: 4, query: "")
    let manager = RecordingLibraryManager(snapshot: initial)
    let model = WorkbenchLibrarySheetModel(manager: manager)

    model.start()
    manager.push(
        .next(
            librarySnapshot(revision: 7, query: ""),
            predecessorRevision: 5,
            lostBeforeBatch: 0
        )
    )

    #expect(model.updateGap != nil)
    #expect(model.updateGap?.lostBeforeBatch == 0)
}
