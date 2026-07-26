import Observation
import SwiftUI

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

    /// Capabilities the user actually decides. A host-managed capability has
    /// no `requestedDecision` because the runtime, not the person, owns it --
    /// so it is never counted toward a submitted batch and never blocks one.
    var decidableCapabilities: [PermissionCapabilityReview] {
        review.capabilities.filter { $0.requestedDecision != nil }
    }

    var managedCapabilities: [PermissionCapabilityReview] {
        review.capabilities.filter { $0.requestedDecision == nil }
    }

    /// Presentation order only: the capabilities most worth reading are put
    /// where they will be read. Rust owns `sensitivity` and `requirement`;
    /// this reorders its projection and changes nothing about it.
    var orderedCapabilities: [PermissionCapabilityReview] {
        decidableCapabilities
            .enumerated()
            .sorted { left, right in
                let leftRank = Self.attentionRank(left.element)
                let rightRank = Self.attentionRank(right.element)
                if leftRank == rightRank {
                    return left.offset < right.offset
                }
                return leftRank < rightRank
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

    var isApplied: Bool {
        snapshot.submissionState == .applied
    }

    var canConfirm: Bool {
        !isSubmitting
            && !isApplied
            && transientIssue == nil
            && invalidSelections.isEmpty
            && hasSomethingToApply
    }

    /// A review with no capabilities at all is confirmable -- there is nothing
    /// to grant and the sheet simply closes. A review whose every capability
    /// is host-managed is *not*: the user has no decision to make, a batch of
    /// zero decisions cannot be constructed, and letting the button through
    /// would mark the review applied without Rust ever seeing it.
    ///
    /// The case in between is the one this distinction exists to unblock:
    /// when a review mixes managed and decidable capabilities, the batch
    /// carries only the decidable ones and confirming is correct. Counting
    /// managed capabilities toward batch completeness made any such review
    /// permanently unconfirmable.
    private var hasSomethingToApply: Bool {
        review.capabilities.isEmpty || !decidableCapabilities.isEmpty
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
        transientIssue = nil
    }

    /// The common outcome, reached without operating a control per capability.
    ///
    /// Every decision applied here is the runtime's own `recommendedDecision`
    /// -- native code neither invents nor ranks them. A capability whose
    /// recommendation the runtime did not offer as valid keeps its requested
    /// default, so this can never widen a grant beyond what Rust projected.
    func allowRecommended() async {
        for capability in decidableCapabilities {
            guard
                let recommended = capability.recommendedDecision,
                capability.option(for: recommended)?.isValid == true
            else {
                continue
            }
            selections[capability.domain] = recommended
        }
        transientIssue = nil
        await confirm()
    }

    /// Discards only transient native form state. It never calls the manager.
    func cancel() {
        selections = Self.defaultSelections(for: review)
        transientIssue = nil
    }

    func confirm() async {
        guard canConfirm else {
            return
        }
        let decidable = decidableCapabilities
        guard !review.capabilities.isEmpty else {
            snapshot = PermissionReviewSnapshot(
                review: review,
                submissionState: .applied
            )
            return
        }
        guard !decidable.isEmpty else {
            // Unreachable while `canConfirm` holds; kept so that a future
            // caller bypassing the guard cannot silently mark an all-managed
            // review applied without Rust.
            return
        }
        guard
            let batch = PermissionDecisionBatch(
                principal: review.principal,
                decisions: decidable.compactMap { capability in
                    guard let selection = selections[capability.domain] else {
                        return nil
                    }
                    return PermissionDecisionSelection(
                        domain: capability.domain,
                        decision: selection
                    )
                }
            ),
            batch.decisions.count == decidable.count
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
            snapshot = updatedSnapshot
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

    private static func defaultSelections(
        for review: PermissionReview
    ) -> [String: PermissionRequestedDecision] {
        Dictionary(
            uniqueKeysWithValues: review.capabilities.compactMap { capability in
                capability.requestedDecision.map { (capability.domain, $0) }
            }
        )
    }

    /// Lower sorts earlier. Sensitive and unclassified capabilities lead,
    /// then ordinary required ones, then ordinary optional ones.
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
