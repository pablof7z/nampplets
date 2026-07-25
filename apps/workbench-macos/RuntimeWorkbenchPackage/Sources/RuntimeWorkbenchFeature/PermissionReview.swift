import Observation
import SwiftUI

/// The Rust-backed permission boundary consumed by the native review sheet.
///
/// `submit` accepts one finite batch for one exact build. Implementations own
/// dependency validation, persistence, revocation, provider cancellation, and
/// the resulting state. This interface deliberately exposes no launch action.
@MainActor
public protocol PermissionReviewManaging: AnyObject {
    func snapshot() -> PermissionReviewSnapshot
    func submit(_ batch: PermissionDecisionBatch) async
}

@MainActor
@Observable
final class PermissionReviewSheetModel {
    private let manager: any PermissionReviewManaging
    private(set) var snapshot: PermissionReviewSnapshot
    private(set) var selections: [String: PermissionRequestedDecision]
    private(set) var transientIssue: PermissionReviewIssue?
    private(set) var isSubmitting = false

    init(manager: any PermissionReviewManaging) {
        self.manager = manager
        let snapshot = manager.snapshot()
        self.snapshot = snapshot
        selections = Dictionary(
            uniqueKeysWithValues: snapshot.review.capabilities.compactMap {
                capability in
                capability.requestedDecision.map {
                    (capability.domain, $0)
                }
            }
        )
    }

    var review: PermissionReview {
        snapshot.review
    }

    var issue: PermissionReviewIssue? {
        if let transientIssue {
            return transientIssue
        }
        guard case let .refused(issue) = snapshot.submissionState else {
            return nil
        }
        return issue
    }

    var isApplied: Bool {
        snapshot.submissionState == .applied
    }

    var canConfirm: Bool {
        !isSubmitting
            && !isApplied
            && transientIssue == nil
            && invalidSelections.isEmpty
    }

    func selection(
        for capability: PermissionCapabilityReview
    ) -> PermissionRequestedDecision? {
        selections[capability.domain] ?? capability.requestedDecision
    }

    func select(
        _ decision: PermissionRequestedDecision,
        for requestedCapability: PermissionCapabilityReview
    ) {
        guard
            let capability = review.capabilities.first(where: {
                $0.domain == requestedCapability.domain
            }),
            let option = capability.option(for: decision),
            option.isValid
        else {
            transientIssue = PermissionReviewIssue(
                title: "Decision unavailable",
                message: requestedCapability.option(for: decision)?.invalidReason
                    ?? "The runtime did not offer this decision.",
                affectedDomains: [requestedCapability.domain]
            )!
            return
        }
        selections[capability.domain] = decision
        transientIssue = nil
    }

    /// Discards only transient native form state. It never calls the manager.
    func cancel() {
        selections = Dictionary(
            uniqueKeysWithValues: review.capabilities.compactMap {
                capability in
                capability.requestedDecision.map {
                    (capability.domain, $0)
                }
            }
        )
        transientIssue = nil
    }

    func confirm() async {
        guard canConfirm else {
            return
        }
        guard !review.capabilities.isEmpty else {
            snapshot = PermissionReviewSnapshot(
                review: review,
                submissionState: .applied
            )
            return
        }
        guard
            let batch = PermissionDecisionBatch(
                principal: review.principal,
                decisions: review.capabilities.compactMap { capability in
                    guard let selection = selections[capability.domain] else {
                        return nil
                    }
                    return PermissionDecisionSelection(
                        domain: capability.domain,
                        decision: selection
                    )
                }
            ),
            batch.decisions.count == review.capabilities.count
        else {
            transientIssue = PermissionReviewIssue(
                title: "Permission review is incomplete",
                message: "Every capability needs one valid decision before confirming."
            )!
            return
        }

        isSubmitting = true
        transientIssue = nil
        await manager.submit(batch)
        let updatedSnapshot = manager.snapshot()
        if updatedSnapshot.review.principal != batch.principal {
            transientIssue = PermissionReviewIssue(
                title: "Exact build changed",
                message: "The permission review no longer matches this verified build."
            )!
        } else {
            snapshot = updatedSnapshot
        }
        isSubmitting = false
    }

    private var invalidSelections: [String] {
        review.capabilities.compactMap { capability in
            guard
                let selected = selection(for: capability),
                capability.option(for: selected)?.isValid == true
            else {
                return capability.domain
            }
            return nil
        }
    }
}

public struct PermissionReviewSheet: View {
    @Environment(\.dismiss) private var dismiss
    @State var model: PermissionReviewSheetModel

