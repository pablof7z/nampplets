import Observation

@MainActor
@Observable
final class ActivityViewModel {
    let scope: ActivityExactBuildScope
    let developerModeAvailable: Bool

    var severityFilter = Set(ActivitySeverity.allCases)
    var categoryFilter = Set(ActivityCategory.allCases)

    private(set) var snapshot: ActivitySnapshot?
    private(set) var updateGap: ActivityUpdateGap?
    private(set) var refreshRefusal: RuntimeWorkbenchActivitySourceRefusal?
    private(set) var developerModeEnabled = false

    private let source: any ActivitySource
    private var subscription: (any ActivitySubscription)?

    init(
        source: any ActivitySource,
        scope: ActivityExactBuildScope,
        developerModeAvailable: Bool
    ) {
        self.source = source
        self.scope = scope
        self.developerModeAvailable = developerModeAvailable
    }

    var visibleFacts: [ActivityFact] {
        guard let snapshot else {
            return []
        }
        return snapshot.facts.filter {
            severityFilter.contains($0.severity)
                && categoryFilter.contains($0.category)
        }
    }

    func start() {
        guard subscription == nil else {
            return
        }
        subscription = source.subscribe(to: scope) { [weak self] update in
            self?.receive(update)
        }
    }

    func stop() {
        subscription?.cancel()
        subscription = nil
    }

    func refresh() {
        do {
            let refreshed = try source.refresh(scope: scope)
            refreshRefusal = nil
            receive(.authoritative(refreshed))
        } catch let refusal as RuntimeWorkbenchActivitySourceRefusal {
            refreshRefusal = refusal
        } catch {
            assertionFailure("Unexpected activity refresh error: \(error)")
        }
    }

    func setSeverity(_ severity: ActivitySeverity, isIncluded: Bool) {
        if isIncluded {
            severityFilter.insert(severity)
        } else {
            severityFilter.remove(severity)
        }
    }

    func setCategory(_ category: ActivityCategory, isIncluded: Bool) {
        if isIncluded {
            categoryFilter.insert(category)
        } else {
            categoryFilter.remove(category)
        }
    }

    func setDeveloperModeEnabled(_ isEnabled: Bool) {
        developerModeEnabled = developerModeAvailable && isEnabled
    }

    func detailFields(for fact: ActivityFact) -> [ActivityDetailField] {
        guard developerModeEnabled else {
            return []
        }
        return fact.detailFields
    }

    private func receive(_ update: ActivityUpdate) {
        switch update {
        case let .authoritative(nextSnapshot):
            guard nextSnapshot.scope == scope else {
                return
            }
            snapshot = nextSnapshot
            updateGap = nil
            refreshRefusal = nil

        case let .next(nextSnapshot, predecessorRevision):
            guard nextSnapshot.scope == scope else {
                return
            }
            guard let currentRevision = snapshot?.revision else {
                updateGap = ActivityUpdateGap(
                    expectedPredecessorRevision: 0,
                    receivedPredecessorRevision: predecessorRevision,
                    receivedRevision: nextSnapshot.revision
                )
                snapshot = nextSnapshot
                return
            }
            guard nextSnapshot.revision > currentRevision else {
                return
            }
            if predecessorRevision != currentRevision {
                updateGap = ActivityUpdateGap(
                    expectedPredecessorRevision: currentRevision,
                    receivedPredecessorRevision: predecessorRevision,
                    receivedRevision: nextSnapshot.revision
                )
            }
            snapshot = nextSnapshot
            refreshRefusal = nil
        }
    }
}
