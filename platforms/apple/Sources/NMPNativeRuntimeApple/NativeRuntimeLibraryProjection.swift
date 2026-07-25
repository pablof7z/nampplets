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
    case totalInstalledBelowVisible(
        totalInstalled: UInt64,
        visibleBuildCount: Int,
    )
    case duplicateBuild(NativeRuntimeLibraryExactBuild)
    case duplicateGlobalSessionID(UInt64)
    case duplicateWorkspaceID(String)
    case duplicateBuildSessionID(
        exactBuild: NativeRuntimeLibraryExactBuild,
        sessionID: UInt64,
    )
    case missingBuildSession(
        exactBuild: NativeRuntimeLibraryExactBuild,
        sessionID: UInt64,
    )
    case mismatchedBuildSession(
        exactBuild: NativeRuntimeLibraryExactBuild,
        sessionID: UInt64,
        sessionExactBuild: NativeRuntimeLibraryExactBuild,
    )
    case unsupportedSessionState(
        sessionID: UInt64,
        rawValue: String,
    )
    case duplicateWorkspaceAssignment(
        exactBuild: NativeRuntimeLibraryExactBuild,
        workspaceID: String,
    )
    case missingWorkspaceAssignment(
        exactBuild: NativeRuntimeLibraryExactBuild,
        workspaceID: String,
    )
    case unsupportedBuildAvailability(
        exactBuild: NativeRuntimeLibraryExactBuild,
    )

    public var errorDescription: String? {
        switch self {
        case let .countExceeded(field, actual, maximum):
            "\(field) contains \(actual) items; the maximum is \(maximum)."
        case let .filterTooLarge(actual, maximum):
            "The library filter is \(actual) UTF-8 bytes; the maximum is \(maximum)."
        case let .totalInstalledBelowVisible(total, visible):
            "The runtime reports \(total) total installs but projects \(visible) visible builds."
        case let .duplicateBuild(exactBuild):
            "The exact build \(exactBuild.dTag) appears more than once."
        case let .duplicateGlobalSessionID(sessionID):
            "Global session \(sessionID) appears more than once."
        case let .duplicateWorkspaceID(workspaceID):
            "Workspace \(workspaceID) appears more than once."
        case let .duplicateBuildSessionID(exactBuild, sessionID):
            "Exact build \(exactBuild.dTag) references session \(sessionID) more than once."
        case let .missingBuildSession(exactBuild, sessionID):
            "Exact build \(exactBuild.dTag) references missing session \(sessionID)."
        case let .mismatchedBuildSession(
            exactBuild,
            sessionID,
            sessionExactBuild,
        ):
            "Session \(sessionID) belongs to \(sessionExactBuild.dTag), not \(exactBuild.dTag)."
        case let .unsupportedSessionState(sessionID, rawValue):
            "Session \(sessionID) has unsupported raw state \(rawValue)."
        case let .duplicateWorkspaceAssignment(exactBuild, workspaceID):
            "Exact build \(exactBuild.dTag) repeats workspace \(workspaceID)."
        case let .missingWorkspaceAssignment(exactBuild, workspaceID):
            "Exact build \(exactBuild.dTag) references missing workspace \(workspaceID)."
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
        guard library.totalInstalled >= UInt64(library.builds.count) else {
            return rejected(
                .totalInstalledBelowVisible(
                    totalInstalled: library.totalInstalled,
                    visibleBuildCount: library.builds.count,
                ),
            )
        }

        var sessionsByID: [UInt64: RuntimeSessionSnapshot] = [:]
        sessionsByID.reserveCapacity(source.sessions.count)
        for session in source.sessions {
            guard sessionsByID.updateValue(session, forKey: session.id) == nil
            else {
                return rejected(.duplicateGlobalSessionID(session.id))
            }
        }

        var workspaceIDs = Set<String>()
        workspaceIDs.reserveCapacity(source.workspaces.count)
        var workspaces: [NativeRuntimeLibraryWorkspace] = []
        workspaces.reserveCapacity(source.workspaces.count)
        for workspace in source.workspaces {
            guard workspaceIDs.insert(workspace.workspaceId).inserted else {
                return rejected(
                    .duplicateWorkspaceID(workspace.workspaceId),
                )
            }
            workspaces.append(
                NativeRuntimeLibraryWorkspace(id: workspace.workspaceId),
            )
        }

        var exactBuilds = Set<NativeRuntimeLibraryExactBuild>()
        exactBuilds.reserveCapacity(library.builds.count)
        var builds: [NativeRuntimeLibraryBuild] = []
        builds.reserveCapacity(library.builds.count)
        for build in library.builds {
            let exactBuild = NativeRuntimeLibraryExactBuild(build.coordinate)
            guard exactBuilds.insert(exactBuild).inserted else {
                return rejected(.duplicateBuild(exactBuild))
            }
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

            var seenSessionIDs = Set<UInt64>()
            seenSessionIDs.reserveCapacity(build.activeSessionIds.count)
            var sessions: [NativeRuntimeLibrarySession] = []
            sessions.reserveCapacity(build.activeSessionIds.count)
            for sessionID in build.activeSessionIds {
                guard seenSessionIDs.insert(sessionID).inserted else {
                    return rejected(
                        .duplicateBuildSessionID(
                            exactBuild: exactBuild,
                            sessionID: sessionID,
                        ),
                    )
                }
                guard let session = sessionsByID[sessionID] else {
                    return rejected(
                        .missingBuildSession(
                            exactBuild: exactBuild,
                            sessionID: sessionID,
                        ),
                    )
                }
                let sessionExactBuild =
                    NativeRuntimeLibraryExactBuild(session)
                guard sessionExactBuild == exactBuild else {
                    return rejected(
                        .mismatchedBuildSession(
                            exactBuild: exactBuild,
                            sessionID: sessionID,
                            sessionExactBuild: sessionExactBuild,
                        ),
                    )
                }
                let state: NativeRuntimeLibrarySessionState
                switch session.state {
                case "running":
                    state = .running
                case "suspended":
                    state = .suspended
                default:
                    return rejected(
                        .unsupportedSessionState(
                            sessionID: sessionID,
                            rawValue: session.state,
                        ),
                    )
                }
                sessions.append(
                    NativeRuntimeLibrarySession(
                        id: sessionID,
                        state: state,
                    ),
                )
            }

            var seenWorkspaceIDs = Set<String>()
            seenWorkspaceIDs.reserveCapacity(
                build.assignedWorkspaceIds.count,
            )
            for workspaceID in build.assignedWorkspaceIds {
                guard seenWorkspaceIDs.insert(workspaceID).inserted else {
                    return rejected(
                        .duplicateWorkspaceAssignment(
                            exactBuild: exactBuild,
                            workspaceID: workspaceID,
                        ),
                    )
                }
                guard workspaceIDs.contains(workspaceID) else {
                    return rejected(
                        .missingWorkspaceAssignment(
                            exactBuild: exactBuild,
                            workspaceID: workspaceID,
                        ),
                    )
                }
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
