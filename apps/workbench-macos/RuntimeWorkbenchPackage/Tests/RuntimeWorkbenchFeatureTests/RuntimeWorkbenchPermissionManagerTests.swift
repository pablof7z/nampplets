import NMPNativeRuntimeApple
@testable import RuntimeWorkbenchFeature
import Testing

@MainActor
@Test func nativePermissionManagerProjectsUnknownProviderStateWithoutCoercion()
    throws
{
    let unknownReason =
        "no provider metadata is registered for this capability on this runtime"
    let native = RecordingNativePermissionService(
        review: nativeReview(
            capabilities: [
                nativeCapability(
                    domain: "outbox",
                    sensitivity: .unknown,
                    availability: .unknown(reason: unknownReason),
                    existing: .denied,
                    requested: .denied,
                    invalidDecisions: [
                        .askEveryTime,
                        .allowSession,
                        .allowExactBuild,
                    ],
                    invalidReason: unknownReason
                ),
            ]
        )
    )

    let manager = try RuntimeWorkbenchPermissionManager(
        native: native,
        principal: permissionManagerPrincipal()
    )
    let capability = try #require(
        manager.snapshot().review.capabilities.first
    )

    #expect(capability.sensitivity == .unknown)
    #expect(
        capability.platformAvailability == .unknown(reason: unknownReason)
    )
    #expect(capability.requestedDecision == .deny)
    #expect(capability.option(for: .deny)?.isValid == true)
    #expect(capability.option(for: .allowExactBuild)?.isValid == false)
}

@MainActor
@Test func grantStateAndRecommendationAreReadFromRustNotRederived() throws {
    let native = RecordingNativePermissionService(
        review: nativeReview(
            capabilities: [
                nativeCapability(
                    domain: "identity",
                    existing: .allowSession,
                    requested: .allowSession
                ),
                nativeCapability(
                    domain: "resource",
                    availability: .unavailable(
                        reason: "no native resource executor is installed"
                    ),
                    existing: .denied,
                    requested: .denied,
                    invalidDecisions: [
                        .askEveryTime,
                        .allowSession,
                        .allowExactBuild,
                    ],
                    invalidReason: "no native resource executor is installed"
                ),
            ]
        )
    )

    let manager = try RuntimeWorkbenchPermissionManager(
        native: native,
        principal: permissionManagerPrincipal()
    )
    let capabilities = manager.snapshot().review.capabilities
    let identity = try #require(capabilities.first)
    let resource = try #require(capabilities.last)

    #expect(identity.isGranted)
    #expect(identity.recommendedDecision == .allowExactBuild)
    #expect(!resource.isGranted)
    #expect(resource.recommendedDecision == .deny)
}

@MainActor
@Test func managedCapabilityIsExplicitlyLockedAndCannotProduceAPartialBatch()
    throws
{
    let reason = "this capability is managed by host policy"
    let native = RecordingNativePermissionService(
        review: nativeReview(
            capabilities: [
                nativeCapability(
                    domain: "identity",
                    sensitivity: .sensitive,
                    availability: .available,
                    existing: .managed,
                    requested: nil,
                    invalidDecisions: Set(
                        NativeRuntimeGrantDecision.allTestCases
                    ),
                    invalidReason: reason
                ),
            ]
        )
    )
    let manager = try RuntimeWorkbenchPermissionManager(
        native: native,
        principal: permissionManagerPrincipal()
    )
    let model = PermissionReviewSheetModel(manager: manager)
    let capability = try #require(model.review.capabilities.first)

    #expect(capability.existingDecision == .managed)
    #expect(capability.requestedDecision == nil)
    #expect(capability.isGranted)
    #expect(capability.recommendedDecision == nil)
    #expect(capability.decisionOptions.allSatisfy { !$0.isValid })
    #expect(model.selection(for: capability) == nil)
    #expect(!model.canConfirm)
}

@MainActor
@Test func nativePermissionManagerSubmitsOneCompleteExactBuildBatchAndDoesNotLaunch()
    async throws
{
    let initial = nativeReview(
        capabilities: [
            nativeCapability(domain: "identity"),
            nativeCapability(domain: "outbox"),
        ]
    )
    let applied = nativeReview(
        capabilities: [
            nativeCapability(
                domain: "identity",
                existing: .allowExactBuild,
                requested: .allowExactBuild
            ),
            nativeCapability(
                domain: "outbox",
                existing: .allowSession,
                requested: .allowSession
            ),
        ]
    )
    let native = RecordingNativePermissionService(review: initial)
    native.nextUpdate = NativeRuntimePermissionBatchUpdate(
        applied: true,
        review: applied,
        refusal: nil
    )
    let manager = try RuntimeWorkbenchPermissionManager(
        native: native,
        principal: permissionManagerPrincipal()
    )
    let batch = PermissionDecisionBatch(
        principal: permissionManagerPrincipal(),
        decisions: [
            PermissionDecisionSelection(
                domain: "identity",
                decision: .allowExactBuild
            )!,
            PermissionDecisionSelection(
                domain: "outbox",
                decision: .allowSession
            )!,
        ]
    )!

    await manager.submit(batch)

    #expect(native.batches.count == 1)
    #expect(native.batches[0].coordinate == nativeCoordinate())
    #expect(
        native.batches[0].decisions == [
            NativeRuntimePermissionDecisionSelection(
                domain: "identity",
                decision: .allowExactBuild
            ),
            NativeRuntimePermissionDecisionSelection(
                domain: "outbox",
                decision: .allowSession
            ),
        ]
    )
    #expect(manager.snapshot().submissionState == .applied)
    #expect(
        manager.snapshot().review.capabilities.map(\.existingDecision)
            == [.allowExactBuild, .allowSession]
    )
}

