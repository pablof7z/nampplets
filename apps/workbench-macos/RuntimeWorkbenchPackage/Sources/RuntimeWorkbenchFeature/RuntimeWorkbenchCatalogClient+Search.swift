import Foundation

extension RuntimeWorkbenchCatalogClient {
    public func search(
        _ request: CatalogSearchRequest
    ) async -> CatalogSearchResponse {
        if let profileBacking {
            return await profileBacking.browseCatalog(request)
        }
        if let loadIssue {
            return .unavailable(loadIssue)
        }

        let query = request.query
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
        let matches = query.isEmpty
            ? records
            : records.filter { $0.searchText.contains(query) }
        let entries = matches.map(\.entry)
        guard
            let evidence = CatalogBrowseEvidence(
                scope: .offlineFixture,
                queryWasLocalFilter: !query.isEmpty,
                locallyFilteredRows: UInt(records.count - matches.count),
                projectedRows: UInt(entries.count),
                projectionLimitedRows: 0,
                refusedRows: 0,
                window: .idle,
                sourceEvidence: [],
                shortfalls: []
            ),
            let page = CatalogSearchPage(
                entries: entries,
                hasMore: false,
                evidence: evidence
            )
        else {
            return .unavailable(
                CatalogIssue(
                    title: "Offline fixture is outside UI limits",
                    message: "The bundled offline projection exceeded its "
                        + "finite page limit and was not displayed."
                )
            )
        }
        return .page(page)
    }

    public func observeChanges(
        _ receive: @escaping @MainActor @Sendable () -> Void
    ) -> CatalogFeedObservation {
        profileBacking?.observeCatalogChanges(receive)
            ?? CatalogFeedObservation()
    }

    static func bundledIndexData(
        named name: String,
        bundle: Bundle = .module
    ) throws -> Data {
        if let nested = bundle.url(
            forResource: "\(name)-index",
            withExtension: "json",
            subdirectory: "Catalog"
        ) {
            return try Data(contentsOf: nested)
        }
        if let flattened = bundle.url(
            forResource: "\(name)-index",
            withExtension: "json"
        ) {
            return try Data(contentsOf: flattened)
        }
        throw CatalogResourceError.missing("\(name)-index.json")
    }

    static func loadRecords(bundle: Bundle) throws -> [CatalogRecord] {
        let decoder = JSONDecoder()
        let published = try decoder.decode(
            PublishedIndex.self,
            from: bundledIndexData(named: "published", bundle: bundle)
        )
        let reference = try decoder.decode(
            ReferenceIndex.self,
            from: bundledIndexData(named: "reference", bundle: bundle)
        )
        let kehto = try decoder.decode(
            KehtoIndex.self,
            from: bundledIndexData(named: "kehto", bundle: bundle)
        )

        guard published.schema == 1,
              published.classification == "published-immutable-artifacts",
              published.digest == publishedDigest,
              reference.schema == 1,
              reference.classification == "runtime-reference-fixtures",
              reference.digest == referenceDigest,
              kehto.schema == 1,
              kehto.classification == "kehto-source-corpus",
              kehto.digest == kehtoDigest,
              kehto.source.commit == kehtoCommit
        else {
            throw CatalogResourceError.unexpectedBaseline
        }

        var result = try published.fixtures.map(publishedRecord)
        result.append(contentsOf: try reference.fixtures.map(referenceRecord))
        result.append(contentsOf: try kehto.applications.map(kehtoRecord))
        return result
    }

    private static let publishedDigest =
        "4bbd1218609000deaa273ef43c232211a90515c481dbd1929c40536c1e44e466"
    private static let referenceDigest =
        "5013983282a03741305b2f9740e2268ea6c038843b6e2214b0f34cbd611fd70a"
    private static let kehtoDigest =
        "ea51178f523a911615ab84a0083d464aa729322384eda3ee35cffc09bbd506b2"
    private static let kehtoCommit =
        "bb3929b3523b75356fd65f658f9bd14c7ff697e4"

