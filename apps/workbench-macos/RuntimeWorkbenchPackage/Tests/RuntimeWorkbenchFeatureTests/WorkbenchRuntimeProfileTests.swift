import Foundation
@testable import RuntimeWorkbenchFeature
import NMPNativeRuntimeApple
import Testing

@Test
func productionPermissionModeRequiresInteractiveExactBuildReview() {
    switch WorkbenchRuntimeProfile.productionPermissionMode {
    case .interactive:
        break
    case .demoPinnedGoodMorning:
        Issue.record(
            "Production must not auto-grant capabilities after the demo."
        )
    }
}

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
