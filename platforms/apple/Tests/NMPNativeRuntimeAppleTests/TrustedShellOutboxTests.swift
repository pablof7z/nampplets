import Foundation
import NMPNativeRuntime
import XCTest
import WebKit
@testable import NMPNativeRuntimeApple

// MARK: - Trusted shell outbox approval and receipt scenarios

final class TrustedShellOutboxTests: RuntimeNappletSessionTestCase {
    @MainActor
    func testTrustedShellOutboxPublishReachesNativeApprovalBoundary() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-outbox-shell-\(UUID().uuidString)",
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
                storageRoot: root
            )
        )
        defer { profile.close() }

        let registration = profile.registerLocalAccount(
            secretKey: String(format: "%064x", 23)
        )
        let handle = try XCTUnwrap(registration.handle)
        XCTAssertTrue(registration.accepted)
        XCTAssertTrue(profile.activateLocalAccount(handle: handle).accepted)

        let launched = try profile.openSignedNamed(
            title: "Good Morning Shell Write",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index],
            grantDomains: requiredGoodMorningDomains
        )
        let artifact = NappletArtifact(
            title: "Outbox bridge probe",
            reader: InMemoryVerifiedArtifactReader(files: [
                SealedArtifactBytes(
                    logicalPath: "/index.html",
                    sha256: String(repeating: "0", count: 64),
                    bytes: Data(
                        """
                        <!doctype html><html><body><script>
                        void window.napplet.outbox.publish({
                          kind: 1,
                          content: "native trusted-shell approval test",
                          tags: [],
                          created_at: Math.floor(Date.now() / 1000)
                        }).catch(function () {});
                        </script></body></html>
                        """.utf8
                    )
                )
            ]),
            runtimeSession: launched.runtimeSession,
            negotiatedDomains: launched.negotiatedDomains
        )
        let pendingUpdates = LockedPendingWriteUpdates()
        let pendingAppeared = expectation(description: "pending write projected")
        let pendingCleared = expectation(description: "pending write removed after rejection")
        let appearedSignaled = LockedFlag()
        let clearedSignaled = LockedFlag()
        let expectClear = LockedFlag()
        let pending = try profile.observePendingWrites { update in
            pendingUpdates.append(update)
            let projection: NativeRuntimePendingWriteProjection?
            switch update {
            case let .authoritative(value),
                 let .next(value, _, _):
                projection = value
            }
            if let projection {
                if !projection.writes.isEmpty && !appearedSignaled.value {
                    appearedSignaled.set(true)
                    pendingAppeared.fulfill()
                } else if projection.writes.isEmpty,
                          expectClear.value,
                          !clearedSignaled.value
                {
                    clearedSignaled.set(true)
                    pendingCleared.fulfill()
                }
            }
        }
        defer { pending.cancel() }

        let mounted = expectation(description: "trusted shell mounted")
        let publishRouted = expectation(description: "outbox publish reached Rust")
        let view = TrustedNappletView(artifact: artifact) { activity in
            switch activity {
            case .mounted:
                mounted.fulfill()
            case .request(type: "outbox.publish"):
                publishRouted.fulfill()
            default:
                break
            }
        }
        let coordinator = view.makeCoordinator()
        let webView = coordinator.makeWebView()
        defer { coordinator.stop(webView) }

        await fulfillment(of: [mounted], timeout: 10)
        // The probe artifact invokes outbox.publish from inside the isolated
        // iframe as soon as the trusted shell mounts it.
        await fulfillment(of: [publishRouted], timeout: 10)
        await fulfillment(of: [pendingAppeared], timeout: 10)
        let write = try XCTUnwrap(
            pendingUpdates.values.reversed().compactMap { update -> NativeRuntimePendingWrite? in
                switch update {
                case let .authoritative(projection),
                     let .next(projection, _, _):
                    return projection.writes.first
                }
            }.first
        )
        XCTAssertEqual(write.scope.manifestAuthor, author)
        XCTAssertEqual(write.scope.dTag, "good-morning")
        XCTAssertEqual(write.account, handle.publicKey)
        XCTAssertTrue(write.draftJSON.contains("native trusted-shell approval test"))

        expectClear.set(true)
        profile.decideProviderWrite(operationID: write.id, approve: false)
        await fulfillment(of: [pendingCleared], timeout: 10)
        XCTAssertTrue(try profile.snapshotForTesting.pendingWrites.isEmpty)
        XCTAssertTrue(try profile.snapshotForTesting.receipts.isEmpty)
    }

    @MainActor
    func testTrustedShellOutboxApprovalProducesCanonicalReceipt() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-outbox-receipt-\(UUID().uuidString)",
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

        let registration = profile.registerLocalAccount(
            secretKey: String(format: "%064x", 29)
        )
        let handle = try XCTUnwrap(registration.handle)
        XCTAssertTrue(registration.accepted)
        XCTAssertTrue(profile.activateLocalAccount(handle: handle).accepted)

        let launched = try profile.openSignedNamed(
            title: "Good Morning Receipt",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index],
            grantDomains: requiredGoodMorningDomains
        )
        let artifact = NappletArtifact(
            title: "Outbox receipt probe",
            reader: InMemoryVerifiedArtifactReader(files: [
                SealedArtifactBytes(
                    logicalPath: "/index.html",
                    sha256: String(repeating: "0", count: 64),
                    bytes: Data(
                        """
                        <!doctype html><html><body><script>
                        void window.napplet.outbox.publish({
                          kind: 1,
                          content: "native trusted-shell receipt test",
                          tags: [],
                          created_at: Math.floor(Date.now() / 1000)
                        }).catch(function () {});
                        </script></body></html>
                        """.utf8
                    )
                )
            ]),
            runtimeSession: launched.runtimeSession,
            negotiatedDomains: launched.negotiatedDomains
        )
        let pendingUpdates = LockedPendingWriteUpdates()
        let receiptUpdates = LockedReceiptUpdates()
        let pendingAppeared = expectation(description: "pending write projected")
        let pendingCleared = expectation(description: "pending write removed after approval")
        let receiptAppeared = expectation(description: "canonical receipt projected")
        let appearedSignaled = LockedFlag()
        let clearedSignaled = LockedFlag()
        let receiptSignaled = LockedFlag()
        let expectClear = LockedFlag()
        let pending = try profile.observePendingWrites { update in
            pendingUpdates.append(update)
            let projection: NativeRuntimePendingWriteProjection?
            switch update {
            case let .authoritative(value),
                 let .next(value, _, _):
                projection = value
            }
            if let projection {
                if !projection.writes.isEmpty && !appearedSignaled.value {
                    appearedSignaled.set(true)
                    pendingAppeared.fulfill()
                } else if projection.writes.isEmpty,
                          expectClear.value,
                          !clearedSignaled.value
                {
                    clearedSignaled.set(true)
                    pendingCleared.fulfill()
                }
            }
        }
        defer { pending.cancel() }
        let receipts = try profile.observeReceipts { update in
            receiptUpdates.append(update)
            let projection: NativeRuntimeReceiptProjection
            switch update {
            case let .authoritative(value),
                 let .next(value, _, _):
                projection = value
            }
            if !projection.receipts.isEmpty && !receiptSignaled.value {
                receiptSignaled.set(true)
                receiptAppeared.fulfill()
            }
        }
        defer { receipts.cancel() }

        let mounted = expectation(description: "trusted shell mounted")
        let publishRouted = expectation(description: "outbox publish reached Rust")
        let view = TrustedNappletView(artifact: artifact) { activity in
            switch activity {
            case .mounted:
                mounted.fulfill()
            case .request(type: "outbox.publish"):
                publishRouted.fulfill()
            default:
                break
            }
        }
        let coordinator = view.makeCoordinator()
        let webView = coordinator.makeWebView()
        defer { coordinator.stop(webView) }

        await fulfillment(of: [mounted, publishRouted, pendingAppeared], timeout: 10)
        let write = try XCTUnwrap(
            pendingUpdates.values.reversed().compactMap { update -> NativeRuntimePendingWrite? in
                switch update {
                case let .authoritative(projection),
                     let .next(projection, _, _):
                    return projection.writes.first
                }
            }.first
        )
        XCTAssertTrue(write.draftJSON.contains("native trusted-shell receipt test"))

        expectClear.set(true)
        profile.decideProviderWrite(operationID: write.id, approve: true)
        await fulfillment(of: [pendingCleared, receiptAppeared], timeout: 10)
        let receipt = try XCTUnwrap(
            receiptUpdates.values.reversed().compactMap { update -> NativeRuntimeReceipt? in
                switch update {
                case let .authoritative(projection),
                     let .next(projection, _, _):
                    return projection.receipts.last
                }
            }.first
        )
        XCTAssertFalse(receipt.id.isEmpty)
        // The old `XCTAssertFalse(receipt.delivery.isEmpty)` claimed only that
        // a delivery status existed at all. #217/#221 replaced that string
        // with a non-optional typed `outcome` plus `observationLifecycle`, so
        // that claim is now guaranteed by the type and cannot be false --
        // there is no equal-strength successor to write.
        //
        // The useful successor is a which-outcome assertion, and it is
        // deliberately NOT written here rather than guessed: this test opens a
        // profile with no relays configured and samples the first receipt
        // update to arrive after approval, so the outcome it observes is
        // timing-dependent (`InProgress` while NMP is still observing/signing/
        // delivering, versus a terminal classification once the empty relay
        // set resolves). Pinning one would be invented coverage and flaky.
        // A test that configures relays and awaits a terminal receipt is where
        // that assertion belongs.
        XCTAssertTrue(
            try profile.snapshotForTesting.receipts.contains {
                $0.receiptId == receipt.id
            }
        )
    }

}

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
