import Foundation
import NMPNativeRuntime
@testable import NMPNativeRuntimeApple
import XCTest

/// The runtime counts every entry its bounded rings evict. Those counts used
/// to cross the FFI and stop dead: `NativeRuntimeActivityProjection` rebuilt
/// itself from `recentActivity`/`recentErrors` and dropped them, so no Swift
/// consumer could ever learn that data had been discarded.
///
/// Rust already proves the counting itself against real evictions
/// (`crates/runtime-app/tests/bounds_honesty.rs`,
/// `crates/runtime-ffi/tests/bounds_honesty.rs`). The ring holds 1,024 facts,
/// so driving a real eviction through a unit test is not practical — and it is
/// not the layer that was broken. These tests pin the seam that was: whether
/// the projection carries the counts across.
final class NativeRuntimeActivityDiscardTests: XCTestCase {
    private let scope = NativeRuntimeActivityScope(
        manifestAuthor: String(repeating: "a", count: 64),
        dTag: "good-morning",
        aggregateHash: String(repeating: "b", count: 64)
    )

    func testProjectionCarriesBothDiscardCountsAcrossTheBoundary() {
        let projection = NativeRuntimeActivityProjection(
            snapshot(droppedActivity: 7, droppedErrors: 5),
            scope: scope
        )

        XCTAssertEqual(projection.runtimeDiscardedCount, 12)
    }

    /// The honesty case. Every retained entry belongs to some other build, so
    /// scope filtering empties the projection — but entries were still
    /// destroyed, and the surface must still be able to say so. Reporting zero
    /// here would be the silent truncation this whole mechanism exists to
    /// prevent.
    func testDiscardCountSurvivesWhenScopeFilteringEmptiesTheProjection() {
        let projection = NativeRuntimeActivityProjection(
            snapshot(droppedActivity: 3, droppedErrors: 0),
            scope: scope
        )

        XCTAssertTrue(projection.records.isEmpty)
        XCTAssertTrue(projection.errors.isEmpty)
        XCTAssertEqual(projection.runtimeDiscardedCount, 3)
    }

    /// A runtime that has discarded nothing must not imply that it has.
    func testNothingDiscardedReportsZero() {
        let projection = NativeRuntimeActivityProjection(
            snapshot(droppedActivity: 0, droppedErrors: 0),
            scope: scope
        )

        XCTAssertEqual(projection.runtimeDiscardedCount, 0)
    }

    private func snapshot(
        droppedActivity: UInt64,
        droppedErrors: UInt64
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
            droppedActivity: droppedActivity,
            recentErrors: [],
            droppedErrors: droppedErrors,
            boundaryRefusals: [],
            droppedBoundaryRefusals: 0,
            activeResources: 0,
            resourceHighWatermark: 0,
            resourceRefusalCount: 0
        )
    }
}
