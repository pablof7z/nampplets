import Foundation
import NMPNativeRuntimeApple

/// Application-owned wrapper for one local Workbench trust profile.
///
/// Every window and workspace slot borrows this same native profile. The app,
/// rather than any napplet view, owns final shutdown.
public final class WorkbenchRuntimeProfile: @unchecked Sendable {
    static let productionPermissionMode =
        NativeRuntimePermissionMode.interactive

    typealias PersistedArtifactResolver = @Sendable (
        NativeRuntimeProfile,
        WorkbenchExactBuildIdentity
    ) -> NativeRuntimeCatalogInstallResult

    struct OperatorNetworkInputs: Equatable {
        let indexerRelays: [String]
        let appRelays: [String]
    }

    private static let maximumOperatorRelaysPerLane = 4

    let native: NativeRuntimeProfile
    let catalogStateLock = NSLock()
    let persistedArtifactResolver: PersistedArtifactResolver
    private var catalogReviews: [String: NativeRuntimeCatalogReview] = [:]
    var catalogArtifacts:
        [WorkbenchExactBuildIdentity: NativeRuntimeInstalledArtifact] = [:]

    public static func openDefault() throws -> WorkbenchRuntimeProfile {
        let network = try operatorNetworkInputs(
            infoDictionary: Bundle.main.infoDictionary ?? [:]
        )
        let base = try FileManager.default.url(
            for: .applicationSupportDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: true
        )
        let storageRoot = base
            .appendingPathComponent(
                "io.f7z.nmp.native-runtime.workbench",
                isDirectory: true
            )
            .appendingPathComponent("runtime", isDirectory: true)
        return try open(
            storageRoot: storageRoot,
            indexerRelays: network.indexerRelays,
            appRelays: network.appRelays,
            accountPersistence: keychainPersistence(
                storageRoot: storageRoot
            ),
            permissionMode: productionPermissionMode
        )
    }

    /// Reads the finite operator relay lanes from the application bundle.
    ///
    /// These are deployment inputs, not napplet-selected routes. NMP remains
    /// the sole owner of discovery, routing, canonical events, and receipts.
    static func operatorNetworkInputs(
        infoDictionary: [String: Any]
    ) throws -> OperatorNetworkInputs {
        let indexers = relayLane(
            key: "NMPIndexerRelays",
            infoDictionary: infoDictionary
        )
        let appRelays = relayLane(
            key: "NMPAppRelays",
            infoDictionary: infoDictionary
        )
        guard !indexers.isEmpty, !appRelays.isEmpty else {
            throw CocoaError(.propertyListReadCorrupt)
        }
        return OperatorNetworkInputs(
            indexerRelays: indexers,
            appRelays: appRelays
        )
    }

    private static func relayLane(
        key: String,
        infoDictionary: [String: Any]
    ) -> [String] {
        guard let configured = infoDictionary[key] as? [String] else {
            return []
        }
        var relays: [String] = []
        for value in configured.prefix(maximumOperatorRelaysPerLane) {
            let relay = value.trimmingCharacters(
                in: .whitespacesAndNewlines
            )
            guard
                !relay.isEmpty,
                relay.hasPrefix("wss://"),
                !relay.contains("@"),
                !relays.contains(relay)
            else {
                continue
            }
            relays.append(relay)
        }
        return relays
    }

    static func open(
        storageRoot: URL,
        indexerRelays: [String] = [],
        appRelays: [String] = [],
        accountPersistence: NativeRuntimeAccountPersistence = .transient,
        permissionMode: NativeRuntimePermissionMode = .interactive,
        persistedArtifactResolver: PersistedArtifactResolver? = nil
    ) throws -> WorkbenchRuntimeProfile {
        let native = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(
                storageRoot: storageRoot,
                indexerRelays: indexerRelays,
                appRelays: appRelays,
                accountPersistence: accountPersistence,
                permissionMode: permissionMode
            )
        )
        return WorkbenchRuntimeProfile(
            native: native,
            persistedArtifactResolver: persistedArtifactResolver
        )
    }

    static func keychainPersistence(
        storageRoot: URL
    ) -> NativeRuntimeAccountPersistence {
        .keychain(namespace: storageRoot.standardizedFileURL.path)
    }

    init(
        native: NativeRuntimeProfile,
        persistedArtifactResolver: PersistedArtifactResolver? = nil
    ) {
        self.native = native
        self.persistedArtifactResolver =
            persistedArtifactResolver ?? Self.resolvePersistedArtifact
    }

    public func close() {
        native.close()
    }

    func storeCatalogReview(_ review: NativeRuntimeCatalogReview) {
        catalogStateLock.lock()
        catalogReviews[review.token] = review
        catalogStateLock.unlock()
    }

    func takeCatalogReview(
        id: String
    ) -> NativeRuntimeCatalogReview? {
        catalogStateLock.lock()
        let review = catalogReviews.removeValue(forKey: id)
        catalogStateLock.unlock()
        return review
    }

    func storeCatalogArtifact(
        _ artifact: NativeRuntimeInstalledArtifact,
        identity: WorkbenchExactBuildIdentity
    ) {
        catalogStateLock.lock()
        catalogArtifacts[identity] = artifact
        catalogStateLock.unlock()
    }

    deinit {
        close()
    }
}
