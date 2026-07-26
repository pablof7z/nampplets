import SwiftUI

enum PermissionPlainCopy {
    static func subtitle(
        hasCapabilities: Bool,
        isManagedReviewBlocked _: Bool
    ) -> String {
        if !hasCapabilities {
            return "There are no access choices in this review."
        }
        return "Review what this napplet can do and any choices available here."
    }
}

/// The permission review used for initial consent and later settings changes.
///
/// Install grants nothing; this is where capability is actually granted, so
/// this is where the weight belongs. It states what the napplet will be able
/// to do in the user's own language and offers one gesture for the common
/// outcome. Per-capability allow/deny switches stay visible, while Rust owns
/// grant scope and exact values remain in the evidence disclosure.
///
/// See `docs/adr/0008-verdicts-on-the-path.md`.
public struct PermissionReviewSheet: View {
    @Environment(\.dismiss) private var dismiss
    @State var model: PermissionReviewSheetModel
    @State private var isChoosingIndividually = false

    @MainActor
    public init(manager: any PermissionReviewManaging) {
        _model = State(
            initialValue: PermissionReviewSheetModel(manager: manager)
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

                        if let plainIssueVerdict = model.plainIssueVerdict {
                            NappletNotice(verdict: plainIssueVerdict)
                            .padding(.top, NappletMetrics.roomy)
                        }

                        capabilitySections
                            .padding(.top, NappletMetrics.spacious)

                        if model.isManagedReviewBlocked {
                            Text(
                                "Some settings in this review are managed here "
                                    + "and can't be changed. Choose Not Now to close."
                            )
                            .font(NappletType.secondary)
                            .foregroundStyle(NappletInk.inkSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                            .padding(.top, NappletMetrics.roomy)
                        }

                        NappletEvidence {
                            PermissionEvidence(
                                review: model.review,
                                issue: model.issue
                            )
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
        // A sheet is its own window and AppKit sizes it against the screen, not
        // against the parent's content area, so on a short display its footer --
        // and with it "Not Now" -- lands under the Dock. See
        // `WorkbenchWindowFitting.maxSheetHeight`. The scroll view above absorbs
        // whatever height this gives back, and the 520 floor is never crossed.
        .frame(maxHeight: PermissionReviewSheetGeometry.maxHeight)
        #endif
        .interactiveDismissDisabled(model.isSubmitting)
    }

    /// The accent appears here and nowhere else on this screen.
    private var actions: some View {
        VStack(spacing: 0) {
            Rectangle()
                .fill(NappletInk.rule)
                .frame(height: 1)
            AdaptiveActionPair {
                Button("Not Now") {
                    model.cancel()
                    dismiss()
                }
                .keyboardShortcut(.cancelAction)
                .disabled(model.isSubmitting)
                .accessibilityHint(
                    "Closes without changing these settings"
                )
            } trailing: {
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
                    confirmHint
                )
            }
            .padding(.horizontal, NappletMetrics.generous)
            .padding(.vertical, NappletMetrics.comfortable)
        }
    }

    private var subtitle: String {
        PermissionPlainCopy.subtitle(
            hasCapabilities: !model.review.capabilities.isEmpty,
            isManagedReviewBlocked: model.isManagedReviewBlocked
        )
    }

    private var confirmTitle: String {
        if model.isSubmitting {
            return "Applying…"
        }
        if model.isManagedReviewBlocked {
            return "Can't Apply Here"
        }
        if model.review.capabilities.isEmpty {
            return "Continue"
        }
        return isChoosingIndividually
            ? "Apply Choices"
            : "Use Recommended Settings"
    }

    private var confirmHint: String {
        if model.isManagedReviewBlocked {
            return "Unavailable because this review includes settings you can't change here"
        }
        if model.review.capabilities.isEmpty {
            return "Continues without applying any permission choices"
        }
        return isChoosingIndividually
            ? "Applies the permission choices shown above"
            : "Applies the recommended settings shown above"
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
                if !model.managedCapabilities.isEmpty {
                    capabilityGroup(
                        title: "Managed here",
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
                        grantBinding: Binding(
                            get: { model.isGranted(capability) },
                            set: { granted in
                                isChoosingIndividually = true
                                model.setGranted(granted, for: capability)
                            }
                        ),
                        hasAffirmativeOption: model.hasAffirmativeOption(
                            capability
                        ),
                        isReviewLocked: model.isManagedReviewBlocked
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

}

/// One thing the napplet is asking to do, said plainly.
