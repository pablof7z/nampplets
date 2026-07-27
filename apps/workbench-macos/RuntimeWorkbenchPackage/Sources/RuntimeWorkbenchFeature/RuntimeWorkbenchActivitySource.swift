import Foundation
import NMPNativeRuntimeApple

public extension ActivityDetailField {
    /// Carry one runtime-classified detail into the presentation model.
    ///
    /// This is the only way a detail value becomes displayable here: the
    /// runtime's visible/redacted decision is transcribed, never revisited.
    /// Nothing in this layer inspects the key or the text to guess secrecy.
    init?(_ detail: NativeRuntimeActivityDetail) {
        switch detail.value {
        case let .visible(text):
            self.init(key: detail.key, value: .visible(text))
        case .redacted:
            self.init(key: detail.key, value: .redacted)
        }
    }
}

public enum RuntimeWorkbenchActivitySourceRefusal:
    Error,
    LocalizedError,
    Equatable
{
    case subscriberCapacity(maximum: Int)
    case scopeMismatch
    case snapshotRefused(code: String, detail: String)

    public var errorDescription: String? {
        switch self {
        case let .subscriberCapacity(maximum):
            "The Workbench activity subscriber limit of \(maximum) was reached."
        case .scopeMismatch:
            "This activity source is bound to a different exact build."
        case let .snapshotRefused(code, detail):
            "Runtime activity projection was refused (\(code)): \(detail)"
        }
    }
}

/// A real Workbench adapter over the profile's single Rust-owned observation.
///
/// The adapter retains only the latest bounded replacement. Delivery into the
/// main actor is coalesced, so native presentation cannot create an unbounded
/// queue when the runtime produces updates faster than the UI renders them.
@MainActor
public final class RuntimeWorkbenchActivitySource: ActivitySource {
    private struct Subscriber {
        let scope: ActivityExactBuildScope
        let receive: @MainActor (ActivityUpdate) -> Void
    }

    private static let maximumSubscribers = 16

    private let profile: WorkbenchRuntimeProfile
    private let scope: ActivityExactBuildScope
    private let nativeScope: NativeRuntimeActivityScope
    private let mailbox: RuntimeActivityUpdateMailbox
    private var nativeObservation: NativeRuntimeActivityObservation?
    private var projection: NativeRuntimeActivityProjection
    private var subscribers: [UUID: Subscriber] = [:]

    public private(set) var latestAdmissionRefusal:
        RuntimeWorkbenchActivitySourceRefusal?

    public init(
        profile: WorkbenchRuntimeProfile,
        scope: ActivityExactBuildScope
    ) throws {
        self.profile = profile
        self.scope = scope
        let nativeScope = NativeRuntimeActivityScope(
            manifestAuthor: scope.manifestAuthor,
            dTag: scope.dTag,
            aggregateHash: scope.aggregateHash
        )
        self.nativeScope = nativeScope
        projection = try profile.native.activityProjection(for: nativeScope)
        let mailbox = RuntimeActivityUpdateMailbox()
        self.mailbox = mailbox
        mailbox.bind { [weak self] update in
            self?.receive(update)
        }
        nativeObservation = try profile.native.observeActivity(
            scope: nativeScope
        ) {
            [mailbox] update in
            mailbox.offer(update)
        }
    }

    public func subscribe(
        to requestedScope: ActivityExactBuildScope,
        receive: @escaping @MainActor (ActivityUpdate) -> Void
    ) -> any ActivitySubscription {
        guard requestedScope == scope else {
            latestAdmissionRefusal = .scopeMismatch
            receive(
                .authoritative(
                    Self.emptySnapshot(
                        revision: projection.revision,
                        scope: requestedScope
                    )
                )
            )
            return RuntimeWorkbenchActivitySubscription(cancellation: {})
        }
        guard subscribers.count < Self.maximumSubscribers else {
            latestAdmissionRefusal = .subscriberCapacity(
                maximum: Self.maximumSubscribers
            )
            receive(.authoritative(Self.snapshot(from: projection, for: scope)))
            return RuntimeWorkbenchActivitySubscription(cancellation: {})
        }

        let identifier = UUID()
        subscribers[identifier] = Subscriber(
            scope: requestedScope,
            receive: receive
        )
        receive(
            .authoritative(
                Self.snapshot(from: projection, for: requestedScope)
            )
        )
        return RuntimeWorkbenchActivitySubscription { [weak self] in
            self?.subscribers.removeValue(forKey: identifier)
        }
    }

