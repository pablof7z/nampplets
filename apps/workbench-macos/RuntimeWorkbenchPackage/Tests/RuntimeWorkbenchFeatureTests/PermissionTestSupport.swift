@testable import RuntimeWorkbenchFeature

final class RecordingPermissionManager: PermissionReviewManaging {
    enum Action: Equatable {
        case submit
    }

    private var currentSnapshot: PermissionReviewSnapshot
    var response: PermissionReviewSnapshot?
    private(set) var submissions: [PermissionDecisionBatch] = []
    private(set) var actions: [Action] = []

    init(snapshot: PermissionReviewSnapshot) {
        currentSnapshot = snapshot
    }

    func snapshot() -> PermissionReviewSnapshot {
        currentSnapshot
    }

    func submit(_ batch: PermissionDecisionBatch) async {
        submissions.append(batch)
        actions.append(.submit)
        if let response {
            currentSnapshot = response
        }
    }
}

func permissionPrincipal(hash: Character = "b")
    -> PermissionExactBuildPrincipal
{
    PermissionExactBuildPrincipal(
        manifestAuthorPublicKey: String(repeating: "a", count: 64),
        dTag: "good-morning",
        aggregateHash: String(repeating: hash, count: 64)
    )!
}

/// Rust derives a review's revision as a SHA-256 over the whole effective
/// review: the principal, and every capability's requirement, sensitivity,
/// platform availability, controller, decisions and decision options. Native
/// code never recomputes it -- it treats the revision as an opaque
/// optimistic-concurrency token, echoes it back on the decision batch, and
/// Rust refuses the batch as stale when the live review no longer hashes to
/// it. These fixtures therefore only need one distinct, well-formed token per
/// distinct review, which is what this stand-in produces.
func permissionRevision(_ digit: Character) -> String {
    String(repeating: digit, count: 64)
}

/// The same review at a different revision: what Rust hands back with a
/// `StaleReview` refusal, where effective policy moved under an open review.
/// A validation refusal returns the review at its *unchanged* revision.
func permissionReview(
    _ review: PermissionReview,
    atRevision digit: Character
) -> PermissionReview {
    PermissionReview(
        principal: review.principal,
        revision: permissionRevision(digit),
        publisherDisplayName: review.publisherDisplayName,
        nappletTitle: review.nappletTitle,
        capabilities: review.capabilities,
        isReadOnly: review.isReadOnly
    )!
}

func validOptions(
    unavailable: Set<PermissionRequestedDecision> = []
) -> [PermissionDecisionOption] {
    PermissionRequestedDecision.allCases.map { decision in
        if unavailable.contains(decision) {
            PermissionDecisionOption(
                decision: decision,
                isValid: false,
                invalidReason: "This decision is unavailable on the current platform."
            )!
        } else {
            PermissionDecisionOption(
                decision: decision,
                isValid: true
            )!
        }
    }
}

func permissionSnapshot() -> PermissionReviewSnapshot {
    let identity = PermissionCapabilityReview(
        domain: "identity",
        title: "Identity",
        requirement: .required,
        sensitivity: .sensitive,
        rationale: "Reads the active public key and follow list.",
        dependencies: [
            PermissionCapabilityDependency(
                domain: "outbox",
                reason: "Routes identity reads through author relay policy."
            )!
        ],
        platformAvailability: .available,
        controller: .user,
        existingDecision: .denied,
        isGranted: false,
        requestedDecision: .askEveryTime,
        recommendedDecision: .allowExactBuild,
        decisionOptions: validOptions()
    )!
    let outbox = PermissionCapabilityReview(
        domain: "outbox",
        title: "Outbox",
        requirement: .required,
        sensitivity: .sensitive,
        rationale: "Publishes approved replies through NMP.",
        dependencies: [],
        platformAvailability: .available,
        controller: .user,
        existingDecision: .askEveryTime,
        isGranted: false,
        requestedDecision: .askEveryTime,
        recommendedDecision: .allowExactBuild,
        decisionOptions: validOptions()
    )!
    let review = PermissionReview(
        principal: permissionPrincipal(),
        revision: permissionRevision("1"),
        publisherDisplayName: "Alice",
        nappletTitle: "Good Morning",
        capabilities: [identity, outbox],
        // Every capability here is user-controlled, so the review offers the
        // user something to decide and is not read-only.
        isReadOnly: false
    )!
    return PermissionReviewSnapshot(review: review)
}

func noCapabilitiesPermissionSnapshot() -> PermissionReviewSnapshot {
    let review = PermissionReview(
        principal: permissionPrincipal(hash: "d"),
        revision: permissionRevision("3"),
        publisherDisplayName: nil,
        nappletTitle: "Good Morning",
        capabilities: [],
        // `isReadOnly` means "no capability in this review is the user's to
        // decide". Rust computes it as `capabilities.iter().all(host policy)`,
        // which is vacuously true for an empty review, and the Swift model
        // enforces the same equality. A napplet that requests nothing
        // therefore presents a review with nothing to decide.
        isReadOnly: true
    )!
    return PermissionReviewSnapshot(review: review)
}

