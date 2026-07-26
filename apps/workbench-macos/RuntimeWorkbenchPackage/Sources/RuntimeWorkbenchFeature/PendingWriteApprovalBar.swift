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

    /// The complete draft, for every shape, always.
    ///
    /// This deliberately does NOT summarise. An earlier version lifted
    /// `content` out and showed it as "what is about to be published", which
    /// is wrong in the way that matters most on this screen: a draft is an
    /// `nmp::UnsignedEvent` -- author, timestamp, kind, tags, content -- and
    /// for a large class of events the effect is not in the content. A
    /// deletion carries an optional human *reason* in `content` and names its
    /// targets in `tags`, so it previewed as that sentence under a heading
    /// reading "Publish this as you?". Someone could read it, approve, and
    /// destroy their own posts having been shown nothing about it.
    ///
    /// A narrower classifier -- kind 1 with no tags -- was written and
    /// rejected, correctly: it is still Swift asserting protocol semantics on
    /// its own authority, and it fails the same way, more rarely and
    /// therefore less catchably. Rust owns that judgement. Until #24 provides
    /// a typed `PendingWriteConsentSummary`, this surface is intentionally
    /// safe and deliberately hostile: unreadable beats untrue, and it is the
    /// one screen in this app where completeness IS the verdict.
    ///
    /// See `docs/adr/0008-verdicts-on-the-path.md`.
    private var draftPreview: some View {
        Text(write.draftJSON)
            .font(NappletType.record)
            .foregroundStyle(NappletInk.ink)
            .textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(NappletMetrics.snug)
            .background(
                NappletInk.fillQuiet,
                in: RoundedRectangle(
                    cornerRadius: NappletMetrics.tight,
                    style: .continuous
                )
            )
            .accessibilityLabel("Exact content to be published")
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
