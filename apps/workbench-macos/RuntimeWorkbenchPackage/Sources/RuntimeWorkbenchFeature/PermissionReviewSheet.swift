import SwiftUI

/// The single consent moment, shown when a napplet is first opened.
///
/// Install grants nothing; this is where capability is actually granted, so
/// this is where the weight belongs. It states what the napplet will be able
/// to do in the user's own language and offers one gesture for the common
/// outcome. Per-capability scope is one disclosure away for anyone who wants
/// it, and every exact value lives one further step down.
///
/// See `docs/adr/0008-verdicts-on-the-path.md`.
public struct PermissionReviewSheet: View {
    @Environment(\.dismiss) private var dismiss
    @State var model: PermissionReviewSheetModel
    @State private var isChoosingIndividually: Bool

    @MainActor
    public init(manager: any PermissionReviewManaging) {
        _model = State(
            initialValue: PermissionReviewSheetModel(manager: manager)
        )
        // UI automation drives the per-capability controls directly, so the
        // same launch-environment signal that gates the scroll hook also
        // starts this sheet in its expanded form. It is never set for a
        // user-facing launch.
        _isChoosingIndividually = State(
            initialValue: ProcessInfo.processInfo.environment[
                "NMP_WORKBENCH_UI_TEST_SCENARIO"
            ] != nil
        )
    }

