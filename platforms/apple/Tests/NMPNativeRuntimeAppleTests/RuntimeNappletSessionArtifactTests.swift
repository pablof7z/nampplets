import Foundation
import NMPNativeRuntime
import XCTest
import WebKit
@testable import NMPNativeRuntimeApple

// MARK: - Signed artifact install, launch, and envelope round trip

final class RuntimeNappletSessionArtifactTests: RuntimeNappletSessionTestCase {
    func testSignedNamedArtifactNegotiatesAndRespondsThroughRust() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-test-\(UUID().uuidString)",
                isDirectory: true
            )
        defer { try? FileManager.default.removeItem(at: root) }
        let repository = repositoryRoot()
        let fixture = repository.appendingPathComponent(
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
            title: "Good Morning Protocol",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index],
            grantDomains: requiredGoodMorningDomains + ["storage"]
        )
        let runtime = try XCTUnwrap(artifact.runtimeSession)
        defer { runtime.stop() }

        XCTAssertEqual(
            artifact.negotiatedDomains,
            ["identity", "inc", "outbox", "shell", "storage"]
        )
        let sealed = try XCTUnwrap(
            try artifact.reader.readSealed(logicalPath: "/index.html")
        )
        XCTAssertEqual(sealed.sha256, indexDigest)
        XCTAssertEqual(sealed.bytes, index)

        let received = expectation(description: "Rust emits the pinned shell.init")
        let response = LockedData()
        runtime.setResponseSink { bytes in
            guard
                let envelope = try? JSONSerialization.jsonObject(with: bytes)
                    as? [String: Any],
                envelope["type"] as? String == "shell.init",
                response.setIfEmpty(bytes)
            else {
                return
            }
            received.fulfill()
        }
        runtime.mappedEnvelope(Data(#"{"type":"shell.ready"}"#.utf8))

        wait(for: [received], timeout: 2)
        let bytes = try XCTUnwrap(response.value)
        let envelope = try XCTUnwrap(
            JSONSerialization.jsonObject(with: bytes) as? [String: Any]
        )
        XCTAssertEqual(envelope["type"] as? String, "shell.init")
        let capabilities = try XCTUnwrap(
            envelope["capabilities"] as? [String: Any]
        )
        XCTAssertEqual(
            capabilities["domains"] as? [String],
            ["identity", "inc", "outbox", "shell", "storage"]
        )
    }

    func testInstallPermissionAndLaunchRemainThreeSeparateOperations() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-staged-launch-\(UUID().uuidString)",
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

        let installed = try profile.installSignedNamed(
            title: "Good Morning Staged",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index]
        )
        XCTAssertEqual(installed.permissionCoordinate.manifestAuthor, author)
        XCTAssertEqual(installed.permissionCoordinate.dTag, "good-morning")
        XCTAssertTrue(try profile.snapshotForTesting.sessions.isEmpty)
        let reacquired: NativeRuntimeInstalledArtifact
        switch profile.reacquireInstalledArtifact(
            installed.permissionCoordinate
        ) {
        case .refused(let failure):
            XCTFail(
                "The exact installed handle should reopen without network work: "
                    + "\(failure.code): \(failure.detail)"
            )
            return
        case .installed(let installation):
            XCTAssertEqual(installation.title, "Good Morning Protocol")
            XCTAssertEqual(
                installation.installedArtifact.permissionCoordinate,
                installed.permissionCoordinate
            )
            reacquired = installation.installedArtifact
        }
        XCTAssertTrue(
            try profile.snapshotForTesting.sessions.isEmpty,
            "reacquisition must never launch"
        )

        let review = try XCTUnwrap(
            profile.permissionReview(for: reacquired.permissionCoordinate).review
        )
        XCTAssertEqual(
            review.capabilities.map(\.domain),
            ["identity", "inc", "outbox", "resource", "theme", "link"]
        )
        XCTAssertEqual(
            review.capabilities.map(\.requirement),
            [.required, .required, .required, .optional, .optional, .optional]
        )

        XCTAssertThrowsError(try profile.launchInstalled(reacquired)) { error in
            guard case RuntimeNappletOpenError.launchRefused = error else {
                return XCTFail("Expected Rust launch refusal, got \(error)")
            }
        }
        XCTAssertTrue(try profile.snapshotForTesting.sessions.isEmpty)

        let update = profile.applyPermissionDecisions(
            NativeRuntimePermissionDecisionBatch(
                coordinate: reacquired.permissionCoordinate,
                decisions: review.capabilities.map {
                    NativeRuntimePermissionDecisionSelection(
                        domain: $0.domain,
                        decision: $0.requirement == .required
                            ? .allowExactBuild
                            : .denied
                    )
                }
            )
        )
        XCTAssertTrue(update.applied)
        XCTAssertTrue(update.review?.launchPermitted == true)
        XCTAssertNil(update.refusal)
        XCTAssertTrue(
            try profile.snapshotForTesting.sessions.isEmpty,
            "applying permissions must never launch"
        )

        let launched = try profile.launchInstalled(reacquired)
        XCTAssertEqual(
            launched.negotiatedDomains,
            ["identity", "inc", "outbox", "shell"]
        )
        launched.runtimeSession?.stop()
    }

    func testDemoPinnedGoodMorningAutoGrantsAndNegotiatesOutbox() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-demo-pinned-\(UUID().uuidString)",
                isDirectory: true
            )
        defer { try? FileManager.default.removeItem(at: root) }
        let fixture = repositoryRoot().appendingPathComponent(
            "conformance/napplet-corpus/published/good-morning",
            isDirectory: true
        )
        let event = try Data(contentsOf: fixture.appendingPathComponent("event.json"))
        let index = try Data(contentsOf: fixture.appendingPathComponent("index.html"))
        let profile = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(
                storageRoot: root,
                permissionMode: .demoPinnedGoodMorning
            )
        )
        defer { profile.close() }

        let installed = try profile.installSignedNamed(
            title: "Good Morning Demo",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index]
        )
        let review = try XCTUnwrap(
            profile.permissionReview(for: installed.permissionCoordinate).review
        )
        XCTAssertTrue(review.launchPermitted)
        XCTAssertTrue(
            review.capabilities.filter {
                if case .available = $0.platformAvailability { return true }
                return false
            }.allSatisfy {
                $0.existingDecision == .allowExactBuild
            },
            "demo mode must grant every available pinned Good Morning capability"
        )

        let launched = try profile.launchInstalled(installed)
        XCTAssertTrue(launched.negotiatedDomains.contains("outbox"))
        XCTAssertTrue(launched.negotiatedDomains.contains("identity"))
        XCTAssertTrue(launched.negotiatedDomains.contains("inc"))
    }

}

private final class LockedData: @unchecked Sendable {
    private let lock = NSLock()
    private var storage: Data?

    var value: Data? {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }

    func set(_ value: Data) {
        lock.lock()
        storage = value
        lock.unlock()
    }

    func setIfEmpty(_ value: Data) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard storage == nil else {
            return false
        }
        storage = value
        return true
    }
}
