import Observation
import SwiftUI

@MainActor
@Observable
final class WorkbenchLibrarySheetModel {
    var filterDraft = ""

    private(set) var snapshot: WorkbenchLibrarySnapshot?
    private(set) var updateGap: WorkbenchLibraryUpdateGap?

    private let manager: any WorkbenchLibraryManaging
    private var subscription: (any WorkbenchLibrarySubscription)?

    init(manager: any WorkbenchLibraryManaging) {
        self.manager = manager
    }

    var commandsAvailable: Bool {
        snapshot?.availability.isAvailable == true
    }

    func start() {
        guard subscription == nil else {
            return
        }
        subscription = manager.subscribe { [weak self] update in
            self?.receive(update)
        }
    }

    func stop() {
        subscription?.cancel()
        subscription = nil
    }

    func applyFilter() {
        guard commandsAvailable else {
            return
        }
        manager.setFilter(filterDraft)
    }

    func clearFilter() {
        filterDraft = ""
        applyFilter()
    }

    func refresh() {
        receive(.authoritative(manager.refresh()))
    }

    func suspend(_ session: WorkbenchLibrarySession) {
        guard commandsAvailable, session.state == .running else {
            return
        }
        manager.suspend(sessionID: session.id)
    }

    func resume(_ session: WorkbenchLibrarySession) {
        guard commandsAvailable, session.state == .suspended else {
            return
        }
        manager.resume(sessionID: session.id)
    }

    func assign(
        _ exactBuild: WorkbenchLibraryExactBuild,
        to workspace: WorkbenchLibraryWorkspace
    ) {
        guard commandsAvailable else {
            return
        }
        manager.assign(exactBuild, toWorkspaceID: workspace.id)
    }

    func clearAssignment(
        _ exactBuild: WorkbenchLibraryExactBuild,
        from workspace: WorkbenchLibraryWorkspace
    ) {
        guard commandsAvailable else {
            return
        }
        manager.clearAssignment(exactBuild, fromWorkspaceID: workspace.id)
    }

    func uninstall(_ exactBuild: WorkbenchLibraryExactBuild) {
        guard commandsAvailable else {
            return
        }
        manager.uninstall(exactBuild)
    }

    private func receive(_ update: WorkbenchLibraryUpdate) {
        switch update {
        case let .authoritative(nextSnapshot):
            snapshot = nextSnapshot
            filterDraft = nextSnapshot.filterQuery
            updateGap = nil

        case let .next(nextSnapshot, predecessorRevision, lostBeforeBatch):
            guard let currentRevision = snapshot?.revision else {
                snapshot = nextSnapshot
                filterDraft = nextSnapshot.filterQuery
                updateGap = WorkbenchLibraryUpdateGap(
                    expectedPredecessorRevision: 0,
                    receivedPredecessorRevision: predecessorRevision,
                    receivedRevision: nextSnapshot.revision,
                    lostBeforeBatch: lostBeforeBatch
                )
                return
            }
            // Reported before the freshness guard on purpose. A stale cursor
            // does not imply a newer revision — the frame layer re-delivers at
            // the same one — so warning only about newer snapshots would drop
            // exactly the case this is meant to catch.
            if lostBeforeBatch > 0 {
                updateGap = WorkbenchLibraryUpdateGap(
                    expectedPredecessorRevision: currentRevision,
                    receivedPredecessorRevision: predecessorRevision,
                    receivedRevision: nextSnapshot.revision,
                    lostBeforeBatch: lostBeforeBatch
                )
            }
            guard nextSnapshot.revision > currentRevision else {
                return
            }
            if predecessorRevision != currentRevision {
                updateGap = WorkbenchLibraryUpdateGap(
                    expectedPredecessorRevision: currentRevision,
                    receivedPredecessorRevision: predecessorRevision,
                    receivedRevision: nextSnapshot.revision,
                    lostBeforeBatch: lostBeforeBatch
                )
            }
            snapshot = nextSnapshot
            filterDraft = nextSnapshot.filterQuery
        }
    }
}