    private static func publishedRecord(
        fixture: PublishedFixture
    ) throws -> CatalogRecord {
        guard fixture.name == "good-morning",
              fixture.artifactMode == "single-file",
              fixture.aggregateSHA256 == GoodMorningFixture.aggregateHash,
              fixture.coordinate.author == GoodMorningFixture.author,
              fixture.coordinate.dTag == GoodMorningFixture.dTag,
              fixture.coordinate.kind == 35_129,
              fixture.eventID
                == "b330bfaefd2ddf268ebe4196403e6163533c54f41dabc3518bdc1a896c68f40e",
              fixture.files == [
                  PublishedFile(
                      artifactPath: nil,
                      bytes: 722,
                      path: "event.json",
                      sha256:
                          "66d2a7ed73973e422c86119c3b5c5f1914cb15bad1bfbddecb61cc2edf1c9c17"
                  ),
                  PublishedFile(
                      artifactPath: "/index.html",
                      bytes: 96_172,
                      path: "index.html",
                      sha256: GoodMorningFixture.indexDigest
                  ),
              ]
        else {
            throw CatalogResourceError.unexpectedPublishedFixture
        }

        let coordinate =
            "\(fixture.coordinate.kind):\(fixture.coordinate.author):"
            + fixture.coordinate.dTag
        guard let entry = CatalogEntry(
            id: "published:\(fixture.eventID)",
            title: "Good Morning Protocol",
            summary: "Pinned signed fixture that passes the shell-only legacy "
                + "host baseline; the complete provider journey is not ratified.",
            publisher: CatalogPublisher(
                displayName: nil,
                publicKey: fixture.coordinate.author
            ),
            coordinate: coordinate,
            compatibility: .incompatible(
                reason: "Current compatibility.lock advertises no macOS NAP "
                    + "domains. The pinned pass proves graceful shell-only boot, "
                    + "not the required identity, inc, and outbox journey."
            )
        ),
            let review = CatalogInstallReview(
                id: "published:\(fixture.eventID):\(fixture.aggregateSHA256)",
                title: "Good Morning Protocol",
                publisher: entry.publisher,
                coordinate: coordinate,
                exactAggregateHash: fixture.aggregateSHA256,
                sources: [
                    CatalogSourceProvenance(
                        id: "published-index",
                        kind: .approvedCatalog,
                        source: "published/index.json",
                        evidence: "published-immutable-artifacts · corpus digest "
                            + publishedDigest
                    ),
                    CatalogSourceProvenance(
                        id: "manifest-event",
                        kind: .manifestEvent,
                        source: "kind 35129 event \(fixture.eventID)",
                        evidence: "Publisher \(fixture.coordinate.author) · "
                            + "d=\(fixture.coordinate.dTag) · event.json "
                            + "SHA-256 \(fixture.files[0].sha256) · 722 bytes"
                    ),
                    CatalogSourceProvenance(
                        id: "artifact-index",
                        kind: .verifiedArtifactIndex,
                        source: "/index.html",
                        evidence: "SHA-256 \(fixture.files[1].sha256) · "
                            + "\(fixture.files[1].bytes) bytes · aggregate "
                            + fixture.aggregateSHA256
                    ),
                    CatalogSourceProvenance(
                        id: "artifact-sources",
                        kind: .artifact,
                        source: "https://cdn.hzrd149.com · "
                            + "https://blossom.ditto.pub",
                        evidence: "Exact server tags preserved by the bundled "
                            + "signed manifest; no fetch is performed here."
                    ),
                ],
                requiredDomains: ["identity", "inc", "outbox"],
                optionalDomains: ["resource", "theme", "link"],
                platformCompatibility: [
                    CatalogPlatformCompatibility(
                        id: "macos",
                        platform: "macOS",
                        status: .incompatible,
                        detail: "compatibility.lock advertises no macOS NAP "
                            + "domains. The pinned legacy-host report observes an "
                            + "exact-byte shell boot and visible capability "
                            + "absence, not the complete provider journey."
                    ),
                    CatalogPlatformCompatibility(
                        id: "ios",
                        platform: "iOS",
                        status: .unavailable,
                        detail: "Not run in the pinned reports; "
                            + "compatibility.lock advertises no iOS domains."
                    ),
                    CatalogPlatformCompatibility(
                        id: "android",
                        platform: "Android",
                        status: .unavailable,
                        detail: "Not run in the pinned reports; "
                            + "compatibility.lock advertises no Android domains."
                    ),
                ],
                warnings: [
                    CatalogWarning(
                        id: "baseline-unratified",
                        severity: .caution,
                        message: "The native-runtime-compat-v1 baseline is "
                            + "unratified and its overall legacy-host report is "
                            + "incomplete."
                    ),
                    CatalogWarning(
                        id: "provider-journey-unproven",
                        severity: .caution,
                        message: "The pass proves secure shell boot and graceful "
                            + "capability absence, not identity, inbox, outbox, "
                            + "resource, theme, or link behavior end to end."
                    ),
                    CatalogWarning(
                        id: "install-boundary-unavailable",
                        severity: .blocking,
                        message: "The Workbench has not connected the Rust "
                            + "resolver/install-only boundary. This review cannot "
                            + "install, launch, or grant the build."
                    ),
                ],
                updateRelationship: .firstInstall,
                canInstall: false
            )
        else {
            throw CatalogResourceError.outsideUILimits
        }

        return CatalogRecord(
            entry: entry,
            review: review,
            reviewIssue: nil,
            searchTerms: [
                fixture.name,
                fixture.coordinate.author,
                fixture.eventID,
                fixture.aggregateSHA256,
                "identity inc outbox resource theme link",
            ]
        )
    }

