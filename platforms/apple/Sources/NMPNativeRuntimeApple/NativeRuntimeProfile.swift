import Foundation
import NMPNativeRuntime

#if os(macOS)
typealias PlatformAppearanceSource = MacOSAppearanceSource
typealias PlatformSettingsExecutor = MacOSSettingsExecutor
#elseif os(iOS)
typealias PlatformAppearanceSource = IOSAppearanceSource
typealias PlatformSettingsExecutor = IOSSettingsExecutor
#endif

// MARK: - Profile lifecycle: open, observation start, and close

/// One application trust profile owns exactly one Rust runtime controller,
/// NMP engine, runtime store, artifact cache, and observation stream.
///
/// Napplet sessions borrow this profile. Stopping or crashing one session
/// cannot close the profile or terminate sibling sessions.
public final class NativeRuntimeProfile: RuntimeObserver, @unchecked Sendable {
    typealias ActivityReceiver =
        @Sendable (NativeRuntimeActivityUpdate) -> Void
    typealias LibraryReceiver =
        @Sendable (NativeRuntimeLibraryUpdate) -> Void
    typealias CatalogReceiver =
        @Sendable (NativeRuntimeCatalogUpdate) -> Void
    typealias PendingWriteReceiver =
        @Sendable (NativeRuntimePendingWriteUpdate) -> Void
    typealias ReceiptReceiver =
        @Sendable (NativeRuntimeReceiptUpdate) -> Void

    struct ActivityObserverEntry {
        let scope: NativeRuntimeActivityScope
        let receive: ActivityReceiver
    }

    struct LibraryObserverEntry {
        let receive: LibraryReceiver
        var lastDeliveredRevision: UInt64
        var isReadyForNext = false
        var pendingUpdate: NativeRuntimeLibraryUpdate?
    }

    struct CatalogObserverEntry {
        let receive: CatalogReceiver
        var lastDeliveredRevision: UInt64
        var isReadyForNext = false
        var pendingUpdate: NativeRuntimeCatalogUpdate?
    }

    struct PendingWriteObserverEntry {
        let receive: PendingWriteReceiver
        var lastDeliveredRevision: UInt64
        var isReadyForNext = false
        var pendingUpdate: NativeRuntimePendingWriteUpdate?
    }

    struct ReceiptObserverEntry {
        let receive: ReceiptReceiver
        var lastDeliveredRevision: UInt64
        var isReadyForNext = false
        var pendingUpdate: NativeRuntimeReceiptUpdate?
    }

    final class WeakSession {
        weak var value: RustRuntimeNappletSession?

        init(_ value: RustRuntimeNappletSession) {
            self.value = value
        }
    }

    static let maximumReadBytes: UInt64 = 8 * 1_024 * 1_024
    static let maximumApplicationActivityObservers = 8
    static let maximumApplicationLibraryObservers = 8
    static let maximumApplicationCatalogObservers = 8
    static let maximumApplicationPendingWriteObservers = 8
    static let maximumApplicationReceiptObservers = 8
    static let maximumAccounts = 32

    let profileID = UUID()
    let controller: RuntimeController
    let source: RegisteredArtifactSource
    let appearanceSource: PlatformAppearanceSource
    let settingsExecutor: PlatformSettingsExecutor
    let incActionExecutor: MacOSIncActionExecutor
    let accountVault: (any NativeAccountVault)?
    let lock = NSLock()
    let accountLock = NSLock()
    var observation: RuntimeObservation?
    var sessions: [UInt64: WeakSession] = [:]
    var activityObservers: [UUID: ActivityObserverEntry] = [:]
    var libraryObservers: [UUID: LibraryObserverEntry] = [:]
    var catalogObservers: [UUID: CatalogObserverEntry] = [:]
    var pendingWriteObservers: [UUID: PendingWriteObserverEntry] = [:]
    var receiptObservers: [UUID: ReceiptObserverEntry] = [:]
    var lastActivityRevision: UInt64
    var lastLibraryRevision: UInt64
    var lastCatalogSnapshot: NativeRuntimeCatalogFeedSnapshot
    var lastPendingWriteRevision: UInt64
    var lastReceiptRevision: UInt64
    var lastAcceptedSnapshot: RuntimeSnapshot
    var accountPersistenceProblem:
        NativeRuntimeAccountPersistenceIssue?
    var isClosed = false

    public static func open(
        configuration: NativeRuntimeProfileConfiguration
    ) throws -> NativeRuntimeProfile {
        try open(configuration: configuration, accountVault: nil)
    }

