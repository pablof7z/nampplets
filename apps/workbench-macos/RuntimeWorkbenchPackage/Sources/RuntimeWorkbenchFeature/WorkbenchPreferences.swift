import Foundation

public enum WorkbenchPermissionDefault:
    String,
    CaseIterable,
    Identifiable,
    Equatable,
    Sendable
{
    case askEveryTime
    case allowSession
    case allowExactBuild

    public var id: String { rawValue }

    var title: String {
        switch self {
        case .askEveryTime:
            "Ask every time"
        case .allowSession:
            "Until I quit"
        case .allowExactBuild:
            "Remember for this version"
        }
    }

    var detail: String {
        switch self {
        case .askEveryTime:
            "The review starts with the most cautious choice."
        case .allowSession:
            "The review suggests access until you close the app."
        case .allowExactBuild:
            "The review suggests remembering access for that napplet version."
        }
    }
}

public struct WorkbenchProfilePreferences: Equatable, Sendable {
    public static let maximumRelaysPerGroup = 4

    public var appRelays: [String]
    public var indexerRelays: [String]
    public var permissionDefault: WorkbenchPermissionDefault

    public init(
        appRelays: [String],
        indexerRelays: [String],
        permissionDefault: WorkbenchPermissionDefault
    ) {
        self.appRelays = appRelays
        self.indexerRelays = indexerRelays
        self.permissionDefault = permissionDefault
    }

    func normalized() throws -> WorkbenchProfilePreferences {
        WorkbenchProfilePreferences(
            appRelays: try Self.normalizedRelays(
                appRelays,
                label: "Everyday relays"
            ),
            indexerRelays: try Self.normalizedRelays(
                indexerRelays,
                label: "Search relays"
            ),
            permissionDefault: permissionDefault
        )
    }

    private static func normalizedRelays(
        _ values: [String],
        label: String
    ) throws -> [String] {
        guard !values.isEmpty else {
            throw WorkbenchPreferencesError.invalid(
                "\(label) needs at least one address."
            )
        }
        guard values.count <= maximumRelaysPerGroup else {
            throw WorkbenchPreferencesError.invalid(
                "\(label) can contain up to \(maximumRelaysPerGroup) addresses."
            )
        }
        var normalized: [String] = []
        for value in values {
            let relay = value.trimmingCharacters(
                in: .whitespacesAndNewlines
            )
            guard
                let components = URLComponents(string: relay),
                components.scheme == "wss",
                components.host?.isEmpty == false,
                components.user == nil,
                components.password == nil,
                !relay.unicodeScalars.contains(where: {
                    CharacterSet.controlCharacters.contains($0)
                })
            else {
                throw WorkbenchPreferencesError.invalid(
                    "\(label) must use secure addresses beginning with wss://."
                )
            }
            guard !normalized.contains(relay) else {
                throw WorkbenchPreferencesError.invalid(
                    "\(label) contains the same address more than once."
                )
            }
            normalized.append(relay)
        }
        return normalized
    }
}

public struct WorkbenchStorageSummary: Equatable, Sendable {
    public let networkBytes: UInt64
    public let appBytes: UInt64
    public let totalBytes: UInt64
    public let isEstimate: Bool

    public init(
        networkBytes: UInt64,
        appBytes: UInt64,
        totalBytes: UInt64,
        isEstimate: Bool
    ) {
        self.networkBytes = networkBytes
        self.appBytes = appBytes
        self.totalBytes = totalBytes
        self.isEstimate = isEstimate
    }
}

public enum WorkbenchProfileAction: Equatable, Sendable {
    case savePreferences(WorkbenchProfilePreferences)
    case clearNetworkCache
}

public typealias WorkbenchProfileActionHandler =
    @MainActor (WorkbenchProfileAction) async throws -> Void

public enum WorkbenchPreferencesError:
    Error,
    LocalizedError,
    Equatable,
    Sendable
{
    case invalid(String)
    case unavailable(String)

    public var errorDescription: String? {
        switch self {
        case let .invalid(message), let .unavailable(message):
            message
        }
    }
}
