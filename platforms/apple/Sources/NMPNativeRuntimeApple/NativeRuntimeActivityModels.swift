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

/// A persisted, runtime-owned activity fact. Native code receives the
/// classification strings verbatim and does not become an activity store.
public struct NativeRuntimeActivityRecord: Sendable {
    public let scope: NativeRuntimeActivityScope
    public let category: String
    public let operation: String
    public let outcome: String
    public let occurredAtMillis: UInt64

    fileprivate init(_ record: RuntimeActivitySnapshot) {
        scope = NativeRuntimeActivityScope(
            manifestAuthor: record.author,
            dTag: record.dTag,
            aggregateHash: record.aggregateHash
        )
        category = record.category
        operation = record.operation
        outcome = record.outcome
        occurredAtMillis = record.occurredAtMillis
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
    }
}

/// Pushed replacement semantics for application-owned native presentation.
public enum NativeRuntimeActivityUpdate: Sendable {
    case authoritative(NativeRuntimeActivityProjection)
    case next(
        NativeRuntimeActivityProjection,
        predecessorRevision: UInt64,
        eventCursorWasStale: Bool
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
