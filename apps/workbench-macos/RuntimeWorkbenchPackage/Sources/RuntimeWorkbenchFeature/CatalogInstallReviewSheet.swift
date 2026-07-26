import SwiftUI

/// What a person sees before adding a napplet.
///
/// Adding acquires verified bytes and grants nothing -- the runtime asks for
/// capability at first run, not here. So this surface is deliberately light:
/// it says what the napplet is, who made it, and what it will ask for later,
/// and it puts every hash, coordinate and provenance record behind one
/// deliberate move. See `docs/adr/0008-verdicts-on-the-path.md`.
struct CatalogInstallReviewSheet: View {
    let review: CatalogInstallReview
    let isInstalling: Bool
    let issue: CatalogIssue?
    let onCancel: () -> Void
    let onConfirm: () -> Void

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: NappletMetrics.roomy) {
                    NappletHeading(title: review.title, subtitle: byline)

                    NappletNotice(verdict: verdict)

                    capabilities

                    reassurance

                    if let issue {
                        NappletNotice(
                            verdict: .caution("\(issue.title). \(issue.message)")
                        )
                    }

                    NappletEvidence {
                        CatalogInstallEvidence(review: review)
                    }
                }
                .padding(NappletMetrics.roomy)
            }
            .navigationTitle("Add Napplet")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", action: onCancel)
                        .keyboardShortcut(.cancelAction)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(
                        isInstalling ? "Adding…" : "Add Napplet",
                        action: onConfirm
                    )
                    .keyboardShortcut(.defaultAction)
                    .disabled(!review.canInstall || isInstalling)
                    .accessibilityIdentifier("catalog-install-exact-build")
                    .accessibilityHint(
                        "Adds this napplet. It cannot do anything until you open it."
                    )
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 520, idealWidth: 580, minHeight: 480, idealHeight: 620)
        #endif
        .interactiveDismissDisabled(isInstalling)
    }

    private var byline: String {
        NappletIdentityPresentation.publisherIsUnnamed(
            displayName: review.publisher.displayName,
            publicKey: review.publisher.publicKey
        )
            ? "From a publisher who hasn't given a name"
            : "by " + NappletIdentityPresentation.publisherName(
                displayName: review.publisher.displayName,
                publicKey: review.publisher.publicKey
            )
    }

    /// The one thing a person is actually deciding about.
    @ViewBuilder
    private var capabilities: some View {
        if review.requiredDomains.isEmpty, review.optionalDomains.isEmpty {
            NappletCard {
                Label(
                    "This napplet doesn't ask for access to anything.",
                    systemImage: "checkmark"
                )
                .font(.callout)
            }
        } else {
            NappletCard {
                VStack(alignment: .leading, spacing: NappletMetrics.comfortable) {
                    Text("What it will ask for")
                        .font(.headline)

                    if !review.requiredDomains.isEmpty {
                        capabilityList(review.requiredDomains)
                    }

                    if !review.optionalDomains.isEmpty {
                        VStack(alignment: .leading, spacing: NappletMetrics.tight) {
                            Text("Only if you say yes")
                                .font(.subheadline.weight(.medium))
                                .foregroundStyle(.secondary)
                            capabilityList(review.optionalDomains)
                        }
                    }
                }
            }
        }
    }

    private func capabilityList(_ domains: [String]) -> some View {
        VStack(alignment: .leading, spacing: NappletMetrics.snug) {
            ForEach(domains, id: \.self) { domain in
                let phrase = NappletVocabulary.phrase(forDomain: domain)
                Label {
                    Text(phrase.sentence)
                        .fixedSize(horizontal: false, vertical: true)
                } icon: {
                    Image(systemName: phrase.symbol)
                        .foregroundStyle(.secondary)
                }
                .font(.callout)
                .accessibilityElement(children: .combine)
            }
        }
    }

    private var reassurance: some View {
        Text(
            "You choose what it can do the first time you open it. "
                + "Adding it now gives it access to nothing."
        )
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }

    /// Verdicts only, and only when there is something to say. Rust owns
    /// whether an install may proceed (`canInstall`) and which warnings are
    /// blocking; this reads those decisions rather than re-deriving them.
    private var verdict: NappletTrustVerdict {
        if
            let blocking = review.warnings.first(where: {
                $0.severity == .blocking
            })
        {
            return .blocked(blocking.message)
        }
        if let incompatible = currentPlatformIncompatibility {
            return .blocked(incompatible)
        }
        if !review.canInstall {
            return .blocked("This napplet can't be added right now.")
        }
        if
            let caution = review.warnings.first(where: {
                $0.severity == .caution
            })
        {
            return .caution(caution.message)
        }
        return relationshipVerdict
    }

    private var relationshipVerdict: NappletTrustVerdict {
        switch review.updateRelationship {
        case .sameBuild:
            .caution("You already have this napplet.")
        case .rollback:
            .caution("This is an older version than the one you already have.")
        case .differentBuild:
            .caution(
                "You already have a different version of this napplet. "
                    + "Adding this one replaces it."
            )
        case .update, .firstInstall, .unknown:
            .settled
        }
    }

    private var currentPlatformIncompatibility: String? {
        #if os(macOS)
        let current = "macos"
        let device = "Mac"
        #else
        let current = "ios"
        let device = "iPhone"
        #endif
        guard
            let row = review.platformCompatibility.first(where: { row in
                row.platform
                    .lowercased()
                    .replacingOccurrences(of: " ", with: "") == current
                    && row.status == .incompatible
            })
        else {
            return nil
        }
        return row.detail.isEmpty
            ? "This napplet doesn't run on \(device)."
            : "This napplet doesn't run on \(device). \(row.detail)"
    }
}

