import Foundation
import XCTest

/// Hands the app under test one signed napplet at launch.
///
/// The Workbench bundles no napplet: #223 removed the fixture from the
/// product, so `bootstrapProfile()` installs nothing and the canvas starts
/// empty. A UI test that needs a napplet on the canvas before it can exercise
/// permission review, the inspector, or a layout transition therefore supplies
/// one itself.
///
/// The bytes come from `conformance/napplet-corpus`, the same pinned corpus
/// the Rust conformance suite runs against, read straight from the checkout by
/// this (unsandboxed) runner process. Nothing is copied into the repository a
/// second time and nothing is retyped: the author, d-tag, aggregate hash and
/// every blob digest are read from the corpus `index.json`, so a corpus
/// re-pin cannot leave a stale constant behind in a test.
///
/// The app receives them through the launch environment because it *is*
/// sandboxed and cannot open a path in this process's world. `UITestNappletSeed`
/// on the other side installs through `installSignedNamed`, the same verified
/// boundary the catalog uses, and the runtime re-derives every digest — so a
/// seeded build is neither privileged nor able to smuggle unverified bytes.
extension RuntimeWorkbenchUITests {
    enum SeedKey {
        static let title = "NMP_WORKBENCH_UI_TEST_SEED_TITLE"
        static let author = "NMP_WORKBENCH_UI_TEST_SEED_AUTHOR"
        static let dTag = "NMP_WORKBENCH_UI_TEST_SEED_D_TAG"
        static let aggregate = "NMP_WORKBENCH_UI_TEST_SEED_AGGREGATE"
        static let event = "NMP_WORKBENCH_UI_TEST_SEED_EVENT"
        static let blobs = "NMP_WORKBENCH_UI_TEST_SEED_BLOBS"
    }

    /// Seeds the pinned `good-morning` build and returns its d-tag, which
    /// tests assert on verbatim.
    @discardableResult
    func seedGoodMorning(into app: XCUIApplication) throws -> String {
        try seedCorpusNapplet(
            named: "good-morning",
            classification: "published",
            into: app
        )
    }

    /// Reads one fixture out of the pinned corpus and writes it into
    /// `app.launchEnvironment`.
    @discardableResult
    func seedCorpusNapplet(
        named name: String,
        classification: String,
        into app: XCUIApplication
    ) throws -> String {
        let directory = Self.corpusRoot
            .appendingPathComponent(classification, isDirectory: true)
        let index = try JSONDecoder().decode(
            CorpusIndex.self,
            from: try Data(
                contentsOf: directory.appendingPathComponent("index.json")
            )
        )
        let fixture = try XCTUnwrap(
            index.fixtures.first { $0.name == name },
            "The pinned \(classification) corpus has no fixture named "
                + "\(name). Looked in \(directory.path)."
        )
        let fixtureDirectory = directory.appendingPathComponent(
            name,
            isDirectory: true
        )

        // The manifest event is the one corpus file that is not an artifact
        // blob: artifacts carry the `artifact_path` they are served at.
        let eventFile = try XCTUnwrap(
            fixture.files.first { $0.artifactPath == nil },
            "Fixture \(name) declares no manifest event file"
        )
        let eventJSON = try Data(
            contentsOf: fixtureDirectory
                .appendingPathComponent(eventFile.path)
        )

        var blobs: [String] = []
        for file in fixture.files where file.artifactPath != nil {
            let bytes = try Data(
                contentsOf: fixtureDirectory
                    .appendingPathComponent(file.path)
            )
            blobs.append(
                "\(file.sha256)=\(bytes.base64EncodedString())"
            )
        }

        app.launchEnvironment[SeedKey.title] = fixture.title ?? name
        app.launchEnvironment[SeedKey.author] = fixture.coordinate.author
        app.launchEnvironment[SeedKey.dTag] = fixture.coordinate.dTag
        app.launchEnvironment[SeedKey.aggregate] = fixture.aggregateSHA256
        app.launchEnvironment[SeedKey.event] = eventJSON.base64EncodedString()
        app.launchEnvironment[SeedKey.blobs] = blobs.joined(separator: ",")
        return fixture.coordinate.dTag
    }

    /// `conformance/napplet-corpus`, located from this file rather than from a
    /// bundle resource so the corpus stays the single copy in the repository.
    /// This runner process is not sandboxed, and the checkout it was compiled
    /// from is the checkout it runs in.
    static var corpusRoot: URL {
        URL(fileURLWithPath: #filePath)
            // …/apps/workbench-macos/RuntimeWorkbenchUITests/<this file>
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("conformance", isDirectory: true)
            .appendingPathComponent("napplet-corpus", isDirectory: true)
    }

    struct CorpusIndex: Decodable {
        let fixtures: [Fixture]

        struct Fixture: Decodable {
            let name: String
            let title: String?
            let aggregateSHA256: String
            let coordinate: Coordinate
            let files: [File]

            private enum CodingKeys: String, CodingKey {
                case name
                case title
                case aggregateSHA256 = "aggregate_sha256"
                case coordinate
                case files
            }
        }

        struct Coordinate: Decodable {
            let author: String
            let dTag: String

            private enum CodingKeys: String, CodingKey {
                case author
                case dTag = "d_tag"
            }
        }

        struct File: Decodable {
            let path: String
            let sha256: String
            let artifactPath: String?

            private enum CodingKeys: String, CodingKey {
                case path
                case sha256
                case artifactPath = "artifact_path"
            }
        }
    }
}
