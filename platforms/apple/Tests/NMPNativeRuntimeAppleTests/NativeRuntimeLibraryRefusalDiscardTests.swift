import Foundation
import NMPNativeRuntime
@testable import NMPNativeRuntimeApple
import XCTest

/// `boundaryRefusals` is only what survived the runtime's bound. The runtime
/// counts the evictions in `droppedBoundaryRefusals` — `record_boundary_refusal`
/// documents that the eviction "is counted, never silent" — but the library
/// projection rebuilt itself from the surviving list alone, so a consumer
/// reporting `refusals.count` understated how many times the runtime refused.
final class NativeRuntimeLibraryRefusalDiscardTests: XCTestCase {
    func testProjectionCarriesTheDiscardedRefusalCount() throws {
        let projection = NativeRuntimeLibraryProjection(
            .snapshot(snapshot(refusals: [refusal("a")], dropped: 9))
        )

        guard case let .snapshot(library) = projection else {
            return XCTFail("expected an accepted projection, got \(projection)")
        }
        XCTAssertEqual(library.refusals.count, 1)
        XCTAssertEqual(library.droppedRefusalCount, 9)
    }

    /// The case the surviving list cannot express: the ring evicted refusals
    /// and retained none, so `refusals` is empty while the runtime refused nine
    /// times. Reporting zero here is the silent truncation the counter exists
    /// to prevent.
    func testDiscardCountSurvivesAnEmptyRetainedList() throws {
        let projection = NativeRuntimeLibraryProjection(
            .snapshot(snapshot(refusals: [], dropped: 9))
        )

        guard case let .snapshot(library) = projection else {
            return XCTFail("expected an accepted projection, got \(projection)")
        }
        XCTAssertTrue(library.refusals.isEmpty)
        XCTAssertEqual(library.droppedRefusalCount, 9)
    }

    /// A runtime that discarded nothing must not imply that it did.
    func testNothingDiscardedReportsZero() throws {
        let projection = NativeRuntimeLibraryProjection(
            .snapshot(snapshot(refusals: [refusal("a")], dropped: 0))
        )

        guard case let .snapshot(library) = projection else {
            return XCTFail("expected an accepted projection, got \(projection)")
        }
        XCTAssertEqual(library.droppedRefusalCount, 0)
    }

    private func refusal(_ code: String) -> RuntimeRefusal {
        RuntimeRefusal(code: code, detail: "detail", occurredAtMillis: 1)
    }

    private func snapshot(
        refusals: [RuntimeRefusal],
        dropped: UInt64
    ) -> RuntimeSnapshot {
        RuntimeSnapshot(
            revision: 1,
            closed: false,
            installedLibrary: RuntimeInstalledLibrarySnapshot(
                query: "",
                totalInstalled: 0,
                builds: []
            ),
            sessions: [],
            bindings: [],
            pendingWrites: [],
            receipts: [],
            workspaces: [],
            recentActivity: [],
            droppedActivity: 0,
            recentErrors: [],
            droppedErrors: 0,
            boundaryRefusals: refusals,
            droppedBoundaryRefusals: dropped,
            refusedOperatorRelays: [],
            activeResources: 0,
            resourceHighWatermark: 0,
            resourceRefusalCount: 0
        )
    }
}
