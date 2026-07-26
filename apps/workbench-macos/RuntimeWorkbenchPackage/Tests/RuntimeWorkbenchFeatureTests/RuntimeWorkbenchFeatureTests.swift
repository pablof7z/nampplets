import Foundation
import Testing
@testable import RuntimeWorkbenchFeature

@MainActor
@Test func workbenchFeatureBuilds() {
    let view = ContentView()
    #expect(String(describing: type(of: view)) == "ContentView")
}

@MainActor
@Test func contentViewAcceptsInjectedInstalledLibraryManager() {
    let view = ContentView(
        libraryManager: UnavailableWorkbenchLibraryManager(
            reason: "Typed installed-library projection is unavailable."
        )
    )

    #expect(String(describing: type(of: view)) == "ContentView")
}

@Test func defaultLayoutIsAnEmptyFreeformCanvas() {
    let layout = WorkbenchLayoutModel()

    #expect(layout.mode == .freeform)
    #expect(layout.windows.isEmpty)
    #expect(layout.selectedWindow == nil)
}

@Test func freeformWindowMovementAndResizeStayInsideCanvasBounds() throws {
    var layout = WorkbenchLayoutModel()
    let added = layout.addWindow(.goodMorning)
    #expect(added)
    let id = try #require(layout.selectedWindow?.id)

    layout.moveWindow(
        id: id,
        x: 10_000,
        y: -200,
        canvasSize: CGSize(width: 1_000, height: 700)
    )
    layout.resizeWindow(
        id: id,
        width: 10_000,
        height: 1,
        canvasSize: CGSize(width: 1_000, height: 700)
    )

    let frame = try #require(layout.window(id: id)?.frame)
    #expect(frame == .init(x: 0, y: 0, width: 1_000, height: 240))
}

@Test func selectingAWindowBringsItToFrontWithoutDuplicatingIt() {
    var layout = WorkbenchLayoutModel()
    let added = layout.addWindow(.goodMorning)
    #expect(added)
    let second = WorkbenchCanvasWindow(
        id: WorkbenchWindowID(rawValue: "second"),
        componentID: WorkbenchComponentID(rawValue: "network:second"),
        title: "Second",
        frame: WorkbenchWindowFrame(
            x: 120,
            y: 100,
            width: 480,
            height: 360
        ),
        stackingOrder: 0
    )
    let admitted = layout.addWindow(second)
    layout.bringToFront(.init(rawValue: "good-morning"))

    #expect(admitted)
    #expect(layout.windows.map(\.id.rawValue) == ["second", "good-morning"])
    #expect(layout.snapshot.windows.count == 2)
    #expect(layout.selectedWindow?.id.rawValue == "good-morning")
}

@Test func persistedSnapshotRoundTripsWithoutPlatformStorage() throws {
    var layout = WorkbenchLayoutModel()
    let added = layout.addWindow(.goodMorning)
    #expect(added)
    layout.setMode(.tiling)
    layout.moveWindow(
        id: .init(rawValue: "good-morning"),
        x: 180,
        y: 90,
        canvasSize: CGSize(width: 1_400, height: 900)
    )

    let data = try JSONEncoder().encode(layout.snapshot)
    let decoded = try JSONDecoder().decode(
        WorkbenchLayoutSnapshot.self,
        from: data
    )
    let restored = WorkbenchLayoutModel(snapshot: decoded)

    #expect(restored == layout)
}

@Test func fullWindowModePersistsAndRoundTrips() throws {
    var layout = WorkbenchLayoutModel()
    let added = layout.addWindow(.goodMorning)
    #expect(added)
    layout.setMode(.fullWindow)

    let data = try JSONEncoder().encode(layout.snapshot)
    let decoded = try JSONDecoder().decode(
        WorkbenchLayoutSnapshot.self,
        from: data
    )
    let restored = WorkbenchLayoutModel(snapshot: decoded)

    #expect(restored.mode == .fullWindow)
    #expect(restored == layout)
}

@Test func networkDiscoveredExactBuildIdentityRoundTripsWithItsWindow() throws {
    var layout = WorkbenchLayoutModel()
    let exactBuild = WorkbenchExactBuildIdentity(
        manifestAuthor: String(repeating: "a", count: 64),
        dTag: "network-napplet",
        aggregateHash: String(repeating: "b", count: 64)
    )
    let added = layout.addWindow(
        WorkbenchCanvasWindow(
            id: WorkbenchWindowID(rawValue: exactBuild.aggregateHash),
            componentID: WorkbenchComponentID(
                rawValue: exactBuild.aggregateHash
            ),
            exactBuild: exactBuild,
            title: "Network Napplet",
            frame: .init(x: 24, y: 24, width: 640, height: 480),
            stackingOrder: 0
        )
    )
    #expect(added)

    let data = try JSONEncoder().encode(layout.snapshot)
    let restored = WorkbenchLayoutModel(
        snapshot: try JSONDecoder().decode(
            WorkbenchLayoutSnapshot.self,
            from: data
        )
    )

    #expect(restored.selectedWindow?.exactBuild == exactBuild)
}

