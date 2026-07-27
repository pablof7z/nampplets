import Foundation
import NMPNativeRuntime

// MARK: - Runtime activity facts, projection, and observation types

/// Exact-build identity attached by the Rust runtime to activity facts.
public struct NativeRuntimeActivityScope: Hashable, Sendable {
    public let manifestAuthor: String
    public let dTag: String
    public let aggregateHash: String

    public init(
        manifestAuthor: String,
        dTag: String,
        aggregateHash: String
    ) {
        self.manifestAuthor = manifestAuthor
        self.dTag = dTag
        self.aggregateHash = aggregateHash
    }
}

/// A runtime-owned refusal or failure attributed to one exact build.
///
/// Errors without a complete principal remain absent from the per-component
/// view so native presentation cannot leak unrelated profile activity.
public struct NativeRuntimeActivityError: Sendable {
    public let scope: NativeRuntimeActivityScope
    public let code: String
    public let sessionID: UInt64?
    public let detail: String
    public let occurredAtMillis: UInt64

    fileprivate init?(_ error: RuntimeErrorSnapshot) {
        guard let author = error.author,
              let dTag = error.dTag,
              let aggregateHash = error.aggregateHash
        else {
            return nil
        }
        scope = NativeRuntimeActivityScope(
            manifestAuthor: author,
            dTag: dTag,
            aggregateHash: aggregateHash
        )
        code = error.code
        sessionID = error.sessionId
        detail = error.detail
        occurredAtMillis = error.occurredAtMillis
    }
}

public struct NativeRuntimeActivitySession: Sendable {
    public let scope: NativeRuntimeActivityScope
    public let sessionID: UInt64
    public let state: String

    fileprivate init(_ session: RuntimeSessionSnapshot) {
        scope = NativeRuntimeActivityScope(
            manifestAuthor: session.author,
            dTag: session.dTag,
            aggregateHash: session.aggregateHash
        )
        sessionID = session.id
        state = session.state
    }
}

/// A bounded replacement projection sourced from the Rust runtime.
///
/// Bindings, receipts, and resources are intentionally not projected here:
/// their current FFI records lack exact-build ownership, so exposing the
/// profile-global totals in a component-scoped drawer would disclose unrelated
/// activity.
public struct NativeRuntimeActivityProjection: Sendable {
    public let revision: UInt64
    public let sessions: [NativeRuntimeActivitySession]
    public let records: [NativeRuntimeActivityRecord]
    public let errors: [NativeRuntimeActivityError]
    /// Entries the runtime's bounded rings already evicted, so they never
    /// reached this projection at all. Cumulative since the runtime opened,
    /// and **runtime-wide** — the rings are not partitioned by exact build, so
    /// this count must never be presented as belonging to one napplet.
    public let runtimeDiscardedCount: UInt64

    init(
        _ snapshot: RuntimeSnapshot,
        scope: NativeRuntimeActivityScope
    ) {
        revision = snapshot.revision
        sessions = snapshot.sessions
            .map(NativeRuntimeActivitySession.init)
            .filter { $0.scope == scope }
        records = snapshot.recentActivity
            .map(NativeRuntimeActivityRecord.init)
            .filter { $0.scope == scope }
        errors = snapshot.recentErrors
            .compactMap(NativeRuntimeActivityError.init)
            .filter { $0.scope == scope }
        // Deliberately not scope-filtered: the evicted entries are gone, so
        // their scope is unknowable. Carrying the runtime-wide total keeps the
        // loss visible instead of discarding the only record that it happened.
        runtimeDiscardedCount = snapshot.droppedActivity &+ snapshot.droppedErrors
    }
}

/// Pushed replacement semantics for application-owned native presentation.
public enum NativeRuntimeActivityUpdate: Sendable {
    case authoritative(NativeRuntimeActivityProjection)
    case next(
        NativeRuntimeActivityProjection,
        predecessorRevision: UInt64,
        eventCursorWasStale: Bool,
        /// How many runtime events were evicted between this observer's cursor
        /// and the oldest event still retained. `eventCursorWasStale` is the
        /// same fact as `lostBeforeBatch > 0` — Rust derives both from the same
        /// comparison — so this is the magnitude the boolean omits, and a
        /// consumer must never have to infer the count from the flag.
        lostBeforeBatch: UInt64
    )
}

public enum NativeRuntimeActivityObservationError:
    Error,
    LocalizedError,
    Equatable
{
    case profileClosed
    case observerCapacity(maximum: Int)

    public var errorDescription: String? {
        switch self {
        case .profileClosed:
            "The native runtime profile is closed."
        case let .observerCapacity(maximum):
            "The native activity observer limit of \(maximum) was reached."
        }
    }
}

/// Cancellation handle for one application observer.
public final class NativeRuntimeActivityObservation: @unchecked Sendable {
    private let lock = NSLock()
    private var cancellation: (@Sendable () -> Void)?

    init(cancellation: @escaping @Sendable () -> Void) {
        self.cancellation = cancellation
    }

    public func cancel() {
        lock.lock()
        let cancellation = cancellation
        self.cancellation = nil
        lock.unlock()
        cancellation?()
    }

    deinit {
        cancel()
    }
}
