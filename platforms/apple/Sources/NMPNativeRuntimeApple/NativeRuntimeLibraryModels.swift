import Foundation
import NMPNativeRuntime

/// Fixed ceilings inherited from the Rust runtime/store configuration used by
/// the Apple profile. Projection refuses an inconsistent frame; it never keeps
/// a local first-N view.
public enum NativeRuntimeLibraryLimits {
    public static let maximumBuilds = 512
    public static let maximumSessions = 16
    public static let maximumSessionsPerBuild = 16
    public static let maximumWorkspaces = 64
    public static let maximumWorkspaceAssignmentsPerBuild = 64
    public static let maximumBoundaryRefusals = 256
    public static let maximumFilterUTF8Bytes = 256
}

/// Exact verified-build identity. No field may be dropped when matching
/// library rows, sessions, or workspace assignments.
public struct NativeRuntimeLibraryExactBuild:
    Hashable,
    Sendable
{
    public let manifestAuthor: String
    public let dTag: String
    public let aggregateHash: String

    public init(
        manifestAuthor: String,
        dTag: String,
        aggregateHash: String
    ) {
        self.manifestAuthor = manifestAuthor
        self.dTag = dTag
        self.aggregateHash = aggregateHash
    }

    init(_ coordinate: RuntimeExactBuildCoordinate) {
        self.init(
            manifestAuthor: coordinate.manifestAuthor,
            dTag: coordinate.dTag,
            aggregateHash: coordinate.aggregateHash,
        )
    }

    init(_ session: RuntimeSessionSnapshot) {
        self.init(
            manifestAuthor: session.author,
            dTag: session.dTag,
            aggregateHash: session.aggregateHash,
        )
    }
}

public enum NativeRuntimeLibraryBuildAvailability:
    Equatable,
    Sendable
{
    case metadataOnly
    case sealedExactBytesReady
}

public enum NativeRuntimeLibrarySessionState:
    Equatable,
    Sendable
{
    case running
    case suspended
}

/// One Rust-owned session referenced by an installed-build row.
public struct NativeRuntimeLibrarySession:
    Equatable,
    Hashable,
    Sendable
{
    public let id: UInt64
    public let state: NativeRuntimeLibrarySessionState

    public init(
        id: UInt64,
        state: NativeRuntimeLibrarySessionState
    ) {
        self.id = id
        self.state = state
    }
}

/// One workspace present in the same Rust replacement frame.
public struct NativeRuntimeLibraryWorkspace:
    Equatable,
    Hashable,
    Sendable
{
    public let id: String

    public init(id: String) {
        self.id = id
    }
}

/// One raw runtime refusal copied without native reclassification.
public struct NativeRuntimeLibraryRefusal:
    Equatable,
    Hashable,
    Sendable
{
    public let code: String
    public let detail: String
    public let occurredAtMillis: UInt64

    public init(
        code: String,
        detail: String,
        occurredAtMillis: UInt64
    ) {
        self.code = code
        self.detail = detail
        self.occurredAtMillis = occurredAtMillis
    }

    init(_ refusal: RuntimeRefusal) {
        self.init(
            code: refusal.code,
            detail: refusal.detail,
            occurredAtMillis: refusal.occurredAtMillis,
        )
    }
}

/// One installed exact build from the Rust-owned library replacement.
///
/// Manifest metadata JSON deliberately remains behind the generated boundary.
/// This native projection does not reinterpret opaque verified metadata as
/// lifecycle or presentation authority.
public struct NativeRuntimeLibraryBuild:
    Equatable,
    Sendable
{
    public let exactBuild: NativeRuntimeLibraryExactBuild
    public let title: String
    public let availability: NativeRuntimeLibraryBuildAvailability
    public let sessions: [NativeRuntimeLibrarySession]
    public let assignedWorkspaceIDs: [String]

    public init(
        exactBuild: NativeRuntimeLibraryExactBuild,
        title: String,
        availability: NativeRuntimeLibraryBuildAvailability,
        sessions: [NativeRuntimeLibrarySession],
        assignedWorkspaceIDs: [String]
    ) {
        self.exactBuild = exactBuild
        self.title = title
        self.availability = availability
        self.sessions = sessions
        self.assignedWorkspaceIDs = assignedWorkspaceIDs
    }
}

/// One complete, bounded replacement suitable for native presentation.
public struct NativeRuntimeLibrarySnapshot:
    Equatable,
    Sendable
{
    /// Monotonic replacement revision owned by the Rust controller.
    public let revision: UInt64
    public let profileClosed: Bool
    public let filterQuery: String
    public let totalInstalled: UInt64
    public let builds: [NativeRuntimeLibraryBuild]
    public let workspaces: [NativeRuntimeLibraryWorkspace]
    public let refusals: [NativeRuntimeLibraryRefusal]

    public init(
        revision: UInt64,
        profileClosed: Bool,
        filterQuery: String,
        totalInstalled: UInt64,
        builds: [NativeRuntimeLibraryBuild],
        workspaces: [NativeRuntimeLibraryWorkspace],
        refusals: [NativeRuntimeLibraryRefusal]
    ) {
        self.revision = revision
        self.profileClosed = profileClosed
        self.filterQuery = filterQuery
        self.totalInstalled = totalInstalled
        self.builds = builds
        self.workspaces = workspaces
        self.refusals = refusals
    }
}
