import SwiftUI

/// One napplet on the shelf.
///
/// A row leads with the name and one action. Session numbers, workspace
/// assignment and exact build identity are all real, and all one deliberate
/// move away. See `docs/adr/0008-verdicts-on-the-path.md`.
struct WorkbenchLibraryBuildRow: View {
    let build: WorkbenchLibraryBuild
    let workspaces: [WorkbenchLibraryWorkspace]
    let commandsAvailable: Bool
    let onOpen: () -> Void
    let onSuspend: (WorkbenchLibrarySession) -> Void
    let onResume: (WorkbenchLibrarySession) -> Void
    let onAssign: (WorkbenchLibraryWorkspace) -> Void
    let onClearAssignment: (WorkbenchLibraryWorkspace) -> Void
    let onRequestUninstall: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.tight) {
            HStack(alignment: .firstTextBaseline, spacing: NappletMetrics.snug) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(build.title)
                        .font(NappletType.title)
                        .foregroundStyle(NappletInk.ink)
                    if let status {
                        Text(status)
                            .font(NappletType.caption)
                            .foregroundStyle(NappletInk.inkSecondary)
                    }
                }

                Spacer()

                Button("Open", action: onOpen)
                    .disabled(!commandsAvailable)
                    .accessibilityLabel("Open \(build.title)")
                    .accessibilityIdentifier("open-installed-napplet")

                Menu {
                    sessionCommands
                    workspaceCommands

                    Divider()

                    Button("Remove…", systemImage: "trash", role: .destructive) {
                        onRequestUninstall()
                    }
                    .disabled(!commandsAvailable)
                } label: {
                    Label("More", systemImage: "ellipsis.circle")
                }
                .menuStyle(.borderlessButton)
                .labelStyle(.iconOnly)
                .fixedSize()
                .accessibilityLabel("More options for \(build.title)")
            }

            if !readyToRun {
                Text(WorkbenchLibraryPlainPresentation.notReadyMessage)
                    .font(NappletType.caption)
                    .foregroundStyle(NappletInk.caution)
                    .fixedSize(horizontal: false, vertical: true)
            }

            NappletEvidence {
                NappletFieldGrid(fields: evidenceFields)
            }
            .font(NappletType.caption)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(NappletMetrics.comfortable)
        .background(
            NappletInk.fillQuiet,
            in: RoundedRectangle(
                cornerRadius: NappletMetrics.cardCorner,
                style: .continuous
            )
        )
        .accessibilityElement(children: .contain)
    }

    /// Silence when a napplet is simply ready. The row speaks only about what
    /// is unusual: that it is running, paused, or not fully downloaded.
    private var status: String? {
        // Counts live sessions, degraded included -- "2 running" must not
        // become "1 running" because one is missing a domain. Degradation is
        // named on the session itself, not subtracted from this total.
        let running = build.sessions.filter(\.state.isLive).count
        let suspended = build.sessions.filter { $0.state == .suspended }.count
        if running > 0 {
            return suspended > 0 ? "Running · \(suspended) paused" : "Running"
        }
        if suspended > 0 {
            return suspended == 1 ? "Paused" : "\(suspended) paused"
        }
        return readyToRun ? nil : "Not ready to run"
    }

    private var readyToRun: Bool {
        build.availability == .sealedExactBytesReady
    }

    @ViewBuilder
    private var sessionCommands: some View {
        if !build.sessions.isEmpty {
            Section("Running now") {
                ForEach(build.sessions) { session in
                    switch session.state {
                    // Degraded offers the same command as running: it is a
                    // live session and pausing it means the same thing. The
                    // shortfall is stated on the session, not by withholding
                    // a control the operator would expect.
                    case .running, .runningDegraded:
                        Button("Pause", systemImage: "pause.circle") {
                            onSuspend(session)
                        }
                        .disabled(!commandsAvailable)
                    case .suspended:
                        Button("Resume", systemImage: "play.circle") {
                            onResume(session)
                        }
                        .disabled(!commandsAvailable)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var workspaceCommands: some View {
        if !workspaces.isEmpty {
            Section("Show in") {
                ForEach(workspaces) { workspace in
                    let isAssigned = assignedWorkspaceIDs.contains(workspace.id)
                    Button {
                        if isAssigned {
                            onClearAssignment(workspace)
                        } else {
                            onAssign(workspace)
                        }
                    } label: {
                        if isAssigned {
                            Label(workspace.displayName, systemImage: "checkmark")
                        } else {
                            Text(workspace.displayName)
                        }
                    }
                    .disabled(!commandsAvailable)
                }
            }
        }
    }

    private var assignedWorkspaceIDs: Set<String> {
        Set(build.assignedWorkspaceIDs)
    }

    private var evidenceFields: [NappletField] {
        var fields = [
            NappletField("Publisher key", build.exactBuild.manifestAuthor),
            NappletField("dTag", build.exactBuild.dTag),
            NappletField("Aggregate hash", build.exactBuild.aggregateHash),
            NappletField("Availability", build.availability.title),
            NappletField("Availability detail", build.availability.detail),
        ]
        for session in build.sessions {
            fields.append(NappletField(
                "Session \(session.id)",
                session.state.title
            ))
        }
        if !build.assignedWorkspaceIDs.isEmpty {
            fields.append(NappletField(
                "Assigned workspaces",
                build.assignedWorkspaceIDs.joined(separator: ", ")
            ))
        }
        return fields
    }
}
