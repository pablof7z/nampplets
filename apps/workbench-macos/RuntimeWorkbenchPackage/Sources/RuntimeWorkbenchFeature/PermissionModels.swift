import Foundation

public enum PermissionLimits {
    public static let maximumCapabilities = 64
    public static let maximumDependenciesPerCapability = 16
    public static let maximumAffectedDomainsPerIssue = 64
    public static let maximumDomainUTF8Bytes = 64
    public static let maximumDTagUTF8Bytes = 256
    public static let maximumDisplayTextUTF8Bytes = 16_384
    public static let maximumReviewUTF8Bytes = 256 * 1_024
}

/// The exact executable identity to which every permission decision is bound.
public struct PermissionExactBuildPrincipal: Hashable, Sendable {
    public let manifestAuthorPublicKey: String
    public let dTag: String
    public let aggregateHash: String

    public init?(
        manifestAuthorPublicKey: String,
        dTag: String,
        aggregateHash: String
    ) {
        guard
            Self.isLowercaseHexDigest(manifestAuthorPublicKey),
            !dTag.isEmpty,
            dTag.utf8.count <= PermissionLimits.maximumDTagUTF8Bytes,
            Self.isLowercaseHexDigest(aggregateHash)
        else {
            return nil
        }

        self.manifestAuthorPublicKey = manifestAuthorPublicKey
        self.dTag = dTag
        self.aggregateHash = aggregateHash
    }

    private static func isLowercaseHexDigest(_ value: String) -> Bool {
        value.utf8.count == 64
            && value.utf8.allSatisfy { byte in
                (48 ... 57).contains(byte) || (97 ... 102).contains(byte)
            }
    }
}

public enum PermissionCapabilityRequirement: String, Equatable, Sendable {
    case required
    case optional

    public var title: String {
        rawValue.capitalized
    }
}

public enum PermissionCapabilitySensitivity: String, Equatable, Sendable {
    case ordinary
    case sensitive
    case unknown

    public var title: String {
        rawValue.capitalized
    }
}

public struct PermissionCapabilityDependency: Identifiable, Equatable, Sendable {
    public let domain: String
    public let reason: String

    public var id: String {
        domain
    }

    public init?(domain: String, reason: String) {
        guard
            Self.isValidDomain(domain),
            reason.utf8.count <= PermissionLimits.maximumDisplayTextUTF8Bytes
        else {
            return nil
        }
        self.domain = domain
        self.reason = reason
    }

    fileprivate static func isValidDomain(_ domain: String) -> Bool {
        !domain.isEmpty
            && domain.utf8.count <= PermissionLimits.maximumDomainUTF8Bytes
            && domain.utf8.allSatisfy { byte in
                (48 ... 57).contains(byte)
                    || (97 ... 122).contains(byte)
                    || byte == 46
                    || byte == 45
                    || byte == 95
            }
    }
}

public enum PermissionPlatformAvailability: Equatable, Sendable {
    case available
    case unknown(reason: String)
    case unavailable(reason: String)

    public var title: String {
        switch self {
        case .available:
            "Available on this Mac"
        case .unknown:
            "Availability unknown"
        case .unavailable:
            "Unavailable on this Mac"
        }
    }

    public var detail: String? {
        switch self {
        case .available:
            nil
        case let .unknown(reason):
            reason
        case let .unavailable(reason):
            reason
        }
    }
}

public enum PermissionExistingDecision: String, Equatable, Sendable {
    case denied
    case askEveryTime
    case allowSession
    case allowExactBuild
    case managed

    public var title: String {
        switch self {
        case .denied:
            "Denied"
        case .askEveryTime:
            "Ask every time"
        case .allowSession:
            "Allowed for session"
        case .allowExactBuild:
            "Allowed for exact build"
        case .managed:
            "Managed by host"
        }
    }
}

public enum PermissionRequestedDecision:
    String,
    CaseIterable,
    Equatable,
    Hashable,
    Identifiable,
    Sendable
{
    case deny
    case askEveryTime
    case allowSession
    case allowExactBuild

    public var id: Self {
        self
    }

    public var title: String {
        switch self {
        case .deny:
            "Deny"
        case .askEveryTime:
            "Ask every time"
        case .allowSession:
            "Allow for this session"
        case .allowExactBuild:
            "Allow for this exact build"
        }
    }
}

