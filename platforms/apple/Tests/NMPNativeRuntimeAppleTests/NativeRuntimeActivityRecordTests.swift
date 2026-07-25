import Foundation
import NMPNativeRuntime
@testable import NMPNativeRuntimeApple
import XCTest

final class NativeRuntimeActivityRecordTests: XCTestCase {
    private func snapshot(
        details: [RuntimeActivityDetail],
        droppedDetailCount: UInt32 = 0
    ) -> RuntimeActivitySnapshot {
        RuntimeActivitySnapshot(
            author: String(repeating: "a", count: 64),
            dTag: "good-morning",
            aggregateHash: String(repeating: "b", count: 64),
            category: "write",
            operation: "accept",
            outcome: "durable-obligation",
            occurredAtMillis: 12,
            details: details,
            droppedDetailCount: droppedDetailCount,
        )
    }

    func testRuntimeClassificationIsCarriedAcrossUnchanged() {
        let record = NativeRuntimeActivityRecord(
            snapshot(details: [
                RuntimeActivityDetail(key: "approved-draft", value: .redacted),
                RuntimeActivityDetail(
                    key: "receipt-id",
                    value: .visible(text: "receipt-1"),
                ),
            ]),
        )

        XCTAssertEqual(record.details.count, 2)
        XCTAssertEqual(record.details[0].key, "approved-draft")
        XCTAssertEqual(record.details[0].value, .redacted)
        XCTAssertEqual(record.details[1].value, .visible("receipt-1"))
    }

    func testASecretLookingKeyIsNotReclassifiedNatively() {
        // The runtime said this value is public. Native code must not apply a
        // second opinion just because the key spells "token".
        let record = NativeRuntimeActivityRecord(
            snapshot(details: [
                RuntimeActivityDetail(
                    key: "token-relay",
                    value: .visible(text: "wss://relay.example"),
                ),
            ]),
        )

        XCTAssertEqual(
            record.details[0].value,
            .visible("wss://relay.example")
        )
    }

    func testDroppedDetailsRemainCounted() {
        let record = NativeRuntimeActivityRecord(
            snapshot(details: [], droppedDetailCount: 3),
        )

        XCTAssertTrue(record.details.isEmpty)
        XCTAssertEqual(record.droppedDetailCount, 3)
    }
}
