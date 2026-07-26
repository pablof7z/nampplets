import Foundation
import NMPNativeRuntime

// MARK: - Profile open errors, account persistence, and configuration

public enum RuntimeNappletOpenError: Error, LocalizedError, Equatable {
    case invalidStorageRoot
    case artifactSourceRefused(detail: String)
    case artifactRefused(code: String, detail: String)
    case installRefused(detail: String)
    case installedArtifactProfileMismatch
    case launchRefused(detail: String)
    case observerRefused(code: String, detail: String)
    case invalidAccountPersistence

    public var errorDescription: String? {
        switch self {
        case .invalidStorageRoot:
            "The native runtime storage directory is unavailable."
        case let .artifactSourceRefused(detail):
            "The native artifact source was refused: \(detail)"
        case let .artifactRefused(code, detail):
            "Artifact verification was refused (\(code)): \(detail)"
        case let .installRefused(detail):
            "The native runtime refused to install the artifact: \(detail)"
        case .installedArtifactProfileMismatch:
            "The installed artifact belongs to a different runtime profile."
        case let .launchRefused(detail):
            "The native runtime refused to launch the artifact: \(detail)"
        case let .observerRefused(code, detail):
            "Runtime observation was refused (\(code)): \(detail)"
        case .invalidAccountPersistence:
            "The native account persistence configuration is invalid."
        }
    }
}

public enum NativeRuntimeAccountPersistence: Equatable, Sendable {
    /// Local credentials live only for this profile process.
    case transient
    /// Local credentials are stored in a profile-scoped macOS Keychain
    /// namespace. The namespace is hashed before becoming a service name.
    case keychain(namespace: String)
}

public enum NativeRuntimeAccountPersistenceIssue:
    String,
    Error,
    LocalizedError,
    Equatable,
    Sendable
{
    case restoreFailed
    case registerFailed
    case activationFailed
    case logoutFailed
    case removalFailed

    public var errorDescription: String? {
        switch self {
        case .restoreFailed:
            "Saved accounts could not be restored securely."
        case .registerFailed:
            "The account is available for this session but was not saved securely."
        case .activationFailed:
            "The active account changed for this session but was not saved securely."
        case .logoutFailed:
            "The account is logged out for this session but secure persistence was not updated."
        case .removalFailed:
            "The account was removed for this session but secure persistence was not fully updated."
        }
    }
}

public struct NativeRuntimeProfileConfiguration: Sendable {
    public let storageRoot: URL
    public let indexerRelays: [String]
    public let appRelays: [String]
    public let fallbackRelays: [String]
    public let allowedLocalRelayHosts: [String]
    public let accountPersistence: NativeRuntimeAccountPersistence
    public let permissionMode: NativeRuntimePermissionMode
    public let permissionDefault: NativeRuntimePermissionDefault

    public init(
        storageRoot: URL,
        indexerRelays: [String] = [],
        appRelays: [String] = [],
        fallbackRelays: [String] = [],
        allowedLocalRelayHosts: [String] = [],
        accountPersistence: NativeRuntimeAccountPersistence = .transient,
        permissionMode: NativeRuntimePermissionMode = .interactive,
        permissionDefault: NativeRuntimePermissionDefault = .askEveryTime
    ) {
        self.storageRoot = storageRoot
        self.indexerRelays = indexerRelays
        self.appRelays = appRelays
        self.fallbackRelays = fallbackRelays
        self.allowedLocalRelayHosts = allowedLocalRelayHosts
        self.accountPersistence = accountPersistence
        self.permissionMode = permissionMode
        self.permissionDefault = permissionDefault
    }
}