/// A Rust-produced option projection. Invalid options remain visible so the
/// native sheet can explain why a broader decision is unavailable.
public struct PermissionDecisionOption: Identifiable, Equatable, Sendable {
    public let decision: PermissionRequestedDecision
    public let isValid: Bool
    public let invalidReason: String?

    public var id: PermissionRequestedDecision {
        decision
    }

    public init?(
        decision: PermissionRequestedDecision,
        isValid: Bool,
        invalidReason: String? = nil
    ) {
        guard
            (invalidReason?.utf8.count ?? 0)
                <= PermissionLimits.maximumDisplayTextUTF8Bytes,
            isValid == (invalidReason == nil)
        else {
            return nil
        }
        self.decision = decision
        self.isValid = isValid
        self.invalidReason = invalidReason
    }
}

public struct PermissionCapabilityReview: Identifiable, Equatable, Sendable {
    public let domain: String
    public let title: String
    public let requirement: PermissionCapabilityRequirement
    public let sensitivity: PermissionCapabilitySensitivity
    public let rationale: String
    public let dependencies: [PermissionCapabilityDependency]
    public let platformAvailability: PermissionPlatformAvailability
    public let existingDecision: PermissionExistingDecision
    /// Rust's own classification of the decision in force: true when this
    /// capability is already allowed without prompting. The native layer
    /// renders this value and never rebuilds it from decision names.
    public let isGranted: Bool
    /// The Rust-owned requested default, absent when host policy manages the
    /// capability and therefore offers no user-selectable decision.
    public let requestedDecision: PermissionRequestedDecision?
    /// The decision Rust recommends when the user accepts this capability
    /// without picking a scope. Absent for host-managed capabilities, which
    /// offer the user no decision at all. Native code never invents this
    /// preference by ordering `decisionOptions` itself.
    public let recommendedDecision: PermissionRequestedDecision?
    public let decisionOptions: [PermissionDecisionOption]

    public var id: String {
        domain
    }

    public init?(
        domain: String,
        title: String,
        requirement: PermissionCapabilityRequirement,
        sensitivity: PermissionCapabilitySensitivity,
        rationale: String,
        dependencies: [PermissionCapabilityDependency],
        platformAvailability: PermissionPlatformAvailability,
        existingDecision: PermissionExistingDecision,
        isGranted: Bool,
        requestedDecision: PermissionRequestedDecision?,
        recommendedDecision: PermissionRequestedDecision?,
        decisionOptions: [PermissionDecisionOption]
    ) {
        let validRequestedOption = requestedDecision.map { requested in
            decisionOptions.contains {
                $0.decision == requested && $0.isValid
            }
        } ?? (
            existingDecision == .managed
                && decisionOptions.allSatisfy { !$0.isValid }
        )
        let managedStateIsConsistent =
            (existingDecision == .managed) == (requestedDecision == nil)
            && (requestedDecision == nil) == (recommendedDecision == nil)
        let recommendationIsOffered = recommendedDecision.map { recommended in
            decisionOptions.contains {
                $0.decision == recommended && $0.isValid
            }
        } ?? true
        let lockedOptionsExplainWhy = requestedDecision != nil
            || decisionOptions.allSatisfy {
                !($0.invalidReason?.isEmpty ?? true)
            }
        let uniqueOptions = Set(decisionOptions.map(\.decision)).count
            == decisionOptions.count
        let uniqueDependencies = Set(dependencies.map(\.domain)).count
            == dependencies.count
        guard
            PermissionCapabilityDependency.isValidDomain(domain),
            title.utf8.count <= PermissionLimits.maximumDisplayTextUTF8Bytes,
            rationale.utf8.count <= PermissionLimits.maximumDisplayTextUTF8Bytes,
            dependencies.count
                <= PermissionLimits.maximumDependenciesPerCapability,
            uniqueDependencies,
            decisionOptions.count
                == PermissionRequestedDecision.allCases.count,
            uniqueOptions,
            validRequestedOption,
            managedStateIsConsistent,
            recommendationIsOffered,
            lockedOptionsExplainWhy,
            (platformAvailability.detail?.utf8.count ?? 0)
                <= PermissionLimits.maximumDisplayTextUTF8Bytes
        else {
            return nil
        }

        self.domain = domain
        self.title = title
        self.requirement = requirement
        self.sensitivity = sensitivity
        self.rationale = rationale
        self.dependencies = dependencies
        self.platformAvailability = platformAvailability
        self.existingDecision = existingDecision
        self.isGranted = isGranted
        self.requestedDecision = requestedDecision
        self.recommendedDecision = recommendedDecision
        self.decisionOptions = decisionOptions
    }

