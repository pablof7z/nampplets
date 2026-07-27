import Foundation

public enum WorkbenchLibraryLimits {
    public static let maximumBuilds = 512
    public static let maximumSessionsPerBuild = 16
    public static let maximumWorkspaces = 64
    public static let maximumWorkspaceAssignmentsPerBuild = 64
    public static let maximumRefusals = 64
    public static let maximumFilterUTF8Bytes = 256
    public static let maximumDTagUTF8Bytes = 256
    public static let maximumWorkspaceIDUTF8Bytes = 256
    public static let maximumTitleUTF8Bytes = 16 * 1_024
    public static let maximumRefusalCodeUTF8Bytes = 128
    public static let maximumRefusalMessageUTF8Bytes = 16 * 1_024
    public static let maximumSnapshotUTF8Bytes = 2 * 1_024 * 1_024
}

/// The immutable, exact-build identity used by every installed-library action.
public struct WorkbenchLibraryExactBuild: Hashable, Identifiable, Sendable {
    public let manifestAuthor: String
    public let dTag: String
    public let aggregateHash: String

    public var id: String {
        "\(manifestAuthor):\(dTag):\(aggregateHash)"
    }

    public init?(
        manifestAuthor: String,
        dTag: String,
        aggregateHash: String
    ) {
        guard
            Self.isLowercaseHexDigest(manifestAuthor),
            Self.isValidIdentifier(dTag, maximumBytes: WorkbenchLibraryLimits.maximumDTagUTF8Bytes),
            Self.isLowercaseHexDigest(aggregateHash)
        else {
            return nil
        }

        self.manifestAuthor = manifestAuthor
        self.dTag = dTag
        self.aggregateHash = aggregateHash
    }

    fileprivate static func isValidIdentifier(
        _ value: String,
        maximumBytes: Int
    ) -> Bool {
        !value.isEmpty
            && value.utf8.count <= maximumBytes
            && isControlFree(value)
    }

    fileprivate static func isControlFree(_ value: String) -> Bool {
        !value.unicodeScalars.contains {
            CharacterSet.controlCharacters.contains($0)
        }
    }

    private static func isLowercaseHexDigest(_ value: String) -> Bool {
        value.utf8.count == 64
            && value.utf8.allSatisfy { byte in
                (48 ... 57).contains(byte) || (97 ... 102).contains(byte)
            }
    }
}

public enum WorkbenchLibraryBuildAvailability: Equatable, Sendable {
    case metadataOnly
    case sealedExactBytesReady

    public var title: String {
        switch self {
        case .metadataOnly:
            "Metadata only"
        case .sealedExactBytesReady:
            "Exact bytes ready"
        }
    }

    public var detail: String {
        switch self {
        case .metadataOnly:
            "The verified installation record is available, but this process does not hold the sealed exact-build bytes."
        case .sealedExactBytesReady:
            "The runtime holds the sealed bytes for this exact aggregate."
        }
    }
}

public enum WorkbenchLibrarySessionState: String, Equatable, Sendable {
    case running
    /// Running without a domain the build's own content requires.
    case runningDegraded = "running-degraded"
    case suspended

    /// True for any live session, degraded or whole. Read this where the
    /// question is "does a session exist"; match the case itself where the
    /// question is "is it whole", so the two can never be confused.
    public var isLive: Bool {
        switch self {
        case .running, .runningDegraded:
            true
        case .suspended:
            false
        }
    }

    public var title: String {
        switch self {
        case .running:
            "Running"
        case .runningDegraded:
            "Running, degraded"
        case .suspended:
            "Suspended"
        }
    }
}

/// A Rust-projected lifecycle row. Native presentation does not infer state
/// from activity, provider traffic, or whether a session id is present.
public struct WorkbenchLibrarySession: Hashable, Identifiable, Sendable {
    public let id: UInt64
    public let state: WorkbenchLibrarySessionState

    public init(id: UInt64, state: WorkbenchLibrarySessionState) {
        self.id = id
        self.state = state
    }
}

public struct WorkbenchLibraryWorkspace: Hashable, Identifiable, Sendable {
    public let id: String
    public let displayName: String

