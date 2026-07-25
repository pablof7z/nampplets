import Foundation
import NMPNativeRuntime

// MARK: - Retained provider write projection and observation types

/// A Rust-retained provider write awaiting one explicit native decision.
/// The draft is display-only; native cannot replace the retained write.
public struct NativeRuntimePendingWrite: Sendable, Identifiable {
    public let id: UInt64
    public let approvalID: String
    public let scope: NativeRuntimeActivityScope
    public let sessionID: UInt64
    public let account: String
    public let draftJSON: String

    fileprivate init(_ pending: RuntimePendingWriteSnapshot) {
        id = pending.operationId
        approvalID = pending.approvalId
        scope = NativeRuntimeActivityScope(
            manifestAuthor: pending.author,
            dTag: pending.dTag,
            aggregateHash: pending.aggregateHash
        )
        sessionID = pending.sessionId
        account = pending.account
        draftJSON = pending.draftJson
    }
}

public struct NativeRuntimePendingWriteProjection: Sendable {
    public let revision: UInt64
    public let writes: [NativeRuntimePendingWrite]

    init(_ snapshot: RuntimeSnapshot) {
        revision = snapshot.revision
        writes = snapshot.pendingWrites.map(NativeRuntimePendingWrite.init)
    }
}

public enum NativeRuntimePendingWriteUpdate: Sendable {
    case authoritative(NativeRuntimePendingWriteProjection)
    case next(
        NativeRuntimePendingWriteProjection,
        predecessorRevision: UInt64,
        eventCursorWasStale: Bool
    )
}

public enum NativeRuntimePendingWriteObservationError:
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
            "The native pending-write observer limit of \(maximum) was reached."
        }
    }
}

public final class NativeRuntimePendingWriteObservation: @unchecked Sendable {
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
