import Foundation
import NMPNativeRuntimeApple
@testable import RuntimeWorkbenchFeature
import Testing

@MainActor
@Test func realProfileLibraryManagerUsesTheNativeObservation() throws {
    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent(
            "nmp-native-runtime-library-\(UUID().uuidString)",
            isDirectory: true
        )
    defer { try? FileManager.default.removeItem(at: root) }
    let profile = try WorkbenchRuntimeProfile.open(storageRoot: root)
    defer { profile.close() }
    let manager = RuntimeWorkbenchLibraryManager(profile: profile)
    var update: WorkbenchLibraryUpdate?

    let subscription = manager.subscribe {
        update = $0
    }
    defer { subscription.cancel() }

    guard case let .authoritative(snapshot) = update else {
        Issue.record("Expected the real native library replacement")
        return
    }
    #expect(snapshot.availability == .available)
    #expect(snapshot.totalInstalled == 0)
    #expect(snapshot.builds.isEmpty)
}

@MainActor
@Test func nativeLibraryManagerMechanicallyProjectsTheWholeSnapshot() throws {
    let native = RecordingNativeLibraryService(
        projection: nativeLibraryProjection(revision: 4)
    )
    let manager = RuntimeWorkbenchLibraryManager(native: native)
    let snapshot = manager.refresh()
    let build = try #require(snapshot.builds.first)

    #expect(snapshot.revision == 4)
    #expect(snapshot.availability == .available)
    #expect(snapshot.filterQuery == "morning")
    #expect(snapshot.totalInstalled == 1)
    #expect(snapshot.workspaces.map(\.id) == ["main"])
    #expect(snapshot.workspaces.map(\.displayName) == ["main"])
    #expect(build.exactBuild == workbenchLibraryExactBuild())
    #expect(build.availability == .sealedExactBytesReady)
    #expect(build.sessions.map(\.state) == [.running, .suspended])
    #expect(build.assignedWorkspaceIDs == ["main"])
    #expect(snapshot.refusals.map(\.code) == ["not-installed"])
    #expect(snapshot.refusals.map(\.message) == ["The previous build is gone."])
}

@MainActor
@Test func nativeLibraryCommandsForwardExactlyWithoutOptimisticMutation() {
    let native = RecordingNativeLibraryService(
        projection: nativeLibraryProjection(revision: 8)
    )
    let manager = RuntimeWorkbenchLibraryManager(native: native)
    let before = manager.refresh()
    let exactBuild = workbenchLibraryExactBuild()

    manager.setFilter("gm")
    manager.suspend(sessionID: 7)
    manager.resume(sessionID: 8)
    manager.assign(exactBuild, toWorkspaceID: "main")
    manager.clearAssignment(exactBuild, fromWorkspaceID: "main")
    manager.uninstall(exactBuild)

    #expect(
        native.actions == [
            .filter("gm"),
            .suspend(7),
            .resume(8),
            .assign(nativeLibraryExactBuild(), "main"),
            .clear(nativeLibraryExactBuild(), "main"),
            .uninstall(nativeLibraryExactBuild()),
        ]
    )
    #expect(manager.refresh() == before)
}

@MainActor
@Test func projectionRefusalAndClosedProfileReplaceTheWholeSnapshot() {
    let refusal = RecordingNativeLibraryService(
        projection: .refused(
            revision: 12,
            profileClosed: false,
            refusal: .filterTooLarge(actualUTF8Bytes: 257, maximum: 256)
        )
    )
    let refusedManager = RuntimeWorkbenchLibraryManager(native: refusal)
    let refused = refusedManager.refresh()

    #expect(refused.revision == 12)
    #expect(!refused.availability.isAvailable)
    #expect(
        refused.availability.unavailableReason?.contains(
            "projection was refused"
        ) == true
    )
    #expect(refused.totalInstalled == 0)
    #expect(refused.builds.isEmpty)
    #expect(refused.workspaces.isEmpty)
    #expect(refused.refusals.isEmpty)

    let closed = RecordingNativeLibraryService(
        projection: nativeLibraryProjection(
            revision: 13,
            profileClosed: true
        )
    )
    let closedManager = RuntimeWorkbenchLibraryManager(native: closed)
    let closedSnapshot = closedManager.refresh()

    #expect(closedSnapshot.revision == 13)
    #expect(
        closedSnapshot.availability.unavailableReason
            == "The native runtime profile is closed."
    )
    #expect(closedSnapshot.builds.isEmpty)
    #expect(closedSnapshot.workspaces.isEmpty)
}

