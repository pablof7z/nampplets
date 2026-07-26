import Foundation
@testable import RuntimeWorkbenchFeature
import Testing

/// Before this fix, a thrown `observePendingWrites`/`observeReceipts` left
/// `writes`/`receipts` empty with no other trace: an approval a napplet is
/// genuinely stuck waiting on was indistinguishable from "nothing pending".
/// A closed profile is a real, reachable way to make the native observer
/// call throw, so it exercises the actual failure path rather than a mock.
@MainActor
@Test
func pendingWriteObservationFailureIsReportedNotSilentlyEmpty() throws {
    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent(
            "workbench-pending-write-failure-\(UUID().uuidString)",
            isDirectory: true
        )
    defer { try? FileManager.default.removeItem(at: root) }

    let profile = try WorkbenchRuntimeProfile.open(storageRoot: root)
    profile.close()

    let model = RuntimeWorkbenchPendingWriteModel(profile: profile)

    #expect(model.writes.isEmpty)
    #expect(
        model.observationFailureReason != nil,
        "an empty writes list alone must not be the only signal of a failed observation"
    )
}

@MainActor
@Test
func receiptObservationFailureIsReportedNotSilentlyEmpty() throws {
    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent(
            "workbench-receipt-failure-\(UUID().uuidString)",
            isDirectory: true
        )
    defer { try? FileManager.default.removeItem(at: root) }

    let profile = try WorkbenchRuntimeProfile.open(storageRoot: root)
    profile.close()

    let model = RuntimeWorkbenchReceiptModel(profile: profile)

    #expect(model.receipts.isEmpty)
    #expect(
        model.observationFailureReason != nil,
        "an empty receipts list alone must not be the only signal of a failed observation"
    )
}
