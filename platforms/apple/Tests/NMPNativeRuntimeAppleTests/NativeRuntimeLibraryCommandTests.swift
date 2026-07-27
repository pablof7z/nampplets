import Foundation
import NMPNativeRuntime
import XCTest
import WebKit
@testable import NMPNativeRuntimeApple

// MARK: - Installed-library commands against Rust-owned profile state

final class NativeRuntimeLibraryCommandTests: RuntimeNappletSessionTestCase {
    func testInstalledLibraryCommandsMapToRustOwnedProfileState() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-library-commands-\(UUID().uuidString)",
                isDirectory: true
            )
        defer { try? FileManager.default.removeItem(at: root) }
        let fixture = repositoryRoot().appendingPathComponent(
            "conformance/napplet-corpus/published/good-morning",
            isDirectory: true
        )
        let event = try Data(
            contentsOf: fixture.appendingPathComponent("event.json")
        )
        let index = try Data(
            contentsOf: fixture.appendingPathComponent("index.html")
        )
        let profile = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(storageRoot: root)
        )
        defer { profile.close() }
        let artifact = try profile.openSignedNamed(
            title: "Good Morning Library Commands",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index],
            grantDomains: requiredGoodMorningDomains
        )
        let runtime = try XCTUnwrap(artifact.runtimeSession)
        defer { runtime.stop() }

        var snapshot = try librarySnapshot(
            profile.installedLibraryProjection()
        )
        XCTAssertEqual(snapshot.totalInstalled, 1)
        let build = try XCTUnwrap(snapshot.builds.first)
        XCTAssertEqual(build.availability, .sealedExactBytesReady)
        // Degraded, not running. The good-morning fixture declares `link` and
        // `resource`, which no provider on this runtime serves, so this
        // session has never been whole -- it read as running only because the
        // state string could not express the difference.
        XCTAssertEqual(
            build.sessions,
            [
                NativeRuntimeLibrarySession(
                    id: runtime.sessionID,
                    state: .runningDegraded
                ),
            ]
        )

        let workspaceID = "library-commands"
        let workspaceUpdate = profile.saveWorkspace(
            NativeRuntimeWorkspaceDefinition(
                schemaVersion: 1,
                workspaceId: workspaceID,
                axis: .horizontal,
                slots: [
                    NativeRuntimeWorkspaceSlot(
                        slotId: "library",
                        role: .toolWindow,
                        renderer: .native,
                        handlerId: "installed-library",
                        manifestAuthor: nil,
                        dTag: nil,
                        aggregateHash: nil,
                        bindingParametersJson: "{}",
                        navigationJson: "{}",
                        visible: true,
                        order: 0,
                        sizePoints: 640,
                        minimumPoints: 320,
                        maximumPoints: 1_200
                    ),
                ],
                focusedSlotId: "library",
                activityDrawerVisible: false,
                preferencesJson: "{}",
                retainedReceiptIds: []
            )
        )
        XCTAssertTrue(workspaceUpdate.accepted)
        profile.assignInstalledBuild(
            build.exactBuild,
            toWorkspaceID: workspaceID
        )
        snapshot = try librarySnapshot(profile.installedLibraryProjection())
        XCTAssertEqual(snapshot.builds.first?.assignedWorkspaceIDs, [workspaceID])
        XCTAssertEqual(snapshot.workspaces.map(\.id), [workspaceID])

        profile.setInstalledLibraryFilter("no-match")
        snapshot = try librarySnapshot(profile.installedLibraryProjection())
        XCTAssertEqual(snapshot.filterQuery, "no-match")
        XCTAssertEqual(snapshot.totalInstalled, 1)
        XCTAssertTrue(snapshot.builds.isEmpty)

        profile.setInstalledLibraryFilter("GOOD-MORNING")
        snapshot = try librarySnapshot(profile.installedLibraryProjection())
        XCTAssertEqual(snapshot.builds.count, 1)

        profile.suspendInstalledSession(runtime.sessionID)
        snapshot = try librarySnapshot(profile.installedLibraryProjection())
        XCTAssertEqual(snapshot.builds.first?.sessions.first?.state, .suspended)

        profile.resumeInstalledSession(runtime.sessionID)
        snapshot = try librarySnapshot(profile.installedLibraryProjection())
        XCTAssertEqual(snapshot.builds.first?.sessions.first?.state, .runningDegraded)

        profile.clearInstalledBuildAssignment(
            build.exactBuild,
            fromWorkspaceID: workspaceID
        )
        snapshot = try librarySnapshot(profile.installedLibraryProjection())
        XCTAssertTrue(
            try XCTUnwrap(snapshot.builds.first).assignedWorkspaceIDs.isEmpty
        )

        profile.uninstallInstalledBuild(build.exactBuild)
        snapshot = try librarySnapshot(profile.installedLibraryProjection())
        XCTAssertEqual(snapshot.totalInstalled, 0)
        XCTAssertTrue(snapshot.builds.isEmpty)
        XCTAssertEqual(snapshot.workspaces.map(\.id), [workspaceID])
    }

}
