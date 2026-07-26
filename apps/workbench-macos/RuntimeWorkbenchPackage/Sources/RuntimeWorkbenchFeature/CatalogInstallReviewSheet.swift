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
        VStack(spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    Text(review.title)
                        .font(NappletType.display)
                        .nappletDisplayFace()
                        .foregroundStyle(NappletInk.ink)
                        .fixedSize(horizontal: false, vertical: true)

                    Text(byline)
                        .font(NappletType.lede)
                        .foregroundStyle(NappletInk.inkSecondary)
                        .padding(.top, NappletMetrics.tight)

                    NappletNotice(verdict: verdict)
                        .padding(.top, NappletMetrics.roomy)

                    if let issue {
                        NappletNotice(
                            verdict: .caution("\(issue.title). \(issue.message)")
                        )
                        .padding(.top, NappletMetrics.snug)
                    }

                    capabilities
                        .padding(.top, NappletMetrics.spacious)

                    reassurance
                        .padding(.top, NappletMetrics.roomy)

                    NappletEvidence {
                        CatalogInstallEvidence(review: review)
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
        .background(NappletInk.paperRaised)
        #if os(macOS)
        .frame(minWidth: 560, idealWidth: 620, minHeight: 480, idealHeight: 660)
        #endif
        .interactiveDismissDisabled(isInstalling)
    }

    /// The accent appears here and nowhere else on this screen.
    private var actions: some View {
        VStack(spacing: 0) {
            Rectangle()
                .fill(NappletInk.rule)
                .frame(height: 1)
            HStack(spacing: NappletMetrics.snug) {
                Spacer()
                Button("Cancel", action: onCancel)
                    .keyboardShortcut(.cancelAction)
                Button(
                    isInstalling ? "Adding…" : "Add Napplet",
                    action: onConfirm
                )
                .buttonStyle(.borderedProminent)
                .tint(NappletInk.accent)
                .keyboardShortcut(.defaultAction)
                .disabled(!review.canInstall || isInstalling)
                .accessibilityIdentifier("catalog-install-exact-build")
                .accessibilityHint(
                    "Adds this napplet. It cannot do anything until you open it."
                )
            }
            .padding(.horizontal, NappletMetrics.generous)
            .padding(.vertical, NappletMetrics.comfortable)
        }
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
    /// The napplet's claim about itself, so it is set as a card. The group
    /// headings live on the page outside it: heading-card-heading-card nesting
    /// is the grouped-Form look this redesign exists to escape.
    @ViewBuilder
    private var capabilities: some View {
        if review.requiredDomains.isEmpty, review.optionalDomains.isEmpty {
            VStack(alignment: .leading, spacing: NappletMetrics.snug) {
                Text("What it will ask for")
                    .font(NappletType.heading)
                    .foregroundStyle(NappletInk.ink)
                Text("Nothing. This napplet doesn't ask for access to anything.")
                    .font(NappletType.secondary)
                    .foregroundStyle(NappletInk.inkSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        } else {
            VStack(alignment: .leading, spacing: NappletMetrics.snug) {
                Text("What it will ask for")
                    .font(NappletType.heading)
                    .foregroundStyle(NappletInk.ink)

                VStack(alignment: .leading, spacing: NappletMetrics.comfortable) {
                    if !review.requiredDomains.isEmpty {
                        capabilityList(review.requiredDomains)
                    }
                    if !review.optionalDomains.isEmpty {
                        VStack(alignment: .leading, spacing: NappletMetrics.snug) {
                            Text("Only if you say yes")
                                .font(NappletType.caption)
                                .foregroundStyle(NappletInk.inkSecondary)
                            capabilityList(review.optionalDomains)
                        }
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

    private func capabilityList(_ domains: [String]) -> some View {
        VStack(alignment: .leading, spacing: NappletMetrics.snug) {
            ForEach(domains, id: \.self) { domain in
                let phrase = NappletVocabulary.phrase(forDomain: domain)
                Label {
                    Text(phrase.sentence)
                        .foregroundStyle(NappletInk.ink)
                        .fixedSize(horizontal: false, vertical: true)
                } icon: {
                    Image(systemName: phrase.symbol)
                        .foregroundStyle(NappletInk.inkSecondary)
                }
                .font(NappletType.secondary)
                .accessibilityElement(children: .combine)
            }
        }
    }

    private var reassurance: some View {
        Text(
            "You choose what it can do the first time you open it. "
                + "Adding it now gives it access to nothing."
        )
        .font(NappletType.secondary)
        .foregroundStyle(NappletInk.inkSecondary)
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
