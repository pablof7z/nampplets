import SwiftUI
import Testing

@testable import RuntimeWorkbenchFeature

private let samplePreferences = WorkbenchProfilePreferences(
    appRelays: ["wss://relay.example"],
    indexerRelays: ["wss://search.example"],
    permissionDefault: .askEveryTime
)

private let sampleStorage = WorkbenchStorageSummary(
    networkBytes: 1_024,
    appBytes: 2_048,
    totalBytes: 3_072,
    isEstimate: false
)

@MainActor
@Test
func settingsSnapshotCarriesPreferencesWithoutPathsOrSecrets() {
    let snapshot = WorkbenchSettingsSnapshot(
        preferences: samplePreferences,
        storage: sampleStorage
    )

    #expect(snapshot.profileStatus == .ready)
    #expect(snapshot.preferences == samplePreferences)
    #expect(snapshot.storage == sampleStorage)
}

@MainActor
@Test
func unavailableSettingsSnapshotRequiresBoundedDisplaySafeEvidence() {
    #expect(
        WorkbenchSettingsSnapshot(
            unavailableReason: "Preferences could not be opened."
        )?.profileStatus
            == .unavailable(reason: "Preferences could not be opened.")
    )
    #expect(WorkbenchSettingsSnapshot(unavailableReason: " ") == nil)
    #expect(
        WorkbenchSettingsSnapshot(
            unavailableReason: String(
                repeating: "x",
                count: WorkbenchSettingsSnapshot.maximumReasonUTF8Bytes + 1
            )
        ) == nil
    )
}

@MainActor
@Test
func settingsSheetBuildsWithEditableNativePreferences() {
    let snapshot = WorkbenchSettingsSnapshot(
        preferences: samplePreferences,
        storage: sampleStorage
    )
    _ = WorkbenchSettingsSheet(
        snapshot: snapshot,
        openDestination: { _ in },
        performAction: { _ in }
    )
}

@Test
func relayPreferencesNormalizeWhitespaceAndRefuseUnsafeOrDuplicateAddresses()
    throws
{
    let normalized = try WorkbenchProfilePreferences(
        appRelays: ["  wss://relay.example  "],
        indexerRelays: ["wss://search.example"],
        permissionDefault: .allowSession
    ).normalized()
    #expect(normalized.appRelays == ["wss://relay.example"])

    #expect(throws: WorkbenchPreferencesError.self) {
        try WorkbenchProfilePreferences(
            appRelays: ["ws://relay.example"],
            indexerRelays: ["wss://search.example"],
            permissionDefault: .askEveryTime
        ).normalized()
    }
    #expect(throws: WorkbenchPreferencesError.self) {
        try WorkbenchProfilePreferences(
            appRelays: [
                "wss://relay.example",
                "wss://relay.example",
            ],
            indexerRelays: ["wss://search.example"],
            permissionDefault: .askEveryTime
        ).normalized()
    }
}

@MainActor
@Test
func settingsDestinationWaitsForDismissalAndIsConsumedExactlyOnce() {
    var route = WorkbenchSettingsRouteState()
    route.schedule(.installedLibrary)

    #expect(
        route.consumeAfterDismiss(settingsIsPresented: true) == nil
    )
    #expect(route.pendingDestination == .installedLibrary)
    #expect(
        route.consumeAfterDismiss(settingsIsPresented: false)
            == .installedLibrary
    )
    #expect(route.pendingDestination == nil)
    #expect(
        route.consumeAfterDismiss(settingsIsPresented: false) == nil
    )
}

@MainActor
@Test
func settingsRouteIsBoundedToOnePendingDestination() {
    var route = WorkbenchSettingsRouteState()
    route.schedule(.account)
    route.schedule(.activity)

    #expect(route.pendingDestination == .activity)
    #expect(
        route.consumeAfterDismiss(settingsIsPresented: false) == .activity
    )
}

@MainActor
@Test
func settingsDestinationsHaveStableAccessibilityIdentifiers() {
    #expect(
        Set(
            [
                WorkbenchSettingsDestination.account,
                .installedLibrary,
                .activity,
            ].map(\.accessibilityIdentifier)
        ) == [
            "settings-account",
            "settings-installed-library",
            "settings-activity",
        ]
    )
}
