import Foundation
import NMPNativeRuntime

// MARK: - Durable receipt projection and observation types

/// A durable NMP receipt mechanically projected for native presentation.
/// NMP remains the sole owner of delivery state and canonical event rows.
public struct NativeRuntimeReceipt: Sendable, Identifiable {
    public let id: String
    public let delivery: String
    public let latestStateJSON: String?

    fileprivate init(_ receipt: RuntimeReceiptSnapshot) {
        id = receipt.receiptId
        delivery = receipt.delivery
        latestStateJSON = receipt.latestStateJson
    }
}

public struct NativeRuntimeReceiptProjection: Sendable {
    public let revision: UInt64
    public let receipts: [NativeRuntimeReceipt]

    init(_ snapshot: RuntimeSnapshot) {
        revision = snapshot.revision
        receipts = snapshot.receipts.map(NativeRuntimeReceipt.init)
    }
}

public enum NativeRuntimeReceiptUpdate: Sendable {
    case authoritative(NativeRuntimeReceiptProjection)
    case next(
        NativeRuntimeReceiptProjection,
        predecessorRevision: UInt64,
        eventCursorWasStale: Bool
    )
}

public enum NativeRuntimeReceiptObservationError:
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
            "The native receipt observer limit of \(maximum) was reached."
        }
    }
}

public final class NativeRuntimeReceiptObservation: @unchecked Sendable {
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
