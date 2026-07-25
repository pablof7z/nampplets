import SwiftUI

struct CatalogInstallReviewSheet: View {
    let review: CatalogInstallReview
    let isInstalling: Bool
    let issue: CatalogIssue?
    let onCancel: () -> Void
    let onConfirm: () -> Void

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    reviewIdentity
                    Divider()
                    sources
                    Divider()
                    capabilities
                    Divider()
                    compatibility
                    Divider()
                    updateRelationship

                    if !review.warnings.isEmpty {
                        Divider()
                        warnings
                    }

                    if let issue {
                        Divider()
                        CatalogIssueView(issue: issue)
                    }

                    Text(
                        "Installing does not launch this napplet or grant any capability."
                    )
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .accessibilityLabel(
                        "Installing does not launch the napplet or grant capabilities"
                    )
                }
                .padding()
            }
            .navigationTitle("Review \(review.title)")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", action: onCancel)
                        .keyboardShortcut(.cancelAction)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Install Exact Build", action: onConfirm)
                        .keyboardShortcut(.defaultAction)
                        .disabled(!review.canInstall || isInstalling)
                        .accessibilityIdentifier("catalog-install-exact-build")
                        .accessibilityHint(
                            "Installs only the hash shown in this review"
                        )
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 680, idealWidth: 760, minHeight: 560, idealHeight: 720)
        #endif
        .interactiveDismissDisabled(isInstalling)
    }

    private var reviewIdentity: some View {
        GroupBox("Verified build") {
            VStack(alignment: .leading, spacing: 8) {
                LabeledContent("Publisher", value: review.publisher.visibleName)
                LabeledContent("Public key", value: review.publisher.publicKey)
                LabeledContent("Coordinate", value: review.coordinate)
                LabeledContent("Exact hash", value: review.exactAggregateHash)
            }
            .font(.body)
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var sources: some View {
        CatalogReviewSection(title: "Sources and provenance") {
            if review.sources.isEmpty {
                Text("No source provenance was supplied.")
                    .foregroundStyle(.secondary)
            } else {
                ForEach(review.sources) { source in
                    VStack(alignment: .leading, spacing: 3) {
                        Text(source.kind.rawValue)
                            .font(.headline)
                        Text(source.source)
                            .font(.body.monospaced())
                            .textSelection(.enabled)
                        Text(source.evidence)
                            .foregroundStyle(.secondary)
                    }
                    .accessibilityElement(children: .combine)
                }
            }
        }
    }

    private var capabilities: some View {
        CatalogReviewSection(title: "Capabilities") {
            domainGroup(title: "Required", domains: review.requiredDomains)
            domainGroup(title: "Optional", domains: review.optionalDomains)
        }
    }

    private func domainGroup(title: String, domains: [String]) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .font(.headline)
            if domains.isEmpty {
                Text("None")
                    .foregroundStyle(.secondary)
            } else {
                Text(domains.joined(separator: ", "))
                    .textSelection(.enabled)
            }
        }
    }

    private var compatibility: some View {
        CatalogReviewSection(title: "Platform compatibility") {
            if review.platformCompatibility.isEmpty {
                Text("No platform compatibility evidence was projected.")
                    .foregroundStyle(.secondary)
            } else {
                ForEach(review.platformCompatibility) { platform in
                    HStack(alignment: .firstTextBaseline) {
                        Image(systemName: platformSymbol(platform.status))
                            .foregroundStyle(platformColor(platform.status))
                        Text(platform.platform)
                            .font(.headline)
                        Text(platform.detail)
                            .foregroundStyle(.secondary)
                        Spacer()
                    }
                    .accessibilityElement(children: .combine)
                }
            }
        }
    }

    private var updateRelationship: some View {
        CatalogReviewSection(title: "Install relationship") {
            Text(review.updateRelationship.title)
                .font(.headline)
            if let detail = review.updateRelationship.detail {
                Text(detail)
                    .foregroundStyle(.secondary)
            }
            if let installedHash = review.updateRelationship.installedHash {
                LabeledContent("Installed hash", value: installedHash)
                    .textSelection(.enabled)
            }
        }
    }

    private var warnings: some View {
        CatalogReviewSection(title: "Warnings") {
            ForEach(review.warnings) { warning in
                Label(warning.message, systemImage: warningSymbol(warning.severity))
                    .foregroundStyle(warningColor(warning.severity))
            }
        }
    }

    private func platformSymbol(_ status: CatalogPlatformStatus) -> String {
        switch status {
        case .compatible:
            "checkmark.circle"
        case .incompatible:
            "xmark.circle"
        case .unavailable:
            "questionmark.circle"
        }
    }

    private func platformColor(_ status: CatalogPlatformStatus) -> Color {
        switch status {
        case .compatible:
            .green
        case .incompatible:
            .red
        case .unavailable:
            .orange
        }
    }

    private func warningSymbol(_ severity: CatalogWarningSeverity) -> String {
        switch severity {
        case .information:
            "info.circle"
        case .caution:
            "exclamationmark.triangle"
        case .blocking:
            "xmark.octagon"
        }
    }

    private func warningColor(_ severity: CatalogWarningSeverity) -> Color {
        switch severity {
        case .information:
            .secondary
        case .caution:
            .orange
        case .blocking:
            .red
        }
    }
}

private struct CatalogReviewSection<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title)
                .font(.title3.bold())
            content
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
