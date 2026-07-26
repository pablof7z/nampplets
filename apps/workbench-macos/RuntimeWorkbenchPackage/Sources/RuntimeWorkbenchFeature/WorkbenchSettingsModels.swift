import Foundation

public enum WorkbenchSettingsDestination: Hashable, Sendable {
    case account
    case installedLibrary
    case activity

    var title: String {
        switch self {
        case .account:
            "Accounts"
        case .installedLibrary:
            "Installed Napplets"
        case .activity:
            "Recent Activity"
        }
    }

    var systemImage: String {
        switch self {
        case .account:
            "person.crop.circle"
        case .installedLibrary:
            "square.stack.3d.up"
        case .activity:
            "clock"
        }
    }

    var detail: String {
        switch self {
        case .account:
            "Sign in or manage saved accounts."
        case .installedLibrary:
            "See the napplets saved on this device."
        case .activity:
            "See what napplets have done recently."
        }
    }

    var accessibilityIdentifier: String {
        switch self {
        case .account:
            "settings-account"
        case .installedLibrary:
            "settings-installed-library"
        case .activity:
            "settings-activity"
        }
    }
}

struct WorkbenchSettingsRouteState: Equatable, Sendable {
    private(set) var pendingDestination: WorkbenchSettingsDestination?

    mutating func schedule(_ destination: WorkbenchSettingsDestination) {
        pendingDestination = destination
    }

    mutating func consumeAfterDismiss(
        settingsIsPresented: Bool
    ) -> WorkbenchSettingsDestination? {
        guard !settingsIsPresented, let pendingDestination else {
            return nil
        }
        self.pendingDestination = nil
        return pendingDestination
    }
}

public enum WorkbenchRuntimeProfileStatus: Equatable, Sendable {
    case ready
    case unavailable(reason: String)
}

public struct WorkbenchSettingsSnapshot: Equatable, Sendable {
    public static let maximumReasonUTF8Bytes = 16 * 1_024

    public let profileStatus: WorkbenchRuntimeProfileStatus
    public let preferences: WorkbenchProfilePreferences?
    public let storage: WorkbenchStorageSummary?

    public init(
        preferences: WorkbenchProfilePreferences,
        storage: WorkbenchStorageSummary
    ) {
        profileStatus = .ready
        self.preferences = preferences
        self.storage = storage
    }

    public init?(unavailableReason: String) {
        let reason = unavailableReason.trimmingCharacters(
            in: .whitespacesAndNewlines
        )
        guard
            !reason.isEmpty,
            reason.utf8.count <= Self.maximumReasonUTF8Bytes,
            !reason.unicodeScalars.contains(where: {
                CharacterSet.controlCharacters.contains($0)
                    && $0 != "\n"
                    && $0 != "\t"
            })
        else {
            return nil
        }
        profileStatus = .unavailable(reason: reason)
        preferences = nil
        storage = nil
    }
}