    public init?(id: String, displayName: String) {
        guard
            WorkbenchLibraryExactBuild.isValidIdentifier(
                id,
                maximumBytes: WorkbenchLibraryLimits.maximumWorkspaceIDUTF8Bytes
            ),
            !displayName.isEmpty,
            displayName.utf8.count <= WorkbenchLibraryLimits.maximumTitleUTF8Bytes,
            WorkbenchLibraryExactBuild.isControlFree(displayName)
        else {
            return nil
        }
        self.id = id
        self.displayName = displayName
    }
}

/// One bounded, non-secret installed row supplied by the Rust runtime.
///
/// Opaque manifest JSON, signer state, artifact paths, relay state, and key
/// material are deliberately absent.
public struct WorkbenchLibraryBuild: Identifiable, Equatable, Sendable {
    public let exactBuild: WorkbenchLibraryExactBuild
    public let title: String
    public let availability: WorkbenchLibraryBuildAvailability
    public let sessions: [WorkbenchLibrarySession]
    public let assignedWorkspaceIDs: [String]

    public var id: WorkbenchLibraryExactBuild {
        exactBuild
    }

    public init?(
        exactBuild: WorkbenchLibraryExactBuild,
        title: String,
        availability: WorkbenchLibraryBuildAvailability,
        sessions: [WorkbenchLibrarySession],
        assignedWorkspaceIDs: [String]
    ) {
        guard
            !title.isEmpty,
            title.utf8.count <= WorkbenchLibraryLimits.maximumTitleUTF8Bytes,
            WorkbenchLibraryExactBuild.isControlFree(title),
            sessions.count <= WorkbenchLibraryLimits.maximumSessionsPerBuild,
            Set(sessions.map(\.id)).count == sessions.count,
            assignedWorkspaceIDs.count
                <= WorkbenchLibraryLimits.maximumWorkspaceAssignmentsPerBuild,
            Set(assignedWorkspaceIDs).count == assignedWorkspaceIDs.count,
            assignedWorkspaceIDs.allSatisfy({
                WorkbenchLibraryExactBuild.isValidIdentifier(
                    $0,
                    maximumBytes: WorkbenchLibraryLimits.maximumWorkspaceIDUTF8Bytes
                )
            })
        else {
            return nil
        }

        self.exactBuild = exactBuild
        self.title = title
        self.availability = availability
        self.sessions = sessions
        self.assignedWorkspaceIDs = assignedWorkspaceIDs
    }

    fileprivate var projectedUTF8ByteCount: Int {
        title.utf8.count
            + exactBuild.manifestAuthor.utf8.count
            + exactBuild.dTag.utf8.count
            + exactBuild.aggregateHash.utf8.count
            + assignedWorkspaceIDs.reduce(0) { $0 + $1.utf8.count }
    }
}

/// One display-safe semantic refusal supplied by Rust.
public struct WorkbenchLibraryRefusal: Identifiable, Equatable, Sendable {
    public let code: String
    public let message: String
    public let occurredAtMillis: UInt64

    public var id: String {
        "\(occurredAtMillis):\(code)"
    }

    public init?(code: String, message: String, occurredAtMillis: UInt64) {
        guard
            WorkbenchLibraryExactBuild.isValidIdentifier(
                code,
                maximumBytes: WorkbenchLibraryLimits.maximumRefusalCodeUTF8Bytes
            ),
            !message.isEmpty,
            message.utf8.count <= WorkbenchLibraryLimits.maximumRefusalMessageUTF8Bytes,
            WorkbenchLibraryExactBuild.isControlFree(message)
        else {
            return nil
        }

        self.code = code
        self.message = message
        self.occurredAtMillis = occurredAtMillis
    }

    fileprivate var projectedUTF8ByteCount: Int {
        code.utf8.count + message.utf8.count
    }
}

public enum WorkbenchLibraryServiceAvailability: Equatable, Sendable {
    case available
    case unavailable(reason: String)

    public var isAvailable: Bool {
        if case .available = self {
            return true
        }
        return false
    }

    public var unavailableReason: String? {
        guard case let .unavailable(reason) = self else {
            return nil
        }
        return reason
    }
}