    public func refresh(
        scope requestedScope: ActivityExactBuildScope
    ) throws -> ActivitySnapshot {
        guard requestedScope == scope else {
            let refusal = RuntimeWorkbenchActivitySourceRefusal.scopeMismatch
            latestAdmissionRefusal = refusal
            throw refusal
        }
        do {
            let latest = try profile.native.activityProjection(for: nativeScope)
            projection = latest
            latestAdmissionRefusal = nil
            return Self.snapshot(from: latest, for: requestedScope)
        } catch let NativeRuntimeSnapshotProjectionError.refused(refusal) {
            let projected = RuntimeWorkbenchActivitySourceRefusal.snapshotRefused(
                code: refusal.code,
                detail: refusal.detail
            )
            latestAdmissionRefusal = projected
            throw projected
        }
    }

    private func receive(_ update: NativeRuntimeActivityUpdate) {
        switch update {
        case let .authoritative(nextProjection):
            projection = nextProjection
            latestAdmissionRefusal = nil
            for subscriber in subscribers.values {
                subscriber.receive(
                    .authoritative(
                        Self.snapshot(
                            from: nextProjection,
                            for: subscriber.scope
                        )
                    )
                )
            }

        case let .next(
            nextProjection,
            predecessorRevision,
            _,
            lostBeforeBatch
        ):
            projection = nextProjection
            latestAdmissionRefusal = nil
            // The staleness signal used to be smuggled downstream by XORing
            // the predecessor revision with 1, so the view model's
            // `predecessorRevision != currentRevision` check would trip. That
            // worked, but the banner renders the received predecessor as
            // evidence, so the evidence panel showed a number the runtime
            // never produced. The real count travels on its own now and the
            // revision stays true.
            for subscriber in subscribers.values {
                subscriber.receive(
                    .next(
                        Self.snapshot(
                            from: nextProjection,
                            for: subscriber.scope
                        ),
                        predecessorRevision: predecessorRevision,
                        lostBeforeBatch: lostBeforeBatch
                    )
                )
            }
        }
    }

    static func snapshot(
        from projection: NativeRuntimeActivityProjection,
        for scope: ActivityExactBuildScope
    ) -> ActivitySnapshot {
        let activeSessions = projection.sessions.filter {
            $0.scope.manifestAuthor == scope.manifestAuthor
                && $0.scope.dTag == scope.dTag
                && $0.scope.aggregateHash == scope.aggregateHash
        }.count
        let inventory = ActivityInventorySummary(
            activeSessions: min(
                activeSessions,
                ActivityInventorySummary.maximumActiveSessions
            ),
            activeBindings: 0,
            activeResources: 0,
            pendingReceipts: 0
        )!

        // The current Rust FFI has no typed activity severity/kind and no
        // exact-build ownership for bindings, resources, or receipts. Keep
        // those presentation fields empty instead of deriving policy in Swift
        // or exposing profile-global facts. `omittedFactCount` makes the
        // unsupported scoped records explicit until Rust supplies the typed
        // screen-shaped projection. Detail secrecy is no longer part of that
        // gap: the runtime classifies each detail, and the
        // `ActivityDetailField` conversion below only carries the decision
        // across.
        //
        // One consequence worth naming, because it looks like an omission and
        // is not. Each record carries `droppedDetailCount` — details the
        // runtime truncated at `MAXIMUM_ACTIVITY_DETAILS` — and it reaches
        // `NativeRuntimeActivityRecord` correctly. It stops here because it
        // describes a single fact and there are no facts to hang it on.
        // Folding it into `omittedFactCount` would be wrong: that counts whole
        // records this app cannot render, not details missing from a record it
        // can. When typed facts arrive and `facts` stops being empty,
        // `ActivityFact` needs its own `droppedDetailCount` and the row states
        // it.
        return ActivitySnapshot(
            scope: scope,
            revision: projection.revision,
            inventory: inventory,
            facts: [],
            omittedFactCount: UInt64(
                projection.records.count + projection.errors.count
            ),
            runtimeDiscardedCount: projection.runtimeDiscardedCount
        )!
    }

    private static func emptySnapshot(
        revision: UInt64,
        scope: ActivityExactBuildScope
    ) -> ActivitySnapshot {
        ActivitySnapshot(
            scope: scope,
            revision: revision,
            inventory: .empty,
            facts: [],
            omittedFactCount: 0
        )!
    }
}
