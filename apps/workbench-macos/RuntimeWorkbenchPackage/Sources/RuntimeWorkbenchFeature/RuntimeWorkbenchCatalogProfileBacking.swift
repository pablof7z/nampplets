import Foundation

/// Profile-owned catalog operations projected by the native runtime.
///
/// The implementation uses the profile's single NMP engine and Rust resolver.
/// Swift never selects relays, interprets replacement events, verifies
/// artifacts, or infers whether an install worked. Async operations must keep
/// blocking NMP receivers and artifact acquisition off the main actor.
@MainActor
public protocol RuntimeWorkbenchCatalogProfileBacking: AnyObject {
    func observeCatalogChanges(
        _ receive: @escaping @MainActor @Sendable () -> Void
    ) -> CatalogFeedObservation

    func browseCatalog(
        _ request: CatalogSearchRequest
    ) async -> CatalogSearchResponse

    func resolveCatalogReview(
        _ target: CatalogReviewTarget
    ) async -> CatalogReviewResponse

    /// Cancels the blocking observation/lookup and wakes its receiver.
    func cancelCatalogWork()

    /// Discards one frozen opaque review that will not be installed.
    func cancelCatalogReview(_ reviewID: String)

    /// Installs only the frozen exact review. It must not launch or grant.
    func installCatalogReview(
        _ confirmation: CatalogInstallConfirmation
    ) async -> CatalogInstallResponse
}

public extension RuntimeWorkbenchCatalogProfileBacking {
    func observeCatalogChanges(
        _: @escaping @MainActor @Sendable () -> Void
    ) -> CatalogFeedObservation {
        CatalogFeedObservation()
    }

    func cancelCatalogReview(_: String) {}
}
