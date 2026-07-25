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