@Test func installedWindowIdentityIncludesTheCompleteExactBuildCoordinate() {
    let author = String(repeating: "a", count: 64)
    let aggregate = String(repeating: "b", count: 64)
    let first = WorkbenchCanvasWindow.installed(
        title: "First",
        identity: WorkbenchExactBuildIdentity(
            manifestAuthor: author,
            dTag: "first",
            aggregateHash: aggregate
        ),
        offset: 0
    )
    let second = WorkbenchCanvasWindow.installed(
        title: "Second",
        identity: WorkbenchExactBuildIdentity(
            manifestAuthor: author,
            dTag: "second",
            aggregateHash: aggregate
        ),
        offset: 0
    )

    #expect(first.id != second.id)
    #expect(first.componentID != second.componentID)
    #expect(first.id.rawValue.count == 72)
}

@Test func restoredCanvasLaunchPlanIsBoundedOrderedAndDeduplicated() {
    let first = WorkbenchExactBuildIdentity(
        manifestAuthor: String(repeating: "a", count: 64),
        dTag: "first",
        aggregateHash: String(repeating: "1", count: 64)
    )
    let second = WorkbenchExactBuildIdentity(
        manifestAuthor: String(repeating: "b", count: 64),
        dTag: "second",
        aggregateHash: String(repeating: "2", count: 64)
    )
    let windows = [
        WorkbenchCanvasWindow(
            id: WorkbenchWindowID(rawValue: "native"),
            componentID: WorkbenchComponentID(rawValue: "native"),
            title: "Native",
            frame: .init(x: 0, y: 0, width: 480, height: 360),
            stackingOrder: 0
        ),
        WorkbenchCanvasWindow(
            id: WorkbenchWindowID(rawValue: "first"),
            componentID: WorkbenchComponentID(rawValue: "first"),
            exactBuild: first,
            title: "First",
            frame: .init(x: 20, y: 20, width: 480, height: 360),
            stackingOrder: 1
        ),
        WorkbenchCanvasWindow(
            id: WorkbenchWindowID(rawValue: "duplicate-first"),
            componentID: WorkbenchComponentID(rawValue: "duplicate-first"),
            exactBuild: first,
            title: "First Duplicate",
            frame: .init(x: 40, y: 40, width: 480, height: 360),
            stackingOrder: 2
        ),
        WorkbenchCanvasWindow(
            id: WorkbenchWindowID(rawValue: "second"),
            componentID: WorkbenchComponentID(rawValue: "second"),
            exactBuild: second,
            title: "Second",
            frame: .init(x: 60, y: 60, width: 480, height: 360),
            stackingOrder: 3
        ),
    ]
    let layout = WorkbenchLayoutModel(
        snapshot: WorkbenchLayoutSnapshot(
            mode: .freeform,
            windows: windows,
            selectedWindowID: windows.last?.id
        )
    )

    let plan = WorkbenchRestoredCanvasLaunchPlan(layout: layout)

    #expect(plan.identities == [first, second])
    #expect(
        plan.identities.count
            <= WorkbenchLayoutSnapshot.maximumWindowCount
    )
    #expect(
        WorkbenchRestoredCanvasLaunchPlan.reviewMatchesPersistedBuild(
            manifestAuthor: first.manifestAuthor,
            dTag: first.dTag,
            aggregateHash: first.aggregateHash,
            identity: first
        )
    )
    #expect(
        !WorkbenchRestoredCanvasLaunchPlan.reviewMatchesPersistedBuild(
            manifestAuthor: first.manifestAuthor,
            dTag: first.dTag,
            aggregateHash: second.aggregateHash,
            identity: first
        )
    )
}

@Test func canvasRefusesWindowsBeyondItsPersistedBound() {
    var layout = WorkbenchLayoutModel()
    for index in 0 ..< WorkbenchLayoutSnapshot.maximumWindowCount {
        let admitted = layout.addWindow(
            WorkbenchCanvasWindow(
                id: WorkbenchWindowID(rawValue: "window-\(index)"),
                componentID: WorkbenchComponentID(
                    rawValue: "network:\(index)"
                ),
                title: "Window \(index)",
                frame: WorkbenchWindowFrame(
                    x: Double(index * 8),
                    y: Double(index * 8),
                    width: 480,
                    height: 360
                ),
                stackingOrder: 0
            )
        )
        #expect(admitted)
    }

    let overflowAdmitted = layout.addWindow(
        WorkbenchCanvasWindow(
            id: WorkbenchWindowID(rawValue: "overflow"),
            componentID: WorkbenchComponentID(rawValue: "network:overflow"),
            title: "Overflow",
            frame: .init(x: 0, y: 0, width: 480, height: 360),
            stackingOrder: 0
        )
    )

    #expect(!overflowAdmitted)
    #expect(
        layout.windows.count == WorkbenchLayoutSnapshot.maximumWindowCount
    )
}

