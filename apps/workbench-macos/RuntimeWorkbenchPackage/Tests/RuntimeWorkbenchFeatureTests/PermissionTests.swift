@testable import RuntimeWorkbenchFeature
import Testing

@MainActor
@Test func cancelDiscardsFormStateWithoutCallingPermissionManager() {
    let manager = RecordingPermissionManager(snapshot: permissionSnapshot())
    let model = PermissionReviewSheetModel(manager: manager)
    let identity = model.review.capabilities[0]

    model.select(.allowExactBuild, for: identity)
    #expect(model.selection(for: identity) == .allowExactBuild)

    model.cancel()

    #expect(model.selection(for: identity) == identity.requestedDecision)
    #expect(manager.submissions.isEmpty)
}

@MainActor
@Test func confirmationSendsOneExactBuildScopedBatchAndNeverLaunches() async {
    let initial = permissionSnapshot()
    let manager = RecordingPermissionManager(snapshot: initial)
    manager.response = PermissionReviewSnapshot(
        review: initial.review,
        submissionState: .applied
    )
    let model = PermissionReviewSheetModel(manager: manager)

    model.select(.allowExactBuild, for: model.review.capabilities[0])
    model.select(.allowSession, for: model.review.capabilities[1])
    await model.confirm()

    #expect(manager.submissions.count == 1)
    #expect(manager.submissions.first?.principal == initial.review.principal)
    #expect(
        manager.submissions.first?.decisions == [
            PermissionDecisionSelection(
                domain: "identity",
                decision: .allowExactBuild
            )!,
            PermissionDecisionSelection(
                domain: "outbox",
                decision: .allowSession
            )!,
        ]
    )
    #expect(manager.actions == [.submit])
    #expect(model.isApplied)
}

@MainActor
@Test func dependencyRefusalIsRenderedFromManagerOwnedErrorState() async {
    let initial = permissionSnapshot()
    let manager = RecordingPermissionManager(snapshot: initial)
    let refusal = PermissionReviewIssue(
        title: "Required dependency denied",
        message: "identity requires outbox for this verified build.",
        affectedDomains: ["identity", "outbox"]
    )!
    manager.response = PermissionReviewSnapshot(
        review: initial.review,
        submissionState: .refused(refusal)
    )
    let model = PermissionReviewSheetModel(manager: manager)

    model.select(.allowExactBuild, for: model.review.capabilities[0])
    model.select(.deny, for: model.review.capabilities[1])
    await model.confirm()

    #expect(manager.submissions.count == 1)
    #expect(model.issue == refusal)
    #expect(!model.isApplied)
    #expect(model.canConfirm)
}

@MainActor
@Test func invalidPlatformDecisionIsRefusedBeforeSubmission() async {
    let initial = unavailablePermissionSnapshot()
    let manager = RecordingPermissionManager(snapshot: initial)
    let model = PermissionReviewSheetModel(manager: manager)
    let resource = model.review.capabilities[0]

    model.select(.allowExactBuild, for: resource)
    await model.confirm()

    #expect(model.selection(for: resource) == .deny)
    #expect(model.issue?.title == "Decision unavailable")
    #expect(manager.submissions.isEmpty)
}

@MainActor
@Test func confirmDismissesImmediatelyWhenNoCapabilitiesAreRequested() async {
    let initial = noCapabilitiesPermissionSnapshot()
    let manager = RecordingPermissionManager(snapshot: initial)
    let model = PermissionReviewSheetModel(manager: manager)

    #expect(model.review.capabilities.isEmpty)
    #expect(model.canConfirm)

    await model.confirm()

    #expect(manager.submissions.isEmpty)
    #expect(model.isApplied)
}

@MainActor
@Test func aReviewMixingManagedAndDecidableCapabilitiesStaysConfirmable()
    async
{
    let initial = mixedManagedPermissionSnapshot()
    let manager = RecordingPermissionManager(snapshot: initial)
    manager.response = PermissionReviewSnapshot(
        review: initial.review,
        submissionState: .applied
    )
    let model = PermissionReviewSheetModel(manager: manager)

    #expect(model.review.capabilities.count == 2)
    #expect(model.decidableCapabilities.map(\.domain) == ["outbox"])
    #expect(model.managedCapabilities.map(\.domain) == ["identity"])
    #expect(model.canConfirm)

    await model.confirm()

    // The host-managed capability is Rust's, so it is absent from the batch
    // rather than counted toward its completeness. Counting it made every
    // mixed review permanently unconfirmable.
    #expect(
        manager.submissions.first?.decisions == [
            PermissionDecisionSelection(
                domain: "outbox",
                decision: .askEveryTime
            )!,
        ]
    )
    #expect(model.isApplied)
}

@MainActor
@Test func allowingTheRecommendedChoiceSubmitsRustsOwnRecommendation() async {
    let initial = permissionSnapshot()
    let manager = RecordingPermissionManager(snapshot: initial)
    manager.response = PermissionReviewSnapshot(
        review: initial.review,
        submissionState: .applied
    )
    let model = PermissionReviewSheetModel(manager: manager)

    await model.allowRecommended()

    // Every submitted decision is the `recommendedDecision` Rust projected --
    // the sheet never invents or ranks one of its own.
    #expect(
        manager.submissions.first?.decisions == [
            PermissionDecisionSelection(
                domain: "identity",
                decision: .allowExactBuild
            )!,
            PermissionDecisionSelection(
                domain: "outbox",
                decision: .allowExactBuild
            )!,
        ]
    )
    #expect(model.isApplied)
}

