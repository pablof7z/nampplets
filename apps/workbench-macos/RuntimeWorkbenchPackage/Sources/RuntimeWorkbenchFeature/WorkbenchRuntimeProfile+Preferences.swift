import NMPNativeRuntimeApple

extension WorkbenchPermissionDefault {
    init(_ native: NativeRuntimePermissionDefault) {
        switch native {
        case .askEveryTime:
            self = .askEveryTime
        case .allowSession:
            self = .allowSession
        case .allowExactBuild:
            self = .allowExactBuild
        }
    }

    var native: NativeRuntimePermissionDefault {
        switch self {
        case .askEveryTime:
            .askEveryTime
        case .allowSession:
            .allowSession
        case .allowExactBuild:
            .allowExactBuild
        }
    }
}

extension WorkbenchRuntimeProfile {
    func settingsSnapshot() -> WorkbenchSettingsSnapshot {
        let preferences = native.profilePreferences()
        let storage = native.storageSnapshot()
        return WorkbenchSettingsSnapshot(
            preferences: WorkbenchProfilePreferences(
                appRelays: preferences.appRelays,
                indexerRelays: preferences.indexerRelays,
                permissionDefault: WorkbenchPermissionDefault(
                    preferences.permissionDefault
                )
            ),
            storage: WorkbenchStorageSummary(
                networkBytes: storage.nmpCacheBytes,
                appBytes: storage.appDataBytes,
                totalBytes: storage.totalBytes,
                isEstimate: storage.incomplete
            )
        )
    }

    public func savePreferences(
        _ preferences: WorkbenchProfilePreferences
    ) throws -> Bool {
        let normalized = try preferences.normalized()
        return try native.updateProfilePreferences(
            NativeRuntimeProfilePreferences(
                indexerRelays: normalized.indexerRelays,
                appRelays: normalized.appRelays,
                permissionDefault: normalized.permissionDefault.native
            )
        )
    }

    public func clearNetworkCache() throws {
        try native.resetNetworkCache()
    }
}