    private static func referenceRecord(
        fixture: ReferenceFixture
    ) throws -> CatalogRecord {
        let missing = fixture.requires.sorted()
        let reason: String
        if fixture.name == "missing-domain" {
            reason = "Incompatible: requires ble, which compatibility.lock "
                + "does not advertise on macOS."
        } else if fixture.name == "external-assets" {
            reason = "Unavailable: the pinned report did not run the external "
                + "asset module because its harness cannot register the native "
                + "artifact URL scheme."
        } else {
            reason = "Unavailable: reference-only compatibility fixture; it is "
                + "not a published signed install."
        }
        let compatibility: CatalogCompatibilitySummary =
            fixture.name == "missing-domain"
            ? .incompatible(reason: reason)
            : .unknown(reason: reason)
        let coordinate =
            "unavailable:reference/\(fixture.name)#\(fixture.aggregateSHA256)"
        guard let entry = CatalogEntry(
            id: "reference:\(fixture.name):\(fixture.aggregateSHA256)",
            title: fixture.name.displayCatalogTitle,
            summary: reason,
            publisher: CatalogPublisher(
                displayName: "Pinned conformance corpus",
                publicKey: "Unavailable — no signed publisher"
            ),
            coordinate: coordinate,
            compatibility: compatibility
        ) else {
            throw CatalogResourceError.outsideUILimits
        }
        return CatalogRecord(
            entry: entry,
            review: nil,
            reviewIssue: CatalogIssue(
                title: "Reference fixture unavailable",
                message: reason + " Exact aggregate: \(fixture.aggregateSHA256)."
            ),
            searchTerms: missing + [fixture.name, fixture.artifactMode, reason]
        )
    }

    private static func kehtoRecord(
        application: KehtoApplication
    ) throws -> CatalogRecord {
        let domains = application.requires.sorted()
        let domainText = domains.isEmpty ? "no declared domains" : domains.joined(
            separator: ", "
        )
        let reason = "Built, not run: the pinned macOS report preflight-blocked "
            + "this source application; required domains: \(domainText)."
        let coordinate =
            "unavailable:kehto/\(application.name)@\(application.gitTree)"
        guard let entry = CatalogEntry(
            id: "kehto:\(application.name):\(application.gitTree)",
            title: application.name.displayCatalogTitle,
            summary: reason,
            publisher: CatalogPublisher(
                displayName: "kehto/web @ \(kehtoCommit.prefix(12))",
                publicKey: "Unavailable — source corpus is not a signed manifest"
            ),
            coordinate: coordinate,
            compatibility: .incompatible(reason: reason)
        ) else {
            throw CatalogResourceError.outsideUILimits
        }
        return CatalogRecord(
            entry: entry,
            review: nil,
            reviewIssue: CatalogIssue(
                title: "Built source is not installable",
                message: reason + " Exact source tree: \(application.gitTree)."
            ),
            searchTerms: domains
                + [application.name, application.gitTree, "built not run"]
        )
    }
}

