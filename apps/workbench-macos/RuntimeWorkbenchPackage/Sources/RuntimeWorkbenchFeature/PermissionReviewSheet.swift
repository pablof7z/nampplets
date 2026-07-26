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
        ScrollViewReader { proxy in
            VStack(spacing: 0) {
                if isUITestScrollHookEnabled {
                    scrollAnchorRow(proxy: proxy)
                }
                ScrollView {
                    VStack(alignment: .leading, spacing: 0) {
                        Text(model.review.nappletTitle)
                            .font(NappletType.display)
                            .nappletDisplayFace()
                            .foregroundStyle(NappletInk.ink)
                            .fixedSize(horizontal: false, vertical: true)

                        Text(subtitle)
                            .font(NappletType.lede)
                            .foregroundStyle(NappletInk.inkSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                            .padding(.top, NappletMetrics.tight)

                        if let issue = model.issue {
                            NappletNotice(
                                verdict: .caution("\(issue.title). \(issue.message)")
                            )
                            .padding(.top, NappletMetrics.roomy)
                        }

                        capabilitySections
                            .padding(.top, NappletMetrics.spacious)

                        customiseToggle
                            .padding(.top, NappletMetrics.roomy)

                        NappletEvidence {
                            PermissionEvidence(review: model.review)
                        }
                        .font(NappletType.caption)
                        .padding(.top, NappletMetrics.roomy)
                    }
                    .frame(maxWidth: NappletMetrics.measure, alignment: .leading)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, NappletMetrics.generous)
                    .padding(.top, NappletMetrics.generous)
                    .padding(.bottom, NappletMetrics.spacious)
                }

                actions
            }
        }
        .background(NappletInk.paperRaised)
        #if os(macOS)
        .frame(minWidth: 580, idealWidth: 640, minHeight: 520, idealHeight: 700)
        #endif
        .interactiveDismissDisabled(model.isSubmitting)
    }

    /// The accent appears here and nowhere else on this screen.
    private var actions: some View {
        VStack(spacing: 0) {
            Rectangle()
                .fill(NappletInk.rule)
                .frame(height: 1)
            HStack(spacing: NappletMetrics.snug) {
                Spacer()
                Button("Not Now") {
                    model.cancel()
                    dismiss()
                }
                .keyboardShortcut(.cancelAction)
                .disabled(model.isSubmitting)
                .accessibilityHint(
                    "Closes without giving this napplet access to anything"
                )

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
                .buttonStyle(.borderedProminent)
                .tint(NappletInk.accent)
                .keyboardShortcut(.defaultAction)
                .disabled(!model.canConfirm)
                .accessibilityIdentifier("permission-confirm")
                .accessibilityHint(
                    "Gives this napplet the access listed above and opens it"
                )
            }
            .padding(.horizontal, NappletMetrics.generous)
            .padding(.vertical, NappletMetrics.comfortable)
        }
    }

    private var subtitle: String {
        model.review.capabilities.isEmpty
            ? "Opening for the first time."
            : "Opening for the first time. Here's what it's asking for."
    }

    private var confirmTitle: String {
        model.isSubmitting ? "Opening…" : "Allow and Open"
    }

    @ViewBuilder
    private var capabilitySections: some View {
        if model.review.capabilities.isEmpty {
            Text("This napplet doesn't need access to anything.")
                .font(NappletType.secondary)
                .foregroundStyle(NappletInk.inkSecondary)
                .fixedSize(horizontal: false, vertical: true)
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
                // Leads with the effect rather than with who decided.
                // "Already decided for you" conflated effect with provenance
                // and read as paternalistic; what matters to the person is
                // that this access is already in force. Provenance is
                // annotated per row, from Rust's own reason text.
                if !model.managedCapabilities.isEmpty {
                    capabilityGroup(
                        title: "It can already",
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
                .font(NappletType.heading)
                .foregroundStyle(NappletInk.ink)

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
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(NappletMetrics.comfortable)
            .background(
                NappletInk.fillQuiet,
                in: RoundedRectangle(
                    cornerRadius: NappletMetrics.cardCorner,
                    style: .continuous
                )
            )
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
                .font(NappletType.secondary)
                .foregroundStyle(NappletInk.inkSecondary)
            }
            // `.plain`, not `.link`: `.link` is macOS-only and broke the iOS
            // build. It would have been wrong here anyway -- this is not the
            // primary action, and the accent belongs to exactly one element
            // per screen.
            .buttonStyle(.plain)
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
                        .font(NappletType.secondary.weight(.medium))
                        .foregroundStyle(NappletInk.ink)
                        .fixedSize(horizontal: false, vertical: true)
                    Text(phrase.explanation)
                        .font(NappletType.caption)
                        .foregroundStyle(NappletInk.inkSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } icon: {
                Image(systemName: phrase.symbol)
                    .foregroundStyle(NappletInk.inkSecondary)
            }
            .accessibilityElement(children: .combine)

            if let unavailable = unavailableMessage {
                Text(unavailable)
                    .font(NappletType.caption)
                    .foregroundStyle(NappletInk.caution)
                    .fixedSize(horizontal: false, vertical: true)
            }

            if capability.requestedDecision == nil {
                Text(managedReason)
                    .font(NappletType.caption)
                    .foregroundStyle(NappletInk.inkSecondary)
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
            "This app can't tell whether that works here."
                .appendingSentence(reason)
        case let .unavailable(reason):
            "Not available on this device, so it won't work."
                .appendingSentence(reason)
        }
    }

    /// Provenance, annotated after the effect rather than instead of it.
    private var managedReason: String {
        capability.decisionOptions
            .compactMap(\.invalidReason)
            .first
            ?? "This one isn't yours to change here."
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

    /// Plain wording for the decisions Rust offers.
    ///
    /// `allowExactBuild` reads "Always allow this version" rather than
    /// "Always allow": ADR 0002 binds a grant to `(manifest author, dTag,
    /// aggregateHash)` and forbids it transferring to a new aggregate, so an
    /// unqualified "always" would promise scope the runtime does not grant --
    /// a verdict the app cannot stand behind, which ADR 0008 forbids.
    ///
    /// This switch is deliberately exhaustive over `PermissionRequestedDecision`.
    /// If Rust ever projects a broader, author-scoped decision it will fail to
    /// compile here, which is the intended outcome: the wording is a decision
    /// someone must make, not something to be defaulted or synthesised from an
    /// existing case.
    private func plainDecisionTitle(
        _ decision: PermissionRequestedDecision
    ) -> String {
        switch decision {
        case .deny: "Don't allow"
        case .askEveryTime: "Ask me each time"
        case .allowSession: "Allow while it's open"
        case .allowExactBuild: "Always allow this version"
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