    @MainActor
    public init(manager: any PermissionReviewManaging) {
        _model = State(
            initialValue: PermissionReviewSheetModel(manager: manager)
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
                        VStack(alignment: .leading, spacing: 20) {
                            exactBuildIdentity
                            Divider()
                            capabilityReview
                            if let issue = model.issue {
                                Divider()
                                issueView(issue)
                            }
                        }
                        .padding(24)
                    }
                }
            }
            .navigationTitle("Review Permissions")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        model.cancel()
                        dismiss()
                    }
                    .keyboardShortcut(.cancelAction)
                    .disabled(model.isSubmitting)
                    .accessibilityHint(
                        "Closes the review without changing any permission"
                    )
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Confirm Decisions") {
                        Task {
                            await model.confirm()
                            if model.isApplied {
                                dismiss()
                            }
                        }
                    }
                    .keyboardShortcut(.return, modifiers: [.command])
                    .disabled(!model.canConfirm)
                    .accessibilityIdentifier("permission-confirm")
                    .accessibilityHint(
                        "Saves this bounded permission batch without launching the napplet"
                    )
                }
            }
        }
        .frame(
            minWidth: 680,
            idealWidth: 780,
            minHeight: 560,
            idealHeight: 720
        )
        .interactiveDismissDisabled(model.isSubmitting)
    }

    private var exactBuildIdentity: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label(model.review.nappletTitle, systemImage: "checkmark.seal")
                .font(.title2.bold())

            Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 8) {
                identityRow(
                    label: "Publisher",
                    value: model.review.publisherDisplayName
                        ?? model.review.principal.manifestAuthorPublicKey
                )
                identityRow(
                    label: "Public key",
                    value: model.review.principal.manifestAuthorPublicKey
                )
                identityRow(
                    label: "dTag",
                    value: model.review.principal.dTag
                )
                identityRow(
                    label: "Exact build hash",
                    value: model.review.principal.aggregateHash
                )
            }
            .font(.callout)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Exact build identity")
    }

    private func identityRow(label: String, value: String) -> some View {
        GridRow {
            Text(label)
                .foregroundStyle(.secondary)
            Text(value)
                .fontDesign(.monospaced)
                .textSelection(.enabled)
                .lineLimit(2)
        }
    }

    private var capabilityReview: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Capability Decisions")
                .font(.headline)

            if model.review.capabilities.isEmpty {
                Label(
                    "This napplet does not request any capabilities.",
                    systemImage: "checkmark.shield"
                )
                .font(.callout)
                .foregroundStyle(.secondary)
            } else {
                Text(
                    "Required capabilities must be permitted for the napplet to run. "
                        + "Optional capabilities may be denied and degrade honestly."
                )
                .font(.callout)
                .foregroundStyle(.secondary)

                ForEach(model.review.capabilities) { capability in
                    capabilityCard(capability)
                        .id(capability.domain)
                }
            }
        }
    }

    private func capabilityCard(
        _ capability: PermissionCapabilityReview
    ) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(capability.title)
                        .font(.headline)
                    Text(capability.domain)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                }
                Spacer()
                requirementBadge(capability.requirement)
                sensitivityBadge(capability.sensitivity)
            }

            Text(capability.rationale)
                .font(.callout)

            availabilityRow(capability.platformAvailability)

            if !capability.dependencies.isEmpty {
                VStack(alignment: .leading, spacing: 5) {
                    Text("Dependencies")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    ForEach(capability.dependencies) { dependency in
                        Label {
                            Text("\(dependency.domain): \(dependency.reason)")
                        } icon: {
                            Image(systemName: "arrow.triangle.branch")
                        }
                        .font(.caption)
                    }
                }
            }

            Divider()

            LabeledContent("Current decision") {
                Label(
                    capability.existingDecision.title,
                    systemImage: capability.isGranted
                        ? "checkmark.shield"
                        : "shield.slash"
                )
                .foregroundStyle(
                    capability.isGranted ? Color.green : Color.secondary
                )
                .accessibilityLabel(
                    capability.isGranted
                        ? "\(capability.existingDecision.title), granted"
                        : "\(capability.existingDecision.title), not granted"
                )
            }

            if capability.requestedDecision == nil {
                lockedManagedDecision(capability)
            } else {
                HStack {
                    Text("New decision")
                    Spacer()
                    decisionMenu(capability)
                }
            }
        }
        .padding(16)
        .background(.quaternary.opacity(0.45), in: RoundedRectangle(cornerRadius: 12))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(
            "\(capability.title), \(capability.requirement.title), "
                + "\(capability.sensitivity.title)"
        )
    }

    private func requirementBadge(
        _ requirement: PermissionCapabilityRequirement
    ) -> some View {
        Text(requirement.title)
            .font(.caption.weight(.semibold))
            .padding(.horizontal, 7)
            .padding(.vertical, 3)
            .background(
                requirement == .required
                    ? Color.orange.opacity(0.18)
                    : Color.secondary.opacity(0.12),
                in: Capsule()
            )
            .accessibilityLabel("\(requirement.title) capability")
    }

    private func sensitivityBadge(
        _ sensitivity: PermissionCapabilitySensitivity
    ) -> some View {
        Label(
            sensitivity.title,
            systemImage: sensitivitySystemImage(sensitivity)
        )
        .font(.caption.weight(.semibold))
        .foregroundStyle(sensitivityColor(sensitivity))
        .accessibilityLabel("\(sensitivity.title) sensitivity")
    }

    private func availabilityRow(
        _ availability: PermissionPlatformAvailability
    ) -> some View {
        let presentation = availabilityPresentation(availability)
        return VStack(alignment: .leading, spacing: 3) {
            Label(
                availability.title,
                systemImage: presentation.systemImage
            )
            .foregroundStyle(presentation.color)
            if let detail = availability.detail {
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .font(.callout)
        .accessibilityElement(children: .combine)
    }

    private func decisionMenu(
        _ capability: PermissionCapabilityReview
    ) -> some View {
        Menu {
            ForEach(capability.decisionOptions) { option in
                Button {
                    model.select(option.decision, for: capability)
                } label: {
                    let title = optionTitle(option, in: capability)
                    if model.selection(for: capability) == option.decision {
                        Label(title, systemImage: "checkmark")
                    } else {
                        Text(title)
                    }
                }
                .disabled(!option.isValid)
                .accessibilityIdentifier(
                    "permission-\(capability.domain)-\(option.decision.rawValue)"
                )
                .help(option.invalidReason ?? optionTitle(option, in: capability))
                .accessibilityLabel(optionTitle(option, in: capability))
                .accessibilityHint(
                    option.invalidReason
                        ?? "Selects this decision for \(capability.title)"
                )
            }
        } label: {
            Text(selectionTitle(for: capability))
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .accessibilityIdentifier("permission-decision-\(capability.domain)")
        .accessibilityLabel("New decision for \(capability.title)")
        .accessibilityValue(selectionTitle(for: capability))
        .accessibilityHint("Shows the decisions permitted by the runtime")
    }

    private func selectionTitle(
        for capability: PermissionCapabilityReview
    ) -> String {
        model.selection(for: capability)?.title ?? "Managed by host"
    }

    /// Marks the decision the runtime itself recommends. The preference is
    /// read from Rust's projected `recommendedDecision`; this sheet never
    /// ranks `decisionOptions` on its own.
    private func optionTitle(
        _ option: PermissionDecisionOption,
        in capability: PermissionCapabilityReview
    ) -> String {
        guard option.decision == capability.recommendedDecision else {
            return option.decision.title
        }
        return "\(option.decision.title) (Recommended)"
    }

    private func lockedManagedDecision(
        _ capability: PermissionCapabilityReview
    ) -> some View {
        let reason = capability.decisionOptions
            .compactMap(\.invalidReason)
            .first
            ?? "This capability is managed by host policy."
        return VStack(alignment: .leading, spacing: 5) {
            Label("Managed by host policy", systemImage: "lock.shield")
                .font(.callout.weight(.semibold))
            Text(reason)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
    }

    private func sensitivitySystemImage(
        _ sensitivity: PermissionCapabilitySensitivity
    ) -> String {
        switch sensitivity {
        case .ordinary:
            "shield"
        case .sensitive:
            "exclamationmark.shield"
        case .unknown:
            "questionmark.diamond"
        }
    }

    private func sensitivityColor(
        _ sensitivity: PermissionCapabilitySensitivity
    ) -> Color {
        switch sensitivity {
        case .ordinary:
            .secondary
        case .sensitive, .unknown:
            .orange
        }
    }

    private func availabilityPresentation(
        _ availability: PermissionPlatformAvailability
    ) -> (systemImage: String, color: Color) {
        switch availability {
        case .available:
            ("checkmark.circle", .green)
        case .unknown:
            ("questionmark.circle", .orange)
        case .unavailable:
            ("xmark.circle", .red)
        }
    }

    private func issueView(_ issue: PermissionReviewIssue) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Label(issue.title, systemImage: "exclamationmark.triangle")
                .font(.headline)
                .foregroundStyle(.orange)
            Text(issue.message)
            if !issue.affectedDomains.isEmpty {
                Text("Affected: \(issue.affectedDomains.joined(separator: ", "))")
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(issue.title). \(issue.message)")
    }
}
