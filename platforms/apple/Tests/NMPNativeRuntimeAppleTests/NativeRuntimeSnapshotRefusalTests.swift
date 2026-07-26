import Foundation
import NMPNativeRuntime
import XCTest
@testable import NMPNativeRuntimeApple

final class NativeRuntimeSnapshotRefusalTests: XCTestCase {
    func testRefusedFrameUpdatesLibraryAndCatalogWithoutDerivedSnapshotUpdates()
        throws
    {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-refused-frame-\(UUID().uuidString)",
                isDirectory: true
            )
        defer { try? FileManager.default.removeItem(at: root) }
        let profile = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(storageRoot: root)
        )
        defer { profile.close() }

        let updates = LockedSnapshotRefusalUpdates()
        let library = try profile.observeInstalledLibrary(updates.appendLibrary)
        let pending = try profile.observePendingWrites(updates.appendPending)
        let receipts = try profile.observeReceipts(updates.appendReceipt)
        let activity = try profile.observeActivity(
            scope: NativeRuntimeActivityScope(
                manifestAuthor: "author",
                dTag: "component",
                aggregateHash: "hash"
            ),
            updates.appendActivity
        )
        let catalog = try profile.observeCatalog(updates.appendCatalog)
        let session = RustRuntimeNappletSession(
            profile: profile,
            sessionID: 42,
            maximumReadBytes: 1_024
        )
        let eventBytes = LockedSnapshotRefusalEventBytes()
        session.setResponseSink(eventBytes.record)
        profile.lock.lock()
        profile.sessions[session.sessionID] = NativeRuntimeProfile.WeakSession(
            session
        )
        profile.lock.unlock()
        defer {
            library.cancel()
            pending.cancel()
            receipts.cancel()
            activity.cancel()
            catalog.cancel()
        }

        let initialSnapshot = try profile.snapshotForTesting
        var nextCatalog = profile.catalogSnapshotForTesting
        nextCatalog.revision += 1
        let refusal = RuntimeRefusal(
            code: "snapshot-integrity-missing-build-session",
            detail: "build references session 42, but it is absent",
            occurredAtMillis: 123
        )
        profile.update(
            frame: RuntimeObservationFrame(
                snapshot: .refused(
                    revision: initialSnapshot.revision + 1,
                    closed: false,
                    refusal: refusal
                ),
                catalog: nextCatalog,
                events: [
                    RuntimeEvent(
                        sequence: 1,
                        kind: "provider-push",
                        detail: "independent event delivery",
                        sessionId: session.sessionID,
                        responseJson: #"{"type":"event-independent"}"#
                    )
                ],
                oldestAvailableEvent: 1,
                newestAvailableEvent: 1,
                eventCursorWasStale: false,
                lostBeforeBatch: 0
            )
        )

        let captured = updates.values
        XCTAssertEqual(captured.pendingCount, 1)
        XCTAssertEqual(captured.receiptCount, 1)
        XCTAssertEqual(captured.activityCount, 1)
        XCTAssertEqual(captured.library.count, 2)
        XCTAssertEqual(captured.catalog.count, 2)
        XCTAssertEqual(
            eventBytes.value,
            Data(#"{"type":"event-independent"}"#.utf8)
        )

        guard case let .next(
            projection,
            predecessorRevision,
            eventCursorWasStale
        ) = captured.library.last else {
            return XCTFail("Refusal must advance the installed-library stream")
        }
        XCTAssertEqual(predecessorRevision, initialSnapshot.revision)
        XCTAssertFalse(eventCursorWasStale)
        XCTAssertEqual(
            projection,
            .refused(
                revision: initialSnapshot.revision + 1,
                profileClosed: false,
                refusal: .runtime(refusal)
            )
        )

        guard case let .next(catalogSnapshot, catalogPredecessor) =
            captured.catalog.last
        else {
            return XCTFail("Catalog delivery must remain independent")
        }
        XCTAssertEqual(catalogSnapshot, nextCatalog)
        XCTAssertEqual(catalogPredecessor, nextCatalog.revision - 1)
    }
}

private final class LockedSnapshotRefusalEventBytes: @unchecked Sendable {
    private let lock = NSLock()
    private var storage: Data?

    var value: Data? {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }

    func record(_ bytes: Data) {
        lock.lock()
        storage = bytes
        lock.unlock()
    }
}

private final class LockedSnapshotRefusalUpdates: @unchecked Sendable {
    struct Values {
        let library: [NativeRuntimeLibraryUpdate]
        let catalog: [NativeRuntimeCatalogUpdate]
        let pendingCount: Int
        let receiptCount: Int
        let activityCount: Int
    }

    private let lock = NSLock()
    private var library: [NativeRuntimeLibraryUpdate] = []
    private var catalog: [NativeRuntimeCatalogUpdate] = []
    private var pendingCount = 0
    private var receiptCount = 0
    private var activityCount = 0

    var values: Values {
        lock.lock()
        defer { lock.unlock() }
        return Values(
            library: library,
            catalog: catalog,
            pendingCount: pendingCount,
            receiptCount: receiptCount,
            activityCount: activityCount
        )
    }

    func appendLibrary(_ update: NativeRuntimeLibraryUpdate) {
        lock.lock()
        library.append(update)
        lock.unlock()
    }

    func appendCatalog(_ update: NativeRuntimeCatalogUpdate) {
        lock.lock()
        catalog.append(update)
        lock.unlock()
    }

    func appendPending(_: NativeRuntimePendingWriteUpdate) {
        lock.lock()
        pendingCount += 1
        lock.unlock()
    }

    func appendReceipt(_: NativeRuntimeReceiptUpdate) {
        lock.lock()
        receiptCount += 1
        lock.unlock()
    }

    func appendActivity(_: NativeRuntimeActivityUpdate) {
        lock.lock()
        activityCount += 1
        lock.unlock()
    }
}