struct CatalogRecord {
    let entry: CatalogEntry
    let review: CatalogInstallReview?
    let reviewIssue: CatalogIssue?
    let searchText: String

    init(
        entry: CatalogEntry,
        review: CatalogInstallReview?,
        reviewIssue: CatalogIssue?,
        searchTerms: [String]
    ) {
        self.entry = entry
        self.review = review
        self.reviewIssue = reviewIssue
        searchText = (
            [
                entry.title,
                entry.summary,
                entry.publisher.visibleName,
                entry.publisher.publicKey,
                entry.coordinate,
                entry.compatibility.title,
                entry.compatibility.detail ?? "",
            ] + searchTerms
        )
        .joined(separator: "\n")
        .lowercased()
    }
}

private enum CatalogResourceError: Error, LocalizedError {
    case missing(String)
    case unexpectedBaseline
    case unexpectedPublishedFixture
    case outsideUILimits

    var errorDescription: String? {
        switch self {
        case let .missing(name):
            "Missing bundled resource \(name)."
        case .unexpectedBaseline:
            "Bundled catalog metadata does not match compatibility.lock."
        case .unexpectedPublishedFixture:
            "The bundled published fixture differs from the pinned exact build."
        case .outsideUILimits:
            "A bundled catalog entry is outside the finite UI limits."
        }
    }
}

private struct PublishedIndex: Decodable {
    let classification: String
    let digest: String
    let fixtures: [PublishedFixture]
    let schema: Int
}

private struct PublishedFixture: Decodable {
    let aggregateSHA256: String
    let artifactMode: String
    let coordinate: PublishedCoordinate
    let eventID: String
    let files: [PublishedFile]
    let name: String

    private enum CodingKeys: String, CodingKey {
        case aggregateSHA256 = "aggregate_sha256"
        case artifactMode = "artifact_mode"
        case coordinate
        case eventID = "event_id"
        case files
        case name
    }
}

private struct PublishedCoordinate: Decodable {
    let author: String
    let dTag: String
    let kind: Int

    private enum CodingKeys: String, CodingKey {
        case author
        case dTag = "d_tag"
        case kind
    }
}

private struct PublishedFile: Decodable, Equatable {
    let artifactPath: String?
    let bytes: Int
    let path: String
    let sha256: String

    private enum CodingKeys: String, CodingKey {
        case artifactPath = "artifact_path"
        case bytes
        case path
        case sha256
    }
}

private struct ReferenceIndex: Decodable {
    let classification: String
    let digest: String
    let fixtures: [ReferenceFixture]
    let schema: Int
}

private struct ReferenceFixture: Decodable {
    let aggregateSHA256: String
    let artifactMode: String
    let name: String
    let requires: [String]

    private enum CodingKeys: String, CodingKey {
        case aggregateSHA256 = "aggregate_sha256"
        case artifactMode = "artifact_mode"
        case name
        case requires
    }
}

private struct KehtoIndex: Decodable {
    let applications: [KehtoApplication]
    let classification: String
    let digest: String
    let schema: Int
    let source: KehtoSource
}

private struct KehtoApplication: Decodable {
    let gitTree: String
    let name: String
    let requires: [String]

    private enum CodingKeys: String, CodingKey {
        case gitTree = "git_tree"
        case name
        case requires
    }
}

private struct KehtoSource: Decodable {
    let commit: String
}

private extension String {
    var displayCatalogTitle: String {
        split(separator: "-")
            .map { word in
                guard let first = word.first else {
                    return ""
                }
                return first.uppercased() + word.dropFirst()
            }
            .joined(separator: " ")
    }
}
