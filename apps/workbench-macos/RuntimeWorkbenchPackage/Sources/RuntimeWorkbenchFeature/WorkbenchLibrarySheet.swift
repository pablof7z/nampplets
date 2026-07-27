import SwiftUI

/// The napplets a person has added.
///
/// This is their shelf, not a build inventory. Exact identity, session numbers
/// and runtime revisions are real and are kept -- one deliberate move down, on
/// each row. See `docs/adr/0008-verdicts-on-the-path.md`.
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
                if let snapshot = model.snapshot {
                    if snapshot.totalInstalled > 0 || !snapshot.filterQuery.isEmpty {
                        filterBar
                        Divider()
                    }

                    banners(snapshot)
                    library(snapshot)
                } else {
                    ProgressView("Loading your napplets…")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            // The place title is set on the page itself, where it can be a
            // name rather than window chrome.
            .navigationTitle("")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") {
                        dismiss()
                    }
                    .keyboardShortcut(.cancelAction)
                }

                ToolbarItem {
                    Button("Refresh", systemImage: "arrow.clockwise") {
                        model.refresh()
                    }
                    .accessibilityHint("Checks for changes")
                }
            }
        }
        .background(NappletInk.paper)
        #if os(macOS)
        .frame(minWidth: 640, idealWidth: 760, minHeight: 520, idealHeight: 680)
        #endif
        .onAppear {
            model.start()
            filterFocused = true
        }
        .onDisappear {
            model.stop()
        }
        .confirmationDialog(
            uninstallCandidate.map { "Remove \($0.title)?" } ?? "Remove napplet?",
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
            Button("Remove", role: .destructive) {
                model.uninstall(build.exactBuild)
                uninstallCandidate = nil
            }
            Button("Cancel", role: .cancel) {
                uninstallCandidate = nil
            }
        } message: { build in
            Text(WorkbenchLibraryRemovalPresentation.message(for: build.title))
        }
    }

    @ViewBuilder
    private func banners(_ snapshot: WorkbenchLibrarySnapshot) -> some View {
        if let reason = snapshot.availability.unavailableReason {
            unavailableBanner(reason)
            Divider()
        }
        if let gap = model.updateGap {
            updateGapBanner(gap)
            Divider()
        }
        if let refusal = snapshot.refusals.last {
            refusalBanner(
                refusal,
                retainedCount: snapshot.refusals.count,
                droppedCount: snapshot.droppedRefusalCount
            )
            Divider()
        }
    }

    private var filterBar: some View {
        HStack(spacing: NappletMetrics.tight) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            TextField("Filter", text: $model.filterDraft)
                .textFieldStyle(.plain)
                .focused($filterFocused)
                .onSubmit {
                    model.applyFilter()
                }
                .disabled(!model.commandsAvailable)
                .accessibilityLabel("Filter your napplets")

            if model.snapshot?.filterQuery.isEmpty == false {
                Button {
                    model.clearFilter()
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .disabled(!model.commandsAvailable)
                .accessibilityLabel("Clear filter")
            }
        }
        .padding(NappletMetrics.tight)
        .background(
            .quaternary.opacity(0.4),
            in: RoundedRectangle(cornerRadius: NappletMetrics.tight)
        )
        .padding(NappletMetrics.comfortable)
    }

    @ViewBuilder
    private func library(_ snapshot: WorkbenchLibrarySnapshot) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                Text("Your Napplets")
                    .font(NappletType.place)
                    .nappletDisplayFace()
                    .foregroundStyle(NappletInk.ink)

                if snapshot.builds.isEmpty {
                    Text(
                        snapshot.filterQuery.isEmpty
                            ? "Nothing here yet."
                            : "Nothing matches that."
                    )
                    .font(NappletType.title)
                    .foregroundStyle(NappletInk.ink)
                    .padding(.top, NappletMetrics.generous)

                    Text(
                        snapshot.filterQuery.isEmpty
                            ? "Napplets you add will live here."
                            : "Try a shorter word."
                    )
                    .font(NappletType.body)
                    .foregroundStyle(NappletInk.inkSecondary)
                    .padding(.top, NappletMetrics.tight)
                } else {
                    VStack(alignment: .leading, spacing: NappletMetrics.snug) {
                        ForEach(snapshot.builds) { build in
                            WorkbenchLibraryBuildRow(
                                build: build,
                                workspaces: snapshot.workspaces,
                                commandsAvailable: model.commandsAvailable,
                                onOpen: { onOpen(build) },
                                onSuspend: model.suspend,
                                onResume: model.resume,
                                onAssign: { workspace in
                                    model.assign(
                                        build.exactBuild,
                                        to: workspace
                                    )
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
                    }
                    .padding(.top, NappletMetrics.spacious)

                    if snapshot.builds.count < Int(snapshot.totalInstalled) {
                        Text(
                            "Showing \(snapshot.builds.count) of "
                                + "\(snapshot.totalInstalled)"
                        )
                        .font(NappletType.caption)
                        .foregroundStyle(NappletInk.inkSecondary)
                        .padding(.top, NappletMetrics.comfortable)
                    }
                }
            }
            .frame(maxWidth: NappletMetrics.measure, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, NappletMetrics.generous)
            .padding(.top, NappletMetrics.roomy)
            .padding(.bottom, NappletMetrics.spacious)
        }
    }
}

struct LibraryStatusBanner: View {
    let title: String
    let message: String
    let symbol: String
    let color: Color
    let accessibilityIdentifier: String
    let evidenceFields: [NappletField]

    var body: some View {
        HStack(alignment: .top, spacing: NappletMetrics.snug) {
            Image(systemName: symbol)
                .foregroundStyle(color)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                Text(title)
                    .font(.headline)
                    .accessibilityIdentifier(accessibilityIdentifier)
                Text(message)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                if !evidenceFields.isEmpty {
                    NappletEvidence(label: "Details") {
                        NappletFieldGrid(fields: evidenceFields)
                    }
                    .font(NappletType.caption)
                }
            }
            Spacer()
        }
        .padding(NappletMetrics.comfortable)
        .background(color.opacity(0.08))
        .accessibilityElement(children: .contain)
    }
}
