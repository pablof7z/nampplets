import Foundation
import NMPNativeRuntimeApple

extension RuntimeWorkbenchLibraryManager {
    static func project(
        _ projection: NativeRuntimeLibraryProjection
    ) -> WorkbenchLibrarySnapshot {
        switch projection {
        case .refused(let revision, let profileClosed, let refusal):
            let reason = profileClosed
                ? "The native runtime profile is closed."
                : "Native installed-library projection was refused: "
                    + displaySafeReason(
                        refusal.localizedDescription,
                        fallback: "The projection could not be represented."
                    )
            return unavailableSnapshot(revision: revision, reason: reason)

        case .snapshot(let snapshot):
            guard !snapshot.profileClosed else {
                return unavailableSnapshot(
                    revision: snapshot.revision,
                    reason: "The native runtime profile is closed."
                )
            }
            do {
                return try project(snapshot)
            } catch {
                return unavailableSnapshot(
                    revision: snapshot.revision,
                    reason:
                        "Native installed-library projection was refused: "
                        + displaySafeReason(
                            error.localizedDescription,
                            fallback: "The projection could not be represented."
                        )
                )
            }
        }
    }

    static func project(
        _ native: NativeRuntimeLibrarySnapshot
    ) throws -> WorkbenchLibrarySnapshot {
        let workspaces = try native.workspaces.map { workspace in
            guard
                let projected = WorkbenchLibraryWorkspace(
                    id: workspace.id,
                    displayName: workspace.id
                )
            else {
                throw RuntimeWorkbenchLibraryProjectionError.invalidWorkspace
            }
            return projected
        }
        let builds = try native.builds.map { build in
            guard
                let exactBuild = WorkbenchLibraryExactBuild(
                    manifestAuthor: build.exactBuild.manifestAuthor,
                    dTag: build.exactBuild.dTag,
                    aggregateHash: build.exactBuild.aggregateHash
                )
            else {
                throw RuntimeWorkbenchLibraryProjectionError.invalidExactBuild
            }
            let sessions = build.sessions.map {
                WorkbenchLibrarySession(
                    id: $0.id,
                    state: sessionState($0.state)
                )
            }
            guard
                let projected = WorkbenchLibraryBuild(
                    exactBuild: exactBuild,
                    title: build.title,
                    availability: availability(build.availability),
                    sessions: sessions,
                    assignedWorkspaceIDs: build.assignedWorkspaceIDs
                )
            else {
                throw RuntimeWorkbenchLibraryProjectionError.invalidBuild
            }
            return projected
        }
        let refusals = try native.refusals.map { refusal in
            guard
                let projected = WorkbenchLibraryRefusal(
                    code: refusal.code,
                    message: refusal.detail,
                    occurredAtMillis: refusal.occurredAtMillis
                )
            else {
                throw RuntimeWorkbenchLibraryProjectionError.invalidRefusal
            }
            return projected
        }
        guard
            let snapshot = WorkbenchLibrarySnapshot(
                revision: native.revision,
                availability: .available,
                filterQuery: native.filterQuery,
                totalInstalled: native.totalInstalled,
                builds: builds,
                workspaces: workspaces,
                refusals: refusals,
                droppedRefusalCount: native.droppedRefusalCount
            )
        else {
            throw RuntimeWorkbenchLibraryProjectionError.invalidSnapshot
        }
        return snapshot
    }

    static func nativeExactBuild(
        _ exactBuild: WorkbenchLibraryExactBuild
    ) -> NativeRuntimeLibraryExactBuild {
        NativeRuntimeLibraryExactBuild(
            manifestAuthor: exactBuild.manifestAuthor,
            dTag: exactBuild.dTag,
            aggregateHash: exactBuild.aggregateHash
        )
    }

    private static func sessionState(
        _ state: NativeRuntimeLibrarySessionState
    ) -> WorkbenchLibrarySessionState {
        switch state {
        case .running:
            .running
        case .suspended:
            .suspended
        }
    }

    static func availability(
        _ availability: NativeRuntimeLibraryBuildAvailability
    ) -> WorkbenchLibraryBuildAvailability {
        switch availability {
        case .metadataOnly:
            .metadataOnly
        case .sealedExactBytesReady:
            .sealedExactBytesReady
        }
    }

    static func unavailableSnapshot(
        revision: UInt64,
        reason: String
    ) -> WorkbenchLibrarySnapshot {
        let safeReason = displaySafeReason(
            reason,
            fallback: "The installed-library projection is unavailable."
        )
        guard
            let snapshot = WorkbenchLibrarySnapshot(
                revision: revision,
                availability: .unavailable(reason: safeReason),
                filterQuery: "",
                totalInstalled: 0,
                builds: [],
                workspaces: [],
                refusals: []
            )
        else {
            preconditionFailure(
                "The fixed unavailable library snapshot must remain valid"
            )
        }
        return snapshot
    }

    static func displaySafeReason(
        _ reason: String,
        fallback: String
    ) -> String {
        guard
            !reason.isEmpty,
            reason.utf8.count
                <= WorkbenchLibraryLimits.maximumRefusalMessageUTF8Bytes,
            !reason.unicodeScalars.contains(where: {
                CharacterSet.controlCharacters.contains($0)
            })
        else {
            return fallback
        }
        return reason
    }
}
