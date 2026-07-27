import Foundation
import NMPNativeRuntime
@testable import NMPNativeRuntimeApple
import XCTest

final class NativeRuntimeLibraryTests: XCTestCase {
    private let first = RuntimeExactBuildCoordinate(
        manifestAuthor: String(repeating: "a", count: 64),
        dTag: "good-morning",
        aggregateHash: String(repeating: "b", count: 64),
    )
    private let second = RuntimeExactBuildCoordinate(
        manifestAuthor: String(repeating: "c", count: 64),
        dTag: "reader",
        aggregateHash: String(repeating: "d", count: 64),
    )

    func testProjectsExactBuildsSessionsWorkspacesAndRawRefusals() {
        let source = snapshot(
            revision: 41,
            library: RuntimeInstalledLibrarySnapshot(
                query: "morning",
                totalInstalled: 2,
                builds: [
                    build(
                        coordinate: first,
                        title: "Good Morning",
                        availability: .sealedExactBytesReady,
                        sessions: [7, 8],
                        workspaces: ["main", "social"],
                    ),
                    build(
                        coordinate: second,
                        title: "Reader",
                        availability: .metadataOnly,
                    ),
                ],
            ),
            sessions: [
                session(id: 7, coordinate: first, state: "running"),
                session(id: 8, coordinate: first, state: "suspended"),
            ],
            workspaces: [
                workspace(id: "main"),
                workspace(id: "social"),
            ],
            refusals: [
                RuntimeRefusal(
                    code: "capacity",
                    detail: "provider queue refused one operation",
                    occurredAtMillis: 123,
                ),
            ],
        )

        guard case let .snapshot(projected) =
            NativeRuntimeLibraryProjection(source)
        else {
            return XCTFail("Expected a complete library replacement")
        }

        XCTAssertEqual(projected.revision, 41)
        XCTAssertFalse(projected.profileClosed)
        XCTAssertEqual(projected.filterQuery, "morning")
        XCTAssertEqual(projected.totalInstalled, 2)
        XCTAssertEqual(
            projected.workspaces,
            [
                NativeRuntimeLibraryWorkspace(id: "main"),
                NativeRuntimeLibraryWorkspace(id: "social"),
            ],
        )
        XCTAssertEqual(
            projected.refusals,
            [
                NativeRuntimeLibraryRefusal(
                    code: "capacity",
                    detail: "provider queue refused one operation",
                    occurredAtMillis: 123,
                ),
            ],
        )
        XCTAssertEqual(projected.builds.count, 2)
        XCTAssertEqual(
            projected.builds[0].exactBuild,
            NativeRuntimeLibraryExactBuild(first),
        )
        XCTAssertEqual(
            projected.builds[0].availability,
            .sealedExactBytesReady,
        )
        XCTAssertEqual(
            projected.builds[0].sessions,
            [
                NativeRuntimeLibrarySession(id: 7, state: .running),
                NativeRuntimeLibrarySession(id: 8, state: .suspended),
            ],
        )
        XCTAssertEqual(
            projected.builds[0].assignedWorkspaceIDs,
            ["main", "social"],
        )
        XCTAssertEqual(
            projected.builds[1].availability,
            .metadataOnly,
        )
    }

    func testOversizedFilterRefusesWithoutTruncation() {
        let query = String(
            repeating: "é",
            count: NativeRuntimeLibraryLimits.maximumFilterUTF8Bytes
        )
        let source = snapshot(
            revision: 42,
            library: RuntimeInstalledLibrarySnapshot(
                query: query,
                totalInstalled: 0,
                builds: []
            )
        )

        XCTAssertEqual(
            NativeRuntimeLibraryProjection(source),
            .refused(
                revision: 42,
                profileClosed: false,
                refusal: .filterTooLarge(
                    actualUTF8Bytes: query.utf8.count,
                    maximum: NativeRuntimeLibraryLimits.maximumFilterUTF8Bytes
                )
            )
        )
    }

