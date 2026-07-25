import Foundation
import NMPNativeRuntimeApple

enum RuntimeWorkbenchPermissionError: Error, LocalizedError, Equatable {
    case refused(code: String, detail: String)
    case malformed(String)

    var errorDescription: String? {
        switch self {
        case .refused(let code, let detail):
            "Permission review was refused (\(code)): \(detail)"
        case .malformed(let detail):
            "Permission review was malformed: \(detail)"
        }
    }
}

/// Main-actor adapter from the exact Rust permission transaction into the
/// native sheet's bounded presentation model.
///
/// This adapter performs only mechanical projection. It calls exactly one
/// atomic Rust batch operation on submit and exposes no launch operation.
@MainActor
public final class RuntimeWorkbenchPermissionManager:
    PermissionReviewManaging
{
    private let native: any NativeRuntimePermissionManaging
    private var current: PermissionReviewSnapshot

    public convenience init(
        profile: WorkbenchRuntimeProfile,
        principal: PermissionExactBuildPrincipal
    ) throws {
        try self.init(native: profile.native, principal: principal)
    }

    init(
        native: any NativeRuntimePermissionManaging,
        principal: PermissionExactBuildPrincipal
    ) throws {
        self.native = native
        let result = native.permissionReview(
            for: Self.nativeCoordinate(principal)
        )
        current = PermissionReviewSnapshot(
            review: try Self.project(result: result)
        )
    }

    public func snapshot() -> PermissionReviewSnapshot {
        current
    }

    public func submit(_ batch: PermissionDecisionBatch) async {
        guard batch.principal == current.review.principal else {
            current = PermissionReviewSnapshot(
                review: current.review,
                submissionState: .refused(
                    Self.issue(
                        title: "Exact build changed",
                        message:
                            "The submitted decisions do not match this permission review."
                    )
                )
            )
            return
        }

        let nativeBatch = NativeRuntimePermissionDecisionBatch(
            coordinate: Self.nativeCoordinate(batch.principal),
            decisions: batch.decisions.map {
                NativeRuntimePermissionDecisionSelection(
                    domain: $0.domain,
                    decision: Self.nativeDecision($0.decision)
                )
            }
        )
        let update = native.applyPermissionDecisions(nativeBatch)

        guard update.applied, update.refusal == nil, let review = update.review
        else {
            let detail = update.refusal?.detail
                ?? "The runtime did not apply the complete permission batch."
            current = PermissionReviewSnapshot(
                review: current.review,
                submissionState: .refused(
                    Self.issue(
                        title: "Permission decisions refused",
                        message: detail,
                        affectedDomains: batch.decisions.map(\.domain)
                    )
                )
            )
            return
        }

        do {
            let projected = try Self.project(review)
            guard projected.principal == batch.principal else {
                throw RuntimeWorkbenchPermissionError.malformed(
                    "the resulting review changed exact-build identity"
                )
            }
            current = PermissionReviewSnapshot(
                review: projected,
                submissionState: .applied
            )
        } catch {
            current = PermissionReviewSnapshot(
                review: current.review,
                submissionState: .refused(
                    Self.issue(
                        title: "Permission result unavailable",
                        message: error.localizedDescription,
                        affectedDomains: batch.decisions.map(\.domain)
                    )
                )
            )
        }
    }

    private static func project(
        result: NativeRuntimePermissionReviewResult
    ) throws -> PermissionReview {
        if let refusal = result.refusal {
            throw RuntimeWorkbenchPermissionError.refused(
                code: refusal.code,
                detail: refusal.detail
            )
        }
        guard let review = result.review else {
            throw RuntimeWorkbenchPermissionError.malformed(
                "the runtime returned neither a review nor a refusal"
            )
        }
        return try project(review)
    }

    private static func project(
        _ native: NativeRuntimePermissionReviewSnapshot
    ) throws -> PermissionReview {
        guard
            let principal = PermissionExactBuildPrincipal(
                manifestAuthorPublicKey: native.coordinate.manifestAuthor,
                dTag: native.coordinate.dTag,
                aggregateHash: native.coordinate.aggregateHash
            )
        else {
            throw RuntimeWorkbenchPermissionError.malformed(
                "the exact-build coordinate is invalid"
            )
        }

        let capabilities = try native.capabilities.map {
            try project($0, nappletTitle: native.title)
        }
        guard
            let review = PermissionReview(
                principal: principal,
                publisherDisplayName: nil,
                nappletTitle: native.title,
                capabilities: capabilities
            )
        else {
            throw RuntimeWorkbenchPermissionError.malformed(
                "the bounded review could not be represented"
            )
        }
        return review
    }

    private static func project(
        _ native: NativeRuntimePermissionCapabilitySnapshot,
        nappletTitle: String
    ) throws -> PermissionCapabilityReview {
        let options = try native.decisionOptions.map { option in
            guard
                let projected = PermissionDecisionOption(
                    decision: requestedDecision(option.decision),
                    isValid: option.valid,
                    invalidReason: option.invalidReason
                )
            else {
                throw RuntimeWorkbenchPermissionError.malformed(
                    "the \(native.domain) decision options are inconsistent"
                )
            }
            return projected
        }
        let dependencies = try native.dependencies.map { dependency in
            guard
                let projected = PermissionCapabilityDependency(
                    domain: dependency,
                    reason: "Declared by the runtime provider descriptor."
                )
            else {
                throw RuntimeWorkbenchPermissionError.malformed(
                    "the \(native.domain) dependency is invalid"
                )
            }
            return projected
        }
        guard
            let capability = PermissionCapabilityReview(
                domain: native.domain,
                title: displayTitle(native.domain),
                requirement: requirement(native.requirement),
                sensitivity: sensitivity(native.sensitivity),
                rationale: "Requested by \(nappletTitle).",
                dependencies: dependencies,
                platformAvailability: availability(
                    native.platformAvailability
                ),
                existingDecision: existingDecision(native.existingDecision),
                isGranted: native.isGranted,
                requestedDecision: native.requestedDecision.map(
                    requestedDecision
                ),
                recommendedDecision: native.recommendedDecision.map(
                    requestedDecision
                ),
                decisionOptions: options
            )
        else {
            throw RuntimeWorkbenchPermissionError.malformed(
                "the \(native.domain) capability could not be represented"
            )
        }
        return capability
    }

    private static func nativeCoordinate(
        _ principal: PermissionExactBuildPrincipal
    ) -> NativeRuntimePermissionCoordinate {
        NativeRuntimePermissionCoordinate(
            manifestAuthor: principal.manifestAuthorPublicKey,
            dTag: principal.dTag,
            aggregateHash: principal.aggregateHash
        )
    }

    private static func requirement(
        _ value: NativeRuntimePermissionRequirement
    ) -> PermissionCapabilityRequirement {
        switch value {
        case .required:
            .required
        case .optional:
            .optional
        }
    }

    private static func sensitivity(
        _ value: NativeRuntimePermissionSensitivity
    ) -> PermissionCapabilitySensitivity {
        switch value {
        case .ordinary:
            .ordinary
        case .sensitive:
            .sensitive
        case .unknown:
            .unknown
        }
    }

    private static func availability(
        _ value: NativeRuntimePermissionPlatformAvailability
    ) -> PermissionPlatformAvailability {
        switch value {
        case .available:
            .available
        case .unknown(let reason):
            .unknown(reason: reason)
        case .unavailable(let reason):
            .unavailable(reason: reason)
        }
    }

    private static func existingDecision(
        _ value: NativeRuntimePermissionExistingDecision
    ) -> PermissionExistingDecision {
        switch value {
        case .denied:
            .denied
        case .askEveryTime:
            .askEveryTime
        case .allowSession:
            .allowSession
        case .allowExactBuild:
            .allowExactBuild
        case .managed:
            .managed
        }
    }

    private static func requestedDecision(
        _ value: NativeRuntimeGrantDecision
    ) -> PermissionRequestedDecision {
        switch value {
        case .denied:
            .deny
        case .askEveryTime:
            .askEveryTime
        case .allowSession:
            .allowSession
        case .allowExactBuild:
            .allowExactBuild
        }
    }

    private static func nativeDecision(
        _ value: PermissionRequestedDecision
    ) -> NativeRuntimeGrantDecision {
        switch value {
        case .deny:
            .denied
        case .askEveryTime:
            .askEveryTime
        case .allowSession:
            .allowSession
        case .allowExactBuild:
            .allowExactBuild
        }
    }

    private static func displayTitle(_ domain: String) -> String {
        domain
            .split(whereSeparator: { $0 == "." || $0 == "-" || $0 == "_" })
            .map { $0.prefix(1).uppercased() + $0.dropFirst() }
            .joined(separator: " ")
    }

    private static func issue(
        title: String,
        message: String,
        affectedDomains: [String] = []
    ) -> PermissionReviewIssue {
        PermissionReviewIssue(
            title: truncate(title),
            message: truncate(message),
            affectedDomains: affectedDomains
        ) ?? PermissionReviewIssue(
            title: "Permission operation refused",
            message: "The runtime returned an invalid bounded error."
        )!
    }

    private static func truncate(_ value: String) -> String {
        guard
            value.utf8.count > PermissionLimits.maximumDisplayTextUTF8Bytes
        else {
            return value
        }
        var result = ""
        result.reserveCapacity(PermissionLimits.maximumDisplayTextUTF8Bytes)
        for character in value {
            guard
                result.utf8.count + String(character).utf8.count
                    <= PermissionLimits.maximumDisplayTextUTF8Bytes
            else {
                break
            }
            result.append(character)
        }
        return result
    }
}
