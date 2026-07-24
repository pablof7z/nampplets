/// One bounded startup plan for exact builds represented by persisted canvas
/// windows. The plan preserves the restored stacking order, ignores native
/// windows, and reacquires a duplicated exact build only once.
struct WorkbenchRestoredCanvasLaunchPlan: Equatable, Sendable {
    let identities: [WorkbenchExactBuildIdentity]

    init(layout: WorkbenchLayoutModel) {
        var admitted = Set<WorkbenchExactBuildIdentity>()
        var identities: [WorkbenchExactBuildIdentity] = []
        identities.reserveCapacity(
            min(
                layout.windows.count,
                WorkbenchLayoutSnapshot.maximumWindowCount
            )
        )
        for window in layout.windows {
            guard
                let identity = window.exactBuild,
                admitted.insert(identity).inserted
            else {
                continue
            }
            identities.append(identity)
            if identities.count == WorkbenchLayoutSnapshot.maximumWindowCount {
                break
            }
        }
        self.identities = identities
    }

    static func reviewMatchesPersistedBuild(
        manifestAuthor: String,
        dTag: String?,
        aggregateHash: String,
        identity: WorkbenchExactBuildIdentity
    ) -> Bool {
        manifestAuthor == identity.manifestAuthor
            && dTag == identity.dTag
            && aggregateHash == identity.aggregateHash
    }
}
