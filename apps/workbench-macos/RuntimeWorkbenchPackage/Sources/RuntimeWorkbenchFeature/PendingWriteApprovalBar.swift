import NMPNativeRuntimeApple
import SwiftUI

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

    var body: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.snug) {
            HStack(alignment: .top, spacing: NappletMetrics.snug) {
                Image(systemName: "signature")
                    .foregroundStyle(.orange)
                    .accessibilityHidden(true)

                VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                    Text("Publish this as you?")
                        .font(.headline)
                    Text(
                        "It will appear under your name. Once it's out, you "
                            + "can't take it back."
                    )
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                }

                Spacer(minLength: NappletMetrics.snug)

                Button("Don't Publish") {
                    onDecision(false)
                }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("nap-outbox-reject")

                Button("Publish") {
                    onDecision(true)
                }
                .buttonStyle(.borderedProminent)
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
                .fill(.orange.opacity(0.55))
                .frame(height: 1)
        }
        .accessibilityIdentifier("nap-outbox-pending-approval")
    }

    /// What is actually about to be published, as close to plainly as the
    /// draft allows. If the draft has no readable message, the raw draft is
    /// shown rather than nothing -- never signing blind is the higher rule.
    @ViewBuilder
    private var draftPreview: some View {
        if let message = draftMessage, !message.isEmpty {
            Text(message)
                .font(.callout)
                .textSelection(.enabled)
                .lineLimit(6)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(NappletMetrics.snug)
                .background(
                    .quaternary.opacity(0.4),
                    in: RoundedRectangle(cornerRadius: NappletMetrics.tight)
                )
                .accessibilityLabel("Message to publish: \(message)")
        } else {
            VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                Text("This one has no readable message. Here it is in full:")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(write.draftJSON)
                    .font(.caption2.monospaced())
                    .textSelection(.enabled)
                    .lineLimit(8)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(NappletMetrics.snug)
            .background(
                .quaternary.opacity(0.4),
                in: RoundedRectangle(cornerRadius: NappletMetrics.tight)
            )
        }
    }

    /// Presentation-only: lifts the human-readable message out of the draft
    /// for display. It interprets nothing and decides nothing -- the draft
    /// that gets signed is `write.draftJSON`, untouched, and the full text is
    /// always one disclosure away regardless of what this returns.
    private var draftMessage: String? {
        guard
            let data = write.draftJSON.data(using: .utf8),
            let object = try? JSONSerialization.jsonObject(with: data),
            let fields = object as? [String: Any],
            let content = fields["content"] as? String
        else {
            return nil
        }
        return content.trimmingCharacters(in: .whitespacesAndNewlines)
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
            NappletField("Draft", write.draftJSON),
        ]
    }
}

/// What happened to something the user published.
///
/// It used to read "NMP receipt", a receipt id, a delivery string and a blob
/// of state JSON across the top of the window. The identifiers belong in
/// activity, where someone can go looking for them; a status bar gets to say
/// one true thing.
struct ReceiptStatusBar: View {
    let receipt: NativeRuntimeReceipt

    var body: some View {
        HStack(spacing: NappletMetrics.tight) {
            Image(systemName: isPending ? "clock" : "checkmark")
                .foregroundStyle(isPending ? .orange : .secondary)
                .accessibilityHidden(true)
            Text(isPending ? "Sending your post…" : "Posted")
                .font(.caption)
            Spacer()
        }
        .padding(.horizontal, NappletMetrics.comfortable)
        .padding(.vertical, NappletMetrics.hairline + 2)
        .background(.regularMaterial)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("nap-outbox-receipt-status")
    }

    /// Reads the runtime's own delivery classification. This is the same test
    /// the previous status bar used to choose its glyph; it is not a new
    /// interpretation of delivery state.
    private var isPending: Bool {
        receipt.delivery.contains("pending")
    }
}
