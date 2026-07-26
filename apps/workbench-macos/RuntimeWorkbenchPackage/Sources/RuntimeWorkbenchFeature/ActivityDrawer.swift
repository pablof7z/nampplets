import SwiftUI

public struct ActivityDrawer: View {
    @State private var model: ActivityViewModel
    @Environment(\.dismiss) private var dismiss

    @MainActor
    public init(
        source: any ActivitySource,
        scope: ActivityExactBuildScope,
        developerModeAvailable: Bool = false
    ) {
        _model = State(
            initialValue: ActivityViewModel(
                source: source,
                scope: scope,
                developerModeAvailable: developerModeAvailable
            )
        )
    }

    public var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                exactBuildHeader
                Divider()
                inventory
                Divider()

                if let gap = model.updateGap {
                    updateGapBanner(gap)
                    Divider()
                }

                facts
            }
            .navigationTitle("Recent Activity")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") {
                        dismiss()
                    }
                    .keyboardShortcut(.cancelAction)
                }

                ToolbarItemGroup {
                    filters

                    Button("Refresh", systemImage: "arrow.clockwise") {
                        model.refresh()
                    }
                    .accessibilityHint(
                        "Requests one fresh activity snapshot for this exact build"
                    )

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
                        .accessibilityHint(
                            "Shows bounded redacted detail fields"
                        )
                    }
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 700, idealWidth: 820, minHeight: 500, idealHeight: 680)
        #endif
        .onAppear {
            model.start()
        }
        .onDisappear {
            model.stop()
        }
        .accessibilityIdentifier("runtime-activity-drawer")
    }

    private var exactBuildHeader: some View {
        VStack(alignment: .leading, spacing: 5) {
            Label("Exact verified build", systemImage: "checkmark.seal")
                .font(.headline)
            LabeledContent("Publisher", value: model.scope.manifestAuthor)
            LabeledContent("d-tag", value: model.scope.dTag)
            LabeledContent("Aggregate hash", value: model.scope.aggregateHash)
        }
        .font(.caption)
        .textSelection(.enabled)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding()
        .accessibilityElement(children: .combine)
        .accessibilityLabel(
            "Activity for exact build \(model.scope.dTag), "
                + "aggregate hash \(model.scope.aggregateHash)"
        )
    }

    @ViewBuilder
    private var inventory: some View {
        let inventory = model.snapshot?.inventory ?? .empty
        HStack(spacing: 12) {
            inventoryCell(
                title: "Sessions",
                value: inventory.activeSessions,
                symbol: "rectangle.connected.to.line.below"
            )
            inventoryCell(
                title: "Bindings",
                value: inventory.activeBindings,
                symbol: "link"
            )
            inventoryCell(
                title: "Resources",
                value: inventory.activeResources,
                symbol: "shippingbox"
            )
            inventoryCell(
                title: "Pending receipts",
                value: inventory.pendingReceipts,
                symbol: "clock.badge.exclamationmark"
            )
        }
        .padding()
        .accessibilityElement(children: .contain)
    }

    private func inventoryCell(
        title: String,
        value: Int,
        symbol: String
    ) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Label(title, systemImage: symbol)
                .font(.caption)
                .foregroundStyle(.secondary)
            Text(value, format: .number)
                .font(.title2.monospacedDigit().weight(.semibold))
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(10)
        .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 8))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(title): \(value)")
    }

    private func updateGapBanner(_ gap: ActivityUpdateGap) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "exclamationmark.arrow.triangle.2.circlepath")
                .foregroundStyle(.orange)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 3) {
                Text("Activity may be incomplete")
                    .font(.headline)
                Text(
                    "Expected update after revision "
                        + "\(gap.expectedPredecessorRevision), received "
                        + "\(gap.receivedPredecessorRevision). Refresh to "
                        + "replace it with an authoritative snapshot."
                )
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            Spacer()
            Button("Refresh") {
                model.refresh()
            }
        }
        .padding()
        .background(.orange.opacity(0.08))
        .accessibilityElement(children: .combine)
        .accessibilityLabel(
            "Activity may be incomplete. Expected predecessor revision "
                + "\(gap.expectedPredecessorRevision), received "
                + "\(gap.receivedPredecessorRevision)."
        )
        .accessibilityHint("Activate Refresh to request a complete snapshot")
        .accessibilityIdentifier("runtime-activity-update-gap")
    }

    @ViewBuilder
    private var facts: some View {
        if model.snapshot == nil {
            ProgressView("Waiting for runtime activity…")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityLabel("Waiting for the first activity snapshot")
        } else if
            model.visibleFacts.isEmpty,
            let omitted = model.snapshot?.omittedFactCount,
            omitted > 0
        {
            ContentUnavailableView(
                "Activity detail unavailable",
                systemImage: "ellipsis.rectangle",
                description: Text(
                    "\(omitted) scoped runtime facts are not present in this "
                        + "typed native projection yet."
                )
            )
        } else if model.visibleFacts.isEmpty {
            ContentUnavailableView(
                "No matching activity",
                systemImage: "line.3.horizontal.decrease.circle",
                description: Text(
                    "Change the severity or category filters to show more facts."
                )
            )
        } else {
            List(model.visibleFacts) { fact in
                ActivityFactRow(
                    fact: fact,
                    detailFields: model.detailFields(for: fact)
                )
            }
            .overlay(alignment: .bottomTrailing) {
                if let omitted = model.snapshot?.omittedFactCount, omitted > 0 {
                    Text("\(omitted) facts omitted by the runtime projection")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .padding(8)
                        .background(.bar, in: Capsule())
                        .padding()
                        .accessibilityLabel(
                            "\(omitted) activity facts omitted by the runtime projection"
                        )
                }
            }
        }
    }

    private var filters: some View {
        Menu {
            Section("Severity") {
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

            Section("Category") {
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
        .accessibilityHint("Filters activity by severity and category")
    }
}

private struct ActivityFactRow: View {
    let fact: ActivityFact
    let detailFields: [ActivityDetailField]

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: symbol)
                .foregroundStyle(color)
                .frame(width: 22)
                .accessibilityHidden(true)

            VStack(alignment: .leading, spacing: 5) {
                HStack {
                    Text(fact.title)
                        .font(.headline)
                    Spacer()
                    Text(fact.kind.title)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Text(fact.summary)
                    .foregroundStyle(.secondary)

                if let evidence = fact.evidenceSummary {
                    LabeledContent("Evidence", value: evidence)
                        .font(.caption)
                        .textSelection(.enabled)
                }

                if !detailFields.isEmpty {
                    DisclosureGroup("Developer detail") {
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(detailFields) { field in
                                LabeledContent(
                                    field.key,
                                    value: field.displayValue
                                )
                                    .font(.caption.monospaced())
                                    .textSelection(.enabled)
                            }
                        }
                        .padding(.top, 4)
                    }
                    .font(.caption)
                }
            }
        }
        .padding(.vertical, 5)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(
            "\(fact.severity.title), \(fact.kind.title), "
                + "\(fact.title). \(fact.summary)"
        )
    }

    private var symbol: String {
        switch fact.kind {
        case .providerCall: "arrow.left.arrow.right"
        case .providerRefusal: "hand.raised"
        case .activeSession: "rectangle.connected.to.line.below"
        case .activeBinding: "link"
        case .activeResource: "shippingbox"
        case .pendingReceipt: "clock.badge.exclamationmark"
        case .crash: "bolt.trianglebadge.exclamationmark"
        case .recovery: "cross.case"
        }
    }

    private var color: Color {
        switch fact.severity {
        case .debug: .secondary
        case .information: .blue
        case .warning: .orange
        case .error: .red
        }
    }
}
