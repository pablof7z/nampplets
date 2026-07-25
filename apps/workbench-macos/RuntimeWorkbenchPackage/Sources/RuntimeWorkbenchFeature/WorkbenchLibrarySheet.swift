import SwiftUI

public struct WorkbenchLibrarySheet: View {
    @Environment(\.dismiss) private var dismiss
    @State var model: WorkbenchLibrarySheetModel
    @State private var uninstallCandidate: WorkbenchLibraryBuild?
    @FocusState private var filterFocused: Bool
    private let onOpen: @MainActor (WorkbenchLibraryBuild) -> Void

    @MainActor
    public init(
        manager: any WorkbenchLibraryManaging,
        onOpen: @escaping @MainActor (WorkbenchLibraryBuild) -> Void = { _ in }
    ) {
        _model = State(
            initialValue: WorkbenchLibrarySheetModel(manager: manager)
        )
        self.onOpen = onOpen
    }

    public var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                filterBar
                Divider()

                if let snapshot = model.snapshot {
                    if let reason = snapshot.availability.unavailableReason {
                        unavailableBanner(reason)
                        Divider()
                    }

                    if let gap = model.updateGap {
                        updateGapBanner(gap)
                        Divider()
                    }

                    if let refusal = snapshot.refusals.last {
                        refusalBanner(refusal)
                        Divider()
                    }

                    library(snapshot)
                } else {
                    ProgressView("Waiting for installed library…")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .accessibilityLabel(
                            "Waiting for the installed library snapshot"
                        )
                }
            }
            .navigationTitle("Installed Napplets")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") {
                        dismiss()
                    }
                    .keyboardShortcut(.cancelAction)
                }

                ToolbarItem {
                    Button("Refresh", systemImage: "arrow.clockwise") {
                        model.refresh()
                    }
                    .accessibilityHint(
                        "Requests one authoritative installed library snapshot"
                    )
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 760, idealWidth: 900, minHeight: 540, idealHeight: 700)
        #endif
        .onAppear {
            model.start()
            filterFocused = true
        }
        .onDisappear {
            model.stop()
        }
        .confirmationDialog(
            uninstallCandidate.map { "Uninstall \($0.title)?" } ?? "Uninstall exact build?",
            isPresented: Binding(
                get: { uninstallCandidate != nil },
                set: { isPresented in
                    if !isPresented {
                        uninstallCandidate = nil
                    }
                }
            ),
            titleVisibility: .visible,
            presenting: uninstallCandidate
        ) { build in
            Button("Uninstall Exact Build", role: .destructive) {
                model.uninstall(build.exactBuild)
                uninstallCandidate = nil
            }
            Button("Cancel", role: .cancel) {
                uninstallCandidate = nil
            }
        } message: { build in
            Text(
                "This asks the runtime to remove only state owned for "
                    + "\(build.exactBuild.dTag) at aggregate "
                    + "\(build.exactBuild.aggregateHash). The row remains "
                    + "visible until Rust confirms the new library snapshot."
            )
        }
        .accessibilityIdentifier("workbench-installed-library")
    }

    private var filterBar: some View {
        HStack(spacing: 10) {
            TextField("Filter installed napplets", text: $model.filterDraft)
                .textFieldStyle(.roundedBorder)
                .focused($filterFocused)
                .onSubmit {
                    model.applyFilter()
                }
                .disabled(!model.commandsAvailable)
                .accessibilityLabel("Filter installed napplets")
                .accessibilityHint(
                    "Sends the filter to the runtime; results are not filtered locally"
                )

            Button("Filter", systemImage: "line.3.horizontal.decrease.circle") {
                model.applyFilter()
            }
            .keyboardShortcut("f", modifiers: [.command, .option])
            .disabled(!model.commandsAvailable)

            if model.snapshot?.filterQuery.isEmpty == false {
                Button("Clear") {
                    model.clearFilter()
                }
                .disabled(!model.commandsAvailable)
            }
        }
        .padding()
    }

    @ViewBuilder
    private func library(_ snapshot: WorkbenchLibrarySnapshot) -> some View {
        if snapshot.builds.isEmpty {
            ContentUnavailableView(
                snapshot.filterQuery.isEmpty
                    ? "No Installed Napplets"
                    : "No Matching Napplets",
                systemImage: "square.stack.3d.up.slash",
                description: Text(
                    snapshot.filterQuery.isEmpty
                        ? "Verified installations will appear here."
                        : "The runtime found no installed build for this filter."
                )
            )
        } else {
            List(snapshot.builds) { build in
                WorkbenchLibraryBuildRow(
                    build: build,
                    workspaces: snapshot.workspaces,
                    commandsAvailable: model.commandsAvailable,
                    onOpen: {
                        onOpen(build)
                    },
                    onSuspend: model.suspend,
                    onResume: model.resume,
                    onAssign: { workspace in
                        model.assign(build.exactBuild, to: workspace)
                    },
                    onClearAssignment: { workspace in
                        model.clearAssignment(
                            build.exactBuild,
                            from: workspace
                        )
                    },
                    onRequestUninstall: {
                        uninstallCandidate = build
                    }
                )
            }
            .overlay(alignment: .bottomTrailing) {
                Text(
                    "Showing \(snapshot.builds.count) of "
                        + "\(snapshot.totalInstalled) installed"
                )
                .font(.caption)
                .foregroundStyle(.secondary)
                .padding(8)
                .background(.bar, in: Capsule())
                .padding()
                .accessibilityLabel(
                    "Showing \(snapshot.builds.count) of "
                        + "\(snapshot.totalInstalled) installed napplets"
                )
            }
        }
    }
}

