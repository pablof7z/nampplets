import Foundation
import NMPNativeRuntimeApple

/// One signed napplet handed to the app by a UI test at launch.
///
/// The Workbench ships no napplet. It has no bundled event, no bundled
/// artifact bytes, and no napplet's author, d-tag, or aggregate hash anywhere
/// in its sources. A UI test that needs a napplet on the canvas before it can
/// exercise permission review, the inspector, or a layout transition therefore
/// supplies one: the test process reads the pinned conformance corpus and
/// passes the bytes and coordinates through the launch environment.
///
/// Everything here is harness data. The seed names no build; it only carries
/// whichever build the test chose, and it installs it through
/// `installSignedNamed` -- the same verified install boundary the catalog
/// uses, so nothing about a seeded build is privileged once it is installed.
/// The runtime re-derives every digest, so a seed cannot smuggle unverified
/// bytes onto the canvas.
struct UITestNappletSeed: Sendable {
    /// Present only when the app is under UI test. Guards the whole mechanism
    /// so a normal launch can never be handed a napplet this way.
    static let scenarioKey = "NMP_WORKBENCH_UI_TEST_SCENARIO"
    static let titleKey = "NMP_WORKBENCH_UI_TEST_SEED_TITLE"
    static let authorKey = "NMP_WORKBENCH_UI_TEST_SEED_AUTHOR"
    static let dTagKey = "NMP_WORKBENCH_UI_TEST_SEED_D_TAG"
    static let aggregateKey = "NMP_WORKBENCH_UI_TEST_SEED_AGGREGATE"
    /// Base64 of the signed manifest event.
    static let eventKey = "NMP_WORKBENCH_UI_TEST_SEED_EVENT"
    /// `<sha256>=<base64>` pairs separated by `,`. The runtime re-derives each
    /// digest, so a mistyped pair fails the install rather than seeding
    /// unverified bytes.
    static let blobsKey = "NMP_WORKBENCH_UI_TEST_SEED_BLOBS"

    let title: String
    let identity: WorkbenchExactBuildIdentity
    let eventJSON: Data
    let blobsBySHA256: [String: Data]

    /// Reads the seed, or `nil` when this launch was handed none.
    ///
    /// A launch that names *some* seed variables but not all of them is a
    /// broken harness, not an unseeded launch, so it throws rather than
    /// quietly producing an empty canvas the test would then have to diagnose
    /// from a missing button.
    static func fromLaunchEnvironment(
        _ environment: [String: String] = ProcessInfo.processInfo.environment
    ) throws -> Self? {
        guard environment[scenarioKey] != nil else {
            return nil
        }
        let required = [authorKey, dTagKey, aggregateKey, eventKey]
        let present = required.filter { environment[$0]?.isEmpty == false }
        guard !present.isEmpty else {
            return nil
        }
        guard present.count == required.count else {
            throw UITestNappletSeedError.incomplete(
                missing: required.filter { environment[$0]?.isEmpty != false }
            )
        }
        guard let encodedEvent = environment[eventKey],
              let eventJSON = Data(base64Encoded: encodedEvent)
        else {
            throw UITestNappletSeedError.undecodable(eventKey)
        }
        guard let blobs = decodeBlobs(environment[blobsKey] ?? "") else {
            throw UITestNappletSeedError.undecodable(blobsKey)
        }
        let dTag = environment[dTagKey] ?? ""
        return Self(
            title: environment[titleKey] ?? dTag,
            identity: WorkbenchExactBuildIdentity(
                manifestAuthor: environment[authorKey] ?? "",
                dTag: dTag,
                aggregateHash: environment[aggregateKey] ?? ""
            ),
            eventJSON: eventJSON,
            blobsBySHA256: blobs
        )
    }

    func install(
        profile: WorkbenchRuntimeProfile
    ) throws -> NativeRuntimeInstalledArtifact {
        try profile.native.installSignedNamed(
            title: title,
            eventJSON: eventJSON,
            author: identity.manifestAuthor,
            dTag: identity.dTag,
            blobsBySHA256: blobsBySHA256
        )
    }

    private static func decodeBlobs(_ encoded: String) -> [String: Data]? {
        guard !encoded.isEmpty else {
            return [:]
        }
        var blobs: [String: Data] = [:]
        for pair in encoded.split(separator: ",") {
            let parts = pair.split(separator: "=", maxSplits: 1)
            guard parts.count == 2,
                  let bytes = Data(base64Encoded: String(parts[1]))
            else {
                return nil
            }
            blobs[String(parts[0])] = bytes
        }
        return blobs
    }
}

enum UITestNappletSeedError: Error, LocalizedError {
    case incomplete(missing: [String])
    case undecodable(String)

    var errorDescription: String? {
        switch self {
        case let .incomplete(missing):
            "The UI test napplet seed is incomplete; missing "
                + missing.joined(separator: ", ")
        case let .undecodable(key):
            "The UI test napplet seed in \(key) is not decodable base64."
        }
    }
}
