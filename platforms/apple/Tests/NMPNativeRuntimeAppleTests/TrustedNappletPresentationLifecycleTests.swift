import Foundation
import XCTest
@testable import NMPNativeRuntimeApple

final class TrustedNappletPresentationLifecycleTests:
    RuntimeNappletSessionTestCase
{
    @MainActor
    func testDismantlingAViewKeepsItsRustSessionAvailableForReparenting()
        throws
    {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-presentation-\(UUID().uuidString)",
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
            configuration: NativeRuntimeProfileConfiguration(
                storageRoot: root
            )
        )
        defer { profile.close() }
        let artifact = try profile.openSignedNamed(
            title: "Good Morning Presentation",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index],
            grantDomains: requiredGoodMorningDomains
        )
        let runtime = try XCTUnwrap(artifact.runtimeSession)
        defer { runtime.stop() }
        let view = TrustedNappletView(artifact: artifact)
        let coordinator = view.makeCoordinator()
        let webView = coordinator.makeWebView()

        coordinator.stop(webView)

        XCTAssertEqual(
            try profile.snapshotForTesting.sessions.first(where: {
                $0.id == runtime.sessionID
            })?.state,
            "running"
        )
        XCTAssertNotNil(
            try runtime.readSealed(logicalPath: "/index.html"),
            "a full-window layout transition must be able to mount a new view"
        )
    }

    func testAnOldPresentationCannotClearItsReplacementResponseSink()
        throws
    {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-response-sink-\(UUID().uuidString)",
                isDirectory: true
            )
        defer { try? FileManager.default.removeItem(at: root) }
        let fixture = repositoryRoot().appendingPathComponent(
            "conformance/napplet-corpus/published/good-morning",
            isDirectory: true
        )
        let profile = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(
                storageRoot: root
            )
        )
        defer { profile.close() }
        let artifact = try profile.openSignedNamed(
            title: "Good Morning Response Sink",
            eventJSON: try Data(
                contentsOf: fixture.appendingPathComponent("event.json")
            ),
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [
                indexDigest: try Data(
                    contentsOf: fixture.appendingPathComponent("index.html")
                ),
            ],
            grantDomains: requiredGoodMorningDomains
        )
        let runtime = try XCTUnwrap(artifact.runtimeSession)
        defer { runtime.stop() }
        let oldOwner = UUID()
        let newOwner = UUID()
        let received = expectation(
            description: "replacement receives the Rust response"
        )
        runtime.setResponseSink(owner: oldOwner) { _ in }
        runtime.setResponseSink(owner: newOwner) { bytes in
            guard
                let envelope = try? JSONSerialization.jsonObject(with: bytes)
                    as? [String: Any],
                envelope["type"] as? String == "shell.init"
            else {
                return
            }
            received.fulfill()
        }

        runtime.clearResponseSink(owner: oldOwner)
        runtime.mappedEnvelope(Data(#"{"type":"shell.ready"}"#.utf8))

        wait(for: [received], timeout: 2)
    }
}