private struct WorkbenchLibraryBuildRow: View {
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
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: availabilitySymbol)
                    .foregroundStyle(availabilityColor)
                    .font(.title2)
                    .frame(width: 28)
                    .accessibilityHidden(true)

                VStack(alignment: .leading, spacing: 5) {
                    Text(build.title)
                        .font(.headline)
                    Label(
                        build.availability.title,
                        systemImage: availabilitySymbol
                    )
                    .font(.caption)
                    .foregroundStyle(availabilityColor)
                    Text(build.availability.detail)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Spacer()

                Button("Open", systemImage: "rectangle.on.rectangle") {
                    onOpen()
                }
                .disabled(!commandsAvailable)
                .accessibilityLabel("Open \(build.title) on canvas")
                .accessibilityIdentifier("open-installed-napplet")

                Menu("Workspace", systemImage: "rectangle.3.group") {
                    if workspaces.isEmpty {
                        Text("No workspaces available")
                    } else {
                        Section("Assign") {
                            ForEach(unassignedWorkspaces) { workspace in
                                Button(workspace.displayName) {
                                    onAssign(workspace)
                                }
                            }
                            if unassignedWorkspaces.isEmpty {
                                Text("Assigned to every workspace")
                            }
                        }

                        if !assignedWorkspaces.isEmpty {
                            Section("Remove assignment") {
                                ForEach(assignedWorkspaces) { workspace in
                                    Button(workspace.displayName) {
                                        onClearAssignment(workspace)
                                    }
                                }
                            }
                        }
                    }
                }
                .disabled(!commandsAvailable || workspaces.isEmpty)
                .accessibilityHint(
                    "Assigns or removes this exact build from a runtime workspace"
                )

                Button(
                    "Uninstall",
                    systemImage: "trash",
                    role: .destructive,
                    action: onRequestUninstall
                )
                .disabled(!commandsAvailable)
                .accessibilityHint(
                    "Opens a confirmation for this exact aggregate"
                )
            }

            exactBuildIdentity

            if !assignedWorkspaces.isEmpty {
                LabeledContent("Assigned workspaces") {
                    Text(
                        assignedWorkspaces
                            .map(\.displayName)
                            .joined(separator: ", ")
                    )
                }
                .font(.caption)
            }

            sessionList
        }
        .padding(.vertical, 8)
        .accessibilityElement(children: .contain)
    }

    private var exactBuildIdentity: some View {
        Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 3) {
            GridRow {
                Text("Publisher")
                    .foregroundStyle(.secondary)
                Text(build.exactBuild.manifestAuthor)
                    .textSelection(.enabled)
            }
            GridRow {
                Text("d-tag")
                    .foregroundStyle(.secondary)
                Text(build.exactBuild.dTag)
                    .textSelection(.enabled)
            }
            GridRow {
                Text("Aggregate")
                    .foregroundStyle(.secondary)
                Text(build.exactBuild.aggregateHash)
                    .textSelection(.enabled)
            }
        }
        .font(.caption.monospaced())
        .accessibilityElement(children: .combine)
        .accessibilityLabel(
            "Exact build \(build.exactBuild.dTag), publisher "
                + "\(build.exactBuild.manifestAuthor), aggregate "
                + "\(build.exactBuild.aggregateHash)"
        )
    }

    @ViewBuilder
    private var sessionList: some View {
        if build.sessions.isEmpty {
            Label("No active sessions", systemImage: "pause.rectangle")
                .font(.caption)
                .foregroundStyle(.secondary)
        } else {
            VStack(alignment: .leading, spacing: 6) {
                Text("Sessions")
                    .font(.caption.weight(.semibold))

                ForEach(build.sessions) { session in
                    HStack {
                        Label(
                            "Session \(session.id): \(session.state.title)",
                            systemImage: session.state == .running
                                ? "play.circle"
                                : "pause.circle"
                        )
                        .font(.caption)

                        Spacer()

                        switch session.state {
                        case .running:
                            Button("Suspend") {
                                onSuspend(session)
                            }
                            .accessibilityHint(
                                "Asks Rust to suspend session \(session.id)"
                            )
                        case .suspended:
                            Button("Resume") {
                                onResume(session)
                            }
                            .accessibilityHint(
                                "Asks Rust to resume session \(session.id)"
                            )
                        }
                    }
                    .disabled(!commandsAvailable)
                }
            }
        }
    }

    private var assignedWorkspaces: [WorkbenchLibraryWorkspace] {
        let assignedIDs = Set(build.assignedWorkspaceIDs)
        return workspaces.filter { assignedIDs.contains($0.id) }
    }

    private var unassignedWorkspaces: [WorkbenchLibraryWorkspace] {
        let assignedIDs = Set(build.assignedWorkspaceIDs)
        return workspaces.filter { !assignedIDs.contains($0.id) }
    }

    private var availabilitySymbol: String {
        switch build.availability {
        case .metadataOnly:
            "doc.text.magnifyingglass"
        case .sealedExactBytesReady:
            "checkmark.seal"
        }
    }

    private var availabilityColor: Color {
        switch build.availability {
        case .metadataOnly:
            .orange
        case .sealedExactBytesReady:
            .green
        }
    }
}

struct LibraryStatusBanner: View {
    let title: String
    let message: String
    let symbol: String
    let color: Color
    let accessibilityIdentifier: String

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: symbol)
                .foregroundStyle(color)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 3) {
                Text(title)
                    .font(.headline)
                Text(message)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
        }
        .padding()
        .background(color.opacity(0.08))
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(title). \(message)")
        .accessibilityIdentifier(accessibilityIdentifier)
    }
}
