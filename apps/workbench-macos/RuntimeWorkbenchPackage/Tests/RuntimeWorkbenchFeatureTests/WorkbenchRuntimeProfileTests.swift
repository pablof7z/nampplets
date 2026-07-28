import Foundation
@testable import RuntimeWorkbenchFeature
import NMPNativeRuntimeApple
import Testing

/// The bundle lane is read, not judged.
///
/// Which relays are usable -- scheme, credentials, duplicates, the per-lane cap
/// -- is the runtime's call, and it records every relay it drops as a boundary
/// refusal. Deciding it here too would mean two hosts each holding an
/// approximate copy of the same rules. Rust's side of this contract is pinned
/// in `crates/runtime-ffi/src/tests/operator_relays.rs`.
@Test
func operatorLanesAreReadVerbatimForTheRuntimeToJudge() throws {
    let inputs = try WorkbenchRuntimeProfile.operatorNetworkInputs(
        infoDictionary: [
            "NMPIndexerRelays": [
                "  wss://purplepag.es  ",
                "wss://relay.primal.net",
            ],
            "NMPAppRelays": [
                "wss://relay.primal.net",
                "ws://insecure.example",
                "wss://relay.damus.io",
                "wss://relay.damus.io",
            ],
        ]
    )

    // Not even whitespace is repaired here: trimming would quietly fix one
    // class of plist mistake while silently discarding another. The runtime
    // judges every entry and names what it refuses.
    #expect(inputs.indexerRelays == ["  wss://purplepag.es  ", "wss://relay.primal.net"])
    // Passed through untouched: the duplicate and the insecure entry are the
    // runtime's to refuse, by name, rather than this layer's to disappear.
    #expect(inputs.appRelays == [
        "wss://relay.primal.net",
        "ws://insecure.example",
        "wss://relay.damus.io",
        "wss://relay.damus.io",
    ])
}

/// An absent lane is a broken bundle rather than a relay to judge, so it is
/// still caught here -- there is nothing to hand the runtime.
@Test
func productionNetworkInputsRefuseAnAbsentOperatorLane() {
    #expect(throws: (any Error).self) {
        try WorkbenchRuntimeProfile.operatorNetworkInputs(
            infoDictionary: ["NMPAppRelays": ["wss://relay.example"]]
        )
    }
    // A whitespace-only entry is a configured entry: it reaches the runtime,
    // which refuses it by name rather than letting this layer vanish it.
    let whitespace = try? WorkbenchRuntimeProfile.operatorNetworkInputs(
        infoDictionary: [
            "NMPIndexerRelays": ["   "],
            "NMPAppRelays": ["wss://relay.example"],
        ]
    )
    #expect(whitespace?.indexerRelays == ["   "])
}

@Test
func profilePreferencesFlowThroughTheSharedAppleConsumer() throws {
    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent(
            "workbench-preferences-\(UUID().uuidString)",
            isDirectory: true
        )
    defer { try? FileManager.default.removeItem(at: root) }

    let profile = try WorkbenchRuntimeProfile.open(storageRoot: root)
    defer { profile.close() }
    let restartRequired = try profile.savePreferences(
        WorkbenchProfilePreferences(
            appRelays: ["wss://home.example"],
            indexerRelays: ["wss://search.example"],
            permissionDefault: .allowSession
        )
    )
    #expect(restartRequired)
    let snapshot = profile.settingsSnapshot()
    #expect(snapshot.preferences?.appRelays == ["wss://home.example"])
    #expect(snapshot.preferences?.indexerRelays == ["wss://search.example"])
    #expect(snapshot.preferences?.permissionDefault == .allowSession)
}

@Test
func catalogInstallWarningsRenderRustsOwnRefusalInsteadOfLocalCopy() {
    let blocked = NativeRuntimeCatalogInstallEligibility(
        canInstall: false,
        blocker: NativeRuntimeCatalogFailure(
            code: "unsupported-manifest-identity",
            detail: "d_tag exceeds 256 bytes",
            provenance: []
        )
    )
    let warnings = WorkbenchCatalogInstallEligibility.warnings(for: blocked)

    #expect(warnings.count == 1)
    #expect(warnings.first?.id == "unsupported-manifest-identity")
    #expect(warnings.first?.severity == .blocking)
    #expect(warnings.first?.message == "d_tag exceeds 256 bytes")
}

@Test
func catalogInstallWarningsStaySilentWhenRustPermitsTheInstall() {
    let permitted = NativeRuntimeCatalogInstallEligibility(
        canInstall: true,
        blocker: nil
    )

    #expect(WorkbenchCatalogInstallEligibility.warnings(for: permitted).isEmpty)
}

@MainActor
@Test
func verifiedInstallEligibilityDoesNotInventPlatformCompatibility() throws {
    let author = String(repeating: "a", count: 64)
    let review = NativeRuntimeCatalogReview(
        token: "review-1",
        eventId: String(repeating: "c", count: 64),
        coordinate: "35129:\(author):good-morning",
        manifestAuthor: author,
        dTag: "good-morning",
        title: "Good Morning",
        description: nil,
        aggregateHash: String(repeating: "b", count: 64),
        capabilities: [],
        blobSources: ["https://example.com/\(String(repeating: "b", count: 64))"],
        provenance: [],
        installEligibility: NativeRuntimeCatalogInstallEligibility(
            canInstall: true,
            blocker: nil
        )
    )

    let projected = try #require(
        WorkbenchRuntimeProfile.projectCatalogReview(review)
    )

    #expect(projected.canInstall)
    #expect(projected.platformCompatibility.isEmpty)
}

/// Regression for an observation setup failure rendering as an empty list.
/// A closed profile makes `observePendingWrites`/`observeReceipts` throw
/// before any subscription is established; the model must record that as a
/// distinct, checkable failure rather than leaving `writes`/`receipts`
/// empty and indistinguishable from "nothing pending."
@MainActor
@Test
func pendingWriteModelRecordsAnObservationFailureInsteadOfLookingIdle() throws {
    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent(
            "workbench-pending-write-closed-\(UUID().uuidString)",
            isDirectory: true
        )
    defer { try? FileManager.default.removeItem(at: root) }
    let profile = try WorkbenchRuntimeProfile.open(storageRoot: root)
    profile.close()

    let model = RuntimeWorkbenchPendingWriteModel(profile: profile)

    #expect(model.writes.isEmpty)
    #expect(model.observationFailure != nil)
}

@MainActor
@Test
func receiptModelRecordsAnObservationFailureInsteadOfLookingIdle() throws {
    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent(
            "workbench-receipt-closed-\(UUID().uuidString)",
            isDirectory: true
        )
    defer { try? FileManager.default.removeItem(at: root) }
    let profile = try WorkbenchRuntimeProfile.open(storageRoot: root)
    profile.close()

    let model = RuntimeWorkbenchReceiptModel(profile: profile)

    #expect(model.receipts.isEmpty)
    #expect(model.observationFailure != nil)
}

@MainActor
@Test
func pendingWriteModelHasNoObservationFailureWhileTheProfileIsOpen() throws {
    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent(
            "workbench-pending-write-open-\(UUID().uuidString)",
            isDirectory: true
        )
    defer { try? FileManager.default.removeItem(at: root) }
    let profile = try WorkbenchRuntimeProfile.open(storageRoot: root)
    defer { profile.close() }

    let model = RuntimeWorkbenchPendingWriteModel(profile: profile)

    #expect(model.observationFailure == nil)
}
