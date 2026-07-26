import NMPNativeRuntimeApple
import SwiftUI

struct PendingWriteConsentPresentation: Equatable, Sendable {
    let exactDraft: String

    init(exactDraft: String) {
        self.exactDraft = exactDraft
    }
}

/// The moment a napplet asks to publish something under the user's name.
///
/// This is the highest-stakes surface in the application and it was the most
/// technical: it led with "NAP-OUTBOX approval required", then an account key,
/// an approval id, a publisher key, a dTag, an aggregate hash, a sentence
/// about relay plans, and the raw draft JSON -- everything except the one
/// thing a person needs, which is *what is about to be said in their name*.
///
/// The draft still appears in full, because a signing prompt that hides what
/// is being signed is worse than a technical one. What changed is that the
/// content leads and the identifiers moved behind a disclosure.
/// See `docs/adr/0008-verdicts-on-the-path.md`.
struct PendingWriteApprovalBar: View {
    let write: NativeRuntimePendingWrite
    let onDecision: (Bool) -> Void

    private var consent: PendingWriteConsentPresentation {
        PendingWriteConsentPresentation(exactDraft: write.draftJSON)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.snug) {
            HStack(alignment: .top, spacing: NappletMetrics.snug) {
                Image(systemName: "signature")
                    .foregroundStyle(NappletInk.caution)
                    .accessibilityHidden(true)

                VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                    Text("Publish this exact event?")
                        .font(.headline)
                    Text(
                        "Review the complete event below. Approval applies to "
                            + "all of it, not only its readable text."
                    )
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                }

            }

            AdaptiveActionPair {
                Button("Don't Publish") {
                    onDecision(false)
                }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("nap-outbox-reject")
            } trailing: {
                Button("Publish Exact Event") {
                    onDecision(true)
                }
                .buttonStyle(.borderedProminent)
                .tint(NappletInk.accent)
                .accessibilityIdentifier("nap-outbox-approve")
            }

            draftPreview

            NappletEvidence {
                NappletFieldGrid(fields: evidenceFields)
            }
            .font(.caption)
        }
        .padding(.horizontal, NappletMetrics.comfortable)
        .padding(.vertical, NappletMetrics.snug)
        .background(.regularMaterial)
        .overlay(alignment: .bottom) {
            Rectangle()
                .fill(NappletInk.caution)
                .frame(height: 1)
        }
    }

    private var draftPreview: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
            Text("Complete event to approve")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            ScrollView(.vertical) {
                Text(consent.exactDraft)
                    .font(.caption2.monospaced())
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(maxHeight: 180)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(NappletMetrics.snug)
        .background(
            .quaternary.opacity(0.4),
            in: RoundedRectangle(cornerRadius: NappletMetrics.tight)
        )
        .accessibilityElement(children: .contain)
    }

    private var evidenceFields: [NappletField] {
        [
            NappletField("Account", write.account),
            NappletField("Approval id", write.approvalID),
            NappletField("Publisher key", write.scope.manifestAuthor),
            NappletField("dTag", write.scope.dTag),
            NappletField("Aggregate hash", write.scope.aggregateHash),
            NappletField(
                "Routing",
                "NMP public relay plan; native cannot retarget it"
            ),
        ]
    }
}

/// An empty pending-write or receipt list is ambiguous: it means either
/// "nothing is happening" or "the observation that would have told us
/// could not even be established." This bar exists so the second case is
/// never silently presented as the first — a napplet genuinely stuck
/// waiting on approval must not look identical to one doing nothing.
struct ObservationUnavailableBar: View {
    let title: String
    let detail: String

    var body: some View {
        HStack(alignment: .top, spacing: NappletMetrics.tight) {
            Image(systemName: "exclamationmark.triangle")
                .foregroundStyle(NappletInk.caution)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                Text(title)
                    .font(.caption.weight(.semibold))
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
        }
        .padding(.horizontal, NappletMetrics.comfortable)
        .padding(.vertical, NappletMetrics.hairline + 2)
        .background(.regularMaterial)
        .accessibilityElement(children: .contain)
    }
}
