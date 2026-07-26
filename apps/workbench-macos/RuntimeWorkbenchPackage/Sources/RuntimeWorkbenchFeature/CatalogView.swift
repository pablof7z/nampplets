import SwiftUI

/// The front door: where a person finds something to run.
///
/// It is a place to browse, not a report on the state of a subscription. What
/// the runtime observed, from which sources, and where the window stopped is
/// real and is kept -- one deliberate move away, in `CatalogBrowseEvidenceView`.
/// See `docs/adr/0008-verdicts-on-the-path.md`.
public struct CatalogSheet: View {
    @State private var model: CatalogViewModel
    @State private var isShowingAddress = false
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
                searchBar
                Divider()
                results
            }
            .navigationTitle("Add a Napplet")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") {
                        model.cancelReview()
                        dismiss()
                    }
                    .keyboardShortcut(.cancelAction)
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 640, idealWidth: 760, minHeight: 520, idealHeight: 660)
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

    private var searchBar: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.snug) {
            HStack(spacing: NappletMetrics.tight) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
                TextField("Search napplets", text: $model.query)
                    .textFieldStyle(.plain)
                    .focused($focus, equals: .search)
                    .onSubmit {
                        Task { await model.search() }
                    }
                    .accessibilityLabel("Search napplets")
                if !model.query.isEmpty {
                    Button {
                        model.query = ""
                        Task { await model.search() }
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Clear search")
                }
            }
            .padding(NappletMetrics.tight)
            .background(
                .quaternary.opacity(0.4),
                in: RoundedRectangle(cornerRadius: NappletMetrics.tight)
            )

            if model.isResolvingReview {
                ProgressView("Checking this napplet…")
                    .controlSize(.small)
            }

            if let issue = model.issue, model.review == nil {
                NappletNotice(
                    verdict: .caution("\(issue.title). \(issue.message)")
                )
            }

            addressField
        }
        .padding(NappletMetrics.comfortable)
    }

    /// Adding by address is a real need and a rare one. It stays available
    /// without being the second thing a newcomer meets.
    @ViewBuilder
    private var addressField: some View {
        DisclosureGroup(isExpanded: $isShowingAddress) {
            VStack(alignment: .leading, spacing: NappletMetrics.tight) {
                HStack {
                    TextField(
                        "Paste a napplet address",
                        text: $model.manualCoordinate
                    )
                    .textFieldStyle(.roundedBorder)
                    .fontDesign(.monospaced)
                    .focused($focus, equals: .coordinate)
                    .onSubmit {
                        Task { await model.reviewManualCoordinate() }
                    }
                    .accessibilityLabel("Napplet address")

                    Button("Find") {
                        Task { await model.reviewManualCoordinate() }
                    }
                    .keyboardShortcut("i", modifiers: [.command])
                    .disabled(
                        model.isResolvingReview
                            || model.manualCoordinate.isEmpty
                    )
                }
                Text(
                    "If someone sent you a napplet's address directly, paste it here."
                )
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            .padding(.top, NappletMetrics.tight)
        } label: {
            Text("Have an address?")
                .font(.callout)
        }
        .accessibilityIdentifier("catalog-manual-address")
    }

    @ViewBuilder
    private var results: some View {
        if model.entries.isEmpty, model.evidence?.window == .requesting {
            ContentUnavailableView {
                Label(
                    "Looking for napplets",
                    systemImage: "antenna.radiowaves.left.and.right"
                )
            } description: {
                Text("This takes a moment the first time.")
            }
        } else if model.entries.isEmpty {
            ContentUnavailableView {
                Label(
                    model.query.isEmpty ? "Nothing here yet" : "No matches",
                    systemImage: "square.grid.2x2"
                )
            } description: {
                Text(emptyDescription)
            }
        } else {
            List(model.entries) { entry in
                Button {
                    Task { await model.review(entry: entry) }
                } label: {
                    CatalogEntryRow(entry: entry)
                }
                .buttonStyle(.plain)
                .disabled(model.isResolvingReview)
                .accessibilityIdentifier("catalog-entry")
                .accessibilityElement(children: .combine)
                .accessibilityLabel(accessibilityLabel(for: entry))
                .accessibilityHint(
                    "Shows what this napplet does before you add it"
                )
            }
            .listStyle(.inset)
            .safeAreaInset(edge: .bottom, spacing: 0) {
                if let evidence = model.evidence ?? model.connectingEvidence {
                    CatalogBrowseEvidenceView(
                        evidence: evidence,
                        hasMore: model.hasMore
                    )
                }
            }
        }
    }

    private var emptyDescription: String {
        if !model.query.isEmpty {
            return "Nothing matched “\(model.query)”. Try a shorter word."
        }
        return model.evidence == nil
            ? "Napplets couldn't reach the network. Check your connection and try again."
            : "No napplets have turned up yet. Try again in a moment."
    }

    private func accessibilityLabel(for entry: CatalogEntry) -> String {
        let publisher = NappletIdentityPresentation.publisherName(
            displayName: entry.publisher.displayName,
            publicKey: entry.publisher.publicKey
        )
        return "\(entry.title), from \(publisher). \(entry.summary)"
    }
}
