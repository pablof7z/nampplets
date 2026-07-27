import SwiftUI

/// What a napplet has actually been doing.
///
/// Runtime refusals remain visible without replacing the last accepted
/// snapshot. Build identity stays one deliberate move away as evidence.
public struct ActivityDrawer: View {
    @State private var model: ActivityViewModel
    @Environment(\.dismiss) private var dismiss
    private let nappletTitle: String?

    @MainActor
    public init(
        source: any ActivitySource,
        scope: ActivityExactBuildScope,
        nappletTitle: String? = nil,
        developerModeAvailable: Bool = false
    ) {
        _model = State(
            initialValue: ActivityViewModel(
                source: source,
                scope: scope,
                developerModeAvailable: developerModeAvailable
            )
        )
        self.nappletTitle = nappletTitle
    }

    public var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                header
                Divider()
                inventory
                Divider()

                if let gap = model.updateGap {
                    updateGapBanner(gap)
                    Divider()
                }

                if let refusal = model.refreshRefusal {
                    ActivityRefreshRefusalBanner(refusal: refusal)
                    Divider()
                }

                if let discarded = model.snapshot?.runtimeDiscardedCount,
                   discarded > 0
                {
                    runtimeDiscardedBanner(discarded)
                    Divider()
                }

                facts
            }
            // Deliberately no container identifier. SwiftUI propagates one
            // through the subtree and can mask the evidence disclosure.
            .navigationTitle(title)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") {
                        dismiss()
                    }
                    .keyboardShortcut(.cancelAction)
                }

                ToolbarItemGroup {
                    filters

                    Button("Refresh", systemImage: "arrow.clockwise") {
                        model.refresh()
                    }
                    .accessibilityHint("Checks for the latest activity")

                    if model.developerModeAvailable {
                        Toggle(
                            "Developer Detail",
                            systemImage: "ladybug",
                            isOn: Binding(
                                get: { model.developerModeEnabled },
                                set: { isEnabled in
                                    model.setDeveloperModeEnabled(isEnabled)
                                }
                            )
                        )
                        .toggleStyle(.button)
                        .accessibilityHint("Shows extra fields on each entry")
                    }
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 640, idealWidth: 760, minHeight: 480, idealHeight: 640)
        #endif
        .onAppear {
            model.start()
        }
        .onDisappear {
            model.stop()
        }
    }

    private var title: String {
        nappletTitle.map { "\($0) Activity" } ?? "Recent Activity"
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.tight) {
            Text(ActivityPlainPresentation.header)
            .font(NappletType.secondary)
            .foregroundStyle(NappletInk.inkSecondary)
            .fixedSize(horizontal: false, vertical: true)

            NappletEvidence(label: "Which build this is") {
                NappletFieldGrid(fields: [
                    NappletField("Publisher key", model.scope.manifestAuthor),
                    NappletField("dTag", model.scope.dTag),
                    NappletField("Aggregate hash", model.scope.aggregateHash),
                ])
            }
            .font(NappletType.caption)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(NappletMetrics.comfortable)
    }

    @ViewBuilder
    private var inventory: some View {
        let inventory = model.snapshot?.inventory ?? .empty
        let presentation = ActivityInventoryPresentation(inventory: inventory)
        HStack(spacing: NappletMetrics.snug) {
            ActivityInventoryCell(
                title: "Open now",
                value: presentation.openNow,
                symbol: "play.rectangle"
            )
            Text(presentation.unavailableCountsMessage)
                .font(NappletType.caption)
                .foregroundStyle(NappletInk.inkSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(NappletMetrics.comfortable)
        .accessibilityElement(children: .contain)
    }

    private func updateGapBanner(_ gap: ActivityUpdateGap) -> some View {
        HStack(alignment: .top, spacing: NappletMetrics.snug) {
            Image(systemName: "exclamationmark.arrow.triangle.2.circlepath")
                .foregroundStyle(NappletInk.caution)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                Text("Some entries may be missing")
                    .font(.headline)
                    .accessibilityIdentifier("runtime-activity-update-gap")
                Text(ActivityPlainPresentation.updateGap)
                    .font(NappletType.caption)
                    .foregroundStyle(NappletInk.inkSecondary)
                NappletEvidence(label: "Why") {
                    NappletFieldGrid(fields: [
                        NappletField(
                            "Expected predecessor revision",
                            "\(gap.expectedPredecessorRevision)"
                        ),
                        NappletField(
                            "Received predecessor revision",
                            "\(gap.receivedPredecessorRevision)"
                        ),
                        NappletField(
                            "Received revision",
                            "\(gap.receivedRevision)"
                        ),
                        // Only stated when the runtime actually reported a
                        // loss. Printing "0 events" for a plain revision
                        // discontinuity would assert something the runtime
                        // never said.
                        gap.lostBeforeBatch > 0
                            ? NappletField(
                                "Events lost before this batch",
                                "\(gap.lostBeforeBatch)"
                            )
                            : nil,
                    ].compactMap { $0 })
                }
                .font(NappletType.caption)
            }
            Spacer()
            Button("Refresh") {
                model.refresh()
            }
        }
        .padding(NappletMetrics.comfortable)
        .background(NappletInk.ground(for: .caution("")))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(
            "Some entries may be missing. \(ActivityPlainPresentation.updateGap)"
        )
    }

    /// Distinct from `updateGapBanner`, which reports that *this observer*
    /// missed a frame. This reports that the runtime itself discarded entries
    /// to stay inside its bound, so they no longer exist to be fetched — a
    /// refresh cannot bring them back, and there is deliberately no Refresh
    /// button here.
    private func runtimeDiscardedBanner(_ discarded: UInt64) -> some View {
        HStack(alignment: .top, spacing: NappletMetrics.snug) {
            Image(systemName: "trash.slash")
                .foregroundStyle(NappletInk.caution)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                Text("\(discarded) older entries were discarded")
                    .font(.headline)
                    .accessibilityIdentifier("runtime-activity-discarded")
                Text(ActivityPlainPresentation.runtimeDiscarded)
                    .font(NappletType.caption)
                    .foregroundStyle(NappletInk.inkSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
        }
        .padding(NappletMetrics.comfortable)
        .background(NappletInk.ground(for: .caution("")))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(
            "\(discarded) older entries were discarded. "
                + ActivityPlainPresentation.runtimeDiscarded
        )
    }

    @ViewBuilder
    private var facts: some View {
        if model.snapshot == nil {
            ProgressView("Loading…")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityLabel("Loading activity")
        } else if
            model.visibleFacts.isEmpty,
            let omitted = model.snapshot?.omittedFactCount,
            omitted > 0
        {
            ContentUnavailableView(
                "Nothing to show here",
                systemImage: "ellipsis.rectangle",
                description: Text(
                    "\(omitted) entries exist but this version of the app "
                        + "can't display them yet."
                )
            )
        } else if model.visibleFacts.isEmpty {
            ContentUnavailableView(
                "Nothing matches",
                systemImage: "line.3.horizontal.decrease.circle",
                description: Text("Change the filters to see more.")
            )
        } else {
            List(model.visibleFacts) { fact in
                ActivityFactRow(
                    fact: fact,
                    detailFields: model.detailFields(for: fact)
                )
            }
            .listStyle(.inset)
            .safeAreaInset(edge: .bottom, spacing: 0) {
                if let omitted = model.snapshot?.omittedFactCount, omitted > 0 {
                    VStack(spacing: 0) {
                        Divider()
                        Text("\(omitted) more entries can't be shown yet")
                            .font(NappletType.caption)
                            .foregroundStyle(NappletInk.inkSecondary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, NappletMetrics.comfortable)
                            .padding(.vertical, NappletMetrics.tight)
                    }
                    .background(.bar)
                }
            }
        }
    }

    private var filters: some View {
        Menu {
            Section("Importance") {
                ForEach(ActivitySeverity.allCases, id: \.self) { severity in
                    Toggle(
                        severity.title,
                        isOn: Binding(
                            get: {
                                model.severityFilter.contains(severity)
                            },
                            set: {
                                model.setSeverity(severity, isIncluded: $0)
                            }
                        )
                    )
                }
            }

            Section("Kind") {
                ForEach(ActivityCategory.allCases, id: \.self) { category in
                    Toggle(
                        category.title,
                        isOn: Binding(
                            get: {
                                model.categoryFilter.contains(category)
                            },
                            set: {
                                model.setCategory(category, isIncluded: $0)
                            }
                        )
                    )
                }
            }
        } label: {
            Label("Filter", systemImage: "line.3.horizontal.decrease.circle")
        }
        .accessibilityHint("Filters what's listed below")
    }
}
