import CryptoKit
import Foundation
import NMPNativeRuntime
@testable import NMPNativeRuntimeApple
import WebKit
import XCTest

/// End-to-end proof that the real, published-shape nip29-groups/nip29-chat
/// napplets work through the genuine NAP-INTENT layer -- no hardcoded
/// routing, no synthetic stand-in content. Fixtures under `Fixtures/` are
/// the actual `dist/index.html` output of both napplets (built from
/// /Users/pablofernandez/Work/29napplet) plus a locally-signed manifest
/// event for each (a throwaway keypair; only the tag content matters here,
/// not the identity). nip29-chat is installed and granted but deliberately
/// never launched -- the assertion is that the intent dispatcher itself
/// launches it in reaction to a real click in nip29-groups' real UI,
/// loaded from its real configured relay.
final class Nip29IntentIntegrationTests: XCTestCase {
    private func fixture(_ napplet: String, _ file: String) throws -> Data {
        let url = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .appendingPathComponent("Fixtures/\(napplet)/\(file)")
        return try Data(contentsOf: url)
    }

    @MainActor
    func testClickingARealGroupLaunchesNip29ChatViaTheIntentLayer() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-nip29-intent-\(UUID().uuidString)",
                isDirectory: true
            )
        defer { try? FileManager.default.removeItem(at: root) }

        let chatEvent = try fixture("nip29-chat", "event.json")
        let chatHTML = try fixture("nip29-chat", "index.html")
        let groupsEvent = try fixture("nip29-groups", "event.json")
        let groupsHTML = try fixture("nip29-groups", "index.html")

        let chatAuthor = try XCTUnwrap(
            (try JSONSerialization.jsonObject(with: chatEvent) as? [String: Any])?["pubkey"]
                as? String
        )
        let groupsAuthor = try XCTUnwrap(
            (try JSONSerialization.jsonObject(with: groupsEvent) as? [String: Any])?["pubkey"]
                as? String
        )
        let chatDigest = sha256Hex(chatHTML)
        let groupsDigest = sha256Hex(groupsHTML)

        let profile = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(storageRoot: root)
        )
        defer { profile.close() }

        let activatedHandlers = LockedActivationRequests()
        profile.setIntentActivationHandler { request in
            activatedHandlers.append(request)
        }
        defer { profile.setIntentActivationHandler(nil) }

        // Install and grant the handler, but never launch it -- the
        // dispatcher itself must be the one that launches it.
        let installedChat = try profile.installSignedNamed(
            title: "NIP-29 Chat",
            eventJSON: chatEvent,
            author: chatAuthor,
            dTag: "nip29-chat",
            blobsBySHA256: [chatDigest: chatHTML]
        )
        let chatReview = try XCTUnwrap(
            profile.permissionReview(for: installedChat.permissionCoordinate).review
        )
        XCTAssertEqual(
            Set(chatReview.capabilities.map(\.domain)),
            Set(["relay", "identity", "inc"]),
            "nip29-chat's real manifest must declare exactly the capabilities its content needs"
        )
        let chatGrant = profile.applyPermissionDecisions(
            NativeRuntimePermissionDecisionBatch(
                coordinate: installedChat.permissionCoordinate,
                // Decisions are bound to the exact review they were read from;
                // Rust refuses the batch as stale if the live review moved.
                reviewRevision: chatReview.revision,
                decisions: chatReview.capabilities.map {
                    NativeRuntimePermissionDecisionSelection(
                        domain: $0.domain,
                        decision: .allowExactBuild
                    )
                }
            )
        )
        XCTAssertTrue(chatGrant.applied)
        XCTAssertTrue(
            try profile.snapshotForTesting.sessions.isEmpty,
            "granting permissions must never launch the handler"
        )

        // Install, grant, and launch the caller.
        let launchedGroups = try profile.openSignedNamed(
            title: "NIP-29 Groups",
            eventJSON: groupsEvent,
            author: groupsAuthor,
            dTag: "nip29-groups",
            blobsBySHA256: [groupsDigest: groupsHTML],
            grantDomains: ["relay", "config", "intent"]
        )

        let artifact = NappletArtifact(
            title: "nip29-groups",
            reader: InMemoryVerifiedArtifactReader(files: [
                SealedArtifactBytes(
                    logicalPath: "/index.html",
                    sha256: groupsDigest,
                    bytes: groupsHTML
                )
            ]),
            runtimeSession: launchedGroups.runtimeSession,
            negotiatedDomains: launchedGroups.negotiatedDomains
        )
        let mounted = expectation(description: "nip29-groups trusted shell mounted")
        let view = TrustedNappletView(artifact: artifact) { activity in
            switch activity {
            case .mounted:
                mounted.fulfill()
            case let .consoleEntry(level, message):
                print("[nip29-groups console \(level)] \(message)")
            case let .refused(reason):
                print("[nip29-groups refused] \(reason)")
            case .crashed:
                print("[nip29-groups crashed]")
            case let .request(type):
                print("[nip29-groups request] \(type)")
            case .loading:
                print("[nip29-groups loading]")
            }
        }
        let coordinator = view.makeCoordinator()
        let webView = coordinator.makeWebView()
        defer { coordinator.stop(webView) }
        await fulfillment(of: [mounted], timeout: 30)

        // The napplet's own rendered DOM lives inside the sandboxed iframe
        // (opaque origin, sandbox="allow-scripts"), not the trusted outer
        // document that `webView.evaluateJavaScript` targets by default --
        // querying it requires evaluating script directly in that frame.
        let groupDeadline = Date().addingTimeInterval(45)
        var didClickAGroup = false
        while Date() < groupDeadline {
            if let count = try? await coordinator.evaluateJavaScriptInSandbox(
                "document.querySelectorAll('.group-row').length"
            ) as? Int, count > 0 {
                _ = try? await coordinator.evaluateJavaScriptInSandbox(
                    "document.querySelector('.group-row').click()"
                )
                didClickAGroup = true
                break
            }
            try await Task.sleep(nanoseconds: 500_000_000)
        }
        // This step depends on a live third-party relay
        // (wss://groups.0xchat.com, the relay nip29-groups itself is
        // configured with). If it never delivers a group inside the
        // deadline, the precondition for exercising intent dispatch was
        // never established -- report that honestly as an unmet external
        // dependency rather than as a runtime defect this repository owns.
        try XCTSkipUnless(
            didClickAGroup,
            "no real NIP-29 group loaded from wss://groups.0xchat.com within the deadline"
        )

        // The intent dispatcher -- not this test -- must launch nip29-chat.
        // nip29-groups' own session already exists, so the wait must look
        // for a *second*, distinctly-tagged session rather than any session.
        // The native activation signal is delivered asynchronously on the
        // main queue, so it can land after the session itself appears --
        // wait for both before asserting either.
        let launchDeadline = Date().addingTimeInterval(15)
        while Date() < launchDeadline {
            let launched = try profile.snapshotForTesting.sessions
                .contains { $0.dTag == "nip29-chat" }
            if launched, !activatedHandlers.values.isEmpty {
                break
            }
            try await Task.sleep(nanoseconds: 200_000_000)
        }
        let sessions = try profile.snapshotForTesting.sessions
        let chatSession = sessions.first { $0.dTag == "nip29-chat" }
        XCTAssertNotNil(
            chatSession,
            "the caller's intent.open('nip29-group', ...) must have launched the real registered handler"
        )
        XCTAssertTrue(
            chatSession?.domains.contains("inc") == true,
            "the launched handler must have negotiated NAP-INC to receive the group payload"
        )
        XCTAssertFalse(
            activatedHandlers.values.isEmpty,
            "the native focus/launch signal must also have fired for the handler"
        )
        XCTAssertEqual(activatedHandlers.values.first?.dTag, "nip29-chat")
    }
}

private final class LockedActivationRequests: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: [NativeIntentActivationHandlerRequest] = []

    var values: [NativeIntentActivationHandlerRequest] {
        lock.lock()
        defer { lock.unlock() }
        return stored
    }

    func append(_ value: NativeIntentActivationHandlerRequest) {
        lock.lock()
        stored.append(value)
        lock.unlock()
    }
}

private func sha256Hex(_ data: Data) -> String {
    SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}