@MainActor
@Test func observerAdmissionFailureRemainsExplicitAcrossRefresh() {
    let native = RecordingNativeLibraryService(
        projection: nativeLibraryProjection(revision: 20),
        observationError: .refused
    )
    let manager = RuntimeWorkbenchLibraryManager(native: native)

    let initial = manager.refresh()
    let refreshed = manager.refresh()

    #expect(!initial.availability.isAvailable)
    #expect(
        initial.availability.unavailableReason?.contains(
            "observation was refused"
        ) == true
    )
    #expect(refreshed.availability == initial.availability)
    #expect(refreshed.builds.isEmpty)
}

@MainActor
@Test func nativeUpdatesCoalesceIntoOneMainActorReplacement() async {
    let native = RecordingNativeLibraryService(
        projection: nativeLibraryProjection(revision: 1)
    )
    let manager = RuntimeWorkbenchLibraryManager(native: native)
    var updates: [WorkbenchLibraryUpdate] = []
    let subscription = manager.subscribe {
        updates.append($0)
    }
    defer { subscription.cancel() }

    native.push(
        .next(
            nativeLibraryProjection(revision: 2),
            predecessorRevision: 1,
            eventCursorWasStale: false,
            lostBeforeBatch: 0
        )
    )
    native.push(
        .next(
            nativeLibraryProjection(revision: 3),
            predecessorRevision: 2,
            eventCursorWasStale: false,
            lostBeforeBatch: 0
        )
    )
    await drainMainQueue()

    #expect(updates.count == 2)
    guard case let .authoritative(initial) = updates[0] else {
        Issue.record("Expected the synchronous authoritative replacement")
        return
    }
    #expect(initial.revision == 1)
    guard case let .next(latest, predecessorRevision, _) = updates[1] else {
        Issue.record("Expected one coalesced pushed replacement")
        return
    }
    #expect(latest.revision == 3)
    #expect(predecessorRevision == 2)
}

@MainActor
@Test func newerNextCannotBeReplacedByLaterStaleAuthoritativeUpdate() async {
    let native = RecordingNativeLibraryService(
        projection: nativeLibraryProjection(revision: 1)
    )
    let manager = RuntimeWorkbenchLibraryManager(native: native)
    var updates: [WorkbenchLibraryUpdate] = []
    let subscription = manager.subscribe {
        updates.append($0)
    }
    defer { subscription.cancel() }

    native.push(
        .next(
            nativeLibraryProjection(revision: 2),
            predecessorRevision: 1,
            eventCursorWasStale: false,
            lostBeforeBatch: 0
        )
    )
    native.push(
        .authoritative(nativeLibraryProjection(revision: 1))
    )
    await drainMainQueue()

    #expect(updates.count == 2)
    guard case let .next(latest, predecessorRevision, _) = updates.last else {
        Issue.record("Expected the newer next replacement to win")
        return
    }
    #expect(latest.revision == 2)
    #expect(predecessorRevision == 1)
    #expect(manager.refresh().revision == 2)
}

@MainActor
@Test func sameRevisionNextKeepsGapMetadataOverAuthoritativeUpdate() async {
    let native = RecordingNativeLibraryService(
        projection: nativeLibraryProjection(revision: 1)
    )
    let manager = RuntimeWorkbenchLibraryManager(native: native)
    var updates: [WorkbenchLibraryUpdate] = []
    let subscription = manager.subscribe {
        updates.append($0)
    }
    defer { subscription.cancel() }

    native.push(
        .next(
            nativeLibraryProjection(revision: 2),
            predecessorRevision: 9,
            eventCursorWasStale: true,
            lostBeforeBatch: 3
        )
    )
    native.push(
        .authoritative(nativeLibraryProjection(revision: 2))
    )
    await drainMainQueue()

    guard case let .next(latest, predecessorRevision, _) = updates.last else {
        Issue.record("Expected same-revision next precedence")
        return
    }
    #expect(latest.revision == 2)
    #expect(predecessorRevision == 9)
}