@MainActor
@Test func allowingTheRecommendedChoiceNeverWidensBeyondAnOfferedOption()
    async
{
    // `resource` recommends `.deny` and offers nothing broader, so the one
    // gesture must leave it denied rather than reaching for a wider grant.
    let initial = unavailablePermissionSnapshot()
    let manager = RecordingPermissionManager(snapshot: initial)
    manager.response = PermissionReviewSnapshot(
        review: initial.review,
        submissionState: .applied
    )
    let model = PermissionReviewSheetModel(manager: manager)

    await model.allowRecommended()

    #expect(
        manager.submissions.first?.decisions == [
            PermissionDecisionSelection(domain: "resource", decision: .deny)!,
        ]
    )
}

@MainActor
@Test func sensitiveCapabilitiesArePresentedWhereTheyWillBeRead() {
    let manager = RecordingPermissionManager(
        snapshot: orderingPermissionSnapshot()
    )
    let model = PermissionReviewSheetModel(manager: manager)

    // Rust owns sensitivity and requirement; the sheet only orders by them.
    #expect(
        model.orderedCapabilities.map(\.domain) == ["outbox", "theme", "link"]
    )
    #expect(model.requiredCapabilities.map(\.domain) == ["outbox", "theme"])
    #expect(model.optionalCapabilities.map(\.domain) == ["link"])
}

@MainActor
@Test func permissionReviewSheetBuildsWithInjectedManagerOnly() {
    let manager = RecordingPermissionManager(snapshot: permissionSnapshot())
    let view = PermissionReviewSheet(manager: manager)

    #expect(String(describing: type(of: view)) == "PermissionReviewSheet")
}

@MainActor
@Test func workbenchAcceptsAnInjectedPermissionManager() {
    let manager = RecordingPermissionManager(snapshot: permissionSnapshot())
    let view = ContentView(permissionManager: manager)

    #expect(String(describing: type(of: view)) == "ContentView")
    #expect(manager.submissions.isEmpty)
}

@MainActor
private final class RecordingPermissionManager: PermissionReviewManaging {
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

private func permissionPrincipal(hash: Character = "b")
    -> PermissionExactBuildPrincipal
{
    PermissionExactBuildPrincipal(
        manifestAuthorPublicKey: String(repeating: "a", count: 64),
        dTag: "good-morning",
        aggregateHash: String(repeating: hash, count: 64)
    )!
}

private func validOptions(
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

private func permissionSnapshot() -> PermissionReviewSnapshot {
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
        existingDecision: .askEveryTime,
        isGranted: false,
        requestedDecision: .askEveryTime,
        recommendedDecision: .allowExactBuild,
        decisionOptions: validOptions()
    )!
    let review = PermissionReview(
        principal: permissionPrincipal(),
        publisherDisplayName: "Alice",
        nappletTitle: "Good Morning",
        capabilities: [identity, outbox]
    )!
    return PermissionReviewSnapshot(review: review)
}

private func noCapabilitiesPermissionSnapshot() -> PermissionReviewSnapshot {
    let review = PermissionReview(
        principal: permissionPrincipal(hash: "d"),
        publisherDisplayName: nil,
        nappletTitle: "Good Morning",
        capabilities: []
    )!
    return PermissionReviewSnapshot(review: review)
}

/// One host-managed capability the user cannot decide, alongside one they can.
private func mixedManagedPermissionSnapshot() -> PermissionReviewSnapshot {
    let managed = PermissionCapabilityReview(
        domain: "identity",
        title: "Identity",
        requirement: .required,
        sensitivity: .sensitive,
        rationale: "Reads the active public key.",
        dependencies: [],
        platformAvailability: .available,
        existingDecision: .managed,
        isGranted: true,
        requestedDecision: nil,
        recommendedDecision: nil,
        decisionOptions: PermissionRequestedDecision.allCases.map { decision in
            PermissionDecisionOption(
                decision: decision,
                isValid: false,
                invalidReason: "This capability is managed by host policy."
            )!
        }
    )!
    let decidable = PermissionCapabilityReview(
        domain: "outbox",
        title: "Outbox",
        requirement: .required,
        sensitivity: .sensitive,
        rationale: "Publishes approved replies through NMP.",
        dependencies: [],
        platformAvailability: .available,
        existingDecision: .askEveryTime,
        isGranted: false,
        requestedDecision: .askEveryTime,
        recommendedDecision: .askEveryTime,
        decisionOptions: validOptions()
    )!
    let review = PermissionReview(
        principal: permissionPrincipal(hash: "e"),
        publisherDisplayName: "Alice",
        nappletTitle: "Good Morning",
        capabilities: [managed, decidable]
    )!
    return PermissionReviewSnapshot(review: review)
}

/// Declared least-attention-first so that ordering cannot pass by accident.
private func orderingPermissionSnapshot() -> PermissionReviewSnapshot {
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
            existingDecision: .askEveryTime,
            isGranted: false,
            requestedDecision: .askEveryTime,
            recommendedDecision: .askEveryTime,
            decisionOptions: validOptions()
        )!
    }
    let review = PermissionReview(
        principal: permissionPrincipal(hash: "f"),
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
        ]
    )!
    return PermissionReviewSnapshot(review: review)
}

private func unavailablePermissionSnapshot() -> PermissionReviewSnapshot {
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
        publisherDisplayName: nil,
        nappletTitle: "Good Morning",
        capabilities: [resource]
    )!
    return PermissionReviewSnapshot(review: review)
}
