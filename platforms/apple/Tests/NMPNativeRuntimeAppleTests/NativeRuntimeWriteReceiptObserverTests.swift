import Foundation
import NMPNativeRuntime
import XCTest
import WebKit
@testable import NMPNativeRuntimeApple

// MARK: - Pending-write and receipt observer replacement delivery

final class NativeRuntimeWriteReceiptObserverTests: RuntimeNappletSessionTestCase {
    func testPendingWriteAndReceiptObserversAreBoundedAndProjectReplacements() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-write-observers-\(UUID().uuidString)",
                isDirectory: true
            )
        defer { try? FileManager.default.removeItem(at: root) }
        let profile = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(storageRoot: root)
        )
        defer { profile.close() }

        let pendingUpdates = LockedPendingWriteUpdates()
        let pendingObservation = try profile.observePendingWrites(
            pendingUpdates.append
        )
        defer { pendingObservation.cancel() }
        let receiptUpdates = LockedReceiptUpdates()
        let receiptObservation = try profile.observeReceipts(
            receiptUpdates.append
        )
        defer { receiptObservation.cancel() }

        guard case let .authoritative(pendingInitial) =
            try XCTUnwrap(pendingUpdates.values.first)
        else {
            return XCTFail("Pending writes must start with an authoritative replacement")
        }
        XCTAssertTrue(pendingInitial.writes.isEmpty)
        guard case let .authoritative(receiptInitial) =
            try XCTUnwrap(receiptUpdates.values.first)
        else {
            return XCTFail("Receipts must start with an authoritative replacement")
        }
        XCTAssertTrue(receiptInitial.receipts.isEmpty)

        var snapshot = profile.snapshotForTesting
        snapshot.revision += 1
        snapshot.receipts = [
            RuntimeReceiptSnapshot(
                receiptId: "receipt-1",
                delivery: "pending",
                latestStateJson: #"{"status":"queued"}"#
            )
        ]
        profile.update(
            frame: RuntimeObservationFrame(
                snapshot: snapshot,
                catalog: profile.catalogSnapshotForTesting,
                events: [],
                oldestAvailableEvent: 0,
                newestAvailableEvent: 0,
                eventCursorWasStale: false,
                lostBeforeBatch: 0
            )
        )

        guard case let .next(receiptNext, predecessorRevision, _) =
            try XCTUnwrap(receiptUpdates.values.last)
        else {
            return XCTFail("Receipt updates must push a next replacement")
        }
        XCTAssertEqual(predecessorRevision, receiptInitial.revision)
        XCTAssertEqual(receiptNext.receipts.first?.id, "receipt-1")
        XCTAssertEqual(
            receiptNext.receipts.first?.latestStateJSON,
            #"{"status":"queued"}"#
        )

        pendingObservation.cancel()
        var pendingObservers: [NativeRuntimePendingWriteObservation] = []
        for _ in 0 ..< 8 {
            pendingObservers.append(try profile.observePendingWrites { _ in })
        }
        XCTAssertThrowsError(try profile.observePendingWrites { _ in }) { error in
            XCTAssertEqual(
                error as? NativeRuntimePendingWriteObservationError,
                .observerCapacity(maximum: 8)
            )
        }
        pendingObservers.removeLast().cancel()
        pendingObservers.forEach { $0.cancel() }
    }

}
