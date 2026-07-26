import Foundation
import NMPNativeRuntime

// MARK: - Installed-library replacement projection

/// A generated frame cannot be represented faithfully by the current typed
/// Apple library surface. The raw unsupported value or exact mismatch remains
/// visible in the refusal; projection never guesses or drops the row.
public enum NativeRuntimeLibraryProjectionRefusal:
    Error,
    LocalizedError,
    Equatable,
    Sendable
{
    case countExceeded(field: String, actual: Int, maximum: Int)
    case filterTooLarge(actualUTF8Bytes: Int, maximum: Int)
    case runtime(RuntimeRefusal)
    case unsupportedBuildAvailability(
        exactBuild: NativeRuntimeLibraryExactBuild,
    )

    public var errorDescription: String? {
        switch self {
        case let .countExceeded(field, actual, maximum):
            "\(field) contains \(actual) items; the maximum is \(maximum)."
        case let .filterTooLarge(actual, maximum):
            "The library filter is \(actual) UTF-8 bytes; the maximum is \(maximum)."
        case let .runtime(refusal):
            "Runtime snapshot projection was refused (\(refusal.code)): \(refusal.detail)"
        case let .unsupportedBuildAvailability(exactBuild):
            "Exact build \(exactBuild.dTag) has unsupported availability."
        }
    }
}

/// Result of mechanically projecting one generated replacement frame.
public enum NativeRuntimeLibraryProjection:
    Equatable,
    Sendable
{
    case snapshot(NativeRuntimeLibrarySnapshot)
    case refused(
        revision: UInt64,
        profileClosed: Bool,
        refusal: NativeRuntimeLibraryProjectionRefusal,
    )

    public var revision: UInt64 {
        switch self {
        case let .snapshot(snapshot):
            snapshot.revision
        case let .refused(revision, _, _):
            revision
        }
    }

    public var profileClosed: Bool {
        switch self {
        case let .snapshot(snapshot):
            snapshot.profileClosed
        case let .refused(_, profileClosed, _):
            profileClosed
        }
    }

    /// Projects only data already present in one generated Rust snapshot.
    public init(_ source: RuntimeSnapshot) {
        self = Self.project(source)
    }

    init(_ source: NativeRuntimeSnapshotProjection) {
        switch source {
        case let .snapshot(snapshot):
            self = Self.project(snapshot)
        case let .refused(revision, profileClosed, refusal):
            self = .refused(
                revision: revision,
                profileClosed: profileClosed,
                refusal: .runtime(refusal)
            )
        }
    }

    private static func project(
        _ source: RuntimeSnapshot
    ) -> NativeRuntimeLibraryProjection {
        let library = source.installedLibrary
        let rejected: (NativeRuntimeLibraryProjectionRefusal)
            -> NativeRuntimeLibraryProjection = { refusal in
                .refused(
                    revision: source.revision,
                    profileClosed: source.closed,
                    refusal: refusal,
                )
            }
        if let refusal = countRefusal(
            library.builds.count,
            field: "installedLibrary.builds",
            maximum: NativeRuntimeLibraryLimits.maximumBuilds,
        ) {
            return rejected(refusal)
        }
        if let refusal = countRefusal(
            source.sessions.count,
            field: "sessions",
            maximum: NativeRuntimeLibraryLimits.maximumSessions,
        ) {
            return rejected(refusal)
        }
        if let refusal = countRefusal(
            source.workspaces.count,
            field: "workspaces",
            maximum: NativeRuntimeLibraryLimits.maximumWorkspaces,
        ) {
            return rejected(refusal)
        }
        if let refusal = countRefusal(
            source.boundaryRefusals.count,
            field: "boundaryRefusals",
            maximum: NativeRuntimeLibraryLimits.maximumBoundaryRefusals,
        ) {
            return rejected(refusal)
        }
        let filterBytes = library.query.utf8.count
        guard
            filterBytes
            <= NativeRuntimeLibraryLimits.maximumFilterUTF8Bytes
        else {
            return rejected(
                .filterTooLarge(
                    actualUTF8Bytes: filterBytes,
                    maximum:
                    NativeRuntimeLibraryLimits.maximumFilterUTF8Bytes,
                ),
            )
        }
        var sessionsByID: [UInt64: RuntimeSessionSnapshot] = [:]
        sessionsByID.reserveCapacity(source.sessions.count)
        for session in source.sessions {
            sessionsByID[session.id] = session
        }

        var workspaces: [NativeRuntimeLibraryWorkspace] = []
        workspaces.reserveCapacity(source.workspaces.count)
        for workspace in source.workspaces {
            workspaces.append(
                NativeRuntimeLibraryWorkspace(id: workspace.workspaceId),
            )
        }

        var builds: [NativeRuntimeLibraryBuild] = []
        builds.reserveCapacity(library.builds.count)
        for build in library.builds {
            let exactBuild = NativeRuntimeLibraryExactBuild(build.coordinate)
            if let refusal = countRefusal(
                build.activeSessionIds.count,
                field: "installedLibrary.build.activeSessionIds",
                maximum:
                NativeRuntimeLibraryLimits.maximumSessionsPerBuild,
            ) {
                return rejected(refusal)
            }
            if let refusal = countRefusal(
                build.assignedWorkspaceIds.count,
                field: "installedLibrary.build.assignedWorkspaceIds",
                maximum:
                NativeRuntimeLibraryLimits
                    .maximumWorkspaceAssignmentsPerBuild,
            ) {
                return rejected(refusal)
            }

            var sessions: [NativeRuntimeLibrarySession] = []
            sessions.reserveCapacity(build.activeSessionIds.count)
            for sessionID in build.activeSessionIds {
                let session = sessionsByID[sessionID]!
                let state: NativeRuntimeLibrarySessionState
                switch session.state {
                case "running":
                    state = .running
                case "suspended":
                    state = .suspended
                default:
                    preconditionFailure(
                        "Rust published unsupported session state \(session.state)"
                    )
                }
                sessions.append(
                    NativeRuntimeLibrarySession(
                        id: sessionID,
                        state: state,
                    ),
                )
            }

            let availability: NativeRuntimeLibraryBuildAvailability
            switch build.availability {
            case .metadataOnly:
                availability = .metadataOnly
            case .sealedExactBytesReady:
                availability = .sealedExactBytesReady
            @unknown default:
                return rejected(
                    .unsupportedBuildAvailability(exactBuild: exactBuild),
                )
            }
            builds.append(
                NativeRuntimeLibraryBuild(
                    exactBuild: exactBuild,
                    title: build.title,
                    availability: availability,
                    sessions: sessions,
                    assignedWorkspaceIDs: build.assignedWorkspaceIds,
                ),
            )
        }

        return .snapshot(
            NativeRuntimeLibrarySnapshot(
                revision: source.revision,
                profileClosed: source.closed,
                filterQuery: library.query,
                totalInstalled: library.totalInstalled,
                builds: builds,
                workspaces: workspaces,
                refusals: source.boundaryRefusals.map(
                    NativeRuntimeLibraryRefusal.init,
                ),
            ),
        )
    }

    private static func countRefusal(
        _ actual: Int,
        field: String,
        maximum: Int,
    ) -> NativeRuntimeLibraryProjectionRefusal? {
        guard actual <= maximum else {
            return .countExceeded(
                field: field,
                actual: actual,
                maximum: maximum,
            )
        }
        return nil
    }
}
