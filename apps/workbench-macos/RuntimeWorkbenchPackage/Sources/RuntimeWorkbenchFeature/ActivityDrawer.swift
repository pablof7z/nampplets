import SwiftUI

/// What a napplet has actually been doing.
///
/// This is the destination that makes the rest of the app able to stay quiet:
/// it is where evidence lives, so its content is deliberately complete. What
/// changed is the frame around it. A person arrives here asking "what has this
/// thing been doing?", so it opens with that answer rather than with the build
/// identity, and the identity moves to where evidence belongs.
/// See `docs/adr/0008-verdicts-on-the-path.md`.
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

                facts
            }
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
        .accessibilityIdentifier("runtime-activity-drawer")
    }

    private var title: String {
        // Keeps main's plain fallback and adds the napplet's name when the
        // caller knows it: "Good Morning Activity" answers "whose?" without
        // the reader having to check which window they came from.
        nappletTitle.map { "\($0) Activity" } ?? "Recent Activity"
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.tight) {
            Text(
                "Everything this napplet has asked for, and everything it was "
                    + "refused."
            )
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
            .font(.caption)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(NappletMetrics.comfortable)
    }

    @ViewBuilder
    private var inventory: some View {
        let inventory = model.snapshot?.inventory ?? .empty
        HStack(spacing: NappletMetrics.snug) {
            inventoryCell(
                title: "Open now",
                value: inventory.activeSessions,
                symbol: "play.rectangle"
            )
            inventoryCell(
                title: "Connections",
                value: inventory.activeBindings,
                symbol: "link"
            )
            inventoryCell(
                title: "Files loaded",
                value: inventory.activeResources,
                symbol: "shippingbox"
            )
            inventoryCell(
                title: "Waiting to send",
                value: inventory.pendingReceipts,
                symbol: "clock"
            )
        }
        .padding(NappletMetrics.comfortable)
        .accessibilityElement(children: .contain)
    }

    private func inventoryCell(
        title: String,
        value: Int,
        symbol: String
    ) -> some View {
        VStack(alignment: .leading, spacing: NappletMetrics.hairline + 2) {
            Label(title, systemImage: symbol)
                .font(NappletType.caption)
                .foregroundStyle(NappletInk.inkSecondary)
            Text(value, format: .number)
                .font(NappletType.title.monospacedDigit())
                .foregroundStyle(NappletInk.ink)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(NappletMetrics.snug)
        .background(
            NappletInk.fillQuiet,
            in: RoundedRectangle(
                cornerRadius: NappletMetrics.tight,
                style: .continuous
            )
        )
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(title): \(value)")
    }

    private func updateGapBanner(_ gap: ActivityUpdateGap) -> some View {
        HStack(alignment: .top, spacing: NappletMetrics.snug) {
            Image(systemName: "exclamationmark.arrow.triangle.2.circlepath")
                .foregroundStyle(NappletInk.caution)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                Text("Some entries may be missing")
                    .font(.headline)
                Text("Refresh to get a complete picture.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
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
                    ])
                }
                .font(.caption)
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
            "Some entries may be missing. Refresh to get a complete picture."
        )
        .accessibilityIdentifier("runtime-activity-update-gap")
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
                            .font(.caption)
                            .foregroundStyle(.secondary)
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
                            get: { model.severityFilter.contains(severity) },
                            set: { model.setSeverity(severity, isIncluded: $0) }
                        )
                    )
                }
            }

            Section("Kind") {
                ForEach(ActivityCategory.allCases, id: \.self) { category in
                    Toggle(
                        category.title,
                        isOn: Binding(
                            get: { model.categoryFilter.contains(category) },
                            set: { model.setCategory(category, isIncluded: $0) }
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

private struct ActivityFactRow: View {
    let fact: ActivityFact
    let detailFields: [ActivityDetailField]

    var body: some View {
        HStack(alignment: .top, spacing: NappletMetrics.snug) {
            Image(systemName: symbol)
                .foregroundStyle(tint)
                .frame(width: 22)
                .accessibilityHidden(true)

            VStack(alignment: .leading, spacing: NappletMetrics.hairline + 1) {
                HStack(alignment: .firstTextBaseline) {
                    Text(fact.title)
                        .font(NappletType.heading)
                        .foregroundStyle(NappletInk.ink)
                    Spacer()
                    Text(fact.kind.title)
                        .font(NappletType.caption)
                        .foregroundStyle(NappletInk.inkSecondary)
                }

                Text(fact.summary)
                    .font(NappletType.secondary)
                    .foregroundStyle(NappletInk.inkSecondary)
                    .fixedSize(horizontal: false, vertical: true)

                if fact.evidenceSummary != nil || !detailFields.isEmpty {
                    NappletEvidence(label: "Details") {
                        NappletFieldGrid(fields: evidenceFields)
                    }
                    .font(.caption)
                }
            }
        }
        .padding(.vertical, NappletMetrics.hairline + 1)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(
            "\(fact.severity.title), \(fact.kind.title), "
                + "\(fact.title). \(fact.summary)"
        )
    }

    private var evidenceFields: [NappletField] {
        var fields: [NappletField] = []
        if let evidence = fact.evidenceSummary {
            fields.append(NappletField("Evidence", evidence))
        }
        fields.append(contentsOf: detailFields.map { field in
            NappletField(field.key, field.displayValue)
        })
        return fields
    }

    private var symbol: String {
        switch fact.kind {
        case .providerCall: "arrow.left.arrow.right"
        case .providerRefusal: "hand.raised"
        case .activeSession: "play.rectangle"
        case .activeBinding: "link"
        case .activeResource: "shippingbox"
        case .pendingReceipt: "clock"
        case .crash: "bolt.trianglebadge.exclamationmark"
        case .recovery: "cross.case"
        }
    }

    /// Colour reinforces the severity word already printed on the row; it is
    /// never the only thing carrying it.
    private var tint: Color {
        switch fact.severity {
        case .debug, .information: NappletInk.inkSecondary
        case .warning: NappletInk.caution
        case .error: NappletInk.refusal
        }
    }
}
