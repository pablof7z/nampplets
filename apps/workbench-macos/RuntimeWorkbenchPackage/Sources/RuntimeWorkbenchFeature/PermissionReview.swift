import Observation

/// The Rust-backed permission boundary consumed by the native review sheet.
///
/// `submit` accepts one finite batch for one exact build. Implementations own
/// dependency validation, persistence, revocation, provider cancellation, and
/// the resulting state. This interface deliberately exposes no launch action.
@MainActor
public protocol PermissionReviewManaging: AnyObject {
    func snapshot() -> PermissionReviewSnapshot
    func submit(_ batch: PermissionDecisionBatch) async
}

@MainActor
@Observable
final class PermissionReviewSheetModel {
    private let manager: any PermissionReviewManaging
    private(set) var snapshot: PermissionReviewSnapshot
    private(set) var selections: [String: PermissionRequestedDecision]
    private(set) var changedDomains: Set<String> = []
    private(set) var transientIssue: PermissionReviewIssue?
    private(set) var isSubmitting = false

    init(manager: any PermissionReviewManaging) {
        self.manager = manager
        let snapshot = manager.snapshot()
        self.snapshot = snapshot
        selections = Self.defaultSelections(for: snapshot.review)
    }

    var review: PermissionReview {
        snapshot.review
    }

    var decidableCapabilities: [PermissionCapabilityReview] {
        review.capabilities.filter { $0.requestedDecision != nil }
    }

    var managedCapabilities: [PermissionCapabilityReview] {
        review.capabilities.filter { $0.requestedDecision == nil }
    }

    /// Presentation order only. Rust owns sensitivity and requirement.
    var orderedCapabilities: [PermissionCapabilityReview] {
        decidableCapabilities
            .enumerated()
            .sorted { left, right in
                let leftRank = Self.attentionRank(left.element)
                let rightRank = Self.attentionRank(right.element)
                return leftRank == rightRank
                    ? left.offset < right.offset
                    : leftRank < rightRank
            }
            .map(\.element)
    }

    var requiredCapabilities: [PermissionCapabilityReview] {
        orderedCapabilities.filter { $0.requirement == .required }
    }

    var optionalCapabilities: [PermissionCapabilityReview] {
        orderedCapabilities.filter { $0.requirement == .optional }
    }

    var issue: PermissionReviewIssue? {
        if let transientIssue {
            return transientIssue
        }
        guard case let .refused(issue) = snapshot.submissionState else {
            return nil
        }
        return issue
    }

    var plainIssueVerdict: NappletTrustVerdict? {
        issue.map { _ in .blocked("Couldn't apply those choices.") }
    }

    var isManagedReviewBlocked: Bool {
        review.isReadOnly
    }

    var isApplied: Bool {
        snapshot.submissionState == .applied
    }

    /// A napplet that requests nothing produces a review with no capabilities.
    /// There is nothing to decide and nothing to submit, so confirming it is a
    /// pure dismissal rather than a transaction. Note such a review is also
    /// vacuously `isReadOnly` -- `allSatisfy` over no capabilities is `true` --
    /// so without this case the sheet would be blocked on two independent
    /// counts and the user could never clear it.
    var hasNothingToDecide: Bool {
        review.capabilities.isEmpty
    }

    var canConfirm: Bool {
        guard !isSubmitting, !isApplied, transientIssue == nil else {
            return false
        }
        guard !hasNothingToDecide else {
            return true
        }
        return !isManagedReviewBlocked
            && !changedDomains.isEmpty
            && invalidSelections.isEmpty
    }

    func selection(
        for capability: PermissionCapabilityReview
    ) -> PermissionRequestedDecision? {
        selections[capability.domain] ?? capability.requestedDecision
    }

    func select(
        _ decision: PermissionRequestedDecision,
        for requestedCapability: PermissionCapabilityReview
    ) {
        guard
            let capability = review.capabilities.first(where: {
                $0.domain == requestedCapability.domain
            }),
            let option = capability.option(for: decision),
            option.isValid
        else {
            transientIssue = PermissionReviewIssue(
                title: "Decision unavailable",
                message: requestedCapability.option(for: decision)?.invalidReason
                    ?? "The runtime did not offer this decision.",
                affectedDomains: [requestedCapability.domain]
            )!
            return
        }
        selections[capability.domain] = decision
        changedDomains.insert(capability.domain)
        transientIssue = nil
        if isApplied {
            snapshot = PermissionReviewSnapshot(review: review)
        }
    }

    /// Applies Rust's recommended grant for one capability, or denies it.
    func setGranted(
        _ granted: Bool,
        for capability: PermissionCapabilityReview
    ) {
        guard granted else {
            if capability.option(for: .deny)?.isValid == true {
                select(.deny, for: capability)
            }
            return
        }
        guard let recommended = grantingRecommendation(for: capability) else {
            return
        }
        select(recommended, for: capability)
    }

