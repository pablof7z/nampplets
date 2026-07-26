import NMPNativeRuntime
import XCTest

final class GeneratedBindingTests: XCTestCase {
    private let publishedAuthor =
        "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5"
    private let publishedIndexDigest =
        "ffd35eea5c84d03cdda74c23e1bbb2c40500f503833503aa688036faa52f3808"

    func testGeneratedSemanticTypesAreReachableFromPublicModule() {
        let profile: RuntimeExecutionProfile = .legacy
        let coordinate = ArtifactCoordinate.named(
            author: String(repeating: "0", count: 64),
            dTag: "fixture"
        )

        XCTAssertEqual(String(describing: profile), "legacy")
        XCTAssertNotNil(coordinate)
    }

    func testControllerOpensSnapshotsAndClosesAcrossActualFFI() throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }

        let controller = try RuntimeController.open(
            config: config(root: root),
            artifactSource: RefusingArtifactSource()
        )
        XCTAssertEqual(controller.snapshot().revision, 0)
        XCTAssertFalse(controller.snapshot().closed)
        controller.close()
        XCTAssertTrue(controller.snapshot().closed)
    }

    func testSignedArtifactInstallLaunchAndVerifiedReadCrossActualFFI() throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let repository = repositoryRoot()
        let event = try Data(
            contentsOf: repository.appendingPathComponent(
                "conformance/napplet-corpus/published/good-morning/event.json"
            )
        )
        let index = try Data(
            contentsOf: repository.appendingPathComponent(
                "conformance/napplet-corpus/published/good-morning/index.html"
            )
        )
        let source = FixtureArtifactSource(
            bytesByDigest: [publishedIndexDigest: index]
        )
        let controller = try RuntimeController.open(
            config: config(root: root),
            artifactSource: source
        )

        let verification = controller.verifyArtifact(
            eventJson: event,
            coordinate: .named(
                author: publishedAuthor,
                dTag: "good-morning"
            )
        )
        let artifact = try XCTUnwrap(verification.artifact)
        XCTAssertNil(verification.refusal)
        controller.install(artifact: artifact)
        let coordinate = RuntimeExactBuildCoordinate(
            manifestAuthor: publishedAuthor,
            dTag: "good-morning",
            aggregateHash: artifact.aggregateHash()
        )
        let review = try XCTUnwrap(
            controller.permissionReview(coordinate: coordinate).review
        )
        XCTAssertFalse(review.launchPermitted)
        XCTAssertEqual(
            review.capabilities.map(\.domain),
            ["identity", "inc", "outbox", "resource", "theme", "link"]
        )
        let permissionUpdate = controller.applyPermissionDecisions(
            batch: RuntimePermissionDecisionBatch(
                coordinate: coordinate,
                decisions: review.capabilities.map {
                    RuntimePermissionDecisionSelection(
                        domain: $0.domain,
                        decision: $0.requirement == .required
                            ? .allowExactBuild
                            : .denied
                    )
                }
            )
        )
        XCTAssertTrue(permissionUpdate.applied)
        XCTAssertTrue(permissionUpdate.review?.launchPermitted == true)
        XCTAssertNil(permissionUpdate.refusal)
        controller.setGrant(
            artifact: artifact,
            capability: "shell",
            sensitivity: .ordinary,
            decision: .allowExactBuild
        )
        controller.setGrant(
            artifact: artifact,
            capability: "storage",
            sensitivity: .ordinary,
            decision: .allowExactBuild
        )
        controller.launch(
            artifact: artifact,
            profile: .legacy
        )

        let session = try XCTUnwrap(controller.snapshot().sessions.first)
        let responses = ResponseRuntimeObserver()
        let observation = try XCTUnwrap(
            controller.observe(observer: responses).observation
        )
        defer { observation.stop() }
        controller.mappedEnvelope(
            sessionId: session.id,
            bytes: Data(#"{"type":"shell.ready"}"#.utf8)
        )
        let shellInit = try XCTUnwrap(
            responses.waitForResponse(type: "shell.init", id: nil, timeout: 2)
        )
        XCTAssertEqual(shellInit["type"] as? String, "shell.init")

        controller.mappedEnvelope(
            sessionId: session.id,
            bytes: Data(
                #"{"type":"storage.set","id":"set-1","key":"greeting","value":"hello"}"#.utf8
            )
        )
        let setResponse = try XCTUnwrap(
            responses.waitForResponse(
                type: "storage.set.result",
                id: "set-1",
                timeout: 2
            )
        )
        XCTAssertNil(setResponse["error"])

        controller.mappedEnvelope(
            sessionId: session.id,
            bytes: Data(
                #"{"type":"storage.get","id":"get-1","key":"greeting"}"#.utf8
            )
        )
        let getResponse = try XCTUnwrap(
            responses.waitForResponse(
                type: "storage.get.result",
                id: "get-1",
                timeout: 2
            )
        )
        XCTAssertEqual(getResponse["value"] as? String, "hello")

        switch controller.readVerified(
            sessionId: session.id,
            logicalPath: "/index.html",
            maximumBytes: 1_048_576
        ) {
        case let .bytes(bytes, mediaType, sha256):
            XCTAssertEqual(bytes, index)
            XCTAssertEqual(mediaType, "text/html; charset=utf-8")
            XCTAssertEqual(sha256, publishedIndexDigest)
        case let .refused(refusal):
            XCTFail("verified read refused: \(refusal.code): \(refusal.detail)")
        }
        controller.close()
    }

    func testConflatedObservationCallbackAndTeardownCrossActualFFI() throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        var boundedConfig = config(root: root)
        boundedConfig.maximumObservers = 1
        let controller = try RuntimeController.open(
            config: boundedConfig,
            artifactSource: RefusingArtifactSource()
        )
        let delivered = expectation(description: "initial conflated frame")
        let observer = RecordingRuntimeObserver(delivered: delivered)

        let start = controller.observe(observer: observer)
        let observation = try XCTUnwrap(start.observation)
        XCTAssertNil(start.refusal)
        let refused = controller.observe(
            observer: RecordingRuntimeObserver(delivered: nil)
        )
        XCTAssertNil(refused.observation)
        XCTAssertEqual(refused.refusal?.code, "observer-capacity")
        wait(for: [delivered], timeout: 2)
        XCTAssertEqual(observer.latestRevision, 0)
        observation.stop()
        controller.close()
    }

    private func temporaryRoot() throws -> URL {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("nmp-native-runtime-\(UUID().uuidString)")
        try FileManager.default.createDirectory(
            at: root,
            withIntermediateDirectories: true
        )
        return root
    }

    private func config(root: URL) -> RuntimeConfig {
        RuntimeConfig(
            runtimeStorePath: root.appendingPathComponent("runtime.sqlite3").path,
            nmpStorePath: nil,
            artifactCachePath: root.appendingPathComponent("artifacts").path,
            indexerRelays: [],
            appRelays: [],
            fallbackRelays: [],
            allowedLocalRelayHosts: [],
            maximumNmpRelays: 8,
            maximumBridgeWorkers: 4,
            maximumObservers: 2,
            maximumBoundaryEvents: 16,
            maximumConfigItems: 16,
            maximumConfigStringBytes: 16_384,
            maximumManifestBytes: 262_144,
            maximumArtifactFiles: 32,
            maximumArtifactFileBytes: 1_048_576,
            maximumArtifactTotalBytes: 4_194_304,
            maximumVerifiedReadBytes: 1_048_576,
            maximumBlobSources: 4,
            permissionMode: .interactive,
            permissionDefault: .askEveryTime
        )
    }

    private func repositoryRoot() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }
}

