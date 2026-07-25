import SwiftUI

struct CatalogBrowseEvidenceView: View {
    let evidence: CatalogBrowseEvidence
    let hasMore: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            HStack {
                Label(scopeTitle, systemImage: scopeSymbol)
                    .font(.headline)
                Spacer()
                Text(
                    "\(evidence.projectedRows) candidates"
                )
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
            }

            Text(scopeDetail)
                .font(.caption)
                .foregroundStyle(.secondary)

            if evidence.scope == .liveNMPWindow {
                Text(windowDetail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            if evidence.locallyFilteredRows > 0 {
                Text(
                    "\(evidence.locallyFilteredRows) rows were excluded by "
                        + "the local filter."
                )
                .font(.caption)
                .foregroundStyle(.secondary)
            }

            if evidence.projectionLimitedRows > 0 {
                Text(
                    "\(evidence.projectionLimitedRows) matching rows were "
                        + "omitted by the bounded screen projection."
                )
                .font(.caption)
                .foregroundStyle(.orange)
            }

            if evidence.refusedRows > 0 {
                Text(
                    "\(evidence.refusedRows) malformed or oversized rows were refused."
                )
                .font(.caption)
                .foregroundStyle(.orange)
            }

            if hasMore {
                Label(
                    "More rows exist outside this projection; refine the local filter.",
                    systemImage: "ellipsis.circle"
                )
                .font(.caption)
                .foregroundStyle(.orange)
            }

            if !evidence.shortfalls.isEmpty {
                Text(evidence.shortfalls.map(shortfallTitle).joined(separator: " · "))
                    .font(.caption)
                    .foregroundStyle(.orange)
            }

            if !evidence.sourceEvidence.isEmpty {
                HStack(spacing: 12) {
                    ForEach(evidence.sourceEvidence.prefix(3)) { source in
                        Label(
                            "\(source.source) · \(accessTitle(source.access))",
                            systemImage: sourceSymbol(source.status)
                        )
                        .font(.caption)
                        .foregroundStyle(sourceColor(source.status))
                    }
                    if evidence.sourceEvidence.count > 3 {
                        Text("+\(evidence.sourceEvidence.count - 3) sources")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                .lineLimit(1)
                .help("Source-scoped evidence from the current NMP observation")
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 10)
        .background(.bar)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(
            "\(scopeTitle) · \(evidence.projectedRows) candidates · \(scopeDetail)"
        )
        .accessibilityValue(
            "\(scopeTitle) · \(evidence.projectedRows) candidates · \(scopeDetail)"
        )
        .accessibilityIdentifier("catalog-feed-evidence")
    }

    private var scopeTitle: String {
        switch evidence.scope {
        case .liveNMPWindow:
            "Live NMP catalog window"
        case .offlineFixture:
            "Offline UI-test catalog"
        }
    }

    private var scopeSymbol: String {
        switch evidence.scope {
        case .liveNMPWindow:
            "network"
        case .offlineFixture:
            "testtube.2"
        }
    }

    private var scopeDetail: String {
        switch evidence.scope {
        case .liveNMPWindow:
            "Source-scoped evidence only; this is not a globally complete network result."
        case .offlineFixture:
            "Deterministic bundled compatibility data; no network lookup is performed."
        }
    }

    private var windowDetail: String {
        switch evidence.window {
        case .idle:
            "The NMP window is idle."
        case .requesting:
            "The NMP window is requesting more rows."
        case let .returned(addedRows):
            "The NMP window added \(addedRows) rows."
        case let .atBound(maximumRows):
            "The NMP window reached its \(maximumRows)-row bound."
        case .unknown:
            "The NMP facade did not classify this bounded window state."
        }
    }

    private func shortfallTitle(_ shortfall: CatalogBrowseShortfall) -> String {
        switch shortfall {
        case .noPlannedSource:
            "No planned source"
        case .noResolvedDemand:
            "No resolved demand"
        case .localLimit:
            "Local limit reached"
        }
    }

    private func sourceSymbol(_ status: CatalogBrowseSourceStatus) -> String {
        switch status {
        case .requesting, .connecting:
            "arrow.trianglehead.2.clockwise"
        case .disconnected:
            "bolt.slash"
        case .awaitingAuthentication:
            "person.badge.clock"
        case .authenticationDenied:
            "person.badge.minus"
        case .error:
            "exclamationmark.triangle"
        }
    }

    private func accessTitle(_ access: CatalogBrowseAccessContext) -> String {
        switch access {
        case .public:
            "public"
        case .nip42:
            "NIP-42"
        }
    }

    private func sourceColor(_ status: CatalogBrowseSourceStatus) -> Color {
        switch status {
        case .requesting, .connecting, .awaitingAuthentication:
            .secondary
        case .disconnected, .authenticationDenied, .error:
            .orange
        }
    }
}
