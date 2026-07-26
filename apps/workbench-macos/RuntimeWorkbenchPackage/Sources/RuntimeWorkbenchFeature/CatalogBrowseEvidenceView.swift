import SwiftUI

/// A quiet footer under the browse list.
///
/// What the runtime observed is kept in full -- source-by-source status, the
/// window state, refused and omitted row counts -- but a person looking for
/// something to install is not made to read it first. The plain line says
/// only whether the list is complete, because that is the only part of this
/// that changes what they should do next.
/// See `docs/adr/0008-verdicts-on-the-path.md`.
struct CatalogBrowseEvidenceView: View {
    let evidence: CatalogBrowseEvidence
    let hasMore: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Divider()
            HStack(spacing: NappletMetrics.tight) {
                Text(summary)
                    .font(.caption)
                    .foregroundStyle(isPartial ? .orange : .secondary)

                Spacer()

                NappletEvidence(label: "Where these came from") {
                    evidenceDetail
                }
                .font(.caption)
            }
            .padding(.horizontal, NappletMetrics.comfortable)
            .padding(.vertical, NappletMetrics.tight)
        }
        .background(.bar)
        .accessibilityIdentifier("catalog-feed-evidence")
        .accessibilityLabel(summary)
    }

    /// The one thing worth saying on the path: is this everything, or not?
    private var summary: String {
        if evidence.window == .requesting, evidence.projectedRows == 0 {
            return "Still looking…"
        }
        let count = evidence.projectedRows
        let noun = count == 1 ? "napplet" : "napplets"
        guard isPartial else {
            return "\(count) \(noun)"
        }
        return "\(count) \(noun) so far — there are more than fit here"
    }

    private var isPartial: Bool {
        hasMore
            || evidence.projectionLimitedRows > 0
            || !evidence.shortfalls.isEmpty
            || evidence.sourceEvidence.contains { source in
                switch source.status {
                case .disconnected, .authenticationDenied, .error: true
                case .requesting, .connecting, .awaitingAuthentication: false
                }
            }
    }

    private var evidenceDetail: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.snug) {
            NappletFieldGrid(fields: countFields)

            if !evidence.sourceEvidence.isEmpty {
                VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
                    Text("Sources")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    NappletFieldGrid(
                        fields: evidence.sourceEvidence.map { source in
                            NappletField(
                                source.source,
                                "\(statusWord(source.status)) · "
                                    + accessWord(source.access)
                                    + reconciledSuffix(source.reconciledThrough)
                            )
                        }
                    )
                }
            }

            if !evidence.shortfalls.isEmpty {
                NappletFieldGrid(fields: [NappletField(
                    "Shortfalls",
                    evidence.shortfalls.map(shortfallWord).joined(separator: ", ")
                )])
            }
        }
    }

    private var countFields: [NappletField] {
        var fields = [
            NappletField("Scope", scopeWord),
            NappletField("Window", windowWord),
            NappletField("Projected rows", "\(evidence.projectedRows)"),
        ]
        if evidence.queryWasLocalFilter {
            fields.append(NappletField(
                "Excluded by local filter",
                "\(evidence.locallyFilteredRows)"
            ))
        }
        if evidence.projectionLimitedRows > 0 {
            fields.append(NappletField(
                "Omitted by projection limit",
                "\(evidence.projectionLimitedRows)"
            ))
        }
        if evidence.refusedRows > 0 {
            fields.append(NappletField(
                "Refused as malformed or oversized",
                "\(evidence.refusedRows)"
            ))
        }
        return fields
    }

    private var scopeWord: String {
        switch evidence.scope {
        case .liveNMPWindow:
            "live NMP window — source-scoped, not a complete network result"
        case .offlineFixture:
            "offline bundled fixture — no network lookup performed"
        }
    }

    private var windowWord: String {
        switch evidence.window {
        case .idle: "idle"
        case .requesting: "requesting more rows"
        case let .returned(addedRows): "returned \(addedRows) rows"
        case let .atBound(maximumRows): "at its \(maximumRows)-row bound"
        case .unknown: "not classified by the NMP facade"
        }
    }

    private func statusWord(_ status: CatalogBrowseSourceStatus) -> String {
        switch status {
        case .requesting: "requesting"
        case .connecting: "connecting"
        case .disconnected: "disconnected"
        case .awaitingAuthentication: "awaiting authentication"
        case .authenticationDenied: "authentication denied"
        case .error: "error"
        }
    }

    private func accessWord(_ access: CatalogBrowseAccessContext) -> String {
        switch access {
        case .public: "public"
        case let .nip42(publicKey): "NIP-42 as \(publicKey)"
        }
    }

    private func reconciledSuffix(_ reconciledThrough: UInt64?) -> String {
        reconciledThrough.map { " · reconciled through \($0)" } ?? ""
    }

    private func shortfallWord(_ shortfall: CatalogBrowseShortfall) -> String {
        switch shortfall {
        case .noPlannedSource: "no planned source"
        case .noResolvedDemand: "no resolved demand"
        case .localLimit: "local limit reached"
        }
    }
}