    func testOversizedGeneratedCollectionRefusesWithoutFirstNProjection() {
        let builds = (0 ... NativeRuntimeLibraryLimits.maximumBuilds).map {
            index in
            build(
                coordinate: first,
                title: "Build \(index)",
                availability: .metadataOnly,
            )
        }
        let source = snapshot(
            revision: 45,
            library: RuntimeInstalledLibrarySnapshot(
                query: "",
                totalInstalled: UInt64(builds.count),
                builds: builds,
            ),
        )

        XCTAssertEqual(
            NativeRuntimeLibraryProjection(source),
            .refused(
                revision: 45,
                profileClosed: false,
                refusal: .countExceeded(
                    field: "installedLibrary.builds",
                    actual: NativeRuntimeLibraryLimits.maximumBuilds + 1,
                    maximum: NativeRuntimeLibraryLimits.maximumBuilds,
                ),
            ),
        )
    }

    func testClosedAndReplacementRevisionRemainExplicitInUpdates() {
        let source = snapshot(
            revision: 52,
            closed: true,
            library: RuntimeInstalledLibrarySnapshot(
                query: "",
                totalInstalled: 0,
                builds: [],
            ),
        )
        let projection = NativeRuntimeLibraryProjection(source)

        XCTAssertEqual(projection.revision, 52)
        XCTAssertTrue(projection.profileClosed)
        XCTAssertEqual(
            NativeRuntimeLibraryUpdate.next(
                projection,
                predecessorRevision: 51,
                eventCursorWasStale: true,
                lostBeforeBatch: 3,
            ),
            .next(
                projection,
                predecessorRevision: 51,
                eventCursorWasStale: true,
                lostBeforeBatch: 3,
            ),
        )
    }

    func testObservationCancellationIsIdempotent() {
        let counter = LockedCounter()
        let observation = NativeRuntimeLibraryObservation {
            counter.increment()
        }

        observation.cancel()
        observation.cancel()

        XCTAssertEqual(counter.value, 1)
        XCTAssertEqual(
            NativeRuntimeLibraryObservationError.profileClosed
                .errorDescription,
            "The native runtime profile is closed.",
        )
    }

    private func build(
        coordinate: RuntimeExactBuildCoordinate,
        title: String,
        availability: RuntimeInstalledBuildAvailability,
        sessions: [UInt64] = [],
        workspaces: [String] = []
    ) -> RuntimeInstalledBuildSnapshot {
        RuntimeInstalledBuildSnapshot(
            coordinate: coordinate,
            title: title,
            manifestMetadataJson: #"{"verified":true}"#,
            availability: availability,
            activeSessionIds: sessions,
            assignedWorkspaceIds: workspaces,
        )
    }

    private func session(
        id: UInt64,
        coordinate: RuntimeExactBuildCoordinate,
        state: String
    ) -> RuntimeSessionSnapshot {
        RuntimeSessionSnapshot(
            id: id,
            author: coordinate.manifestAuthor,
            dTag: coordinate.dTag,
            aggregateHash: coordinate.aggregateHash,
            profile: .legacy,
            state: state,
            domains: ["shell"],
            unavailableDomains: [],
        )
    }

    private func workspace(id: String) -> RuntimeWorkspaceDefinition {
        RuntimeWorkspaceDefinition(
            schemaVersion: 1,
            workspaceId: id,
            axis: .horizontal,
            slots: [],
            focusedSlotId: nil,
            activityDrawerVisible: false,
            preferencesJson: "{}",
            retainedReceiptIds: [],
        )
    }

    private func snapshot(
        revision: UInt64,
        closed: Bool = false,
        library: RuntimeInstalledLibrarySnapshot,
        sessions: [RuntimeSessionSnapshot] = [],
        workspaces: [RuntimeWorkspaceDefinition] = [],
        refusals: [RuntimeRefusal] = []
    ) -> RuntimeSnapshot {
        RuntimeSnapshot(
            revision: revision,
            closed: closed,
            installedLibrary: library,
            sessions: sessions,
            bindings: [],
            pendingWrites: [],
            receipts: [],
            workspaces: workspaces,
            recentActivity: [],
            droppedActivity: 0,
            recentErrors: [],
            droppedErrors: 0,
            boundaryRefusals: refusals,
            droppedBoundaryRefusals: 0,
            activeResources: 0,
            resourceHighWatermark: 0,
            resourceRefusalCount: 0,
        )
    }
}

private final class LockedCounter: @unchecked Sendable {
    private let lock = NSLock()
    private var storage = 0

    var value: Int {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }

    func increment() {
        lock.lock()
        storage += 1
        lock.unlock()
    }
}
