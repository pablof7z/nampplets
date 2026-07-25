import Foundation
import NMPNativeRuntime
import XCTest
import WebKit
@testable import NMPNativeRuntimeApple

final class RuntimeNappletSessionTests: XCTestCase {
    let author =
        "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5"
    let indexDigest =
        "ffd35eea5c84d03cdda74c23e1bbb2c40500f503833503aa688036faa52f3808"
    private let requiredGoodMorningDomains = ["identity", "inc", "outbox"]

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
        XCTAssertTrue(profile.snapshotForTesting.sessions.isEmpty)
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
            profile.snapshotForTesting.sessions.isEmpty,
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
        XCTAssertTrue(profile.snapshotForTesting.sessions.isEmpty)

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
            profile.snapshotForTesting.sessions.isEmpty,
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
                storageRoot: root,
                permissionMode: .demoPinnedGoodMorning
            )
        )
        defer { profile.close() }

        let registration = profile.registerLocalAccount(
            secretKey: String(format: "%064x", 23)
        )
        let handle = try XCTUnwrap(registration.handle)
        XCTAssertTrue(registration.accepted)
        XCTAssertTrue(profile.activateLocalAccount(handle: handle).accepted)

        let installed = try profile.installSignedNamed(
            title: "Good Morning Shell Write",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index]
        )
        let launched = try profile.launchInstalled(installed)
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
        XCTAssertTrue(profile.snapshotForTesting.pendingWrites.isEmpty)
        XCTAssertTrue(profile.snapshotForTesting.receipts.isEmpty)
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
                storageRoot: root,
                permissionMode: .demoPinnedGoodMorning
            )
        )
        defer { profile.close() }

        let registration = profile.registerLocalAccount(
            secretKey: String(format: "%064x", 29)
        )
        let handle = try XCTUnwrap(registration.handle)
        XCTAssertTrue(registration.accepted)
        XCTAssertTrue(profile.activateLocalAccount(handle: handle).accepted)

        let installed = try profile.installSignedNamed(
            title: "Good Morning Receipt",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index]
        )
        let launched = try profile.launchInstalled(installed)
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
        XCTAssertFalse(receipt.delivery.isEmpty)
        XCTAssertTrue(
            profile.snapshotForTesting.receipts.contains {
                $0.receiptId == receipt.id
            }
        )
    }

    func testInstalledHandleCannotCrossRuntimeProfiles() throws {
        let firstRoot = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-installed-owner-a-\(UUID().uuidString)",
                isDirectory: true
            )
        let secondRoot = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-installed-owner-b-\(UUID().uuidString)",
                isDirectory: true
            )
        defer {
            try? FileManager.default.removeItem(at: firstRoot)
            try? FileManager.default.removeItem(at: secondRoot)
        }
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
        let first = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(
                storageRoot: firstRoot
            )
        )
        let second = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(
                storageRoot: secondRoot
            )
        )
        defer {
            first.close()
            second.close()
        }

        let installed = try first.installSignedNamed(
            title: "Good Morning Owner",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index]
        )
        XCTAssertThrowsError(try second.launchInstalled(installed)) { error in
            XCTAssertEqual(
                error as? RuntimeNappletOpenError,
                .installedArtifactProfileMismatch
            )
        }
        XCTAssertTrue(second.snapshotForTesting.sessions.isEmpty)
    }

    func testStoppingOneSessionDoesNotCloseSharedProfileOrSibling() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-shared-profile-\(UUID().uuidString)",
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

        let first = try profile.openSignedNamed(
            title: "Good Morning One",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index],
            grantDomains: requiredGoodMorningDomains + ["storage"]
        )
        let second = try profile.openSignedNamed(
            title: "Good Morning Two",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index],
            grantDomains: requiredGoodMorningDomains + ["storage"]
        )
        let firstRuntime = try XCTUnwrap(first.runtimeSession)
        let secondRuntime = try XCTUnwrap(second.runtimeSession)
        XCTAssertNotEqual(firstRuntime.sessionID, secondRuntime.sessionID)

        firstRuntime.stop()
        XCTAssertFalse(profile.snapshotForTesting.closed)
        XCTAssertEqual(
            profile.snapshotForTesting.sessions.first(where: {
                $0.id == secondRuntime.sessionID
            })?.state,
            "running"
        )

        let received = expectation(
            description: "Sibling still receives provider responses"
        )
        secondRuntime.setResponseSink { bytes in
            guard let envelope = try? JSONSerialization.jsonObject(with: bytes)
                    as? [String: Any],
                  envelope["type"] as? String == "shell.init"
            else {
                return
            }
            received.fulfill()
        }
        secondRuntime.mappedEnvelope(Data(#"{"type":"shell.ready"}"#.utf8))
        wait(for: [received], timeout: 2)
        secondRuntime.stop()
    }

    func testClosingProfileInvalidatesEveryBorrowedSession() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-profile-close-\(UUID().uuidString)",
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
        let artifact = try profile.openSignedNamed(
            title: "Good Morning Close",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index],
            grantDomains: requiredGoodMorningDomains + ["storage"]
        )
        let runtime = try XCTUnwrap(artifact.runtimeSession)
        let unexpectedResponse = expectation(
            description: "Closed sessions cannot receive responses"
        )
        unexpectedResponse.isInverted = true
        runtime.setResponseSink { _ in unexpectedResponse.fulfill() }

        profile.close()

        XCTAssertTrue(profile.snapshotForTesting.closed)
        XCTAssertNil(
            try runtime.readSealed(logicalPath: "/index.html")
        )
        runtime.mappedEnvelope(Data(#"{"type":"shell.ready"}"#.utf8))
        wait(for: [unexpectedResponse], timeout: 0.1)
    }

    func testProfileRegistersNativeThemeAndConfigCapabilities() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-native-providers-\(UUID().uuidString)",
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
            configuration: NativeRuntimeProfileConfiguration(storageRoot: root)
        )
        defer { profile.close() }
        let artifact = try profile.openSignedNamed(
            title: "Good Morning Native Providers",
            eventJSON: event,
            author: author,
            dTag: "good-morning",
            blobsBySHA256: [indexDigest: index],
            grantDomains: requiredGoodMorningDomains + ["theme", "config"]
        )
        XCTAssertTrue(artifact.negotiatedDomains.contains("theme"))
        XCTAssertTrue(artifact.negotiatedDomains.contains("config"))
        let runtime = try XCTUnwrap(artifact.runtimeSession)
        defer { runtime.stop() }

        let themeReceived = expectation(description: "native theme response")
        let configReceived = expectation(description: "persisted config defaults")
        runtime.setResponseSink { bytes in
            guard let envelope = try? JSONSerialization.jsonObject(with: bytes)
                    as? [String: Any],
                  let type = envelope["type"] as? String
            else {
                return
            }
            if type == "theme.get.result",
               let theme = envelope["theme"] as? [String: Any],
               let colors = theme["colors"] as? [String: String],
               colors["background"]?.hasPrefix("#") == true,
               colors["text"]?.hasPrefix("#") == true,
               colors["primary"]?.hasPrefix("#") == true
            {
                themeReceived.fulfill()
            }
            if type == "config.values",
               let values = envelope["values"] as? [String: Any],
               values["enabled"] as? Bool == true
            {
                configReceived.fulfill()
            }
        }
        runtime.mappedEnvelope(Data(#"{"type":"shell.ready"}"#.utf8))
        runtime.mappedEnvelope(Data(#"{"type":"theme.get","id":"theme-1"}"#.utf8))
        runtime.mappedEnvelope(
            Data(
                #"{"type":"config.registerSchema","id":"schema-1","schema":{"$version":1,"type":"object","properties":{"enabled":{"type":"boolean","default":true}},"additionalProperties":false},"version":1}"#.utf8
            )
        )
        runtime.mappedEnvelope(Data(#"{"type":"config.get","id":"config-1"}"#.utf8))
        wait(for: [themeReceived, configReceived], timeout: 2)
    }

    func testInstalledLibraryObserverStartsWithAuthoritativeReplacementAndPushesNextRevision() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-library-observer-\(UUID().uuidString)",
                isDirectory: true
            )
        defer { try? FileManager.default.removeItem(at: root) }
        let profile = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(storageRoot: root)
        )
        defer { profile.close() }

        let receivedNext = expectation(
            description: "library observer receives the next replacement"
        )
        let updates = LockedLibraryUpdates()
        let observation = try profile.observeInstalledLibrary { update in
            updates.append(update)
            if case .next = update {
                receivedNext.fulfill()
            }
        }
        defer { observation.cancel() }

        let initial = try XCTUnwrap(updates.values.first)
        guard case let .authoritative(initialProjection) = initial else {
            return XCTFail("The first update must be authoritative")
        }
        let initialSnapshot = try librarySnapshot(initialProjection)
        XCTAssertEqual(initialSnapshot.filterQuery, "")
        XCTAssertEqual(initialSnapshot.totalInstalled, 0)
        XCTAssertTrue(initialSnapshot.builds.isEmpty)

        profile.setInstalledLibraryFilter("morning")
        wait(for: [receivedNext], timeout: 2)

        let next = try XCTUnwrap(
            updates.values.first(where: {
                if case .next = $0 {
                    return true
                }
                return false
            })
        )
        guard case let .next(
            nextProjection,
            predecessorRevision,
            eventCursorWasStale
        ) = next else {
            return XCTFail("Expected a next library replacement")
        }
        let nextSnapshot = try librarySnapshot(nextProjection)
        XCTAssertEqual(predecessorRevision, initialSnapshot.revision)
        XCTAssertGreaterThan(nextSnapshot.revision, initialSnapshot.revision)
        XCTAssertFalse(eventCursorWasStale)
        XCTAssertEqual(nextSnapshot.filterQuery, "morning")
        XCTAssertEqual(profile.installedLibraryProjection(), nextProjection)
    }

    func testInstalledLibraryObserverQueuesLatestUpdateUntilAuthoritativeDeliveryCompletes() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-library-ordering-\(UUID().uuidString)",
                isDirectory: true
            )
        defer { try? FileManager.default.removeItem(at: root) }
        let profile = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(storageRoot: root)
        )
        defer { profile.close() }

        let initialSnapshot = profile.snapshotForTesting
        let authoritativeStarted = expectation(
            description: "authoritative delivery started"
        )
        let registrationFinished = expectation(
            description: "observer registration drained pending replacement"
        )
        let allowAuthoritativeToFinish = DispatchSemaphore(value: 0)
        let updates = LockedLibraryUpdates()
        let observation = LockedLibraryObservation()

        DispatchQueue.global().async {
            let registered = try? profile.observeInstalledLibrary { update in
                if case .authoritative = update {
                    authoritativeStarted.fulfill()
                    _ = allowAuthoritativeToFinish.wait(
                        timeout: .now() + 5
                    )
                }
                updates.append(update)
            }
            observation.set(registered)
            registrationFinished.fulfill()
        }

        wait(for: [authoritativeStarted], timeout: 2)

        var intermediate = initialSnapshot
        intermediate.revision += 1
        intermediate.installedLibrary.query = "intermediate"
        profile.update(
            frame: RuntimeObservationFrame(
                snapshot: intermediate,
                catalog: profile.catalogSnapshotForTesting,
                events: [],
                oldestAvailableEvent: 0,
                newestAvailableEvent: 0,
                eventCursorWasStale: false
            )
        )
        var latest = intermediate
        latest.revision += 1
        latest.installedLibrary.query = "latest"
        profile.update(
            frame: RuntimeObservationFrame(
                snapshot: latest,
                catalog: profile.catalogSnapshotForTesting,
                events: [],
                oldestAvailableEvent: 0,
                newestAvailableEvent: 0,
                eventCursorWasStale: true
            )
        )

        allowAuthoritativeToFinish.signal()
        wait(for: [registrationFinished], timeout: 2)
        defer { observation.value?.cancel() }

        let delivered = updates.values
        XCTAssertEqual(delivered.count, 2)
        guard case let .authoritative(authoritative) = delivered.first else {
            return XCTFail("Authoritative replacement must be delivered first")
        }
        XCTAssertEqual(authoritative.revision, initialSnapshot.revision)
        guard case let .next(
            nextProjection,
            predecessorRevision,
            eventCursorWasStale
        ) = delivered.last else {
            return XCTFail("The newest pending replacement must drain second")
        }
        let nextSnapshot = try librarySnapshot(nextProjection)
        XCTAssertEqual(nextSnapshot.revision, latest.revision)
        XCTAssertEqual(nextSnapshot.filterQuery, "latest")
        XCTAssertEqual(predecessorRevision, intermediate.revision)
        XCTAssertTrue(eventCursorWasStale)
    }

    func testInstalledLibraryObserverCapacityCancellationAndClosedRefusal() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-library-capacity-\(UUID().uuidString)",
                isDirectory: true
            )
        defer { try? FileManager.default.removeItem(at: root) }
        let profile = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(storageRoot: root)
        )
        var observations: [NativeRuntimeLibraryObservation] = []
        for _ in 0 ..< 8 {
            let updates = LockedLibraryUpdates()
            let observation = try profile.observeInstalledLibrary(updates.append)
            observations.append(observation)
            guard case .authoritative = try XCTUnwrap(updates.values.first) else {
                return XCTFail("Every admitted observer needs an immediate replacement")
            }
        }

        XCTAssertThrowsError(
            try profile.observeInstalledLibrary { _ in }
        ) { error in
            XCTAssertEqual(
                error as? NativeRuntimeLibraryObservationError,
                .observerCapacity(maximum: 8)
            )
        }

        observations.removeLast().cancel()
        let replacementUpdates = LockedLibraryUpdates()
        let replacement = try profile.observeInstalledLibrary(
            replacementUpdates.append
        )
        guard case .authoritative =
            try XCTUnwrap(replacementUpdates.values.first)
        else {
            return XCTFail("Cancellation must release observer capacity")
        }
        replacement.cancel()
        observations.forEach { $0.cancel() }

        profile.close()
        XCTAssertThrowsError(
            try profile.observeInstalledLibrary { _ in }
        ) { error in
            XCTAssertEqual(
                error as? NativeRuntimeLibraryObservationError,
                .profileClosed
            )
        }
        XCTAssertTrue(
            try librarySnapshot(profile.installedLibraryProjection())
                .profileClosed
        )
    }

    func testPendingWriteAndReceiptObserversAreBoundedAndProjectReplacements() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-apple-write-observers-\(UUID().uuidString)",
                isDirectory: true
            )
        defer { try? FileManager.default.removeItem(at: root) }
        let profile = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(storageRoot: root)
        )
        defer { profile.close() }

        let pendingUpdates = LockedPendingWriteUpdates()
        let pendingObservation = try profile.observePendingWrites(
            pendingUpdates.append
        )
        defer { pendingObservation.cancel() }
        let receiptUpdates = LockedReceiptUpdates()
        let receiptObservation = try profile.observeReceipts(
            receiptUpdates.append
        )
        defer { receiptObservation.cancel() }

        guard case let .authoritative(pendingInitial) =
            try XCTUnwrap(pendingUpdates.values.first)
        else {
            return XCTFail("Pending writes must start with an authoritative replacement")
        }
        XCTAssertTrue(pendingInitial.writes.isEmpty)
        guard case let .authoritative(receiptInitial) =
            try XCTUnwrap(receiptUpdates.values.first)
        else {
            return XCTFail("Receipts must start with an authoritative replacement")
        }
        XCTAssertTrue(receiptInitial.receipts.isEmpty)

        var snapshot = profile.snapshotForTesting
        snapshot.revision += 1
        snapshot.receipts = [
            RuntimeReceiptSnapshot(
                receiptId: "receipt-1",
                delivery: "pending",
                latestStateJson: #"{"status":"queued"}"#
            )
        ]
        profile.update(
            frame: RuntimeObservationFrame(
                snapshot: snapshot,
                catalog: profile.catalogSnapshotForTesting,
                events: [],
                oldestAvailableEvent: 0,
                newestAvailableEvent: 0,
                eventCursorWasStale: false
            )
        )

        guard case let .next(receiptNext, predecessorRevision, _) =
            try XCTUnwrap(receiptUpdates.values.last)
        else {
            return XCTFail("Receipt updates must push a next replacement")
        }
        XCTAssertEqual(predecessorRevision, receiptInitial.revision)
        XCTAssertEqual(receiptNext.receipts.first?.id, "receipt-1")
        XCTAssertEqual(
            receiptNext.receipts.first?.latestStateJSON,
            #"{"status":"queued"}"#
        )

        pendingObservation.cancel()
        var pendingObservers: [NativeRuntimePendingWriteObservation] = []
        for _ in 0 ..< 8 {
            pendingObservers.append(try profile.observePendingWrites { _ in })
        }
        XCTAssertThrowsError(try profile.observePendingWrites { _ in }) { error in
            XCTAssertEqual(
                error as? NativeRuntimePendingWriteObservationError,
                .observerCapacity(maximum: 8)
            )
        }
        pendingObservers.removeLast().cancel()
        pendingObservers.forEach { $0.cancel() }
    }

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
        XCTAssertEqual(
            build.sessions,
            [
                NativeRuntimeLibrarySession(
                    id: runtime.sessionID,
                    state: .running
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
        XCTAssertEqual(snapshot.builds.first?.sessions.first?.state, .running)

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

    func testNativeSettingsDocumentFailsClosedForInvalidOrOversizedJSON() {
        let request = NativeSettingsRequest(
            manifestAuthor: String(repeating: "a", count: 64),
            dTag: "settings",
            aggregateHash: String(repeating: "b", count: 64),
            sessionId: 7,
            section: nil,
            schemaJson: #"{"type":"object","properties":{}}"#,
            valuesJson: "{}"
        )
        XCTAssertNotNil(NativeSettingsDocument.decode(request))
        var invalid = request
        invalid.schemaJson = "[]"
        XCTAssertNil(NativeSettingsDocument.decode(invalid))
        invalid = request
        invalid.valuesJson = String(repeating: "x", count: 192 * 1_024 + 1)
        XCTAssertNil(NativeSettingsDocument.decode(invalid))
    }

    func repositoryRoot() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }

    private func librarySnapshot(
        _ projection: NativeRuntimeLibraryProjection
    ) throws -> NativeRuntimeLibrarySnapshot {
        guard case let .snapshot(snapshot) = projection else {
            XCTFail("Expected a complete installed-library snapshot")
            throw RuntimeNappletSessionTestError.expectedLibrarySnapshot
        }
        return snapshot
    }
}

private enum RuntimeNappletSessionTestError: Error {
    case expectedLibrarySnapshot
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

private final class LockedLibraryUpdates: @unchecked Sendable {
    private let lock = NSLock()
    private var storage: [NativeRuntimeLibraryUpdate] = []

    var values: [NativeRuntimeLibraryUpdate] {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }

    func append(_ update: NativeRuntimeLibraryUpdate) {
        lock.lock()
        storage.append(update)
        lock.unlock()
    }
}

private final class LockedPendingWriteUpdates: @unchecked Sendable {
    private let lock = NSLock()
    private var storage: [NativeRuntimePendingWriteUpdate] = []

    var values: [NativeRuntimePendingWriteUpdate] {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }

    func append(_ update: NativeRuntimePendingWriteUpdate) {
        lock.lock()
        storage.append(update)
        lock.unlock()
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

private final class LockedReceiptUpdates: @unchecked Sendable {
    private let lock = NSLock()
    private var storage: [NativeRuntimeReceiptUpdate] = []

    var values: [NativeRuntimeReceiptUpdate] {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }

    func append(_ update: NativeRuntimeReceiptUpdate) {
        lock.lock()
        storage.append(update)
        lock.unlock()
    }
}

private final class LockedLibraryObservation: @unchecked Sendable {
    private let lock = NSLock()
    private var storage: NativeRuntimeLibraryObservation?

    var value: NativeRuntimeLibraryObservation? {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }

    func set(_ observation: NativeRuntimeLibraryObservation?) {
        lock.lock()
        storage = observation
        lock.unlock()
    }
}