/// Everything the runtime verified, verbatim, for the person who asked.
///
/// Nothing here is truncated or summarised on purpose: this is the tier that
/// lets the plain one be confident.
private struct CatalogInstallEvidence: View {
    let review: CatalogInstallReview

    var body: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.comfortable) {
            NappletFieldGrid(fields: identityFields)

            if !review.requiredDomains.isEmpty || !review.optionalDomains.isEmpty {
                evidenceSection("Capability domains") {
                    NappletFieldGrid(fields: [
                        NappletField(
                            "Required",
                            review.requiredDomains.isEmpty
                                ? "none"
                                : review.requiredDomains.joined(separator: ", ")
                        ),
                        NappletField(
                            "Optional",
                            review.optionalDomains.isEmpty
                                ? "none"
                                : review.optionalDomains.joined(separator: ", ")
                        ),
                    ])
                }
            }

            if !review.sources.isEmpty {
                evidenceSection("Sources and provenance") {
                    VStack(alignment: .leading, spacing: NappletMetrics.snug) {
                        ForEach(review.sources) { source in
                            VStack(alignment: .leading, spacing: 2) {
                                Text(source.kind.rawValue)
                                    .font(.caption.weight(.semibold))
                                Text(source.source)
                                    .font(.caption.monospaced())
                                Text(source.evidence)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            .accessibilityElement(children: .combine)
                        }
                    }
                }
            }

            if !review.platformCompatibility.isEmpty {
                evidenceSection("Platform compatibility") {
                    NappletFieldGrid(
                        fields: review.platformCompatibility.map { row in
                            NappletField(
                                row.platform,
                                "\(statusWord(row.status)) — \(row.detail)"
                            )
                        }
                    )
                }
            }

            if !review.warnings.isEmpty {
                evidenceSection("Warnings") {
                    NappletFieldGrid(
                        fields: review.warnings.map { warning in
                            NappletField(
                                severityWord(warning.severity),
                                warning.message
                            )
                        }
                    )
                }
            }
        }
    }

    private var identityFields: [NappletField] {
        var fields = [
            NappletField("Publisher key", review.publisher.publicKey),
            NappletField("Coordinate", review.coordinate),
            NappletField("Aggregate hash", review.exactAggregateHash),
            NappletField("Relationship", review.updateRelationship.title),
        ]
        if let installedHash = review.updateRelationship.installedHash {
            fields.append(NappletField("Installed hash", installedHash))
        }
        if let detail = review.updateRelationship.detail {
            fields.append(NappletField("Relationship detail", detail))
        }
        return fields
    }

    private func evidenceSection(
        _ title: String,
        @ViewBuilder content: () -> some View
    ) -> some View {
        VStack(alignment: .leading, spacing: NappletMetrics.tight) {
            Text(title)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            content()
        }
    }

    private func statusWord(_ status: CatalogPlatformStatus) -> String {
        switch status {
        case .compatible: "compatible"
        case .incompatible: "incompatible"
        case .unavailable: "unavailable"
        }
    }

    private func severityWord(_ severity: CatalogWarningSeverity) -> String {
        switch severity {
        case .information: "info"
        case .caution: "caution"
        case .blocking: "blocking"
        }
    }
}
