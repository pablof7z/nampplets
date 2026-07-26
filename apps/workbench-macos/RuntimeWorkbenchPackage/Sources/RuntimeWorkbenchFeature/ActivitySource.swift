/// A pushed activity update.
///
/// `authoritative` establishes or replaces the local projection and clears any
/// detected delivery gap. `next` must name the revision it follows so the
/// native presentation can refuse to silently conceal missed diagnostics.
public enum ActivityUpdate: Equatable, Sendable {
    case authoritative(ActivitySnapshot)
    case next(ActivitySnapshot, predecessorRevision: UInt64)
}

@MainActor
public protocol ActivitySubscription: AnyObject {
    func cancel()
}

/// Injectable presentation seam for a Rust-owned activity projection.
///
/// Implementations immediately deliver an authoritative snapshot when
/// subscribing, then push bounded replacement updates. `refresh` is called
/// only by an explicit user action; implementations must not poll.
@MainActor
public protocol ActivitySource: AnyObject {
    func subscribe(
        to scope: ActivityExactBuildScope,
        receive: @escaping @MainActor (ActivityUpdate) -> Void
    ) -> any ActivitySubscription

    func refresh(scope: ActivityExactBuildScope) throws -> ActivitySnapshot
}