    static func open(
        configuration: NativeRuntimeProfileConfiguration,
        accountVault injectedAccountVault: (any NativeAccountVault)?
    ) throws -> NativeRuntimeProfile {
        do {
            try FileManager.default.createDirectory(
                at: configuration.storageRoot,
                withIntermediateDirectories: true
            )
        } catch {
            throw RuntimeNappletOpenError.invalidStorageRoot
        }

        let source = RegisteredArtifactSource()
        let appearanceSource = PlatformAppearanceSource()
        let settingsExecutor = PlatformSettingsExecutor()
        let incActionExecutor = MacOSIncActionExecutor()
        let accountVault: (any NativeAccountVault)?
        if let injectedAccountVault {
            accountVault = injectedAccountVault
        } else {
            switch configuration.accountPersistence {
            case .transient:
                accountVault = nil
            case let .keychain(namespace):
                do {
                    accountVault = try MacOSKeychainAccountVault(
                        namespace: namespace
                    )
                } catch {
                    throw RuntimeNappletOpenError.invalidAccountPersistence
                }
            }
        }
        let controller = try RuntimeController.openWithAllNativeCapabilities(
            config: RuntimeConfig(
                runtimeStorePath: configuration.storageRoot
                    .appendingPathComponent("runtime.sqlite3")
                    .path,
                nmpStorePath: configuration.storageRoot
                    .appendingPathComponent("nmp.redb")
                    .path,
                artifactCachePath: configuration.storageRoot
                    .appendingPathComponent("artifacts", isDirectory: true)
                    .path,
                indexerRelays: configuration.indexerRelays,
                appRelays: configuration.appRelays,
                fallbackRelays: configuration.fallbackRelays,
                allowedLocalRelayHosts: configuration.allowedLocalRelayHosts,
                maximumNmpRelays: 64,
                maximumBridgeWorkers: 12,
                maximumObservers: 4,
                maximumBoundaryEvents: 256,
                maximumConfigItems: 64,
                maximumConfigStringBytes: 16_384,
                maximumManifestBytes: 262_144,
                maximumArtifactFiles: 256,
                maximumArtifactFileBytes: Self.maximumReadBytes,
                maximumArtifactTotalBytes: 32 * 1_024 * 1_024,
                maximumVerifiedReadBytes: Self.maximumReadBytes,
                maximumBlobSources: 8,
                permissionMode: configuration.permissionMode,
                permissionDefault: configuration.permissionDefault
            ),
            artifactSource: source,
            appearanceSource: appearanceSource,
            settingsExecutor: settingsExecutor,
            incActionExecutor: incActionExecutor
        )
        appearanceSource.bind(controller: controller)
        settingsExecutor.bind(controller: controller)
        let profile = try NativeRuntimeProfile(
            controller: controller,
            source: source,
            appearanceSource: appearanceSource,
            settingsExecutor: settingsExecutor,
            incActionExecutor: incActionExecutor,
            accountVault: accountVault
        )
        profile.restorePersistedAccounts()
        do {
            try profile.startObservation()
        } catch {
            controller.close()
            throw error
        }
        return profile
    }

    private init(
        controller: RuntimeController,
        source: RegisteredArtifactSource,
        appearanceSource: PlatformAppearanceSource,
        settingsExecutor: PlatformSettingsExecutor,
        incActionExecutor: MacOSIncActionExecutor,
        accountVault: (any NativeAccountVault)?
    ) throws {
        self.controller = controller
        self.source = source
        self.appearanceSource = appearanceSource
        self.settingsExecutor = settingsExecutor
        self.incActionExecutor = incActionExecutor
        self.accountVault = accountVault
        let initialSnapshot = try Self.initialSnapshot(
            from: controller.snapshot()
        )
        lastAcceptedSnapshot = initialSnapshot
        let revision = initialSnapshot.revision
        lastActivityRevision = revision
        lastLibraryRevision = revision
        lastCatalogSnapshot = controller.catalogFeedSnapshot()
        lastPendingWriteRevision = revision
        lastReceiptRevision = revision
        accountPersistenceProblem = nil
    }

    private func startObservation() throws {
        let start = controller.observe(observer: self)
        if let refusal = start.refusal {
            throw RuntimeNappletOpenError.observerRefused(
                code: refusal.code,
                detail: refusal.detail
            )
        }
        guard let observation = start.observation else {
            throw RuntimeNappletOpenError.observerRefused(
                code: "missing-observation",
                detail: "The controller admitted no observation handle"
            )
        }
        lock.lock()
        self.observation = observation
        lock.unlock()
    }

    public func close() {
        accountLock.lock()
        lock.lock()
        guard !isClosed else {
            lock.unlock()
            accountLock.unlock()
            return
        }
        isClosed = true
        let observation = observation
        self.observation = nil
        let activeSessions = sessions.values.compactMap(\.value)
        sessions.removeAll()
        activityObservers.removeAll()
        libraryObservers.removeAll()
        catalogObservers.removeAll()
        pendingWriteObservers.removeAll()
        receiptObservers.removeAll()
        lock.unlock()

        observation?.stop()
        for session in activeSessions {
            session.profileDidClose()
        }
        appearanceSource.close()
        settingsExecutor.close()
        incActionExecutor.close()
        controller.close()
        accountLock.unlock()
    }

    /// Installs or removes the application-owned NAP-INC action handler.
    /// Delivery occurs on the main dispatch queue and is bounded by the
    /// native executor; removing the handler purges queued actions.
    public func setIncActionHandler(
        _ handler: NativeWorkbenchActionHandler?
    ) {
        incActionExecutor.setHandler(handler)
    }

    var snapshotForTesting: RuntimeSnapshot {
        get throws {
            try validatedSnapshot()
        }
    }

    var catalogSnapshotForTesting: RuntimeCatalogFeedSnapshot {
        controller.catalogFeedSnapshot()
    }

    deinit {
        close()
    }
}
