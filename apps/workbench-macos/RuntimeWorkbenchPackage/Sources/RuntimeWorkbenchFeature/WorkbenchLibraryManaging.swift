import Observation
import SwiftUI

@MainActor
public protocol WorkbenchLibrarySubscription: AnyObject {
    func cancel()
}

/// Injectable boundary for the Rust-owned installed-library projection.
///
/// Implementations immediately push one authoritative snapshot, then bounded
/// replacement updates. Commands are fire-and-observe: they never report
/// operation success to Swift. Filtering, lifecycle legality, exact-build
/// cleanup, workspace validation, persistence, and refusal semantics remain
/// in Rust.
@MainActor
public protocol WorkbenchLibraryManaging: AnyObject {
    func subscribe(
        receive: @escaping @MainActor (WorkbenchLibraryUpdate) -> Void
    ) -> any WorkbenchLibrarySubscription

    func refresh() -> WorkbenchLibrarySnapshot
    func setFilter(_ query: String)
    func suspend(sessionID: UInt64)
    func resume(sessionID: UInt64)
    func assign(
        _ exactBuild: WorkbenchLibraryExactBuild,
        toWorkspaceID workspaceID: String
    )
    func clearAssignment(
        _ exactBuild: WorkbenchLibraryExactBuild,
        fromWorkspaceID workspaceID: String
    )
    func uninstall(_ exactBuild: WorkbenchLibraryExactBuild)
}

/// Truthful fallback used when this runtime build does not expose the typed
/// installed-library projection.
///
/// It publishes one immutable authoritative snapshot and never accepts
/// commands. The sheet therefore remains reachable without suggesting that
/// local filtering or lifecycle mutations succeeded.
@MainActor
public final class UnavailableWorkbenchLibraryManager:
    WorkbenchLibraryManaging
{
    public static let defaultReason =
        "Installed-library APIs are unavailable in this runtime build."

    private let snapshot: WorkbenchLibrarySnapshot

    public init(reason: String = defaultReason) {
        let requestedSnapshot = WorkbenchLibrarySnapshot(
            revision: 0,
            availability: .unavailable(reason: reason),
            filterQuery: "",
            totalInstalled: 0,
            builds: [],
            workspaces: []
        )
        guard
            let snapshot = requestedSnapshot
            ?? WorkbenchLibrarySnapshot(
                revision: 0,
                availability: .unavailable(reason: Self.defaultReason),
                filterQuery: "",
                totalInstalled: 0,
                builds: [],
                workspaces: []
            )
        else {
            preconditionFailure(
                "The fixed unavailable library snapshot must remain valid"
            )
        }
        self.snapshot = snapshot
    }

    public func subscribe(
        receive: @escaping @MainActor (WorkbenchLibraryUpdate) -> Void
    ) -> any WorkbenchLibrarySubscription {
        receive(.authoritative(snapshot))
        return UnavailableWorkbenchLibrarySubscription()
    }

    public func refresh() -> WorkbenchLibrarySnapshot {
        snapshot
    }

    public func setFilter(_: String) {}
    public func suspend(sessionID _: UInt64) {}
    public func resume(sessionID _: UInt64) {}

    public func assign(
        _: WorkbenchLibraryExactBuild,
        toWorkspaceID _: String
    ) {}

    public func clearAssignment(
        _: WorkbenchLibraryExactBuild,
        fromWorkspaceID _: String
    ) {}

    public func uninstall(_: WorkbenchLibraryExactBuild) {}
}

@MainActor
private final class UnavailableWorkbenchLibrarySubscription:
    WorkbenchLibrarySubscription
{
    func cancel() {}
}
