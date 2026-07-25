import Foundation
import NMPNativeRuntime
import XCTest
import WebKit
@testable import NMPNativeRuntimeApple

final class RuntimeNappletSessionSignedBridgeTests: RuntimeNappletSessionTestCase {
    @MainActor
    func testSignedGoodMorningExecutesAuthoredBridgeTrafficInTrustedView() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-signed-good-morning-webview-\(UUID().uuidString)",
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

        let registration = profile.registerLocalAccount(
            secretKey: String(format: "%064x", 31)
        )
        let account = try XCTUnwrap(registration.handle)
        XCTAssertTrue(registration.accepted)
        XCTAssertTrue(profile.activateLocalAccount(handle: account).accepted)

        let installed = try profile.installSignedNamed(
            title: "Good Morning Signed WebView",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index]
        )
        let review = try XCTUnwrap(
            profile.permissionReview(for: installed.permissionCoordinate).review
        )
        let decisions = review.capabilities.map { capability in
            let decision: NativeRuntimeGrantDecision
            switch capability.platformAvailability {
            case .available:
                decision = .allowExactBuild
            case .unknown, .unavailable:
                decision = .denied
            }
            return NativeRuntimePermissionDecisionSelection(
                domain: capability.domain,
                decision: decision
            )
        }
        let permissionUpdate = profile.applyPermissionDecisions(
            NativeRuntimePermissionDecisionBatch(
                coordinate: installed.permissionCoordinate,
                decisions: decisions
            )
        )
        XCTAssertTrue(permissionUpdate.applied)
        XCTAssertNil(permissionUpdate.refusal)
        XCTAssertTrue(permissionUpdate.review?.launchPermitted == true)
        XCTAssertTrue(
            permissionUpdate.review?.capabilities.allSatisfy { capability in
                guard case .available = capability.platformAvailability else {
                    return true
                }
                return capability.existingDecision == .allowExactBuild
            } == true
        )

        let launched = try profile.launchInstalled(installed)
        let sealed = try XCTUnwrap(
            try launched.reader.readSealed(logicalPath: "/index.html")
        )
        XCTAssertEqual(sealed.logicalPath, "/index.html")
        XCTAssertEqual(sealed.sha256, indexDigest)
        XCTAssertEqual(sealed.bytes, index)
        XCTAssertTrue(launched.negotiatedDomains.contains("identity"))
        XCTAssertTrue(launched.negotiatedDomains.contains("outbox"))

        let mounted = expectation(description: "signed Good Morning mounted")
        let shellReady = expectation(
            description: "signed Good Morning emitted shell.ready"
        )
        let identityRequest = expectation(
            description: "signed Good Morning requested its active public key"
        )
        let mountedSignaled = LockedFlag()
        let shellReadySignaled = LockedFlag()
        let identitySignaled = LockedFlag()
        let view = TrustedNappletView(artifact: launched) { activity in
            switch activity {
            case .mounted:
                if !mountedSignaled.value {
                    mountedSignaled.set(true)
                    mounted.fulfill()
                }
            case .request(type: "shell.ready"):
                if !shellReadySignaled.value {
                    shellReadySignaled.set(true)
                    shellReady.fulfill()
                }
            case .request(type: "identity.getPublicKey"):
                if !identitySignaled.value {
                    identitySignaled.set(true)
                    identityRequest.fulfill()
                }
            case .refused(let reason):
                XCTFail("Signed Good Morning was refused: \(reason)")
            case .crashed:
                XCTFail("Signed Good Morning crashed its WebKit content process")
            default:
                break
            }
        }
        let coordinator = view.makeCoordinator()
        let webView = coordinator.makeWebView()
        defer { coordinator.stop(webView) }

        await fulfillment(
            of: [mounted, shellReady, identityRequest],
            timeout: 10
        )
    }
}

// A local copy rather than a shared/module-visible type: this test target
// already has more than one file-scoped `LockedFlag` (each file's own
// trivial atomic-bool test helper). Widening any one of them to internal
// previously caused Swift to misresolve an unrelated same-named type at a
// different call site in another file — see the commit that introduced this
// duplicate for the exact compiler error. Keeping each copy private avoids
// that class of cross-file ambiguity entirely.
private final class LockedFlag: @unchecked Sendable {
    private let lock = NSLock()
    private var storage = false

    var value: Bool {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }

    func set(_ value: Bool) {
        lock.lock()
        storage = value
        lock.unlock()
    }
}