@MainActor
@Test func nativePermissionRefusalPreservesTheReviewedExactBuild() async throws {
    let native = RecordingNativePermissionService(
        review: nativeReview(
            capabilities: [nativeCapability(domain: "identity")]
        )
    )
    native.nextUpdate = NativeRuntimePermissionBatchUpdate(
        applied: false,
        review: nil,
        refusal: NativeRuntimePermissionRefusal(
            code: "permission-batch-refused",
            detail: "the dependency closure was refused",
            occurredAtMillis: 42
        )
    )
    let manager = try RuntimeWorkbenchPermissionManager(
        native: native,
        principal: permissionManagerPrincipal()
    )
    let reviewed = manager.snapshot().review
    let batch = PermissionDecisionBatch(
        principal: reviewed.principal,
        decisions: [
            PermissionDecisionSelection(
                domain: "identity",
                decision: .allowExactBuild
            )!,
        ]
    )!

    await manager.submit(batch)

    #expect(manager.snapshot().review == reviewed)
    guard case let .refused(issue) = manager.snapshot().submissionState else {
        Issue.record("Expected the Rust refusal to remain visible")
        return
    }
    #expect(issue.affectedDomains == ["identity"])
    #expect(issue.message == "the dependency closure was refused")
}

private final class RecordingNativePermissionService:
    NativeRuntimePermissionManaging,
    @unchecked Sendable
{
    private let initialReview: NativeRuntimePermissionReviewSnapshot
    var nextUpdate: NativeRuntimePermissionBatchUpdate?
    private(set) var reviewCoordinates: [
        NativeRuntimePermissionCoordinate
    ] = []
    private(set) var batches: [NativeRuntimePermissionDecisionBatch] = []

    init(review: NativeRuntimePermissionReviewSnapshot) {
        initialReview = review
    }

    func permissionReview(
        for coordinate: NativeRuntimePermissionCoordinate
    ) -> NativeRuntimePermissionReviewResult {
        reviewCoordinates.append(coordinate)
        return NativeRuntimePermissionReviewResult(
            review: initialReview,
            refusal: nil
        )
    }

    func applyPermissionDecisions(
        _ batch: NativeRuntimePermissionDecisionBatch
    ) -> NativeRuntimePermissionBatchUpdate {
        batches.append(batch)
        return nextUpdate ?? NativeRuntimePermissionBatchUpdate(
            applied: false,
            review: nil,
            refusal: NativeRuntimePermissionRefusal(
                code: "missing-test-update",
                detail: "the test did not configure an update",
                occurredAtMillis: 0
            )
        )
    }
}

private func permissionManagerPrincipal() -> PermissionExactBuildPrincipal {
    PermissionExactBuildPrincipal(
        manifestAuthorPublicKey: String(repeating: "a", count: 64),
        dTag: "good-morning",
        aggregateHash: String(repeating: "b", count: 64)
    )!
}

private func nativeCoordinate() -> NativeRuntimePermissionCoordinate {
    NativeRuntimePermissionCoordinate(
        manifestAuthor: String(repeating: "a", count: 64),
        dTag: "good-morning",
        aggregateHash: String(repeating: "b", count: 64)
    )
}

private func nativeReview(
    capabilities: [NativeRuntimePermissionCapabilitySnapshot]
) -> NativeRuntimePermissionReviewSnapshot {
    NativeRuntimePermissionReviewSnapshot(
        coordinate: nativeCoordinate(),
        title: "Good Morning",
        capabilities: capabilities,
        launchPermitted: false
    )
}

private func nativeCapability(
    domain: String,
    sensitivity: NativeRuntimePermissionSensitivity = .sensitive,
    availability: NativeRuntimePermissionPlatformAvailability = .available,
    existing: NativeRuntimePermissionExistingDecision = .denied,
    requested: NativeRuntimeGrantDecision? = .askEveryTime,
    invalidDecisions: Set<NativeRuntimeGrantDecision> = [],
    invalidReason: String = "decision unavailable"
) -> NativeRuntimePermissionCapabilitySnapshot {
    // This helper stands in for Rust, so it reproduces the projection Rust
    // performs: granted means the decision in force allows without prompting,
    // and the recommendation is the broadest still-valid affirmative option.
    let granted: [NativeRuntimePermissionExistingDecision] = [
        .allowSession,
        .allowExactBuild,
        .managed,
    ]
    let recommended: NativeRuntimeGrantDecision? = requested == nil
        ? nil
        : ([.allowExactBuild, .allowSession].first {
            !invalidDecisions.contains($0)
        } ?? .denied)
    return NativeRuntimePermissionCapabilitySnapshot(
        domain: domain,
        requirement: .required,
        sensitivity: sensitivity,
        dependencies: [],
        platformAvailability: availability,
        existingDecision: existing,
        isGranted: granted.contains(existing),
        requestedDecision: requested,
        recommendedDecision: recommended,
        decisionOptions: NativeRuntimeGrantDecision.allTestCases.map {
            decision in
            let invalid = invalidDecisions.contains(decision)
            return NativeRuntimePermissionDecisionOption(
                decision: decision,
                valid: !invalid,
                invalidReason: invalid ? invalidReason : nil
            )
        }
    )
}

private extension NativeRuntimeGrantDecision {
    static var allTestCases: [Self] {
        [.denied, .askEveryTime, .allowSession, .allowExactBuild]
    }
}
