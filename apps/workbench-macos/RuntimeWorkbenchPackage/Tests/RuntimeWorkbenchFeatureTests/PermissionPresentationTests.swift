@testable import RuntimeWorkbenchFeature
import Testing

@MainActor
@Test func permissionPlainPathNeverLeaksRuntimeReasonText() {
    let unavailable = unavailablePermissionSnapshot().review.capabilities[0]
    let unavailableRow = PermissionCapabilityRow(
        capability: unavailable,
        grantBinding: .constant(unavailable.isGranted),
        hasAffirmativeOption: false,
        isReviewLocked: false
    )

    #expect(
        unavailableRow.unavailableMessage
            == "Not available on this device, so it won't work."
    )
    #expect(
        unavailable.platformAvailability.detail.map {
            unavailableRow.unavailableMessage?.contains($0)
        } == false
    )
    let allowedManagedRow = PermissionCapabilityRow(
        capability: managedPermissionCapability(isGranted: true),
        grantBinding: .constant(true),
        hasAffirmativeOption: false,
        isReviewLocked: true
    )
    #expect(
        allowedManagedRow.managedReason
            == "Allowed by a managed setting; you can't change it here."
    )
    let deniedManagedRow = PermissionCapabilityRow(
        capability: managedPermissionCapability(isGranted: false),
        grantBinding: .constant(false),
        hasAffirmativeOption: false,
        isReviewLocked: true
    )
    #expect(
        deniedManagedRow.managedReason
            == "Not allowed by a managed setting; you can't change it here."
    )
}

@MainActor
@Test func decidableSwitchExplainsWhyAMixedReviewLocksIt() {
    let capability = mixedManagedPermissionSnapshot().review.capabilities[1]
    let row = PermissionCapabilityRow(
        capability: capability,
        grantBinding: .constant(false),
        hasAffirmativeOption: true,
        isReviewLocked: true
    )

    #expect(row.isGrantDisabled)
    #expect(
        row.grantHint
            == "Unavailable because this review includes managed settings"
    )
}

@MainActor
@Test func permissionIssueUsesBoundedPlainCopyAndExactEvidence() async {
    let initial = permissionSnapshot()
    let manager = RecordingPermissionManager(snapshot: initial)
    let rawIssue = PermissionReviewIssue(
        title: "Exact build changed",
        message: "Grant batch rejected by runtime-app revision 42.",
        affectedDomains: ["outbox"]
    )!
    manager.response = PermissionReviewSnapshot(
        review: initial.review,
        submissionState: .refused(rawIssue)
    )
    let model = PermissionReviewSheetModel(manager: manager)

    // A batch now carries only the domains the user actually changed, so
    // `confirm()` is a no-op until something has been changed. This edit is
    // the precondition for reaching a refusal at all; the assertions below,
    // which are about how a refusal is worded and preserved, are unchanged.
    model.select(.allowExactBuild, for: model.review.capabilities[0])
    await model.confirm()

    #expect(
        model.plainIssueVerdict
            == .blocked("Couldn't apply those choices.")
    )
    #expect(model.plainIssueVerdict?.message?.contains(rawIssue.title) == false)
    #expect(model.plainIssueVerdict?.message?.contains(rawIssue.message) == false)
    #expect(model.issue == rawIssue)
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