/// A bounded, screen-shaped replacement projection from the Rust runtime.
public struct WorkbenchLibrarySnapshot: Equatable, Sendable {
    public let revision: UInt64
    public let availability: WorkbenchLibraryServiceAvailability
    public let filterQuery: String
    public let totalInstalled: UInt64
    public let builds: [WorkbenchLibraryBuild]
    public let workspaces: [WorkbenchLibraryWorkspace]
    public let refusals: [WorkbenchLibraryRefusal]
    /// Refusals the runtime evicted to stay inside its bound. `refusals` holds
    /// only the survivors, so this is the difference between "the runtime
    /// refused this many times" and "this many refusals are still readable".
    public let droppedRefusalCount: UInt64

    public init?(
        revision: UInt64,
        availability: WorkbenchLibraryServiceAvailability,
        filterQuery: String,
        totalInstalled: UInt64,
        builds: [WorkbenchLibraryBuild],
        workspaces: [WorkbenchLibraryWorkspace],
        refusals: [WorkbenchLibraryRefusal] = [],
        droppedRefusalCount: UInt64 = 0
    ) {
        let unavailableReason = availability.unavailableReason ?? ""
        let projectedBytes = filterQuery.utf8.count
            + unavailableReason.utf8.count
            + builds.reduce(0) { $0 + $1.projectedUTF8ByteCount }
            + workspaces.reduce(0) {
                $0 + $1.id.utf8.count + $1.displayName.utf8.count
            }
            + refusals.reduce(0) { $0 + $1.projectedUTF8ByteCount }
        let knownWorkspaceIDs = Set(workspaces.map(\.id))

        guard
            filterQuery.utf8.count <= WorkbenchLibraryLimits.maximumFilterUTF8Bytes,
            WorkbenchLibraryExactBuild.isControlFree(filterQuery),
            unavailableReason.utf8.count
                <= WorkbenchLibraryLimits.maximumRefusalMessageUTF8Bytes,
            WorkbenchLibraryExactBuild.isControlFree(unavailableReason),
            builds.count <= WorkbenchLibraryLimits.maximumBuilds,
            totalInstalled >= UInt64(builds.count),
            Set(builds.map(\.exactBuild)).count == builds.count,
            workspaces.count <= WorkbenchLibraryLimits.maximumWorkspaces,
            knownWorkspaceIDs.count == workspaces.count,
            refusals.count <= WorkbenchLibraryLimits.maximumRefusals,
            Set(refusals.map(\.id)).count == refusals.count,
            builds.allSatisfy({
                Set($0.assignedWorkspaceIDs).isSubset(of: knownWorkspaceIDs)
            }),
            projectedBytes <= WorkbenchLibraryLimits.maximumSnapshotUTF8Bytes
        else {
            return nil
        }

        self.revision = revision
        self.availability = availability
        self.filterQuery = filterQuery
        self.totalInstalled = totalInstalled
        self.builds = builds
        self.workspaces = workspaces
        self.refusals = refusals
        self.droppedRefusalCount = droppedRefusalCount
    }
}

public enum WorkbenchLibraryUpdate: Equatable, Sendable {
    case authoritative(WorkbenchLibrarySnapshot)
    case next(
        WorkbenchLibrarySnapshot,
        predecessorRevision: UInt64,
        /// Runtime events lost before this batch. Carried explicitly so a
        /// stale cursor never has to be inferred from, or encoded into, the
        /// revision numbers.
        lostBeforeBatch: UInt64
    )
}

public struct WorkbenchLibraryUpdateGap: Equatable, Sendable {
    public let expectedPredecessorRevision: UInt64
    public let receivedPredecessorRevision: UInt64
    public let receivedRevision: UInt64
    /// Runtime events the observer's cursor fell behind, reported by the
    /// runtime rather than inferred from the revisions above. Zero means the
    /// revisions themselves disagreed without a reported loss.
    public let lostBeforeBatch: UInt64

    public init(
        expectedPredecessorRevision: UInt64,
        receivedPredecessorRevision: UInt64,
        receivedRevision: UInt64,
        lostBeforeBatch: UInt64 = 0
    ) {
        self.expectedPredecessorRevision = expectedPredecessorRevision
        self.receivedPredecessorRevision = receivedPredecessorRevision
        self.receivedRevision = receivedRevision
        self.lostBeforeBatch = lostBeforeBatch
    }
}
