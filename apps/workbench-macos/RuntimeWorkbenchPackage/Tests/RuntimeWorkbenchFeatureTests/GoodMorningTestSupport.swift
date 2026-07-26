import Foundation
import NMPNativeRuntimeApple
@testable import RuntimeWorkbenchFeature

enum GoodMorningFixtureError: Error, LocalizedError {
    case missingResource(String)

    var errorDescription: String? {
        switch self {
        case let .missingResource(name):
            "The test-only signed fixture is missing \(name)."
        }
    }
}

struct GoodMorningFixture: Sendable {
    static let author =
        "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5"
    static let dTag = "good-morning"
    static let indexDigest =
        "ffd35eea5c84d03cdda74c23e1bbb2c40500f503833503aa688036faa52f3808"
    static let aggregateHash =
        "828a6df02afd56782ea20f805084acce65c53f7c37554948c1e0a64aa5a2b0a8"

    let eventJSON: Data
    let indexHTML: Data

    static func load() throws -> Self {
        let eventURL = try resourceURL(name: "event", extension: "json")
        let indexURL = try resourceURL(name: "index", extension: "html")
        return Self(
            eventJSON: try Data(contentsOf: eventURL),
            indexHTML: try Data(contentsOf: indexURL)
        )
    }

    func install(
        profile: WorkbenchRuntimeProfile
    ) throws -> NativeRuntimeInstalledArtifact {
        try profile.native.installSignedNamed(
            title: "Good Morning Protocol",
            eventJSON: eventJSON,
            author: Self.author,
            dTag: Self.dTag,
            blobsBySHA256: [Self.indexDigest: indexHTML]
        )
    }

    private static func resourceURL(
        name: String,
        extension pathExtension: String
    ) throws -> URL {
        if let nested = Bundle.module.url(
            forResource: name,
            withExtension: pathExtension,
            subdirectory: "GoodMorning"
        ) {
            return nested
        }
        if let flattened = Bundle.module.url(
            forResource: name,
            withExtension: pathExtension
        ) {
            return flattened
        }
        throw GoodMorningFixtureError.missingResource(
            "\(name).\(pathExtension)"
        )
    }
}

extension WorkbenchComponentID {
    static let goodMorning = Self(rawValue: "good-morning")
}

extension WorkbenchCanvasWindow {
    static let goodMorning = Self(
        id: WorkbenchWindowID(rawValue: "good-morning"),
        componentID: .goodMorning,
        exactBuild: WorkbenchExactBuildIdentity(
            manifestAuthor: GoodMorningFixture.author,
            dTag: GoodMorningFixture.dTag,
            aggregateHash: GoodMorningFixture.aggregateHash
        ),
        title: "Good Morning",
        frame: WorkbenchWindowFrame(
            x: 40,
            y: 40,
            width: 760,
            height: 520
        ),
        stackingOrder: 0
    )
}

enum GoodMorningTestSupportError: Error {
    case missingReview
    case permissionRefused
}

/// Exercises the production three-step boundary while keeping focused tests
/// concise. No test grants one capability at a time or launches as a side
/// effect of installation.
@MainActor
func installApproveAndLaunchGoodMorning(
    fixture: GoodMorningFixture,
    profile: WorkbenchRuntimeProfile
) throws -> NappletArtifact {
    let installed = try fixture.install(profile: profile)
    let result = profile.native.permissionReview(
        for: installed.permissionCoordinate
    )
    guard result.refusal == nil, let review = result.review else {
        throw GoodMorningTestSupportError.missingReview
    }
    let update = profile.native.applyPermissionDecisions(
        NativeRuntimePermissionDecisionBatch(
            coordinate: installed.permissionCoordinate,
            // The batch is bound to the exact review these decisions were read
            // from. Rust re-derives the live review's revision and refuses the
            // batch as stale if anything in the effective policy moved in
            // between, so this must be the revision of `review` itself and
            // never a value the caller made up.
            reviewRevision: review.revision,
            // Decide on provider availability, not on requirement. The
            // fixture declares every domain it wants as required, and a
            // domain the runtime registers no provider for can only ever be
            // denied -- `permission_decision_policy` invalidates every other
            // option for it, and launch drops it rather than injecting it.
            decisions: review.capabilities.map { capability in
                let available: Bool
                if case .available = capability.platformAvailability {
                    available = true
                } else {
                    available = false
                }
                return NativeRuntimePermissionDecisionSelection(
                    domain: capability.domain,
                    decision: available ? .allowExactBuild : .denied
                )
            }
        )
    )
    guard update.applied, update.refusal == nil else {
        throw GoodMorningTestSupportError.permissionRefused
    }
    return try profile.native.launchInstalled(installed)
}
