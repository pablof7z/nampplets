import Foundation
import NMPNativeRuntimeApple

/// Application-owned wrapper for one local Workbench trust profile.
///
/// Every window and workspace slot borrows this same native profile. The app,
/// rather than any napplet view, owns final shutdown.
public final class WorkbenchRuntimeProfile: @unchecked Sendable {
    typealias PersistedArtifactResolver = @Sendable (
        NativeRuntimeProfile,
        WorkbenchExactBuildIdentity
    ) -> NativeRuntimeCatalogInstallResult

    struct OperatorNetworkInputs: Equatable {
        let indexerRelays: [String]
        let appRelays: [String]
    }

    struct OpeningConfiguration: Sendable {
        let storageRoot: URL
        let indexerRelays: [String]
        let appRelays: [String]
        let accountPersistence: NativeRuntimeAccountPersistence
        let permissionDefault: NativeRuntimePermissionDefault
    }


    let native: NativeRuntimeProfile
    let openingConfiguration: OpeningConfiguration
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
            )
        )
    }

    /// Reads the finite operator relay lanes from the application bundle.
    ///
    /// These are deployment inputs, not napplet-selected routes. NMP remains
    /// the sole owner of discovery, routing, canonical events, and receipts,
    /// and the runtime is the sole judge of which of these relays are usable:
    /// it applies the scheme, credential, duplicate and cap rules, admits what
    /// it can, and records every relay it drops as a boundary refusal.
    ///
    /// Reading the plist is all that is left here on purpose. Deciding which
    /// relays count belongs in one place, or a second host reproduces the
    /// rules approximately and routes differently for reasons nobody can see.
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
        // An absent lane is a broken bundle, which is this layer's to catch:
        // there is nothing for the runtime to judge and no relay to name.
        guard !indexers.isEmpty, !appRelays.isEmpty else {
            throw CocoaError(.propertyListReadCorrupt)
        }
        return OperatorNetworkInputs(
            indexerRelays: indexers,
            appRelays: appRelays
        )
    }

    /// The configured lane, read verbatim.
    ///
    /// Not even whitespace is stripped. Trimming would quietly repair one
    /// class of plist mistake and silently discard another (an all-whitespace
    /// entry), which is the shell making relay decisions again in miniature.
    /// The runtime judges every entry and names what it refuses.
    private static func relayLane(
        key: String,
        infoDictionary: [String: Any]
    ) -> [String] {
        infoDictionary[key] as? [String] ?? []
    }

    static func open(
        storageRoot: URL,
        indexerRelays: [String] = [],
        appRelays: [String] = [],
        accountPersistence: NativeRuntimeAccountPersistence = .transient,
        permissionDefault: NativeRuntimePermissionDefault = .askEveryTime,
        persistedArtifactResolver: PersistedArtifactResolver? = nil
    ) throws -> WorkbenchRuntimeProfile {
        let native = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(
                storageRoot: storageRoot,
                indexerRelays: indexerRelays,
                appRelays: appRelays,
                accountPersistence: accountPersistence,
                permissionDefault: permissionDefault
            )
        )
        return WorkbenchRuntimeProfile(
            native: native,
            openingConfiguration: OpeningConfiguration(
                storageRoot: storageRoot,
                indexerRelays: indexerRelays,
                appRelays: appRelays,
                accountPersistence: accountPersistence,
                permissionDefault: permissionDefault
            ),
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
        openingConfiguration: OpeningConfiguration,
        persistedArtifactResolver: PersistedArtifactResolver? = nil
    ) {
        self.native = native
        self.openingConfiguration = openingConfiguration
        self.persistedArtifactResolver =
            persistedArtifactResolver ?? Self.resolvePersistedArtifact
    }

    public func close() {
        native.close()
    }

    public func reopened() throws -> WorkbenchRuntimeProfile {
        try Self.open(
            storageRoot: openingConfiguration.storageRoot,
            indexerRelays: openingConfiguration.indexerRelays,
            appRelays: openingConfiguration.appRelays,
            accountPersistence: openingConfiguration.accountPersistence,
            permissionDefault: openingConfiguration.permissionDefault,
            persistedArtifactResolver: persistedArtifactResolver
        )
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
