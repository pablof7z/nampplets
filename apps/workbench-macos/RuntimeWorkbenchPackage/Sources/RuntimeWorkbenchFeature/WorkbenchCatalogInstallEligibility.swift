import Foundation
import NMPNativeRuntimeApple

/// Renders the exact-install decision Rust already made for one review.
///
/// Rust decides install eligibility in `catalog::install_eligibility` with the
/// very `Principal` invariants it re-checks at confirmation time, and states
/// its own refusal code and reason text. Native never re-derives that decision
/// from the review's raw shape (a `dTag != nil` mirror would silently drift the
/// moment Rust's rule gains, loses, or changes a constraint); it only turns the
/// Rust answer into screen affordances.
enum WorkbenchCatalogInstallEligibility {
    /// The blocking warnings a review sheet shows for Rust's decision.
    ///
    /// A blocker is present exactly when Rust refused, so the severity is
    /// always blocking; nothing here re-judges whether it should be.
    static func warnings(
        for eligibility: NativeRuntimeCatalogInstallEligibility
    ) -> [CatalogWarning] {
        guard let blocker = eligibility.blocker else {
            return []
        }
        return [
            CatalogWarning(
                id: blocker.code,
                severity: .blocking,
                message: blocker.detail
            ),
        ]
    }
}
