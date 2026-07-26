import Foundation
@testable import RuntimeWorkbenchFeature
import NMPNativeRuntimeApple
import Testing

@Test
func productionNetworkInputsAreFiniteSecureAndRoleSeparated() throws {
    let inputs = try WorkbenchRuntimeProfile.operatorNetworkInputs(
        infoDictionary: [
            "NMPIndexerRelays": [
                "wss://purplepag.es",
                "wss://relay.primal.net",
            ],
            "NMPAppRelays": [
                "wss://relay.primal.net",
                "wss://relay.damus.io",
                "wss://nos.lol",
                "wss://relay.damus.io",
            ],
        ]
    )
    let indexers = inputs.indexerRelays
    let appRelays = inputs.appRelays

    #expect(indexers.count == 2)
    #expect(appRelays.count == 3)
    #expect(Set(indexers).count == indexers.count)
    #expect(Set(appRelays).count == appRelays.count)
    #expect((indexers + appRelays).allSatisfy { $0.hasPrefix("wss://") })
    #expect(indexers.contains("wss://purplepag.es"))
    #expect(appRelays.contains("wss://relay.damus.io"))
}

@Test
func productionNetworkInputsRefuseMissingOperatorLanes() {
    #expect(throws: (any Error).self) {
        try WorkbenchRuntimeProfile.operatorNetworkInputs(
            infoDictionary: [
                "NMPIndexerRelays": ["ws://insecure.example"],
                "NMPAppRelays": ["wss://relay.example"],
            ]
        )
    }
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
