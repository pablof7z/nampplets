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
    // Applying the decisions moves both grants, so Rust would hash a different
    // effective review and hand back a different revision.
    let applied = nativeReview(
        revision: "2",
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
        // `changed` is not a synonym for `applied`. Rust accepts a batch whose
        // decisions all already match the decision in force and reports
        // `applied: true, changed: false` for it. Here both domains move from
        // `.denied`, so at least one durable grant really changed.
        changed: true,
        review: applied,
        refusal: nil
    )
    let manager = try RuntimeWorkbenchPermissionManager(
        native: native,
        principal: permissionManagerPrincipal()
    )
    let batch = PermissionDecisionBatch(
        principal: permissionManagerPrincipal(),
        reviewRevision: initial.revision,
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
    // The revision is what binds these decisions to the review the user saw.
    // The manager must forward it untouched or Rust cannot detect staleness.
    #expect(native.batches[0].reviewRevision == initial.revision)
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
    // The change boundary no longer answers with the open-coded, timestamped
    // `RuntimeRefusal` used for diagnostics elsewhere. It answers with the
    // closed `RuntimePermissionChangeRefusal`, whose `code` is one of the
    // reasons a permission change can be turned down and which native code is
    // expected to switch on. `.dependencyDenied` is the typed name for the
    // refusal this test drives.
    native.nextUpdate = NativeRuntimePermissionBatchUpdate(
        applied: false,
        // Rust's refusal path always reports `changed: false`: a refused batch
        // is atomic and moves no grant.
        changed: false,
        review: nil,
        refusal: NativeRuntimePermissionChangeRefusal(
            code: .dependencyDenied,
            detail: "the dependency closure was refused"
        )
    )
    let manager = try RuntimeWorkbenchPermissionManager(
        native: native,
        principal: permissionManagerPrincipal()
    )
    let reviewed = manager.snapshot().review
    let batch = PermissionDecisionBatch(
        principal: reviewed.principal,
        reviewRevision: reviewed.revision,
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
        guard let update = nextUpdate else {
            // Every refusal code in the closed set names a reason Rust turned
            // a change down. None of them means "the harness was not set up",
            // so rather than borrow one and let a misconfigured test read as a
            // meaningful runtime refusal, fail loudly and answer with a
            // refusal whose code is asserted nowhere.
            Issue.record(
                "The test did not configure a permission batch update."
            )
            return NativeRuntimePermissionBatchUpdate(
                applied: false,
                changed: false,
                review: nil,
                refusal: NativeRuntimePermissionChangeRefusal(
                    code: .closed,
                    detail: "the test did not configure an update"
                )
            )
        }
        return update
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

/// Rust hashes the whole effective review into `revision` and refuses any
/// decision batch that does not echo the live value back, so a review whose
/// content differs must carry a different revision. Callers pass one distinct
/// well-formed token per distinct review rather than recomputing the digest.
private func nativeReview(
    revision: Character = "1",
    capabilities: [NativeRuntimePermissionCapabilitySnapshot]
) -> NativeRuntimePermissionReviewSnapshot {
    NativeRuntimePermissionReviewSnapshot(
        coordinate: nativeCoordinate(),
        revision: String(repeating: revision, count: 64),
        title: "Good Morning",
        capabilities: capabilities,
        // Rust derives `readOnly` as "every capability is host-policy
        // controlled" (vacuously true for an empty review), so the stand-in
        // derives it the same way instead of letting callers assert it.
        readOnly: capabilities.allSatisfy {
            if case .hostPolicy = $0.controller {
                return true
            }
            return false
        },
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
    // `controller` says whose decision this is. Rust projects `.hostPolicy`
    // exactly when the decision in force is `Managed` and `.user` otherwise --
    // platform unavailability narrows the offered options but never moves
    // ownership -- so the stand-in derives it rather than accepting it.
    let controller: NativeRuntimePermissionDecisionController = existing
        == .managed
        ? .hostPolicy(reason: invalidReason)
        : .user
    return NativeRuntimePermissionCapabilitySnapshot(
        domain: domain,
        requirement: .required,
        sensitivity: sensitivity,
        dependencies: [],
        platformAvailability: availability,
        controller: controller,
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