    public var body: some View {
        NavigationStack {
            ScrollViewReader { proxy in
                VStack(spacing: 0) {
                    if isUITestScrollHookEnabled {
                        scrollAnchorRow(proxy: proxy)
                    }
                    ScrollView {
                        VStack(
                            alignment: .leading,
                            spacing: NappletMetrics.roomy
                        ) {
                            NappletHeading(
                                title: "Open \(model.review.nappletTitle)?",
                                subtitle: subtitle
                            )

                            if let issue = model.issue {
                                NappletNotice(
                                    verdict: .caution("\(issue.title). \(issue.message)")
                                )
                            }

                            capabilitySections

                            customiseToggle

                            NappletEvidence {
                                PermissionEvidence(review: model.review)
                            }
                        }
                        .padding(NappletMetrics.roomy)
                    }
                }
            }
            .navigationTitle("Permissions")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Not Now") {
                        model.cancel()
                        dismiss()
                    }
                    .keyboardShortcut(.cancelAction)
                    .disabled(model.isSubmitting)
                    .accessibilityHint(
                        "Closes without giving this napplet access to anything"
                    )
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(confirmTitle) {
                        Task {
                            if isChoosingIndividually {
                                await model.confirm()
                            } else {
                                await model.allowRecommended()
                            }
                            if model.isApplied {
                                dismiss()
                            }
                        }
                    }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!model.canConfirm)
                    .accessibilityIdentifier("permission-confirm")
                    .accessibilityHint(
                        "Gives this napplet the access listed above and opens it"
                    )
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 520, idealWidth: 580, minHeight: 480, idealHeight: 640)
        #endif
        .interactiveDismissDisabled(model.isSubmitting)
    }

    private var subtitle: String {
        let publisher = NappletIdentityPresentation.publisherName(
            displayName: model.review.publisherDisplayName,
            publicKey: model.review.principal.manifestAuthorPublicKey
        )
        return model.review.capabilities.isEmpty
            ? "From \(publisher)."
            : "From \(publisher). Here's what it's asking for."
    }

    private var confirmTitle: String {
        model.isSubmitting ? "Opening…" : "Allow and Open"
    }

    @ViewBuilder
    private var capabilitySections: some View {
        if model.review.capabilities.isEmpty {
            NappletCard {
                Label(
                    "This napplet doesn't need access to anything.",
                    systemImage: "checkmark"
                )
                .font(.callout)
            }
        } else {
            VStack(alignment: .leading, spacing: NappletMetrics.comfortable) {
                if !model.requiredCapabilities.isEmpty {
                    capabilityGroup(
                        title: "It needs to",
                        capabilities: model.requiredCapabilities
                    )
                }
                if !model.optionalCapabilities.isEmpty {
                    capabilityGroup(
                        title: "It would also like to",
                        capabilities: model.optionalCapabilities
                    )
                }
                if !model.managedCapabilities.isEmpty {
                    capabilityGroup(
                        title: "Already decided for you",
                        capabilities: model.managedCapabilities
                    )
                }
            }
        }
    }

    private func capabilityGroup(
        title: String,
        capabilities: [PermissionCapabilityReview]
    ) -> some View {
        VStack(alignment: .leading, spacing: NappletMetrics.snug) {
            Text(title)
                .font(.headline)
            NappletCard {
                VStack(alignment: .leading, spacing: NappletMetrics.comfortable) {
                    ForEach(capabilities) { capability in
                        PermissionCapabilityRow(
                            capability: capability,
                            isChoosingIndividually: isChoosingIndividually,
                            selection: model.selection(for: capability),
                            onSelect: { decision in
                                model.select(decision, for: capability)
                            }
                        )
                        .id(capability.domain)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var customiseToggle: some View {
        if !model.decidableCapabilities.isEmpty {
            Button {
                withAnimation(.easeInOut(duration: 0.15)) {
                    isChoosingIndividually.toggle()
                }
            } label: {
                Label(
                    isChoosingIndividually
                        ? "Use the recommended choices"
                        : "Choose each one myself",
                    systemImage: isChoosingIndividually
                        ? "chevron.up"
                        : "slider.horizontal.3"
                )
                .font(.callout)
            }
            .buttonStyle(.link)
            .accessibilityIdentifier("permission-choose-individually")
            .accessibilityHint(
                "Shows a separate choice for each thing this napplet asked for"
            )
        }
    }
}

/// One thing the napplet is asking to do, said plainly.
private struct PermissionCapabilityRow: View {
    let capability: PermissionCapabilityReview
    let isChoosingIndividually: Bool
    let selection: PermissionRequestedDecision?
    let onSelect: (PermissionRequestedDecision) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.tight) {
            Label {
                VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                    Text(phrase.sentence)
                        .font(.callout.weight(.medium))
                        .fixedSize(horizontal: false, vertical: true)
                    Text(phrase.explanation)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } icon: {
                Image(systemName: phrase.symbol)
                    .foregroundStyle(.secondary)
            }
            .accessibilityElement(children: .combine)

            if let unavailable = unavailableMessage {
                Text(unavailable)
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }

            if capability.requestedDecision == nil {
                Text(managedReason)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            } else if isChoosingIndividually {
                decisionMenu
            }
        }
    }

    private var phrase: NappletCapabilityPhrase {
        NappletVocabulary.phrase(
            forDomain: capability.domain,
            fallbackTitle: capability.title
        )
    }

    private var unavailableMessage: String? {
        switch capability.platformAvailability {
        case .available:
            nil
        case let .unknown(reason):
            "This app can't tell whether that works here. \(reason)"
        case let .unavailable(reason):
            "Not available on this device, so it won't work. \(reason)"
        }
    }

    private var managedReason: String {
        capability.decisionOptions
            .compactMap(\.invalidReason)
            .first
            ?? "This one isn't yours to change."
    }

    /// Deliberately a `Menu` of `Button`s rather than a `Picker`: a
    /// menu-styled `Picker` lets AppKit render the options, which drops the
    /// per-option accessibility identifiers the UI suite drives. The control
    /// looks the same and stays automatable.
    private var decisionMenu: some View {
        Menu {
            ForEach(capability.decisionOptions) { option in
                Button {
                    onSelect(option.decision)
                } label: {
                    if selection == option.decision {
                        Label(optionTitle(option), systemImage: "checkmark")
                    } else {
                        Text(optionTitle(option))
                    }
                }
                .disabled(!option.isValid)
                .accessibilityIdentifier(
                    "permission-\(capability.domain)-\(option.decision.rawValue)"
                )
                .help(option.invalidReason ?? optionTitle(option))
                .accessibilityLabel(optionTitle(option))
                .accessibilityHint(
                    option.invalidReason ?? "Chooses this for \(phrase.sentence)"
                )
            }
        } label: {
            Text(selectionTitle)
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .accessibilityIdentifier("permission-decision-\(capability.domain)")
        .accessibilityLabel("Choice for \(phrase.sentence)")
        .accessibilityValue(selectionTitle)
        .accessibilityHint("Shows the choices the runtime allows here")
    }

    private var selectionTitle: String {
        selection.map(plainDecisionTitle) ?? "Managed for you"
    }

    /// Marks the decision the runtime itself recommends. The preference is
    /// read from Rust's projected `recommendedDecision`; this sheet never
    /// ranks `decisionOptions` on its own.
    private func optionTitle(_ option: PermissionDecisionOption) -> String {
        let base = plainDecisionTitle(option.decision)
        guard option.decision == capability.recommendedDecision else {
            return base
        }
        return "\(base) (Recommended)"
    }

    private func plainDecisionTitle(
        _ decision: PermissionRequestedDecision
    ) -> String {
        switch decision {
        case .deny: "Don't allow"
        case .askEveryTime: "Ask me each time"
        case .allowSession: "Allow while it's open"
        case .allowExactBuild: "Always allow"
        }
    }
}

/// The exact values behind the sentences above, for the person who asked.
private struct PermissionEvidence: View {
    let review: PermissionReview

    var body: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.comfortable) {
            NappletFieldGrid(fields: [
                NappletField(
                    "Publisher key",
                    review.principal.manifestAuthorPublicKey
                ),
                NappletField("dTag", review.principal.dTag),
                NappletField("Aggregate hash", review.principal.aggregateHash),
            ])

            ForEach(review.capabilities) { capability in
                VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                    Text(capability.domain)
                        .font(.caption.monospaced().weight(.semibold))
                    NappletFieldGrid(fields: fields(for: capability))
                }
            }
        }
    }

    private func fields(
        for capability: PermissionCapabilityReview
    ) -> [NappletField] {
        var fields = [
            NappletField("Title", capability.title),
            NappletField("Requirement", capability.requirement.title),
            NappletField("Sensitivity", capability.sensitivity.title),
            NappletField("Rationale", capability.rationale),
            NappletField("Availability", capability.platformAvailability.title),
            NappletField("Current decision", capability.existingDecision.title),
            NappletField("Granted", capability.isGranted ? "yes" : "no"),
        ]
        if let detail = capability.platformAvailability.detail {
            fields.append(NappletField("Availability detail", detail))
        }
        if let recommended = capability.recommendedDecision {
            fields.append(NappletField("Recommended", recommended.title))
        }
        for dependency in capability.dependencies {
            fields.append(NappletField(
                "Depends on \(dependency.domain)",
                dependency.reason
            ))
        }
        for option in capability.decisionOptions where !option.isValid {
            fields.append(NappletField(
                "\(option.decision.title) unavailable",
                option.invalidReason ?? "no reason projected"
            ))
        }
        return fields
    }
}