func mixedManagedPermissionSnapshot() -> PermissionReviewSnapshot {
    let managed = managedPermissionCapability(isGranted: true)
    let decidable = PermissionCapabilityReview(
        domain: "outbox",
        title: "Outbox",
        requirement: .required,
        sensitivity: .sensitive,
        rationale: "Publishes approved replies through NMP.",
        dependencies: [],
        platformAvailability: .available,
        controller: .user,
        existingDecision: .askEveryTime,
        isGranted: false,
        requestedDecision: .askEveryTime,
        recommendedDecision: .askEveryTime,
        decisionOptions: validOptions()
    )!
    let review = PermissionReview(
        principal: permissionPrincipal(hash: "e"),
        revision: permissionRevision("4"),
        publisherDisplayName: "Alice",
        nappletTitle: "Good Morning",
        capabilities: [managed, decidable],
        // Mixed: `outbox` is still the user's to decide, so the review as a
        // whole is not read-only even though `identity` is host-managed.
        isReadOnly: false
    )!
    return PermissionReviewSnapshot(review: review)
}

func managedPermissionCapability(
    isGranted: Bool
) -> PermissionCapabilityReview {
    // `controller` names who owns this capability's decision. Rust projects
    // `.hostPolicy` exactly when the decision in force is `Managed`, and
    // reuses one reason string for both the controller and every locked
    // option, so the fixture does the same.
    let reason = "This capability is managed by host policy."
    return PermissionCapabilityReview(
        domain: "identity",
        title: "Identity",
        requirement: .required,
        sensitivity: .sensitive,
        rationale: "Reads the active public key.",
        dependencies: [],
        platformAvailability: .available,
        controller: .hostPolicy(reason: reason),
        existingDecision: .managed,
        isGranted: isGranted,
        requestedDecision: nil,
        recommendedDecision: nil,
        decisionOptions: PermissionRequestedDecision.allCases.map { decision in
            PermissionDecisionOption(
                decision: decision,
                isValid: false,
                invalidReason: reason
            )!
        }
    )!
}

func orderingPermissionSnapshot() -> PermissionReviewSnapshot {
    func capability(
        domain: String,
        requirement: PermissionCapabilityRequirement,
        sensitivity: PermissionCapabilitySensitivity
    ) -> PermissionCapabilityReview {
        PermissionCapabilityReview(
            domain: domain,
            title: domain.capitalized,
            requirement: requirement,
            sensitivity: sensitivity,
            rationale: "Rationale for \(domain).",
            dependencies: [],
            platformAvailability: .available,
            controller: .user,
            existingDecision: .askEveryTime,
            isGranted: false,
            requestedDecision: .askEveryTime,
            recommendedDecision: .askEveryTime,
            decisionOptions: validOptions()
        )!
    }
    let review = PermissionReview(
        principal: permissionPrincipal(hash: "f"),
        revision: permissionRevision("5"),
        publisherDisplayName: "Alice",
        nappletTitle: "Good Morning",
        capabilities: [
            capability(
                domain: "link",
                requirement: .optional,
                sensitivity: .ordinary
            ),
            capability(
                domain: "theme",
                requirement: .required,
                sensitivity: .ordinary
            ),
            capability(
                domain: "outbox",
                requirement: .required,
                sensitivity: .sensitive
            ),
        ],
        isReadOnly: false
    )!
    return PermissionReviewSnapshot(review: review)
}

func unavailablePermissionSnapshot() -> PermissionReviewSnapshot {
    let resource = PermissionCapabilityReview(
        domain: "resource",
        title: "Resource",
        requirement: .optional,
        sensitivity: .ordinary,
        rationale: "Loads bounded avatar resources.",
        dependencies: [],
        platformAvailability: .unavailable(
            reason: "No native resource executor is installed."
        ),
        // Platform unavailability narrows which decisions are offered; it does
        // not move ownership of the decision away from the user.
        controller: .user,
        existingDecision: .denied,
        isGranted: false,
        requestedDecision: .deny,
        recommendedDecision: .deny,
        decisionOptions: validOptions(
            unavailable: [.askEveryTime, .allowSession, .allowExactBuild]
        )
    )!
    let review = PermissionReview(
        principal: permissionPrincipal(hash: "c"),
        revision: permissionRevision("2"),
        publisherDisplayName: nil,
        nappletTitle: "Good Morning",
        capabilities: [resource],
        isReadOnly: false
    )!
    return PermissionReviewSnapshot(review: review)
}