    /// Before an edit this is Rust's `isGranted`; after an edit it reflects
    /// whether the pending choice is Rust's recommended affirmative decision.
    func isGranted(_ capability: PermissionCapabilityReview) -> Bool {
        guard let selected = selection(for: capability) else {
            return capability.isGranted
        }
        guard selected != capability.requestedDecision else {
            return capability.isGranted
        }
        return selected == grantingRecommendation(for: capability)
    }

    func hasAffirmativeOption(
        _ capability: PermissionCapabilityReview
    ) -> Bool {
        grantingRecommendation(for: capability) != nil
    }

    /// The common path still submits only Rust-projected decisions.
    func allowRecommended() async {
        selectAllRecommended()
        await confirm()
    }

    func selectAllRecommended() {
        for capability in decidableCapabilities {
            guard
                let recommended = capability.recommendedDecision,
                capability.option(for: recommended)?.isValid == true
            else {
                continue
            }
            selections[capability.domain] = recommended
            changedDomains.insert(capability.domain)
        }
        transientIssue = nil
    }

    /// Discards only transient native form state. It never calls the manager.
    func cancel() {
        selections = Self.defaultSelections(for: review)
        changedDomains.removeAll()
        transientIssue = nil
    }

    func confirm() async {
        guard canConfirm else {
            return
        }
        guard !hasNothingToDecide else {
            snapshot = PermissionReviewSnapshot(
                review: review,
                submissionState: .applied
            )
            return
        }
        guard
            let batch = PermissionDecisionBatch(
                principal: review.principal,
                reviewRevision: review.revision,
                decisions: review.capabilities.compactMap { capability in
                    guard
                        changedDomains.contains(capability.domain),
                        let selection = selections[capability.domain]
                    else {
                        return nil
                    }
                    return PermissionDecisionSelection(
                        domain: capability.domain,
                        decision: selection
                    )
                }
            ),
            batch.decisions.count == changedDomains.count
        else {
            transientIssue = PermissionReviewIssue(
                title: "Permission review is incomplete",
                message: "Every capability needs one valid decision before confirming."
            )!
            return
        }

        isSubmitting = true
        transientIssue = nil
        await manager.submit(batch)
        let updatedSnapshot = manager.snapshot()
        if updatedSnapshot.review.principal != batch.principal {
            transientIssue = PermissionReviewIssue(
                title: "Exact build changed",
                message: "The permission review no longer matches this verified build."
            )!
        } else {
            // Pending choices were made against one exact review revision.
            // Discard them only when that revision moved out from under them,
            // which is exactly the stale-review case: Rust ships the *current*
            // review with a `StaleReview` refusal, so its revision differs from
            // the one this batch carried. A validation refusal (unknown or
            // managed capability, unavailable decision, denied dependency)
            // returns the same review at the same revision, and the user's
            // input is still meaningful -- keeping it is what lets them correct
            // one domain and retry instead of starting over.
            //
            // The revision is the honest signal here rather than the refusal
            // code, which this layer never sees: `PermissionSubmissionState`
            // carries only a bounded issue, and matching on its title would
            // couple the model to the manager's user-facing copy.
            let reviewMoved = updatedSnapshot.review.revision
                != batch.reviewRevision
            let wasApplied = updatedSnapshot.submissionState == .applied
            snapshot = updatedSnapshot
            if wasApplied || reviewMoved {
                selections = Self.defaultSelections(for: updatedSnapshot.review)
                changedDomains.removeAll()
            }
        }
        isSubmitting = false
    }

    private var invalidSelections: [String] {
        decidableCapabilities.compactMap { capability in
            guard
                let selected = selection(for: capability),
                capability.option(for: selected)?.isValid == true
            else {
                return capability.domain
            }
            return nil
        }
    }

    private func grantingRecommendation(
        for capability: PermissionCapabilityReview
    ) -> PermissionRequestedDecision? {
        guard
            let recommended = capability.recommendedDecision,
            recommended != .deny,
            recommended != .askEveryTime,
            capability.option(for: recommended)?.isValid == true
        else {
            return nil
        }
        return recommended
    }

    private static func defaultSelections(
        for review: PermissionReview
    ) -> [String: PermissionRequestedDecision] {
        Dictionary(
            uniqueKeysWithValues: review.capabilities.compactMap { capability in
                capability.requestedDecision.map { (capability.domain, $0) }
            }
        )
    }

    private static func attentionRank(
        _ capability: PermissionCapabilityReview
    ) -> Int {
        switch (capability.sensitivity, capability.requirement) {
        case (.sensitive, _), (.unknown, _): 0
        case (.ordinary, .required): 1
        case (.ordinary, .optional): 2
        }
    }
}