@Test func loadingAPersistedLayoutBeyondCapacityDropsExcessButReportsIt() {
    let windowCount = WorkbenchLayoutSnapshot.maximumWindowCount + 5
    let windows = (0 ..< windowCount).map { index in
        WorkbenchCanvasWindow(
            id: WorkbenchWindowID(rawValue: "persisted-\(index)"),
            componentID: WorkbenchComponentID(rawValue: "network:\(index)"),
            title: "Persisted \(index)",
            frame: .init(
                x: Double(index * 4),
                y: Double(index * 4),
                width: 480,
                height: 360
            ),
            stackingOrder: UInt16(index)
        )
    }
    let snapshot = WorkbenchLayoutSnapshot(
        mode: .freeform,
        windows: windows,
        selectedWindowID: windows.last?.id
    )

    // A layout beyond maximumWindowCount can never be saved back to Rust
    // (MAXIMUM_WORKSPACE_SLOTS rejects it), so normalized() still caps it --
    // but it must report the drop instead of discarding it silently.
    let restored = WorkbenchLayoutModel(snapshot: snapshot)

    #expect(restored.windows.count == WorkbenchLayoutSnapshot.maximumWindowCount)
    #expect(restored.windowsDroppedForCapacityOnLoad == 5)
    // The most recently focused windows sit at the end of the array (later
    // array position mirrors higher stackingOrder / more recent focus), so
    // dropping for capacity must discard from the front, keeping the suffix.
    #expect(
        Set(restored.windows.map(\.id))
            == Set(windows.suffix(WorkbenchLayoutSnapshot.maximumWindowCount).map(\.id))
    )
}

@Test func loadingAPersistedLayoutWithinCapacityReportsNoDrops() {
    let restored = WorkbenchLayoutModel(snapshot: .workbenchDefault)

    #expect(restored.windowsDroppedForCapacityOnLoad == 0)
}

@Test func loadingAPersistedLayoutStillDropsDuplicateWindowIDs() {
    let duplicated = WorkbenchCanvasWindow(
        id: WorkbenchWindowID(rawValue: "duplicate"),
        componentID: WorkbenchComponentID(rawValue: "network:duplicate"),
        title: "Duplicate A",
        frame: .init(x: 0, y: 0, width: 480, height: 360),
        stackingOrder: 0
    )
    var duplicateAgain = duplicated
    duplicateAgain.title = "Duplicate B"
    let snapshot = WorkbenchLayoutSnapshot(
        mode: .freeform,
        windows: [duplicated, duplicateAgain],
        selectedWindowID: duplicated.id
    )

    let restored = WorkbenchLayoutModel(snapshot: snapshot)

    #expect(restored.windows.count == 1)
    #expect(restored.windows.first?.title == "Duplicate A")
}

@Test func unsupportedPersistedLayoutVersionFallsBackSafely() {
    var snapshot = WorkbenchLayoutSnapshot.workbenchDefault
    snapshot.version = WorkbenchLayoutSnapshot.currentVersion + 1
    snapshot.windows = []
    snapshot.selectedWindowID = nil

    let restored = WorkbenchLayoutModel(snapshot: snapshot)

    #expect(restored.snapshot == .workbenchDefault)
}

/// A version 1 record carries a slot role, a component name, and slot sizes,
/// but never an `exactBuild` — and `exactBuild` is what every launch path
/// keys on. So there is nothing in a v1 layout that can be honestly
/// reacquired and relaunched, and migration projects no windows at all rather
/// than resurrecting one pinned to a build the app used to bundle.
///
/// Decoding still succeeds rather than throwing, deliberately: a v1 layout
/// left on disk must not brick startup. It lands the user on an empty canvas.
@Test func versionOneSlotLayoutMigratesToAnEmptyCanvas() throws {
    let data = try #require(
        """
        {
          "version": 1,
          "visibleRoles": ["feed", "detail", "composer", "tool"],
          "assignments": ["composer", "good-morning"],
          "focusedRole": "composer",
          "sizes": ["composer", {"width": 880, "height": 300}]
        }
        """.data(using: .utf8)
    )
    let decoded = try JSONDecoder().decode(
        WorkbenchLayoutSnapshot.self,
        from: data
    )
    let restored = WorkbenchLayoutModel(snapshot: decoded)

    #expect(restored.snapshot.version == WorkbenchLayoutSnapshot.currentVersion)
    #expect(restored.mode == .freeform)
    #expect(restored.snapshot.windows.isEmpty)
    #expect(restored.selectedWindow == nil)
}

@MainActor
@Test func bundledSignedFixtureOpensThroughTheRustRuntime() throws {
    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent(
            "runtime-workbench-test-\(UUID().uuidString)",
            isDirectory: true
        )
    defer { try? FileManager.default.removeItem(at: root) }

    let fixture = try GoodMorningFixture.load()
    let profile = try WorkbenchRuntimeProfile.open(storageRoot: root)
    defer { profile.close() }
    let artifact = try installApproveAndLaunchGoodMorning(
        fixture: fixture,
        profile: profile
    )

    #expect(artifact.title == "Good Morning Protocol")
}