    public func option(
        for decision: PermissionRequestedDecision
    ) -> PermissionDecisionOption? {
        decisionOptions.first { $0.decision == decision }
    }
}

public struct PermissionReview: Equatable, Sendable {
    public let principal: PermissionExactBuildPrincipal
    public let publisherDisplayName: String?
    public let nappletTitle: String
    public let capabilities: [PermissionCapabilityReview]

    public init?(
        principal: PermissionExactBuildPrincipal,
        publisherDisplayName: String?,
        nappletTitle: String,
        capabilities: [PermissionCapabilityReview]
    ) {
        let uniqueDomains = Set(capabilities.map(\.domain)).count
            == capabilities.count
        let displayTexts = [
            publisherDisplayName ?? "",
            nappletTitle,
        ] + capabilities.flatMap {
            [
                $0.domain,
                $0.title,
                $0.rationale,
                $0.platformAvailability.detail ?? "",
            ] + $0.dependencies.flatMap { [$0.domain, $0.reason] }
                + $0.decisionOptions.compactMap(\.invalidReason)
        }
        guard
            capabilities.count <= PermissionLimits.maximumCapabilities,
            uniqueDomains,
            displayTexts.allSatisfy({
                $0.utf8.count <= PermissionLimits.maximumDisplayTextUTF8Bytes
            }),
            displayTexts.reduce(0, { $0 + $1.utf8.count })
                <= PermissionLimits.maximumReviewUTF8Bytes
        else {
            return nil
        }

        self.principal = principal
        self.publisherDisplayName = publisherDisplayName
        self.nappletTitle = nappletTitle
        self.capabilities = capabilities
    }
}

public struct PermissionDecisionSelection: Identifiable, Equatable, Sendable {
    public let domain: String
    public let decision: PermissionRequestedDecision

    public var id: String {
        domain
    }

    public init?(domain: String, decision: PermissionRequestedDecision) {
        guard PermissionCapabilityDependency.isValidDomain(domain) else {
            return nil
        }
        self.domain = domain
        self.decision = decision
    }
}

public struct PermissionDecisionBatch: Equatable, Sendable {
    public let principal: PermissionExactBuildPrincipal
    public let decisions: [PermissionDecisionSelection]

    public init?(
        principal: PermissionExactBuildPrincipal,
        decisions: [PermissionDecisionSelection]
    ) {
        guard
            !decisions.isEmpty,
            decisions.count <= PermissionLimits.maximumCapabilities,
            Set(decisions.map(\.domain)).count == decisions.count
        else {
            return nil
        }
        self.principal = principal
        self.decisions = decisions
    }
}

public struct PermissionReviewIssue: Equatable, Sendable {
    public let title: String
    public let message: String
    public let affectedDomains: [String]

    public init?(
        title: String,
        message: String,
        affectedDomains: [String] = []
    ) {
        guard
            title.utf8.count <= PermissionLimits.maximumDisplayTextUTF8Bytes,
            message.utf8.count <= PermissionLimits.maximumDisplayTextUTF8Bytes,
            affectedDomains.count
                <= PermissionLimits.maximumAffectedDomainsPerIssue,
            Set(affectedDomains).count == affectedDomains.count,
            affectedDomains.allSatisfy({
                PermissionCapabilityDependency.isValidDomain($0)
            })
        else {
            return nil
        }
        self.title = title
        self.message = message
        self.affectedDomains = affectedDomains
    }
}

public enum PermissionSubmissionState: Equatable, Sendable {
    case reviewing
    case applied
    case refused(PermissionReviewIssue)
}

public struct PermissionReviewSnapshot: Equatable, Sendable {
    public let review: PermissionReview
    public let submissionState: PermissionSubmissionState

    public init(
        review: PermissionReview,
        submissionState: PermissionSubmissionState = .reviewing
    ) {
        self.review = review
        self.submissionState = submissionState
    }
}