@MainActor
@Test func refreshCannotBeRegressedByAnOlderQueuedNextUpdate() async {
    let native = RecordingNativeLibraryService(
        projection: nativeLibraryProjection(revision: 1)
    )
    let manager = RuntimeWorkbenchLibraryManager(native: native)
    var updates: [WorkbenchLibraryUpdate] = []
    let subscription = manager.subscribe {
        updates.append($0)
    }
    defer { subscription.cancel() }

    native.push(
        .next(
            nativeLibraryProjection(revision: 2),
            predecessorRevision: 1,
            eventCursorWasStale: false,
            lostBeforeBatch: 0
        )
    )
    native.currentProjection = nativeLibraryProjection(revision: 3)
    #expect(manager.refresh().revision == 3)
    await drainMainQueue()

    #expect(updates.count == 1)
    var replacementRevision: UInt64?
    let replacement = manager.subscribe { update in
        guard case let .authoritative(snapshot) = update else {
            Issue.record("Expected an authoritative subscription replacement")
            return
        }
        replacementRevision = snapshot.revision
    }
    defer { replacement.cancel() }
    #expect(replacementRevision == 3)
}

@MainActor
@Test func librarySubscriberFanoutIsBoundedAndCancellationReleasesCapacity()
    async
{
    let native = RecordingNativeLibraryService(
        projection: nativeLibraryProjection(revision: 1)
    )
    let manager = RuntimeWorkbenchLibraryManager(native: native)
    var deliveryCounts = Array(repeating: 0, count: 17)
    var subscriptions: [any WorkbenchLibrarySubscription] = []
    for index in 0..<16 {
        subscriptions.append(
            manager.subscribe { _ in
                deliveryCounts[index] += 1
            }
        )
    }
    let refused = manager.subscribe { _ in
        deliveryCounts[16] += 1
    }

    #expect(
        manager.latestAdmissionRefusal
            == .subscriberCapacity(maximum: 16)
    )
    #expect(deliveryCounts == Array(repeating: 1, count: 17))

    subscriptions.removeFirst().cancel()
    let replacement = manager.subscribe { _ in
        deliveryCounts[0] += 1
    }
    #expect(deliveryCounts[0] == 2)
    native.push(
        .next(
            nativeLibraryProjection(revision: 2),
            predecessorRevision: 1,
            eventCursorWasStale: false,
            lostBeforeBatch: 0
        )
    )
    await drainMainQueue()

    #expect(deliveryCounts[0] == 3)
    #expect(deliveryCounts[16] == 1)
    refused.cancel()
    replacement.cancel()
    for subscription in subscriptions {
        subscription.cancel()
    }
}

private enum RecordingNativeLibraryError: Error {
    case refused
}

private final class RecordingNativeLibraryObservation:
    RuntimeWorkbenchNativeLibraryObservation,
    @unchecked Sendable
{
    private(set) var isCancelled = false

    func cancel() {
        isCancelled = true
    }
}

private final class RecordingNativeLibraryService:
    RuntimeWorkbenchNativeLibraryService,
    @unchecked Sendable
{
    enum Action: Equatable {
        case filter(String)
        case suspend(UInt64)
        case resume(UInt64)
        case assign(NativeRuntimeLibraryExactBuild, String)
        case clear(NativeRuntimeLibraryExactBuild, String)
        case uninstall(NativeRuntimeLibraryExactBuild)
    }

    var currentProjection: NativeRuntimeLibraryProjection
    let observationError: RecordingNativeLibraryError?
    private(set) var observation = RecordingNativeLibraryObservation()
    private(set) var actions: [Action] = []
    private var receiver:
        (@Sendable (NativeRuntimeLibraryUpdate) -> Void)?

    init(
        projection: NativeRuntimeLibraryProjection,
        observationError: RecordingNativeLibraryError? = nil
    ) {
        currentProjection = projection
        self.observationError = observationError
    }

    func projection() -> NativeRuntimeLibraryProjection {
        currentProjection
    }

    func observe(
        _ receive: @escaping @Sendable (NativeRuntimeLibraryUpdate) -> Void
    ) throws -> any RuntimeWorkbenchNativeLibraryObservation {
        if let observationError {
            throw observationError
        }
        receiver = receive
        return observation
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
        _ exactBuild: NativeRuntimeLibraryExactBuild,
        toWorkspaceID workspaceID: String
    ) {
        actions.append(.assign(exactBuild, workspaceID))
    }

    func clearAssignment(
        _ exactBuild: NativeRuntimeLibraryExactBuild,
        fromWorkspaceID workspaceID: String
    ) {
        actions.append(.clear(exactBuild, workspaceID))
    }

    func uninstall(_ exactBuild: NativeRuntimeLibraryExactBuild) {
        actions.append(.uninstall(exactBuild))
    }

    func push(_ update: NativeRuntimeLibraryUpdate) {
        receiver?(update)
    }
}

