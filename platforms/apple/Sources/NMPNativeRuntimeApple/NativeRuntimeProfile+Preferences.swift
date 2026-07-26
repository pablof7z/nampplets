import Foundation

public enum NativeRuntimeProfilePreferencesError:
    Error,
    LocalizedError,
    Equatable,
    Sendable
{
    case refused(code: String, detail: String)
    case incompleteResult

    public var errorDescription: String? {
        switch self {
        case let .refused(_, detail):
            detail
        case .incompleteResult:
            "The settings change did not complete."
        }
    }
}

extension NativeRuntimeProfile {
    public func profilePreferences() -> NativeRuntimeProfilePreferences {
        controller.profilePreferences()
    }

    public func storageSnapshot() -> NativeRuntimeStorageSnapshot {
        controller.storageSnapshot()
    }

    /// Persists validated profile choices. A `true` result means the profile
    /// owner must close and reopen this profile before the choices take effect.
    public func updateProfilePreferences(
        _ preferences: NativeRuntimeProfilePreferences
    ) throws -> Bool {
        let result = controller.updateProfilePreferences(
            preferences: preferences
        )
        if let refusal = result.refusal {
            throw NativeRuntimeProfilePreferencesError.refused(
                code: refusal.code,
                detail: refusal.detail
            )
        }
        guard result.applied, result.preferences != nil else {
            throw NativeRuntimeProfilePreferencesError.incompleteResult
        }
        return result.restartRequired
    }

    /// Closes the complete native profile before asking NMP to remove its own
    /// persistent network state.
    public func resetNetworkCache() throws {
        close()
        let result = controller.resetNmpCache()
        if let refusal = result.refusal {
            throw NativeRuntimeProfilePreferencesError.refused(
                code: refusal.code,
                detail: refusal.detail
            )
        }
        guard result.reset else {
            throw NativeRuntimeProfilePreferencesError.incompleteResult
        }
    }
}
