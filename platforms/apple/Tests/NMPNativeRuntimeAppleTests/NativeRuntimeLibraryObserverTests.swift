import Foundation
import NMPNativeRuntime
import XCTest
import WebKit
@testable import NMPNativeRuntimeApple

// MARK: - Installed-library observer admission and replacement delivery

final class NativeRuntimeLibraryObserverTests: RuntimeNappletSessionTestCase {
    func testInstalledLibraryObserverStartsWithAuthoritativeReplacementAndPushesNextRevision() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-library-observer-\(UUID().uuidString)",
                isDirectory: true
            )
        defer { try? FileManager.default.removeItem(at: root) }
        let profile = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(storageRoot: root)
        )
        defer { profile.close() }

        let receivedNext = expectation(
            description: "library observer receives the next replacement"
        )
        let updates = LockedLibraryUpdates()
        let observation = try profile.observeInstalledLibrary { update in
            updates.append(update)
            if case .next = update {
                receivedNext.fulfill()
            }
        }
        defer { observation.cancel() }

        let initial = try XCTUnwrap(updates.values.first)
        guard case let .authoritative(initialProjection) = initial else {
            return XCTFail("The first update must be authoritative")
        }
        let initialSnapshot = try librarySnapshot(initialProjection)
        XCTAssertEqual(initialSnapshot.filterQuery, "")
        XCTAssertEqual(initialSnapshot.totalInstalled, 0)
        XCTAssertTrue(initialSnapshot.builds.isEmpty)

        profile.setInstalledLibraryFilter("morning")
        wait(for: [receivedNext], timeout: 2)

        let next = try XCTUnwrap(
            updates.values.first(where: {
                if case .next = $0 {
                    return true
                }
                return false
            })
        )
        guard case let .next(
            nextProjection,
            predecessorRevision,
            eventCursorWasStale
        ) = next else {
            return XCTFail("Expected a next library replacement")
        }
        let nextSnapshot = try librarySnapshot(nextProjection)
        XCTAssertEqual(predecessorRevision, initialSnapshot.revision)
        XCTAssertGreaterThan(nextSnapshot.revision, initialSnapshot.revision)
        XCTAssertFalse(eventCursorWasStale)
        XCTAssertEqual(nextSnapshot.filterQuery, "morning")
        XCTAssertEqual(profile.installedLibraryProjection(), nextProjection)
    }

    func testInstalledLibraryObserverQueuesLatestUpdateUntilAuthoritativeDeliveryCompletes() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-library-ordering-\(UUID().uuidString)",
                isDirectory: true
            )
        defer { try? FileManager.default.removeItem(at: root) }
        let profile = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(storageRoot: root)
        )
        defer { profile.close() }

        let initialSnapshot = try profile.snapshotForTesting
        let authoritativeStarted = expectation(
            description: "authoritative delivery started"
        )
        let registrationFinished = expectation(
            description: "observer registration drained pending replacement"
        )
        let allowAuthoritativeToFinish = DispatchSemaphore(value: 0)
        let updates = LockedLibraryUpdates()
        let observation = LockedLibraryObservation()

        DispatchQueue.global().async {
            let registered = try? profile.observeInstalledLibrary { update in
                if case .authoritative = update {
                    authoritativeStarted.fulfill()
                    _ = allowAuthoritativeToFinish.wait(
                        timeout: .now() + 5
                    )
                }
                updates.append(update)
            }
            observation.set(registered)
            registrationFinished.fulfill()
        }

        wait(for: [authoritativeStarted], timeout: 2)

        var intermediate = initialSnapshot
        intermediate.revision += 1
        intermediate.installedLibrary.query = "intermediate"
        profile.update(
            frame: RuntimeObservationFrame(
                snapshot: .snapshot(snapshot: intermediate),
                catalog: profile.catalogSnapshotForTesting,
                events: [],
                oldestAvailableEvent: 0,
                newestAvailableEvent: 0,
                eventCursorWasStale: false,
                lostBeforeBatch: 0
            )
        )
        var latest = intermediate
        latest.revision += 1
        latest.installedLibrary.query = "latest"
        profile.update(
            frame: RuntimeObservationFrame(
                snapshot: .snapshot(snapshot: latest),
                catalog: profile.catalogSnapshotForTesting,
                events: [],
                oldestAvailableEvent: 0,
                newestAvailableEvent: 0,
                eventCursorWasStale: true,
                lostBeforeBatch: 0
            )
        )

        allowAuthoritativeToFinish.signal()
        wait(for: [registrationFinished], timeout: 2)
        defer { observation.value?.cancel() }

        let delivered = updates.values
        XCTAssertEqual(delivered.count, 2)
        guard case let .authoritative(authoritative) = delivered.first else {
            return XCTFail("Authoritative replacement must be delivered first")
        }
        XCTAssertEqual(authoritative.revision, initialSnapshot.revision)
        guard case let .next(
            nextProjection,
            predecessorRevision,
            eventCursorWasStale
        ) = delivered.last else {
            return XCTFail("The newest pending replacement must drain second")
        }
        let nextSnapshot = try librarySnapshot(nextProjection)
        XCTAssertEqual(nextSnapshot.revision, latest.revision)
        XCTAssertEqual(nextSnapshot.filterQuery, "latest")
        XCTAssertEqual(predecessorRevision, intermediate.revision)
        XCTAssertTrue(eventCursorWasStale)
    }

    func testInstalledLibraryObserverCapacityCancellationAndClosedRefusal() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-library-capacity-\(UUID().uuidString)",
                isDirectory: true
            )
        defer { try? FileManager.default.removeItem(at: root) }
        let profile = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(storageRoot: root)
        )
        var observations: [NativeRuntimeLibraryObservation] = []
        for _ in 0 ..< 8 {
            let updates = LockedLibraryUpdates()
            let observation = try profile.observeInstalledLibrary(updates.append)
            observations.append(observation)
            guard case .authoritative = try XCTUnwrap(updates.values.first) else {
                return XCTFail("Every admitted observer needs an immediate replacement")
            }
        }

        XCTAssertThrowsError(
            try profile.observeInstalledLibrary { _ in }
        ) { error in
            XCTAssertEqual(
                error as? NativeRuntimeLibraryObservationError,
                .observerCapacity(maximum: 8)
            )
        }

        observations.removeLast().cancel()
        let replacementUpdates = LockedLibraryUpdates()
        let replacement = try profile.observeInstalledLibrary(
            replacementUpdates.append
        )
        guard case .authoritative =
            try XCTUnwrap(replacementUpdates.values.first)
        else {
            return XCTFail("Cancellation must release observer capacity")
        }
        replacement.cancel()
        observations.forEach { $0.cancel() }

        profile.close()
        XCTAssertThrowsError(
            try profile.observeInstalledLibrary { _ in }
        ) { error in
            XCTAssertEqual(
                error as? NativeRuntimeLibraryObservationError,
                .profileClosed
            )
        }
        XCTAssertTrue(
            try librarySnapshot(profile.installedLibraryProjection())
                .profileClosed
        )
    }

}

private final class LockedLibraryUpdates: @unchecked Sendable {
    private let lock = NSLock()
    private var storage: [NativeRuntimeLibraryUpdate] = []

    var values: [NativeRuntimeLibraryUpdate] {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }

    func append(_ update: NativeRuntimeLibraryUpdate) {
        lock.lock()
        storage.append(update)
        lock.unlock()
    }
}

private final class LockedLibraryObservation: @unchecked Sendable {
    private let lock = NSLock()
    private var storage: NativeRuntimeLibraryObservation?

    var value: NativeRuntimeLibraryObservation? {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }

    func set(_ observation: NativeRuntimeLibraryObservation?) {
        lock.lock()
        storage = observation
        lock.unlock()
    }
}
