@testable import RuntimeWorkbenchFeature
import Testing

/// The Inspector reads "Running" vs "Session ended" from the set of builds the
/// runtime reports a live session for. A degraded session is live -- it is
/// missing a domain its own content requires, not absent -- so it has to be in
/// that set.
///
/// The failure this guards against is worse than the one the Inspector change
/// fixes. Today a dead session wrongly reads "Running", because the status came
/// from window bookkeeping. With an equality check on `.running`, a *live*
/// session would wrongly read "Session ended" -- and a napplet running without
/// a domain it declared is precisely the case a user most needs the Inspector
/// honest about. Reporting a working napplet as ended is a worse lie than
/// reporting a dead one as running.
///
/// `WorkbenchLibrarySessionState.isLive` exists for exactly this distinction:
/// read it for "does a session exist", match the case for "is it whole".

private let inspectedBuild = WorkbenchLibraryExactBuild(
    manifestAuthor: String(repeating: "a", count: 64),
    dTag: "good-morning",
    aggregateHash: String(repeating: "b", count: 64)
)!

private func build(
    sessions: [WorkbenchLibrarySession]
) -> WorkbenchLibraryBuild {
    WorkbenchLibraryBuild(
        exactBuild: inspectedBuild,
        title: "Good Morning",
        availability: .sealedExactBytesReady,
        sessions: sessions,
        // No workspace assignment: the snapshot validates that assigned IDs
        // are a subset of known workspaces, and none are declared here.
        assignedWorkspaceIDs: []
    )!
}

private func snapshot(
    sessions: [WorkbenchLibrarySession]
) -> WorkbenchLibrarySnapshot {
    WorkbenchLibrarySnapshot(
        revision: 1,
        availability: .available,
        filterQuery: "",
        totalInstalled: 1,
        builds: [build(sessions: sessions)],
        workspaces: [],
        refusals: []
    )!
}

/// The case that matters. This is the only assertion that distinguishes
/// `isLive` from `== .running`, and it fails against the equality check.
@Test func aDegradedSessionCountsAsLive() {
    let live = ContentView.buildsWithLiveSessions(
        in: snapshot(sessions: [
            WorkbenchLibrarySession(id: 1, state: .runningDegraded),
        ])
    )

    #expect(live.contains(inspectedBuild))
}

/// A whole session obviously still counts -- pinned so a future change cannot
/// satisfy the degraded case by accepting everything.
@Test func aWholeSessionCountsAsLive() {
    let live = ContentView.buildsWithLiveSessions(
        in: snapshot(sessions: [
            WorkbenchLibrarySession(id: 1, state: .running),
        ])
    )

    #expect(live.contains(inspectedBuild))
}

/// Suspended is not live. `isLive` must not degenerate into "any session at
/// all", or the Inspector goes back to reporting Running for something that
/// is not.
@Test func aSuspendedSessionIsNotLive() {
    let live = ContentView.buildsWithLiveSessions(
        in: snapshot(sessions: [
            WorkbenchLibrarySession(id: 1, state: .suspended),
        ])
    )

    #expect(!live.contains(inspectedBuild))
}

/// No sessions at all is the case the Inspector must report as ended. This is
/// the bug the Inspector change exists to fix, kept green alongside the others
/// so neither correction can be traded for the other.
@Test func aBuildWithNoSessionsIsNotLive() {
    let live = ContentView.buildsWithLiveSessions(in: snapshot(sessions: []))

    #expect(!live.contains(inspectedBuild))
}

/// A build whose only live session is degraded, alongside a suspended one, is
/// still live. Mixed state is the realistic shape once a napplet has been
/// paused and resumed, and it must not resolve to "ended".
@Test func aDegradedSessionBesideASuspendedOneIsStillLive() {
    let live = ContentView.buildsWithLiveSessions(
        in: snapshot(sessions: [
            WorkbenchLibrarySession(id: 1, state: .suspended),
            WorkbenchLibrarySession(id: 2, state: .runningDegraded),
        ])
    )

    #expect(live.contains(inspectedBuild))
}