private final class RefusingArtifactSource: ArtifactSource, @unchecked Sendable {
    func fetch(request: ArtifactFetchRequest) -> ArtifactFetchResponse {
        .refused(reason: "not used by controller open smoke")
    }
}

private final class FixtureArtifactSource: ArtifactSource, @unchecked Sendable {
    private let bytesByDigest: [String: Data]

    init(bytesByDigest: [String: Data]) {
        self.bytesByDigest = bytesByDigest
    }

    func fetch(request: ArtifactFetchRequest) -> ArtifactFetchResponse {
        guard
            let bytes = bytesByDigest[request.expectedSha256],
            let sourceURL = request.candidateUrls.first
        else {
            return .refused(reason: "fixture digest not found")
        }
        return .body(sourceUrl: sourceURL, httpStatus: 200, bytes: bytes)
    }
}

private final class RecordingRuntimeObserver: RuntimeObserver, @unchecked Sendable {
    private let lock = NSLock()
    private let delivered: XCTestExpectation?
    private var revision: UInt64?

    init(delivered: XCTestExpectation?) {
        self.delivered = delivered
    }

    var latestRevision: UInt64? {
        lock.lock()
        defer { lock.unlock() }
        return revision
    }

    func update(frame: RuntimeObservationFrame) {
        lock.lock()
        revision = frame.snapshot.revision
        lock.unlock()
        delivered?.fulfill()
    }
}

private final class ResponseRuntimeObserver: RuntimeObserver, @unchecked Sendable {
    private let condition = NSCondition()
    private var responses: [String] = []

    func update(frame: RuntimeObservationFrame) {
        let delivered = frame.events.compactMap(\.responseJson)
        guard !delivered.isEmpty else { return }
        condition.lock()
        responses.append(contentsOf: delivered)
        condition.broadcast()
        condition.unlock()
    }

    func waitForResponse(
        type: String,
        id: String?,
        timeout: TimeInterval
    ) -> [String: Any]? {
        let deadline = Date().addingTimeInterval(timeout)
        condition.lock()
        defer { condition.unlock() }
        while true {
            if let response = responses.compactMap(decode).first(where: {
                $0["type"] as? String == type
                    && (id == nil || $0["id"] as? String == id)
            }) {
                return response
            }
            guard condition.wait(until: deadline) else {
                return nil
            }
        }
    }

    private func decode(_ raw: String) -> [String: Any]? {
        guard
            let data = raw.data(using: .utf8),
            let value = try? JSONSerialization.jsonObject(with: data)
        else {
            return nil
        }
        return value as? [String: Any]
    }
}
