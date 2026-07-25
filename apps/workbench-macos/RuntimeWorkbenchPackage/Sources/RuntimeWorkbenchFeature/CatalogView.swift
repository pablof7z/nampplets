import SwiftUI

public struct CatalogSheet: View {
    @State private var model: CatalogViewModel
    @Environment(\.dismiss) private var dismiss
    @FocusState private var focus: FocusTarget?

    private enum FocusTarget {
        case search
        case coordinate
    }

    @MainActor
    public init(
        client: any CatalogClient,
        onInstalled: @escaping @MainActor (CatalogInstalledBuild) -> Void = {
            _ in
        }
    ) {
        _model = State(
            initialValue: CatalogViewModel(
                client: client,
                onInstalled: onInstalled
            )
        )
    }

    public var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                searchControls
                Divider()
                results
            }
            .navigationTitle("Napplet Catalog")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") {
                        model.cancelReview()
                        dismiss()
                    }
                    .keyboardShortcut(.cancelAction)
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 720, idealWidth: 860, minHeight: 540, idealHeight: 680)
        #endif
        .sheet(
            item: Binding(
                get: { model.review },
                set: { review in
                    if review == nil {
                        model.cancelReview()
                    }
                }
            )
        ) { review in
            CatalogInstallReviewSheet(
                review: review,
                isInstalling: model.isInstalling,
                issue: model.issue,
                onCancel: model.cancelReview,
                onConfirm: {
                    Task {
                        if await model.confirmInstall() != nil {
                            dismiss()
                        }
                    }
                }
            )
        }
        .onAppear {
            focus = .search
        }
        .task {
            await model.start()
        }
        .onDisappear {
            model.stop()
        }
    }

    private var searchControls: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                TextField("Filter the current catalog window", text: $model.query)
                    .textFieldStyle(.roundedBorder)
                    .focused($focus, equals: .search)
                    .onSubmit {
                        Task {
                            await model.search()
                        }
                    }
                    .accessibilityLabel("Search napplet catalog")
                    .accessibilityHint(
                        "Filters the current bounded NMP window locally"
                    )

                Button("Search", systemImage: "magnifyingglass") {
                    Task {
                        await model.search()
                    }
                }
                .keyboardShortcut(.return, modifiers: [.command])
            }

            Text(
                "The pinned NMP facade does not expose NIP-50 full-text search. "
                    + "Live queries filter the current finite window locally."
            )
            .font(.caption)
            .foregroundStyle(.secondary)

            HStack {
                TextField(
                    "Manual manifest coordinate",
                    text: $model.manualCoordinate
                )
                .textFieldStyle(.roundedBorder)
                .focused($focus, equals: .coordinate)
                .onSubmit {
                    Task {
                        await model.reviewManualCoordinate()
                    }
                }
                .accessibilityLabel("Manual napplet coordinate")
                .accessibilityHint(
                    "Resolves the coordinate before showing an install review"
                )

                Button("Review Coordinate", systemImage: "doc.text.magnifyingglass") {
                    Task {
                        await model.reviewManualCoordinate()
                    }
                }
                .keyboardShortcut("i", modifiers: [.command])
                .disabled(model.isResolvingReview)
            }

            if model.isResolvingReview {
                ProgressView(
                    "Resolving verified build"
                )
                .controlSize(.small)
                .accessibilityLabel(
                    "Resolving napplet coordinate"
                )
            }

            if let issue = model.issue, model.review == nil {
                CatalogIssueView(issue: issue)
            }
        }
        .padding()
    }

    @ViewBuilder
    private var results: some View {
        VStack(spacing: 0) {
            if let evidence = model.evidence ?? model.connectingEvidence {
                CatalogBrowseEvidenceView(
                    evidence: evidence,
                    hasMore: model.hasMore
                )
                Divider()
            }

            resultRows
        }
    }

    @ViewBuilder
    private var resultRows: some View {
        if model.entries.isEmpty,
           model.evidence?.window == .requesting
        {
            ContentUnavailableView(
                "Connecting to the live catalog",
                systemImage: "antenna.radiowaves.left.and.right",
                description: Text(
                    "The permanent NMP subscription is waiting for its next bounded replacement."
                )
            )
        } else if model.entries.isEmpty {
            ContentUnavailableView(
                "No napplets in this feed",
                systemImage: "square.grid.2x2",
                description: Text(
                    model.evidence == nil
                        ? "The live catalog is unavailable for this profile."
                        : "The current bounded live replacement has no matching napplets."
                )
            )
        } else {
            List(model.entries) { entry in
                Button {
                    Task {
                        await model.review(entry: entry)
                    }
                } label: {
                    CatalogEntryRow(entry: entry)
                }
                .buttonStyle(.plain)
                .disabled(model.isResolvingReview)
                .accessibilityIdentifier("catalog-entry")
                .accessibilityElement(children: .combine)
                .accessibilityLabel(
                    "\(entry.title), by \(entry.publisher.visibleName), "
                        + "\(entry.compatibility.title)"
                )
                .accessibilityHint("Opens the verified install review")
            }
        }
    }
}
