import Foundation
import NMPNativeRuntime
import XCTest
import WebKit
@testable import NMPNativeRuntimeApple

// MARK: - Profile ownership, session lifecycle, and capability registration

final class RuntimeProfileLifecycleTests: RuntimeNappletSessionTestCase {
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
}