private func nativeLibraryProjection(
    revision: UInt64,
    profileClosed: Bool = false
) -> NativeRuntimeLibraryProjection {
    .snapshot(
        NativeRuntimeLibrarySnapshot(
            revision: revision,
            profileClosed: profileClosed,
            filterQuery: "morning",
            totalInstalled: 1,
            builds: [
                NativeRuntimeLibraryBuild(
                    exactBuild: nativeLibraryExactBuild(),
                    title: "Good Morning",
                    availability: .sealedExactBytesReady,
                    sessions: [
                        NativeRuntimeLibrarySession(
                            id: 7,
                            state: .running
                        ),
                        NativeRuntimeLibrarySession(
                            id: 8,
                            state: .suspended
                        ),
                    ],
                    assignedWorkspaceIDs: ["main"]
                ),
            ],
            workspaces: [
                NativeRuntimeLibraryWorkspace(id: "main"),
            ],
            refusals: [
                NativeRuntimeLibraryRefusal(
                    code: "not-installed",
                    detail: "The previous build is gone.",
                    occurredAtMillis: 9
                ),
            ]
        )
    )
}

private func nativeLibraryExactBuild()
    -> NativeRuntimeLibraryExactBuild
{
    NativeRuntimeLibraryExactBuild(
        manifestAuthor: String(repeating: "a", count: 64),
        dTag: "good-morning",
        aggregateHash: String(repeating: "b", count: 64)
    )
}

private func workbenchLibraryExactBuild() -> WorkbenchLibraryExactBuild {
    WorkbenchLibraryExactBuild(
        manifestAuthor: String(repeating: "a", count: 64),
        dTag: "good-morning",
        aggregateHash: String(repeating: "b", count: 64)
    )!
}

@MainActor
private func drainMainQueue() async {
    await withCheckedContinuation { continuation in
        DispatchQueue.main.async {
            continuation.resume()
        }
    }
}

/// The frame layer re-delivers a library replacement at the *same* revision
/// when the event cursor was stale — that is what `isCurrentStaleReplacement`
/// exists for. The manager used to discard `eventCursorWasStale` outright and
/// return early on any non-advancing revision, so the frame layer's deliberate
/// warning was thrown away before it reached anyone.
@MainActor
@Test func aStaleReplacementAtTheSameRevisionStillReachesSubscribers() async {
    let native = RecordingNativeLibraryService(
        projection: nativeLibraryProjection(revision: 1)
    )
    let manager = RuntimeWorkbenchLibraryManager(native: native)
    var updates: [WorkbenchLibraryUpdate] = []
    let subscription = manager.subscribe { updates.append($0) }
    defer { subscription.cancel() }

    native.push(
        .next(
            nativeLibraryProjection(revision: 1),
            predecessorRevision: 1,
            eventCursorWasStale: true,
            lostBeforeBatch: 4
        )
    )
    await drainMainQueue()

    guard case let .next(_, _, lostBeforeBatch) = updates.last else {
        Issue.record("The stale replacement was swallowed by the freshness guard")
        return
    }
    #expect(lostBeforeBatch == 4)
}

/// The mirror: a non-advancing revision with nothing lost carries no news, and
/// must stay suppressed. Forwarding it would turn the fix into a noise source.
@MainActor
@Test func aNonAdvancingReplacementWithNoLossStaysSuppressed() async {
    let native = RecordingNativeLibraryService(
        projection: nativeLibraryProjection(revision: 1)
    )
    let manager = RuntimeWorkbenchLibraryManager(native: native)
    var updates: [WorkbenchLibraryUpdate] = []
    let subscription = manager.subscribe { updates.append($0) }
    defer { subscription.cancel() }

    native.push(
        .next(
            nativeLibraryProjection(revision: 1),
            predecessorRevision: 1,
            eventCursorWasStale: false,
            lostBeforeBatch: 0
        )
    )
    await drainMainQueue()

    #expect(updates.count == 1)
    guard case .authoritative = updates[0] else {
        Issue.record("Expected only the initial authoritative replacement")
        return
    }
}
